use crate::config::{AlertConfig, MqttConfig};
use crate::database::Database;
use crate::models::{
    Alert, AlertLevel, Anomaly, AnomalyType, ChannelStatus, RATED_CAPACITY,
    CHANNELS_PER_CABINET, CABINET_ABNORMAL_RATIO_THRESHOLD, CAPACITY_WARNING_THRESHOLD,
};
use chrono::{Duration, Utc};
use dashmap::DashMap;
use rumqttc::{AsyncClient, MqttOptions, QoS};
use std::sync::Arc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

#[derive(Clone)]
pub struct AlertManager {
    config: AlertConfig,
    mqtt_config: MqttConfig,
    db: Database,
    mqtt_client: Option<AsyncClient>,
    recent_alerts: Arc<DashMap<String, chrono::DateTime<Utc>>>,
    cabinet_alert_states: Arc<DashMap<u16, CabinetAlertState>>,
}

#[derive(Debug, Clone)]
struct CabinetAlertState {
    last_level2_alert: Option<chrono::DateTime<Utc>>,
    level2_active: bool,
    abnormal_channels: Vec<u32>,
}

impl CabinetAlertState {
    fn new() -> Self {
        Self {
            last_level2_alert: None,
            level2_active: false,
            abnormal_channels: Vec::new(),
        }
    }
}

impl AlertManager {
    pub async fn new(config: AlertConfig, mqtt_config: MqttConfig, db: Database) -> Self {
        let mqtt_client = if mqtt_config.broker.is_empty() {
            None
        } else {
            Some(Self::create_mqtt_client(&mqtt_config).await)
        };

        Self {
            config,
            mqtt_config,
            db,
            mqtt_client,
            recent_alerts: Arc::new(DashMap::new()),
            cabinet_alert_states: Arc::new(DashMap::new()),
        }
    }

    async fn create_mqtt_client(config: &MqttConfig) -> AsyncClient {
        let mut options = MqttOptions::new(
            format!("{}-alert-{}", config.client_id, Uuid::new_v4()),
            &config.broker,
            config.port,
        );
        options.set_keep_alive(Duration::seconds(30).to_std().unwrap());

        let (client, mut eventloop) = AsyncClient::new(options, 10);

        let client_clone = client.clone();
        tokio::spawn(async move {
            loop {
                match eventloop.poll().await {
                    Ok(_) => {}
                    Err(e) => {
                        warn!("MQTT alert client error: {}", e);
                        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    }
                }
            }
        });

        client
    }

    pub async fn process_anomalies(&self, anomalies: &[Anomaly], channel_data: &crate::models::ChannelData) {
        for anomaly in anomalies {
            if matches!(anomaly.anomaly_type, AnomalyType::CapacityLow) {
                if let Some(alert) = self.create_level1_alert(anomaly).await {
                    self.send_alert(&alert).await;
                }
            }
        }
    }

    async fn create_level1_alert(&self, anomaly: &Anomaly) -> Option<Alert> {
        let dedup_key = format!(
            "level1-{}-{}-{}",
            anomaly.cabinet_id, anomaly.channel_id, anomaly.anomaly_type as u8
        );

        if !self.should_alert(&dedup_key) {
            return None;
        }

        let alert = Alert {
            timestamp: Utc::now(),
            alert_id: Uuid::new_v4(),
            alert_level: AlertLevel::Level1,
            alert_type: "capacity_degradation".to_string(),
            cabinet_id: anomaly.cabinet_id,
            channel_ids: vec![anomaly.channel_id],
            message: format!(
                "【一级告警】化成柜{}通道{}容量低于额定值90%，当前容量{:.3}Ah，预测容量{:.3}Ah",
                anomaly.cabinet_id,
                anomaly.channel_id,
                anomaly.value,
                self.get_predicted_capacity(anomaly.cabinet_id, anomaly.channel_id).await
            ),
            notified_mes: false,
            notified_screen: false,
            acknowledged: false,
        };

        self.record_alert(&dedup_key);

        if let Err(e) = self.db.insert_alert(&alert).await {
            error!("Failed to insert level1 alert: {}", e);
        }

        info!(
            "Level 1 alert created: cabinet={}, channel={}, alert_id={}",
            anomaly.cabinet_id, anomaly.channel_id, alert.alert_id
        );

        Some(alert)
    }

