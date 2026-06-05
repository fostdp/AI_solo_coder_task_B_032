use crate::config::DetectionConfig;
use crate::database::Database;
use crate::models::{
    Anomaly, AnomalyType, ChannelData, ChannelStatus, Severity, RATED_CAPACITY,
};
use chrono::Utc;
use dashmap::DashMap;
use std::sync::Arc;
use tracing::{debug, info, warn};

#[derive(Clone)]
pub struct AnomalyDetector {
    config: DetectionConfig,
    db: Database,
    recent_voltage_deviations: Arc<DashMap<(u16, u32), Vec<(chrono::DateTime<Utc>, f64)>>>,
    anomaly_cooldown: Arc<DashMap<(u16, u32, AnomalyType), chrono::DateTime<Utc>>>,
}

impl AnomalyDetector {
    pub fn new(config: DetectionConfig, db: Database) -> Self {
        Self {
            config,
            db,
            recent_voltage_deviations: Arc::new(DashMap::new()),
            anomaly_cooldown: Arc::new(DashMap::new()),
        }
    }

    pub async fn detect_anomalies(&self, data: &ChannelData) -> Vec<Anomaly> {
        let mut anomalies = Vec::new();

        if let Some(anomaly) = self.detect_voltage_deviation(data).await {
            anomalies.push(anomaly);
        }

        if let Some(anomaly) = self.detect_capacity_low(data) {
            anomalies.push(anomaly);
        }

        if let Some(anomaly) = self.detect_temperature_high(data) {
            anomalies.push(anomaly);
        }

        if let Some(anomaly) = self.detect_current_abnormal(data) {
            anomalies.push(anomaly);
        }

        if !anomalies.is_empty() {
            for anomaly in &anomalies {
                if let Err(e) = self.db.insert_anomaly(anomaly).await {
                    warn!("Failed to insert anomaly: {}", e);
                }
            }
            
            self.update_channel_status(data, &anomalies).await;
        }

        anomalies
    }

    async fn detect_voltage_deviation(&self, data: &ChannelData) -> Option<Anomaly> {
        let (avg, std) = self
            .db
            .get_cabinet_voltage_stats(data.cabinet_id, data.timestamp)
            .await
            .unwrap_or((0.0, 0.0));

        if std == 0.0 {
            return None;
        }

        let deviation = (data.voltage - avg).abs();
        let sigma_threshold = self.config.voltage_deviation_sigma * std;

        let key = (data.cabinet_id, data.channel_id);
        let mut deviations = self
            .recent_voltage_deviations
            .entry(key)
            .or_insert_with(Vec::new);
        
        deviations.push((data.timestamp, deviation));
        deviations.retain(|(t, _)| {
            (data.timestamp - *t).num_seconds() < 60
        });

        let sustained_deviations = deviations
            .iter()
            .filter(|(_, d)| *d > sigma_threshold)
            .count();

        if sustained_deviations >= 3 {
            let anomaly_type = AnomalyType::VoltageDeviation;
            if self.is_in_cooldown(data.cabinet_id, data.channel_id, anomaly_type) {
                return None;
            }

            self.set_cooldown(data.cabinet_id, data.channel_id, anomaly_type);

            info!(
                "Voltage deviation detected: cabinet={}, channel={}, voltage={}, avg={}, sigma={}",
                data.cabinet_id, data.channel_id, data.voltage, avg, deviation / std
            );

            return Some(Anomaly {
                timestamp: Utc::now(),
                cabinet_id: data.cabinet_id,
                channel_id: data.channel_id,
                anomaly_type,
                severity: Severity::Warning,
                description: format!(
                    "电压偏离平均值 {:.2}σ，当前电压 {:.4}V，柜平均 {:.4}V",
                    deviation / std,
                    data.voltage,
                    avg
                ),
                value: data.voltage,
                threshold: avg + sigma_threshold,
                is_paused: false,
                resolved: false,
            });
        }

        None
    }

    fn detect_capacity_low(&self, data: &ChannelData) -> Option<Anomaly> {
        let capacity_ratio = data.capacity / RATED_CAPACITY;
        
        if capacity_ratio < self.config.capacity_warning_ratio {
            let anomaly_type = AnomalyType::CapacityLow;
            
            if self.is_in_cooldown(data.cabinet_id, data.channel_id, anomaly_type) {
                return None;
            }

            self.set_cooldown(data.cabinet_id, data.channel_id, anomaly_type);

            info!(
                "Low capacity detected: cabinet={}, channel={}, capacity={:.4}, ratio={:.2}%",
                data.cabinet_id, data.channel_id, data.capacity, capacity_ratio * 100.0
            );

            return Some(Anomaly {
                timestamp: Utc::now(),
                cabinet_id: data.cabinet_id,
                channel_id: data.channel_id,
                anomaly_type,
                severity: Severity::Critical,
                description: format!(
                    "容量低于额定值 {:.1}%，当前容量 {:.4}Ah，额定 {:.1}Ah",
                    self.config.capacity_warning_ratio * 100.0,
                    data.capacity,
                    RATED_CAPACITY
                ),
                value: data.capacity,
                threshold: RATED_CAPACITY * self.config.capacity_warning_ratio,
                is_paused: false,
                resolved: false,
            });
        }

        None
    }

