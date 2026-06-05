use crate::database::Database;
use crate::messages::{
    AnomalySender, DataBatch, DataBatchSender, PredictionSender,
};
use crate::models::{ChannelData, PredictionStatus, Stage, NUM_CABINETS};
use crate::mqtt_collector::{RawChannelData, DataReceiver};
use crate::stage_detector::StageDetector;
use anyhow::Result;
use chrono::DateTime;
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

type CabinetSender = mpsc::UnboundedSender<Vec<RawChannelData>>;

pub struct DataPipeline {
    db: Database,
    stage_detector: StageDetector,
    channel_cycle_counts: Arc<DashMap<(u16, u32), u16>>,
    cabinet_senders: DashMap<u16, CabinetSender>,
    cabinet_tasks: Arc<Mutex<Vec<JoinHandle<()>>>>,
    data_batch_sender: DataBatchSender,
    anomaly_sender: AnomalySender,
    prediction_sender: PredictionSender,
    prediction_min_cycles: usize,
}

impl DataPipeline {
    pub fn new(
        db: Database,
        stage_detector: StageDetector,
        data_batch_sender: DataBatchSender,
        anomaly_sender: AnomalySender,
        prediction_sender: PredictionSender,
        prediction_min_cycles: usize,
    ) -> Self {
        Self {
            db,
            stage_detector,
            channel_cycle_counts: Arc::new(DashMap::new()),
            cabinet_senders: DashMap::new(),
            cabinet_tasks: Arc::new(Mutex::new(Vec::new())),
            data_batch_sender,
            anomaly_sender,
            prediction_sender,
            prediction_min_cycles,
        }
    }

    pub async fn start(&mut self, mut raw_data_receiver: DataReceiver) -> Result<()> {
        self.spawn_cabinet_processors().await;
        info!("Data pipeline started with {} cabinet processors", NUM_CABINETS);

        let cabinet_senders = self.cabinet_senders.clone();

        tokio::spawn(async move {
            while let Some(batch) = raw_data_receiver.recv().await {
                if batch.is_empty() {
                    continue;
                }
                let cabinet_id = batch[0].cabinet_id;
                if let Some(sender) = cabinet_senders.get(&cabinet_id) {
                    if let Err(e) = sender.send(batch) {
                        warn!("Failed to send to cabinet {} processor: {}", cabinet_id, e);
                    }
                }
            }
        });

        Ok(())
    }

    async fn spawn_cabinet_processors(&mut self) {
        let mut tasks = self.cabinet_tasks.lock().await;

        for cabinet_id in 0..NUM_CABINETS as u16 {
            let (tx, rx) = mpsc::unbounded_channel::<Vec<RawChannelData>>();

            let processor = CabinetProcessor::new(
                cabinet_id,
                self.db.clone(),
                self.stage_detector.clone(),
                self.channel_cycle_counts.clone(),
                self.data_batch_sender.clone(),
                self.anomaly_sender.clone(),
                self.prediction_sender.clone(),
                self.prediction_min_cycles,
            );

            let handle = tokio::spawn(async move {
                processor.run(rx).await;
            });

            self.cabinet_senders.insert(cabinet_id, tx);
            tasks.push(handle);
        }
    }
}

struct CabinetProcessor {
    cabinet_id: u16,
    db: Database,
    stage_detector: StageDetector,
    channel_cycle_counts: Arc<DashMap<(u16, u32), u16>>,
    data_batch_sender: DataBatchSender,
    anomaly_sender: AnomalySender,
    prediction_sender: PredictionSender,
    prediction_min_cycles: usize,
}

impl CabinetProcessor {
    fn new(
        cabinet_id: u16,
        db: Database,
        stage_detector: StageDetector,
        channel_cycle_counts: Arc<DashMap<(u16, u32), u16>>,
        data_batch_sender: DataBatchSender,
        anomaly_sender: AnomalySender,
        prediction_sender: PredictionSender,
        prediction_min_cycles: usize,
    ) -> Self {
        Self {
            cabinet_id,
            db,
            stage_detector,
            channel_cycle_counts,
            data_batch_sender,
            anomaly_sender,
            prediction_sender,
            prediction_min_cycles,
        }
    }