    pub async fn check_cabinet_level2_alerts(&self, cabinet_id: u16) -> Option<Alert> {
        let abnormal_count = self
            .db
            .get_cabinet_abnormal_count(cabinet_id)
            .await
            .unwrap_or(0);

        let ratio = abnormal_count as f64 / CHANNELS_PER_CABINET as f64;
        let threshold = CABINET_ABNORMAL_RATIO_THRESHOLD;

        let mut state = self
            .cabinet_alert_states
            .entry(cabinet_id)
            .or_insert_with(CabinetAlertState::new);

        state.abnormal_channels = self.get_abnormal_channels(cabinet_id).await;

        if ratio > threshold {
            let dedup_key = format!("level2-{}", cabinet_id);

            if !state.level2_active && self.should_alert(&dedup_key) {
                state.level2_active = true;
                state.last_level2_alert = Some(Utc::now());
                self.record_alert(&dedup_key);

                let alert = Alert {
                    timestamp: Utc::now(),
                    alert_id: Uuid::new_v4(),
                    alert_level: AlertLevel::Level2,
                    alert_type: "cabinet_malfunction".to_string(),
                    cabinet_id,
                    channel_ids: state.abnormal_channels.clone(),
                    message: format!(
                        "【二级告警】化成柜{}异常通道比例超过10%，当前异常{}个通道，占比{:.1}%",
                        cabinet_id,
                        abnormal_count,
                        ratio * 100.0
                    ),
                    notified_mes: false,
                    notified_screen: false,
                    acknowledged: false,
                };

                if let Err(e) = self.db.insert_alert(&alert).await {
                    error!("Failed to insert level2 alert: {}", e);
                }

                info!(
                    "Level 2 alert created: cabinet={}, abnormal={}, alert_id={}",
                    cabinet_id, abnormal_count, alert.alert_id
                );

                return Some(alert);
            }
        } else if state.level2_active {
            state.level2_active = false;
            info!("Level 2 alert cleared for cabinet {}", cabinet_id);
        }

        None
    }

