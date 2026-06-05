use crate::alert_manager::AlertManager;
use crate::anomaly_detector::AnomalyDetector;
use crate::config::MqttConfig;
use crate::database::Database;
use crate::models::{ChannelData, PredictionStatus, Stage, NUM_CABINETS};
use crate::prediction::CapacityPredictor;
use crate::stage_detector::StageDetector;
use anyhow::Result;
use chrono::DateTime;
use dashmap::DashMap;
use rumqttc::{AsyncClient, Event, EventLoop, MqttOptions, Packet, QoS};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

#[derive(Debug, Deserialize)]
struct RawChannelData {
    timestamp: String,
    cabinet_id: u16,
    channel_id: u32,
    voltage: f64,
    current: f64,
    temperature: f64,
    capacity: f64,
    cycle_index: u16,
    stage: String,
    stage_duration: u32,
}

struct CabinetProcessor {
    cabinet_id: u16,
    db: Database,
    stage_detector: StageDetector,
    anomaly_detector: AnomalyDetector,
    alert_manager: AlertManager,
    predictor: CapacityPredictor,
    channel_cycle_counts: DashMap<u32, u16>,
}

impl CabinetProcessor {
    fn new(
        cabinet_id: u16,
        db: Database,
        stage_detector: StageDetector,
        anomaly_detector: AnomalyDetector,
        alert_manager: AlertManager,
        predictor: CapacityPredictor,
    ) -> Self {
        Self {
            cabinet_id,
            db,
            stage_detector,
            anomaly_detector,
            alert_manager,
            predictor,
            channel_cycle_counts: DashMap::new(),
        }
    }

    async fn process_batch(&self, batch: Vec<RawChannelData>) -> Result<()> {
        let mut channel_data_batch = Vec::with_capacity(batch.len());
        let mut prediction_candidates = Vec::new();

        for raw in batch {
            let timestamp = DateTime::parse_from_rfc3339(&raw.timestamp)?;
            let timestamp = timestamp.with_timezone(&chrono::Utc);

            let reported_stage = Stage::from_str(&raw.stage).unwrap_or(Stage::Rest);

            let mut data = ChannelData {
                timestamp,
                cabinet_id: raw.cabinet_id,
                channel_id: raw.channel_id,
                voltage: raw.voltage,
                current: raw.current,
                temperature: raw.temperature,
                capacity: raw.capacity,
                cycle_index: raw.cycle_index,
                stage: reported_stage,
                stage_duration: raw.stage_duration,
            };

            let (detected_stage, duration) = self.stage_detector.detect_stage(&data);
            data.stage = detected_stage;
            data.stage_duration = duration;

            if raw.channel_id == 0 {
                debug!(
                    "Cabinet {} processing: channel={}, stage={:?}, voltage={:.4}",
                    self.cabinet_id, raw.channel_id, detected_stage, raw.voltage
                );
            }

            self.track_cycle_progress(raw.channel_id, raw.cycle_index, detected_stage);

            channel_data_batch.push(data.clone());

            if self.should_try_prediction(&data) {
                prediction_candidates.push((raw.channel_id, raw.cycle_index));
            }
        }

        if !channel_data_batch.is_empty() {
            self.batch_insert_data(&channel_data_batch).await?;
            self.batch_detect_anomalies(&channel_data_batch).await;
        }

        if !prediction_candidates.is_empty() {
            self.batch_process_predictions(&prediction_candidates).await;
        }

        Ok(())
    }

    fn track_cycle_progress(&self, channel_id: u32, cycle_index: u16, stage: Stage) {
        if matches!(stage, Stage::Rest) {
            self.channel_cycle_counts
                .entry(channel_id)
                .and_modify(|count| {
                    if *count < cycle_index {
                        *count = cycle_index;
                    }
                })
                .or_insert_with(|| cycle_index);
        }
    }

    fn should_try_prediction(&self, data: &ChannelData) -> bool {
        if !matches!(data.stage, Stage::Rest) {
            return false;
        }

        if data.channel_id % 50 != 0 {
            return false;
        }

        let completed_cycles = self
            .channel_cycle_counts
            .get(&data.channel_id)
            .map(|c| *c)
            .unwrap_or(0);

        completed_cycles >= 3
    }