    async fn run(&self, mut rx: mpsc::UnboundedReceiver<Vec<RawChannelData>>) {
        info!("Cabinet {} data processor started", self.cabinet_id);

        while let Some(batch) = rx.recv().await {
            if let Err(e) = self.process_batch(batch).await {
                warn!("Cabinet {} processor error: {}", self.cabinet_id, e);
            }
        }

        warn!("Cabinet {} data processor stopped", self.cabinet_id);
    }

    async fn process_batch(&self, batch: Vec<RawChannelData>) -> Result<()> {
        let mut channel_data_batch = Vec::with_capacity(batch.len());
        let mut cycle_updates = Vec::new();
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

            let completed_cycles = self.track_cycle_progress(raw.channel_id, raw.cycle_index, detected_stage);

            channel_data_batch.push(data.clone());

            if self.should_try_prediction(&data, completed_cycles) {
                prediction_candidates.push(crate::messages::PredictionCandidate {
                    cabinet_id: self.cabinet_id,
                    channel_id: raw.channel_id,
                    current_cycle: raw.cycle_index,
                    completed_cycles,
                });
            }

            cycle_updates.push(crate::messages::CycleUpdate {
                channel_id: raw.channel_id,
                completed_cycles,
            });
        }

        if !channel_data_batch.is_empty() {
            for data in &channel_data_batch {
                self.db.insert_data(data.clone()).await?;
            }

            let data_batch = DataBatch {
                cabinet_id: self.cabinet_id,
                data: channel_data_batch.clone(),
                cycle_updates,
                prediction_candidates: prediction_candidates.clone(),
            };

            if let Err(e) = self.data_batch_sender.send(data_batch).await {
                warn!("Failed to send data batch: {}", e);
            }

            for candidate in &prediction_candidates {
                let completed_cycles = candidate.completed_cycles;
                if completed_cycles < self.prediction_min_cycles as u16 {
                    debug!(
                        "Cabinet {} channel {}: insufficient cycles for prediction (completed={}, required={})",
                        self.cabinet_id, candidate.channel_id, completed_cycles, self.prediction_min_cycles
                    );
                    self.update_prediction_status(candidate.channel_id, PredictionStatus::Predicting, completed_cycles).await;
                } else {
                    let req = crate::messages::PredictionRequest {
                        cabinet_id: self.cabinet_id,
                        channel_id: candidate.channel_id,
                        n_cycles: self.prediction_min_cycles,
                    };
                    if let Err(e) = self.prediction_sender.send(req).await {
                        warn!("Failed to send prediction request: {}", e);
                    }
                }
            }
        }

        Ok(())
    }

    fn track_cycle_progress(&self, channel_id: u32, cycle_index: u16, stage: Stage) -> u16 {
        let key = (self.cabinet_id, channel_id);
        if matches!(stage, Stage::Rest) {
            self.channel_cycle_counts
                .entry(key)
                .and_modify(|count| {
                    if *count < cycle_index {
                        *count = cycle_index;
                    }
                })
                .or_insert_with(|| cycle_index);
        }

        self.channel_cycle_counts
            .get(&key)
            .map(|c| *c)
            .unwrap_or(0)
    }

    fn should_try_prediction(&self, data: &ChannelData, completed_cycles: u16) -> bool {
        if !matches!(data.stage, Stage::Rest) {
            return false;
        }

        if data.channel_id % 50 != 0 {
            return false;
        }

        completed_cycles >= 1
    }

    async fn update_prediction_status(&self, channel_id: u32, status: PredictionStatus, completed_cycles: u16) {
        if let Some(mut channel_status) = self
            .db
            .get_channel_status(self.cabinet_id, channel_id)
            .await
            .ok()
            .flatten()
        {
            channel_status.prediction_status = status;
            channel_status.completed_cycles = completed_cycles;
            channel_status.predicted_capacity = 0.0;
            let _ = self.db.update_channel_status(&channel_status).await;
        }
    }
}
