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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum PredictionStatus {
    Pending = 0,
    Predicting = 1,
    Completed = 2,
    InsufficientData = 3,
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
    pub prediction_status: PredictionStatus,
    pub completed_cycles: u16,
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
    pub status: PredictionStatus,
    pub message: String,
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

// ============================================
// 新增：分容配组优化数据模型
// ============================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize_repr, Deserialize_repr, clickhouse::Enum)]
#[repr(u8)]
pub enum GroupingAlgorithm {
    Greedy = 1,
    Genetic = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize_repr, Deserialize_repr, clickhouse::Enum)]
#[repr(u8)]
pub enum CellGrade {
    A = 1,
    B = 2,
    C = 3,
    Rejected = 4,
}

impl CellGrade {
    pub fn from_capacity_ratio(ratio: f64) -> Self {
        if ratio >= 0.95 {
            Self::A
        } else if ratio >= 0.90 {
            Self::B
        } else if ratio >= 0.85 {
            Self::C
        } else {
            Self::Rejected
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::A => "A级",
            Self::B => "B级",
            Self::C => "C级",
            Self::Rejected => "不合格",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, clickhouse::Row)]
pub struct CellInfo {
    pub date: chrono::NaiveDate,
    pub batch_id: String,
    pub cabinet_id: u16,
    pub channel_id: u32,
    pub predicted_capacity: f64,
    pub measured_capacity: f64,
    pub internal_resistance: f64,
    pub capacity_ratio: f64,
    pub grade: CellGrade,
    pub cycle_index: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, clickhouse::Row)]
pub struct BatteryGroup {
    pub date: chrono::NaiveDate,
    pub group_id: String,
    pub batch_id: String,
    pub group_number: u32,
    pub algorithm: GroupingAlgorithm,
    pub cell_count: u16,
    pub avg_capacity: f64,
    pub capacity_std: f64,
    pub capacity_max_diff: f64,
    pub avg_resistance: f64,
    pub resistance_std: f64,
    pub resistance_max_diff: f64,
    pub consistency_score: f64,
    pub cell_ids: Vec<(u16, u32)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupingResult {
    pub batch_id: String,
    pub algorithm: GroupingAlgorithm,
    pub total_cells: usize,
    pub rejected_cells: usize,
    pub group_count: usize,
    pub cells_per_group: usize,
    pub groups: Vec<BatteryGroup>,
    pub avg_consistency_score: f64,
    pub processing_time_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupingRequest {
    pub batch_id: String,
    pub cells_per_group: usize,
    pub algorithm: Option<GroupingAlgorithm>,
    pub max_capacity_diff: Option<f64>,
    pub max_resistance_diff: Option<f64>,
    pub min_capacity_ratio: Option<f64>,
}

// ============================================
// 新增：电解液注液量优化数据模型
// ============================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize_repr, Deserialize_repr, clickhouse::Enum)]
#[repr(u8)]
pub enum InjectionStatus {
    Normal = 1,
    OverInjected = 2,
    UnderInjected = 3,
    Optimized = 4,
}

#[derive(Debug, Clone, Serialize, Deserialize, clickhouse::Row)]
pub struct GasGenerationData {
    pub timestamp: DateTime<Utc>,
    pub cabinet_id: u16,
    pub channel_id: u32,
    pub cycle_index: u16,
    pub stage: Stage,
    pub pressure: f64,
    pub temperature: f64,
    pub gas_volume: f64,
    pub gas_generation_rate: f64,
    pub cumulative_gas: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, clickhouse::Row)]
pub struct ElectrolyteInjection {
    pub date: chrono::NaiveDate,
    pub batch_id: String,
    pub injection_id: String,
    pub cabinet_id: u16,
    pub channel_id: u32,
    pub cycle_index: u16,
    pub nominal_volume: f64,
    pub actual_volume: f64,
    pub gas_volume: f64,
    pub suggested_volume: f64,
    pub adjustment: f64,
    pub status: InjectionStatus,
    pub confidence: f64,
    pub requires_manual_confirmation: bool,
    pub used_fallback: bool,
    pub data_completeness: f64,
    pub hard_limit_applied: bool,
    pub pressure_data_available: bool,
    pub confirmation_notes: Option<String>,
    pub confirmed_by: Option<String>,
    pub confirmed_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InjectionOptimizationResult {
    pub batch_id: String,
    pub total_channels: usize,
    pub avg_nominal_volume: f64,
    pub avg_suggested_volume: f64,
    pub avg_adjustment: f64,
    pub over_injected_count: usize,
    pub under_injected_count: usize,
    pub estimated_gas_reduction: f64,
    pub estimated_capacity_improvement: f64,
    pub next_batch_suggestion: f64,
    pub channels_with_missing_data: usize,
    pub channels_requiring_confirmation: usize,
    pub used_fallback_strategy: bool,
    pub avg_data_completeness: f64,
    pub hard_limits_applied_count: usize,
    pub fallback_explanation: String,
}

// ============================================
// 新增：老化模式识别数据模型
// ============================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize_repr, Deserialize_repr, clickhouse::Enum)]
#[repr(u8)]
pub enum DegradationMode {
    Normal = 0,
    CathodeDegradation = 1,
    AnodeDegradation = 2,
    ElectrolyteConsumption = 3,
    SEIGrowth = 4,
    MixedDegradation = 5,
}

impl DegradationMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Normal => "正常老化",
            Self::CathodeDegradation => "正极衰减",
            Self::AnodeDegradation => "负极衰减",
            Self::ElectrolyteConsumption => "电解液消耗",
            Self::SEIGrowth => "SEI膜过度生长",
            Self::MixedDegradation => "混合衰减",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::Normal => "电池处于正常老化阶段，衰减速率稳定",
            Self::CathodeDegradation => "正极材料结构发生变化，导致容量衰减加速，主要表现为高SOC区域dQ/dV峰值偏移",
            Self::AnodeDegradation => "负极活性物质损失，导致可循环锂减少，主要表现为低SOC区域dQ/dV峰值变化",
            Self::ElectrolyteConsumption => "电解液逐渐消耗，导致内阻上升，倍率性能下降",
            Self::SEIGrowth => "SEI膜过度生长，消耗活性锂，导致容量损失和内阻增加",
            Self::MixedDegradation => "多种衰减机制同时存在，需要综合分析",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, clickhouse::Row)]
