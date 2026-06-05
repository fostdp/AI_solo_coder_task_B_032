use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize_repr, Deserialize_repr, clickhouse::Enum)]
#[repr(u8)]
pub enum Stage {
    Precharge = 1,
    CcCharge = 2,
    CvCharge = 3,
    Rest = 4,
    Discharge = 5,
}

impl Stage {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "precharge" => Some(Self::Precharge),
            "cc_charge" => Some(Self::CcCharge),
            "cv_charge" => Some(Self::CvCharge),
            "rest" => Some(Self::Rest),
            "discharge" => Some(Self::Discharge),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Precharge => "预充",
            Self::CcCharge => "恒流充电",
            Self::CvCharge => "恒压充电",
            Self::Rest => "搁置",
            Self::Discharge => "放电",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize_repr, Deserialize_repr, clickhouse::Enum)]
#[repr(u8)]
pub enum AnomalyType {
    VoltageDeviation = 1,
    CapacityLow = 2,
    TemperatureHigh = 3,
    CurrentAbnormal = 4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize_repr, Deserialize_repr, clickhouse::Enum)]
#[repr(u8)]
pub enum Severity {
    Warning = 1,
    Critical = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize_repr, Deserialize_repr, clickhouse::Enum)]
#[repr(u8)]
pub enum AlertLevel {
    Level1 = 1,
    Level2 = 2,
}

#[derive(Debug, Clone, Serialize, Deserialize, clickhouse::Row)]
pub struct ChannelData {
    pub timestamp: DateTime<Utc>,
    pub cabinet_id: u16,
    pub channel_id: u32,
    pub voltage: f64,
    pub current: f64,
    pub temperature: f64,
    pub capacity: f64,
    pub cycle_index: u16,
    pub stage: Stage,
    pub stage_duration: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, clickhouse::Row)]
pub struct ChannelStatus {
    pub cabinet_id: u16,
    pub channel_id: u32,
    pub last_update: DateTime<Utc>,
    pub current_stage: Stage,
    pub current_voltage: f64,
    pub current_current: f64,
    pub current_temperature: f64,
    pub current_capacity: f64,
    pub cycle_index: u16,
    pub is_abnormal: bool,
    pub is_paused: bool,
    pub capacity_ratio: f64,
    pub predicted_capacity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, clickhouse::Row)]
pub struct CycleFeatures {
    pub date: chrono::NaiveDate,
    pub cabinet_id: u16,
    pub channel_id: u32,
    pub cycle_index: u16,
    pub cc_charge_time: u32,
    pub cv_charge_time: u32,
    pub discharge_time: u32,
    pub discharge_platform_voltage: f64,
    pub cc_end_voltage: f64,
    pub cv_end_current: f64,
    pub max_charge_temp: f64,
    pub max_discharge_temp: f64,
    pub charge_capacity: f64,
    pub discharge_capacity: f64,
    pub efficiency: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, clickhouse::Row)]
pub struct PredictionResult {
    pub timestamp: DateTime<Utc>,
    pub cabinet_id: u16,
    pub channel_id: u32,
    pub cycle_index: u16,
    pub predicted_capacity: f64,
    pub actual_capacity: Option<f64>,
    pub rated_capacity: f64,
    pub prediction_error: Option<f64>,
    pub model_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, clickhouse::Row)]
pub struct Anomaly {
    pub timestamp: DateTime<Utc>,
    pub cabinet_id: u16,
    pub channel_id: u32,
    pub anomaly_type: AnomalyType,
    pub severity: Severity,
    pub description: String,
    pub value: f64,
    pub threshold: f64,
    pub is_paused: bool,
    pub resolved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, clickhouse::Row)]
pub struct Alert {
    pub timestamp: DateTime<Utc>,
    pub alert_id: uuid::Uuid,
    pub alert_level: AlertLevel,
    pub alert_type: String,
    pub cabinet_id: u16,
    pub channel_ids: Vec<u32>,
    pub message: String,
    pub notified_mes: bool,
    pub notified_screen: bool,
    pub acknowledged: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, clickhouse::Row)]
pub struct CabinetStats {
    pub timestamp: DateTime<Utc>,
    pub cabinet_id: u16,
    pub avg_voltage: f64,
    pub std_voltage: f64,
    pub avg_current: f64,
    pub avg_temperature: f64,
    pub abnormal_channel_count: u16,
    pub total_channels: u16,
    pub abnormal_ratio: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelHistory {
    pub timestamps: Vec<DateTime<Utc>>,
    pub voltages: Vec<f64>,
    pub currents: Vec<f64>,
    pub temperatures: Vec<f64>,
    pub capacities: Vec<f64>,
    pub stages: Vec<Stage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapacityTrend {
    pub cycle_indices: Vec<u16>,
    pub charge_capacities: Vec<f64>,
    pub discharge_capacities: Vec<f64>,
    pub predicted_capacities: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, clickhouse::Row)]
pub struct StageSummary {
    pub stage: Stage,
    pub duration: u32,
    pub start_voltage: f64,
    pub end_voltage: f64,
    pub avg_current: f64,
    pub max_temperature: f64,
    pub capacity_gain: f64,
}

pub const RATED_CAPACITY: f64 = 3.2;
pub const CHANNELS_PER_CABINET: usize = 512;
pub const NUM_CABINETS: usize = 20;
pub const CAPACITY_GOOD_THRESHOLD: f64 = 0.95;
pub const CAPACITY_WARNING_THRESHOLD: f64 = 0.90;
pub const CABINET_ABNORMAL_RATIO_THRESHOLD: f64 = 0.10;
