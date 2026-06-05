use crate::models::{Anomaly, ChannelData, PredictionResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub enum SystemMessage {
    DataBatch(DataBatch),
    AnomalyDetected(AnomalyEvent),
    PredictionRequest(PredictionRequest),
    PredictionComplete(PredictionResult),
    AlertGenerated(AlertEvent),
    PauseCommand(PauseCommand),
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
