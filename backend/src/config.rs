use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub mqtt: MqttConfig,
    pub clickhouse: ClickHouseConfig,
    pub server: ServerConfig,
    pub detection: DetectionConfig,
    pub alert: AlertConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MqttConfig {
    pub broker: String,
    pub port: u16,
    pub subscribe_topic: String,
    pub alert_topic: String,
    pub client_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClickHouseConfig {
    pub url: String,
    pub database: String,
    pub user: String,
    pub password: String,
    pub insert_batch_size: usize,
    pub insert_interval_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionConfig {
    pub voltage_deviation_sigma: f64,
    pub capacity_warning_ratio: f64,
    pub temperature_high_threshold: f64,
    pub cabinet_abnormal_ratio_threshold: f64,
    pub prediction_model_cycles: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertConfig {
    pub enable_mes_notification: bool,
    pub enable_screen_notification: bool,
    pub dedup_window_seconds: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mqtt: MqttConfig {
                broker: env::var("MQTT_BROKER").unwrap_or_else(|_| "localhost".to_string()),
                port: env::var("MQTT_PORT")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1883),
                subscribe_topic: env::var("MQTT_SUB_TOPIC")
                    .unwrap_or_else(|_| "battery/cabinet/+/data".to_string()),
                alert_topic: env::var("MQTT_ALERT_TOPIC")
                    .unwrap_or_else(|_| "battery/alerts".to_string()),
                client_id: env::var("MQTT_CLIENT_ID")
                    .unwrap_or_else(|_| "battery-monitor".to_string()),
            },
            clickhouse: ClickHouseConfig {
                url: env::var("CLICKHOUSE_URL")
                    .unwrap_or_else(|_| "http://localhost:8123".to_string()),
                database: env::var("CLICKHOUSE_DB")
                    .unwrap_or_else(|_| "battery_monitor".to_string()),
                user: env::var("CLICKHOUSE_USER").unwrap_or_else(|_| "default".to_string()),
                password: env::var("CLICKHOUSE_PASSWORD").unwrap_or_else(|_| "".to_string()),
                insert_batch_size: env::var("CH_BATCH_SIZE")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1024),
                insert_interval_ms: env::var("CH_INTERVAL")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1000),
            },
            server: ServerConfig {
                host: env::var("SERVER_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
                port: env::var("SERVER_PORT")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(8080),
            },
            detection: DetectionConfig {
                voltage_deviation_sigma: env::var("VOLTAGE_SIGMA")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(3.0),
                capacity_warning_ratio: env::var("CAPACITY_WARNING")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.90),
                temperature_high_threshold: env::var("TEMP_THRESHOLD")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(50.0),
                cabinet_abnormal_ratio_threshold: env::var("CABINET_ABNORMAL_RATIO")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.10),
                prediction_model_cycles: env::var("PREDICTION_CYCLES")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(3),
            },
            alert: AlertConfig {
                enable_mes_notification: env::var("ENABLE_MES")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(true),
                enable_screen_notification: env::var("ENABLE_SCREEN")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(true),
                dedup_window_seconds: env::var("ALERT_DEDUP")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(300),
            },
        }
    }
}