    async fn batch_insert_data(&self, batch: &[ChannelData]) -> Result<()> {
        for data in batch {
            self.db.insert_data(data.clone()).await?;
        }
        Ok(())
    }

    async fn batch_detect_anomalies(&self, batch: &[ChannelData]) {
        for data in batch {
            let anomalies = self.anomaly_detector.detect_anomalies(data).await;
            if !anomalies.is_empty() {
                self.alert_manager.process_anomalies(&anomalies, data).await;

                for anomaly in &anomalies {
                    use crate::models::AnomalyType::VoltageDeviation;
                    if matches!(anomaly.anomaly_type, VoltageDeviation) {
                        self.alert_manager
                            .send_pause_command(anomaly.cabinet_id, anomaly.channel_id)
                            .await;
                    }
                }
            }

            let completed_cycles = self
                .channel_cycle_counts
                .get(&data.channel_id)
                .map(|c| *c)
                .unwrap_or(0);

            let prediction_status = if completed_cycles < 3 {
                PredictionStatus::Predicting
            } else {
                PredictionStatus::Completed
            };

            if let Some(mut status) = self
                .db
                .get_channel_status(data.cabinet_id, data.channel_id)
                .await
                .ok()
                .flatten()
            {
                status.prediction_status = prediction_status;
                status.completed_cycles = completed_cycles;
                let _ = self.db.update_channel_status(&status).await;
            }
        }
    }

    async fn batch_process_predictions(&self, candidates: &[(u32, u16)]) {
        let min_cycles = self.anomaly_detector.config.prediction_min_cycles;

        for &(channel_id, _current_cycle) in candidates {
            let completed_cycles = self
                .channel_cycle_counts
                .get(&channel_id)
                .map(|c| *c)
                .unwrap_or(0);

            if completed_cycles < min_cycles as u16 {
                debug!(
                    "Cabinet {} channel {}: insufficient cycles for prediction (completed={}, required={})",
                    self.cabinet_id, channel_id, completed_cycles, min_cycles
                );

                if let Some(mut status) = self
                    .db
                    .get_channel_status(self.cabinet_id, channel_id)
                    .await
                    .ok()
                    .flatten()
                {
                    status.prediction_status = PredictionStatus::InsufficientData;
                    status.completed_cycles = completed_cycles;
                    status.predicted_capacity = 0.0;
                    let _ = self.db.update_channel_status(&status).await;
                }
                continue;
            }

            let cabinet_id = self.cabinet_id;
            let predictor = self.predictor.clone();

            debug!(
                "Cabinet {} channel {}: triggering prediction (completed_cycles={})",
                cabinet_id, channel_id, completed_cycles
            );

            let _ = predictor
                .predict_capacity(cabinet_id, channel_id, min_cycles)
                .await;
        }
    }
}

type CabinetSender = mpsc::UnboundedSender<Vec<RawChannelData>>;

pub struct MqttDataClient {
    config: MqttConfig,
    client: AsyncClient,
    eventloop: Arc<Mutex<Option<EventLoop>>>,
    db: Database,
    stage_detector: StageDetector,
    anomaly_detector: AnomalyDetector,
    alert_manager: AlertManager,
    predictor: CapacityPredictor,
    cabinet_senders: DashMap<u16, CabinetSender>,
    cabinet_tasks: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

impl MqttDataClient {
    pub fn new(
        config: MqttConfig,
        db: Database,
        stage_detector: StageDetector,
        anomaly_detector: AnomalyDetector,
        alert_manager: AlertManager,
        predictor: CapacityPredictor,
    ) -> Result<Self> {
        let mut options = MqttOptions::new(&config.client_id, &config.broker, config.port);
        options.set_keep_alive(std::time::Duration::from_secs(30));

        let (client, eventloop) = AsyncClient::new(options, 2048);

        Ok(Self {
            config,
            client,
            eventloop: Arc::new(Mutex::new(Some(eventloop))),
            db,
            stage_detector,
            anomaly_detector,
            alert_manager,
            predictor,
            cabinet_senders: DashMap::new(),
            cabinet_tasks: Arc::new(Mutex::new(Vec::new())),
        })
    }

