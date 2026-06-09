use crate::models::{Anomaly, ChannelData, DegradationAnalysis, GasGenerationData, PredictionResult, ProcessParamRecord, DegradedCellRecord};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub enum SystemMessage {
    DataBatch(DataBatch),
    AnomalyDetected(AnomalyEvent),
    PredictionRequest(PredictionRequest),
    PredictionComplete(PredictionResult),
    AlertGenerated(AlertEvent),
    PauseCommand(PauseCommand),
    GasGenerationDataAvailable(GasGenerationEvent),
    DegradationAnalysisComplete(DegradationAnalysisEvent),
    ProcessParamRecordAvailable(ProcessParamEvent),
    DegradedCellDetected(DegradedCellEvent),
    GroupingRequest(GroupingEvent),
    InjectionOptimizationRequest(InjectionOptimizationEvent),
    MesSyncRequest(MesSyncEvent),
}

#[derive(Debug, Clone)]
pub struct DataBatch {
    pub cabinet_id: u16,
    pub data: Vec<ChannelData>,
    pub cycle_updates: Vec<CycleUpdate>,
    pub prediction_candidates: Vec<PredictionCandidate>,
}

#[derive(Debug, Clone)]
pub struct CycleUpdate {
    pub channel_id: u32,
    pub completed_cycles: u16,
}

#[derive(Debug, Clone)]
pub struct PredictionCandidate {
    pub cabinet_id: u16,
    pub channel_id: u32,
    pub current_cycle: u16,
    pub completed_cycles: u16,
}

#[derive(Debug, Clone)]
pub struct AnomalyEvent {
    pub anomaly: Anomaly,
    pub channel_data: ChannelData,
}

#[derive(Debug, Clone)]
pub struct PredictionRequest {
    pub cabinet_id: u16,
    pub channel_id: u32,
    pub n_cycles: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertEvent {
    pub alert: crate::models::Alert,
}

#[derive(Debug, Clone)]
pub struct PauseCommand {
    pub cabinet_id: u16,
    pub channel_id: u32,
    pub reason: String,
}

pub type DataBatchSender = tokio::sync::mpsc::Sender<DataBatch>;
pub type DataBatchReceiver = tokio::sync::mpsc::Receiver<DataBatch>;

pub type AnomalySender = tokio::sync::mpsc::Sender<AnomalyEvent>;
pub type AnomalyReceiver = tokio::sync::mpsc::Receiver<AnomalyEvent>;

pub type PredictionSender = tokio::sync::mpsc::Sender<PredictionRequest>;
pub type PredictionReceiver = tokio::sync::mpsc::Receiver<PredictionRequest>;

pub type PredictionResultSender = tokio::sync::mpsc::Sender<PredictionResult>;
pub type PredictionResultReceiver = tokio::sync::mpsc::Receiver<PredictionResult>;

pub type AlertSender = tokio::sync::mpsc::Sender<AlertEvent>;
pub type AlertReceiver = tokio::sync::mpsc::Receiver<AlertEvent>;

pub type PauseSender = tokio::sync::mpsc::Sender<PauseCommand>;
pub type PauseReceiver = tokio::sync::mpsc::Receiver<PauseCommand>;

// ============================================
// 新增：分容配组消息
// ============================================
#[derive(Debug, Clone)]
pub struct GroupingEvent {
    pub batch_id: String,
    pub cells_per_group: usize,
    pub algorithm: crate::models::GroupingAlgorithm,
    pub max_capacity_diff: Option<f64>,
    pub max_resistance_diff: Option<f64>,
}

pub type GroupingSender = tokio::sync::mpsc::Sender<GroupingEvent>;
pub type GroupingReceiver = tokio::sync::mpsc::Receiver<GroupingEvent>;

// ============================================
// 新增：电解液注液量优化消息
// ============================================
#[derive(Debug, Clone)]
pub struct GasGenerationEvent {
    pub data: GasGenerationData,
}

#[derive(Debug, Clone)]
pub struct InjectionOptimizationEvent {
    pub batch_id: String,
    pub cycle_index: u16,
}

pub type GasGenerationSender = tokio::sync::mpsc::Sender<GasGenerationEvent>;
pub type GasGenerationReceiver = tokio::sync::mpsc::Receiver<GasGenerationEvent>;

pub type InjectionOptimizationSender = tokio::sync::mpsc::Sender<InjectionOptimizationEvent>;
pub type InjectionOptimizationReceiver = tokio::sync::mpsc::Receiver<InjectionOptimizationEvent>;

// ============================================
// 新增：老化模式识别消息
// ============================================
#[derive(Debug, Clone)]
pub struct DegradationAnalysisEvent {
    pub analysis: DegradationAnalysis,
    pub dvdq_curve: Vec<crate::models::DvDqPoint>,
}

pub type DegradationAnalysisSender = tokio::sync::mpsc::Sender<DegradationAnalysisEvent>;
pub type DegradationAnalysisReceiver = tokio::sync::mpsc::Receiver<DegradationAnalysisEvent>;

// ============================================
// 新增：MES系统对接消息
// ============================================
#[derive(Debug, Clone)]
pub struct ProcessParamEvent {
    pub record: ProcessParamRecord,
}

#[derive(Debug, Clone)]
pub struct DegradedCellEvent {
    pub record: DegradedCellRecord,
}

#[derive(Debug, Clone)]
pub struct MesSyncEvent {
    pub batch_id: String,
    pub sync_type: MesSyncType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MesSyncType {
    ProcessParams,
    DegradedCells,
    BatchSummary,
    All,
}

pub type ProcessParamSender = tokio::sync::mpsc::Sender<ProcessParamEvent>;
pub type ProcessParamReceiver = tokio::sync::mpsc::Receiver<ProcessParamEvent>;

pub type DegradedCellSender = tokio::sync::mpsc::Sender<DegradedCellEvent>;
pub type DegradedCellReceiver = tokio::sync::mpsc::Receiver<DegradedCellEvent>;

pub type MesSyncSender = tokio::sync::mpsc::Sender<MesSyncEvent>;
pub type MesSyncReceiver = tokio::sync::mpsc::Receiver<MesSyncEvent>;