pub struct DvDqPoint {
    pub voltage: f64,
    pub dq_dv: f64,
    pub capacity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, clickhouse::Row)]
pub struct DegradationAnalysis {
    pub timestamp: DateTime<Utc>,
    pub cabinet_id: u16,
    pub channel_id: u32,
    pub cycle_index: u16,
    pub mode: DegradationMode,
    pub confidence: f64,
    pub cathode_score: f64,
    pub anode_score: f64,
    pub electrolyte_score: f64,
    pub sei_score: f64,
    pub peak_positions: Vec<f64>,
    pub peak_heights: Vec<f64>,
    pub capacity_fade_rate: f64,
    pub resistance_growth_rate: f64,
    pub recommendations: String,
    pub battery_model: Option<String>,
    pub used_transfer_learning: bool,
    pub transfer_source_model: Option<String>,
    pub transfer_similarity: Option<f64>,
    pub baseline_sample_count: usize,
    pub requires_manual_confirmation: bool,
    pub is_new_model: bool,
    pub manually_corrected_mode: Option<DegradationMode>,
    pub correction_notes: Option<String>,
    pub corrected_by: Option<String>,
    pub corrected_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DegradationDetail {
    pub analysis: DegradationAnalysis,
    pub dvdq_curve: Vec<DvDqPoint>,
    pub historical_modes: Vec<(u16, DegradationMode, f64)>,
}

// ============================================
// 新增：MES系统深度对接数据模型
// ============================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize_repr, Deserialize_repr, clickhouse::Enum)]
#[repr(u8)]
pub enum MesSyncStatus {
    Pending = 0,
    Synced = 1,
    Failed = 2,
    Acked = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize_repr, Deserialize_repr, clickhouse::Enum)]
#[repr(u8)]
pub enum ProcessParamType {
    ChargeCurrent = 1,
    DischargeCurrent = 2,
    ChargeVoltage = 3,
    DischargeVoltage = 4,
    Temperature = 5,
    TimeDuration = 6,
}

#[derive(Debug, Clone, Serialize, Deserialize, clickhouse::Row)]
pub struct ProcessParamRecord {
    pub timestamp: DateTime<Utc>,
    pub batch_id: String,
    pub cabinet_id: u16,
    pub channel_id: u32,
    pub cycle_index: u16,
    pub stage: Stage,
    pub param_type: ProcessParamType,
    pub param_value: f64,
    pub param_unit: String,
    pub upper_limit: f64,
    pub lower_limit: f64,
    pub is_out_of_spec: bool,
    pub mes_sync_status: MesSyncStatus,
    pub mes_sync_time: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, clickhouse::Row)]
pub struct DegradedCellRecord {
    pub timestamp: DateTime<Utc>,
    pub batch_id: String,
    pub cabinet_id: u16,
    pub channel_id: u32,
    pub cycle_index: u16,
    pub capacity: f64,
    pub capacity_ratio: f64,
    pub internal_resistance: f64,
    pub degradation_reason: String,
    pub grade: CellGrade,
    pub mes_sync_status: MesSyncStatus,
    pub mes_sync_time: Option<DateTime<Utc>>,
    pub mes_ack_time: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, clickhouse::Row)]
pub struct BatchInfo {
    pub date: chrono::NaiveDate,
    pub batch_id: String,
    pub product_code: String,
    pub battery_model: String,
    pub rated_capacity: f64,
    pub total_cells: u32,
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    pub operator: String,
    pub shift: String,
    pub avg_capacity: f64,
    pub yield_rate: f64,
    pub grade_a_ratio: f64,
    pub grade_b_ratio: f64,
    pub grade_c_ratio: f64,
    pub rejected_ratio: f64,
    pub avg_internal_resistance: f64,
    pub process_params: Vec<ProcessParamRecord>,
    pub remarks: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchQueryRequest {
    pub batch_id: Option<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub product_code: Option<String>,
    pub battery_model: Option<String>,
    pub min_yield_rate: Option<f64>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchCapacityDistribution {
    pub batch_id: String,
    pub capacity_bins: Vec<(f64, f64, u32)>,
    pub mean: f64,
    pub std_dev: f64,
    pub median: f64,
    pub skewness: f64,
    pub kurtosis: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MesSyncResult {
    pub batch_id: String,
    pub total_records: usize,
    pub synced_records: usize,
    pub failed_records: usize,
    pub error_messages: Vec<String>,
    pub sync_time_ms: u64,
}