    async fn get_abnormal_channels(&self, cabinet_id: u16) -> Vec<u32> {
        match self.db.get_cabinet_status(cabinet_id).await {
            Ok(statuses) => statuses
                .iter()
                .filter(|s| s.is_abnormal)
                .map(|s| s.channel_id)
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    async fn get_predicted_capacity(&self, cabinet_id: u16, channel_id: u32) -> f64 {
        match self.db.get_channel_status(cabinet_id, channel_id).await {
            Ok(Some(status)) => status.predicted_capacity,
            _ => RATED_CAPACITY * CAPACITY_WARNING_THRESHOLD,
        }
    }

    async fn send_alert(&self, alert: &Alert) {
        self.publish_to_mes(alert).await;
        self.publish_to_screen(alert).await;
    }

    async fn publish_to_mes(&self, alert: &Alert) {
        if !self.config.enable_mes_notification {
            return;
        }

        if let Some(client) = &self.mqtt_client {
            let topic = format!("{}/mes", self.mqtt_config.alert_topic);
            let payload = match serde_json::to_string(alert) {
                Ok(p) => p,
                Err(e) => {
                    error!("Failed to serialize alert for MES: {}", e);
                    return;
                }
            };

            match client.publish(&topic, QoS::AtLeastOnce, false, payload).await {
                Ok(_) => {
                    debug!("Alert published to MES: {}", alert.alert_id);
                    self.mark_notified_mes(alert.alert_id).await;
                }
                Err(e) => {
                    error!("Failed to publish alert to MES: {}", e);
                }
            }
        }
    }

    async fn publish_to_screen(&self, alert: &Alert) {
        if !self.config.enable_screen_notification {
            return;
        }

        if let Some(client) = &self.mqtt_client {
            let topic = format!("{}/screen", self.mqtt_config.alert_topic);
            
            let screen_payload = serde_json::json!({
                "alert_id": alert.alert_id.to_string(),
                "timestamp": alert.timestamp.to_rfc3339(),
                "level": match alert.alert_level {
                    AlertLevel::Level1 => "warning",
                    AlertLevel::Level2 => "critical",
                },
                "type": alert.alert_type,
                "cabinet_id": alert.cabinet_id,
                "channel_ids": alert.channel_ids,
                "message": alert.message,
                "is_blinking": matches!(alert.alert_level, AlertLevel::Level2),
            });

            let payload = match serde_json::to_string(&screen_payload) {
                Ok(p) => p,
                Err(e) => {
                    error!("Failed to serialize alert for screen: {}", e);
                    return;
                }
            };

            match client.publish(&topic, QoS::AtLeastOnce, false, payload).await {
                Ok(_) => {
                    debug!("Alert published to screen: {}", alert.alert_id);
                    self.mark_notified_screen(alert.alert_id).await;
                }
                Err(e) => {
                    error!("Failed to publish alert to screen: {}", e);
                }
            }
        }
    }

    async fn mark_notified_mes(&self, alert_id: Uuid) {
        if let Err(e) = self.db.mark_alert_notified_mes(alert_id).await {
            warn!("Failed to mark MES notification: {}", e);
        }
    }

    async fn mark_notified_screen(&self, alert_id: Uuid) {
        if let Err(e) = self.db.mark_alert_notified_screen(alert_id).await {
            warn!("Failed to mark screen notification: {}", e);
        }
    }

    fn should_alert(&self, dedup_key: &str) -> bool {
        if let Some(last_time) = self.recent_alerts.get(dedup_key) {
            (Utc::now() - *last_time).num_seconds() > self.config.dedup_window_seconds as i64
        } else {
            true
        }
    }

    fn record_alert(&self, dedup_key: &str) {
        self.recent_alerts
            .insert(dedup_key.to_string(), Utc::now());
        self.cleanup_old_alerts();
    }

    fn cleanup_old_alerts(&self) {
        let cutoff = Utc::now() - Duration::seconds(self.config.dedup_window_seconds as i64 * 2);
        self.recent_alerts.retain(|_, v| *v > cutoff);
    }

    pub async fn acknowledge_alert(&self, alert_id: Uuid) -> anyhow::Result<()> {
        self.db.acknowledge_alert(alert_id).await?;
        info!("Alert acknowledged: {}", alert_id);
        Ok(())
    }

    pub async fn send_pause_command(&self, cabinet_id: u16, channel_id: u32) {
        if let Some(client) = &self.mqtt_client {
            let topic = format!("battery/cabinet/{}/command", cabinet_id);
            let payload = serde_json::json!({
                "command": "pause",
                "channel_id": channel_id,
                "timestamp": Utc::now().to_rfc3339(),
                "reason": "voltage_anomaly"
            });

            if let Ok(payload_str) = serde_json::to_string(&payload) {
                match client.publish(&topic, QoS::AtLeastOnce, false, payload_str).await {
                    Ok(_) => info!("Pause command sent to {}-{}", cabinet_id, channel_id),
                    Err(e) => error!("Failed to send pause command: {}", e),
                }
            }
        }
    }

    pub async fn send_resume_command(&self, cabinet_id: u16, channel_id: u32) {
        if let Some(client) = &self.mqtt_client {
            let topic = format!("battery/cabinet/{}/command", cabinet_id);
            let payload = serde_json::json!({
                "command": "resume",
                "channel_id": channel_id,
                "timestamp": Utc::now().to_rfc3339()
            });

            if let Ok(payload_str) = serde_json::to_string(&payload) {
                match client.publish(&topic, QoS::AtLeastOnce, false, payload_str).await {
                    Ok(_) => info!("Resume command sent to {}-{}", cabinet_id, channel_id),
                    Err(e) => error!("Failed to send resume command: {}", e),
                }
            }
        }
    }

    pub async fn get_cabinet_stats(&self, cabinet_id: u16) -> Option<crate::models::CabinetStats> {
        let statuses = self.db.get_cabinet_status(cabinet_id).await.ok()?;
        
        if statuses.is_empty() {
            return None;
        }

        let total_channels = statuses.len() as u16;
        let abnormal_count = statuses.iter().filter(|s| s.is_abnormal).count() as u16;
        
        let avg_voltage = statuses.iter().map(|s| s.current_voltage).sum::<f64>() / statuses.len() as f64;
        let avg_current = statuses.iter().map(|s| s.current_current).sum::<f64>() / statuses.len() as f64;
        let avg_temperature = statuses.iter().map(|s| s.current_temperature).sum::<f64>() / statuses.len() as f64;
        
        let variance = statuses.iter()
            .map(|s| (s.current_voltage - avg_voltage).powi(2))
            .sum::<f64>() / statuses.len() as f64;
        let std_voltage = variance.sqrt();

        let stats = crate::models::CabinetStats {
            timestamp: Utc::now(),
            cabinet_id,
            avg_voltage,
            std_voltage,
            avg_current,
            avg_temperature,
            abnormal_channel_count: abnormal_count,
            total_channels,
            abnormal_ratio: abnormal_count as f64 / total_channels as f64,
        };

        if let Err(e) = self.db.insert_cabinet_stats(&stats).await {
            warn!("Failed to insert cabinet stats: {}", e);
        }

        Some(stats)
    }
}
