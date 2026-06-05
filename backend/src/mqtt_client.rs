use crate::alert_manager::AlertManager;
use crate::anomaly_detector::AnomalyDetector;
use crate::config::MqttConfig;
use crate::database::Database;
use crate::models::{ChannelData, Stage, NUM_CABINETS};
use crate::prediction::CapacityPredictor;
use crate::stage_detector::StageDetector;
use anyhow::Result;
use chrono::DateTime;
use rumqttc::{AsyncClient, Event, EventLoop, MqttOptions, Packet, QoS};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::Mutex;
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

pub struct MqttDataClient {
    config: MqttConfig,
    client: AsyncClient,
    eventloop: Arc<Mutex<Option<EventLoop>>>,
    db: Database,
    stage_detector: StageDetector,
    anomaly_detector: AnomalyDetector,
    alert_manager: AlertManager,
    predictor: CapacityPredictor,
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

        let (client, eventloop) = AsyncClient::new(options, 1024);

        Ok(Self {
            config,
            client,
            eventloop: Arc::new(Mutex::new(Some(eventloop))),
            db,
            stage_detector,
            anomaly_detector,
            alert_manager,
            predictor,
        })
    }

    pub async fn start(&self) -> Result<()> {
        let subscribe_topic = self.config.subscribe_topic.clone();
        
        self.client
            .subscribe(&subscribe_topic, QoS::AtLeastOnce)
            .await?;

        info!("Subscribed to MQTT topic: {}", subscribe_topic);

        let eventloop = self.eventloop.lock().await.take().unwrap();
        let client = self.client.clone();
        let db = self.db.clone();
        let stage_detector = self.stage_detector.clone();
        let anomaly_detector = self.anomaly_detector.clone();
        let alert_manager = self.alert_manager.clone();
        let predictor = self.predictor.clone();

        tokio::spawn(async move {
            Self::run_event_loop(
                eventloop,
                client,
                db,
                stage_detector,
                anomaly_detector,
                alert_manager,
                predictor,
            )
            .await;
        });

        Ok(())
    }

    async fn run_event_loop(
        mut eventloop: EventLoop,
        client: AsyncClient,
        db: Database,
        stage_detector: StageDetector,
        anomaly_detector: AnomalyDetector,
        alert_manager: AlertManager,
        predictor: CapacityPredictor,
    ) {
        loop {
            match eventloop.poll().await {
                Ok(Event::Incoming(Packet::Publish(publish))) => {
                    if let Err(e) = Self::handle_message(
                        publish.payload.as_ref(),
                        &db,
                        &stage_detector,
                        &anomaly_detector,
                        &alert_manager,
                        &predictor,
                    )
                    .await
                    {
                        warn!("Failed to handle MQTT message: {}", e);
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

    async fn handle_message(
        payload: &[u8],
        db: &Database,
        stage_detector: &StageDetector,
        anomaly_detector: &AnomalyDetector,
        alert_manager: &AlertManager,
        predictor: &CapacityPredictor,
    ) -> Result<()> {
        let raw_data: Vec<RawChannelData> = serde_json::from_slice(payload)?;

        for raw in raw_data {
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

            let (detected_stage, duration) = stage_detector.detect_stage(&data);
            data.stage = detected_stage;
            data.stage_duration = duration;

            if raw.channel_id == 0 && raw.cabinet_id < NUM_CABINETS as u16 {
                debug!(
                    "Processing data: cabinet={}, channel={}, stage={:?}, voltage={:.4}",
                    raw.cabinet_id, raw.channel_id, detected_stage, raw.voltage
                );
            }

            db.insert_data(data.clone()).await?;

            let anomalies = anomaly_detector.detect_anomalies(&data).await;
            if !anomalies.is_empty() {
                alert_manager.process_anomalies(&anomalies, &data).await;

                for anomaly in &anomalies {
                    use crate::models::AnomalyType::VoltageDeviation;
                    if matches!(anomaly.anomaly_type, VoltageDeviation) {
                        alert_manager
                            .send_pause_command(anomaly.cabinet_id, anomaly.channel_id)
                            .await;
                    }
                }
            }

            if data.cycle_index >= 3 && data.stage == Stage::Rest && data.channel_id % 100 == 0 {
                let predictor_clone = predictor.clone();
                let cabinet_id = data.cabinet_id;
                let channel_id = data.channel_id;
                
                tokio::spawn(async move {
                    predictor_clone
                        .predict_capacity(cabinet_id, channel_id, 3)
                        .await;
                });
            }
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