    pub async fn start(&self) -> Result<()> {
        self.spawn_cabinet_processors().await;
        info!("Spawned {} cabinet processors", NUM_CABINETS);

        let subscribe_topic = self.config.subscribe_topic.clone();
        self.client
            .subscribe(&subscribe_topic, QoS::AtLeastOnce)
            .await?;
        info!("Subscribed to MQTT topic: {}", subscribe_topic);

        let eventloop = self.eventloop.lock().await.take().unwrap();
        let client = self.client.clone();
        let cabinet_senders = self.cabinet_senders.clone();

        tokio::spawn(async move {
            Self::run_event_loop(eventloop, client, cabinet_senders).await;
        });

        Ok(())
    }

    async fn spawn_cabinet_processors(&self) {
        let mut tasks = self.cabinet_tasks.lock().await;

        for cabinet_id in 0..NUM_CABINETS as u16 {
            let (tx, rx) = mpsc::unbounded_channel::<Vec<RawChannelData>>();

            let processor = CabinetProcessor::new(
                cabinet_id,
                self.db.clone(),
                self.stage_detector.clone(),
                self.anomaly_detector.clone(),
                self.alert_manager.clone(),
                self.predictor.clone(),
            );

            let handle = tokio::spawn(async move {
                Self::run_cabinet_processor(processor, rx).await;
            });

            self.cabinet_senders.insert(cabinet_id, tx);
            tasks.push(handle);
        }
    }

    async fn run_cabinet_processor(
        processor: CabinetProcessor,
        mut rx: mpsc::UnboundedReceiver<Vec<RawChannelData>>,
    ) {
        info!("Cabinet {} processor started", processor.cabinet_id);

        while let Some(batch) = rx.recv().await {
            if let Err(e) = processor.process_batch(batch).await {
                warn!(
                    "Cabinet {} processor error: {}",
                    processor.cabinet_id, e
                );
            }
        }

        warn!("Cabinet {} processor stopped", processor.cabinet_id);
    }

    async fn run_event_loop(
        mut eventloop: EventLoop,
        _client: AsyncClient,
        cabinet_senders: DashMap<u16, CabinetSender>,
    ) {
        info!("MQTT event loop started");

        loop {
            match eventloop.poll().await {
                Ok(Event::Incoming(Packet::Publish(publish))) => {
                    if let Err(e) =
                        Self::route_message(publish.payload.as_ref(), &cabinet_senders)
                    {
                        warn!("Failed to route MQTT message: {}", e);
                    }
                }
                Ok(Event::Outgoing(_)) => {}
                Ok(_) => {}
                Err(e) => {
                    error!("MQTT eventloop error: {}", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
            }
        }
    }

    fn route_message(
        payload: &[u8],
        cabinet_senders: &DashMap<u16, CabinetSender>,
    ) -> Result<()> {
        let raw_data: Vec<RawChannelData> = serde_json::from_slice(payload)?;

        if raw_data.is_empty() {
            return Ok(());
        }

        let cabinet_id = raw_data[0].cabinet_id;

        if let Some(sender) = cabinet_senders.get(&cabinet_id) {
            if let Err(e) = sender.send(raw_data) {
                warn!(
                    "Failed to send batch to cabinet {} processor: {}",
                    cabinet_id, e
                );
            }
        } else {
            warn!(
                "No processor found for cabinet {}, message dropped",
                cabinet_id
            );
        }

        Ok(())
    }

    pub async fn periodic_tasks(&self) {
        let alert_manager = self.alert_manager.clone();
        let db = self.db.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));

            loop {
                interval.tick().await;

                for cabinet_id in 0..NUM_CABINETS as u16 {
                    let alert_manager_clone = alert_manager.clone();

                    tokio::spawn(async move {
                        if let Some(alert) = alert_manager_clone
                            .check_cabinet_level2_alerts(cabinet_id)
                            .await
                        {
                            alert_manager_clone.send_alert(&alert).await;
                        }

                        alert_manager_clone.get_cabinet_stats(cabinet_id).await;
                    });
                }

                if let Err(e) = db.flush().await {
                    warn!("Periodic flush failed: {}", e);
                }
            }
        });
    }
}