    fn detect_temperature_high(&self, data: &ChannelData) -> Option<Anomaly> {
        if data.temperature > self.config.temperature_high_threshold {
            let anomaly_type = AnomalyType::TemperatureHigh;
            
            if self.is_in_cooldown(data.cabinet_id, data.channel_id, anomaly_type) {
                return None;
            }

            self.set_cooldown(data.cabinet_id, data.channel_id, anomaly_type);

            info!(
                "High temperature detected: cabinet={}, channel={}, temp={:.2}",
                data.cabinet_id, data.channel_id, data.temperature
            );

            return Some(Anomaly {
                timestamp: Utc::now(),
                cabinet_id: data.cabinet_id,
                channel_id: data.channel_id,
                anomaly_type,
                severity: Severity::Warning,
                description: format!(
                    "温度过高 {:.2}°C，阈值 {:.1}°C",
                    data.temperature, self.config.temperature_high_threshold
                ),
                value: data.temperature,
                threshold: self.config.temperature_high_threshold,
                is_paused: false,
                resolved: false,
            });
        }

        None
    }

    fn detect_current_abnormal(&self, data: &ChannelData) -> Option<Anomaly> {
        let abs_current = data.current.abs();
        
        if abs_current > 2.0 || abs_current < -2.0 {
            let anomaly_type = AnomalyType::CurrentAbnormal;
            
            if self.is_in_cooldown(data.cabinet_id, data.channel_id, anomaly_type) {
                return None;
            }

            self.set_cooldown(data.cabinet_id, data.channel_id, anomaly_type);

            info!(
                "Abnormal current detected: cabinet={}, channel={}, current={:.4}",
                data.cabinet_id, data.channel_id, data.current
            );

            return Some(Anomaly {
                timestamp: Utc::now(),
                cabinet_id: data.cabinet_id,
                channel_id: data.channel_id,
                anomaly_type,
                severity: Severity::Warning,
                description: format!("电流异常 {:.4}A，超出正常范围", data.current),
                value: data.current,
                threshold: 2.0,
                is_paused: false,
                resolved: false,
            });
        }

        None
    }

    async fn update_channel_status(&self, data: &ChannelData, anomalies: &[Anomaly]) {
        let has_critical = anomalies.iter().any(|a| matches!(a.severity, Severity::Critical));
        let has_voltage_dev = anomalies
            .iter()
            .any(|a| matches!(a.anomaly_type, AnomalyType::VoltageDeviation));

        let status = ChannelStatus {
            cabinet_id: data.cabinet_id,
            channel_id: data.channel_id,
            last_update: Utc::now(),
            current_stage: data.stage,
            current_voltage: data.voltage,
            current_current: data.current,
            current_temperature: data.temperature,
            current_capacity: data.capacity,
            cycle_index: data.cycle_index,
            is_abnormal: !anomalies.is_empty(),
            is_paused: has_voltage_dev,
            capacity_ratio: data.capacity / RATED_CAPACITY,
            predicted_capacity: 0.0,
        };

        if let Err(e) = self.db.update_channel_status(&status).await {
            warn!("Failed to update channel status: {}", e);
        }

        if has_voltage_dev {
            if let Err(e) = self.db.pause_channel(data.cabinet_id, data.channel_id).await {
                warn!("Failed to pause channel: {}", e);
            }
        }
    }

    fn is_in_cooldown(&self, cabinet_id: u16, channel_id: u32, anomaly_type: AnomalyType) -> bool {
        let key = (cabinet_id, channel_id, anomaly_type);
        if let Some(last_time) = self.anomaly_cooldown.get(&key) {
            (Utc::now() - *last_time).num_seconds() < 300
        } else {
            false
        }
    }

    fn set_cooldown(&self, cabinet_id: u16, channel_id: u32, anomaly_type: AnomalyType) {
        let key = (cabinet_id, channel_id, anomaly_type);
        self.anomaly_cooldown.insert(key, Utc::now());
    }

    pub async fn check_cabinet_level_anomaly(&self, cabinet_id: u16, total_channels: usize) -> Option<Anomaly> {
        let abnormal_count = self
            .db
            .get_cabinet_abnormal_count(cabinet_id)
            .await
            .unwrap_or(0);

        let ratio = abnormal_count as f64 / total_channels as f64;

        if ratio > self.config.cabinet_abnormal_ratio_threshold {
            debug!(
                "Cabinet level anomaly: cabinet={}, abnormal={}, ratio={:.2}%",
                cabinet_id,
                abnormal_count,
                ratio * 100.0
            );
        }

        None
    }
}
