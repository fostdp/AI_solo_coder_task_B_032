use crate::models::{BatchCapacityDistribution, BatchInfo, BatchQueryRequest, DegradedCellRecord, MesSyncResult, MesSyncStatus, ProcessParamRecord};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ConnectorConfig {
    pub mes_api_url: String,
    pub mes_api_key: String,
    pub sync_interval_seconds: u64,
    pub retry_count: u32,
    pub retry_interval_seconds: u64,
    pub batch_size: usize,
    pub enable_automatic_sync: bool,
    pub enable_offline_cache: bool,
    pub offline_cache_path: String,
    pub max_pending_records: usize,
    pub backpressure_threshold: usize,
    pub auto_recovery_enabled: bool,
    pub max_batch_per_sync: usize,
    pub health_check_interval: u64,
    pub max_retry_delay_seconds: u64,
    pub channel_buffer: usize,
}

impl Default for ConnectorConfig {
    fn default() -> Self {
        Self {
            mes_api_url: "http://mes-server/api/v1".to_string(),
            mes_api_key: "".to_string(),
            sync_interval_seconds: 300,
            retry_count: 3,
            retry_interval_seconds: 10,
            batch_size: 100,
            enable_automatic_sync: true,
            enable_offline_cache: true,
            offline_cache_path: "./data/mes_offline_cache".to_string(),
            max_pending_records: 100000,
            backpressure_threshold: 50000,
            auto_recovery_enabled: true,
            max_batch_per_sync: 10,
            health_check_interval: 60,
            max_retry_delay_seconds: 300,
            channel_buffer: 100,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfflineCacheHeader {
    pub created_at: DateTime<Utc>,
    pub record_count: usize,
    pub data_type: String,
}

#[derive(Debug, Clone)]
pub struct ManualLabel {
    pub cabinet_id: u16,
    pub channel_id: u32,
    pub cycle_index: u16,
    pub corrected_mode: String,
    pub notes: String,
    pub operator: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct RecordParamRequest {
    pub request_id: String,
    pub record: ProcessParamRecord,
    pub respond_to: Option<oneshot::Sender<RecordParamResult>>,
}

#[derive(Debug)]
pub struct RecordParamResult {
    pub request_id: String,
    pub success: bool,
    pub message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RecordDegradedRequest {
    pub request_id: String,
    pub record: DegradedCellRecord,
    pub respond_to: Option<oneshot::Sender<RecordDegradedResult>>,
}

#[derive(Debug)]
pub struct RecordDegradedResult {
    pub request_id: String,
    pub success: bool,
    pub message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SyncRequest {
    pub request_id: String,
    pub sync_type: SyncType,
    pub respond_to: Option<oneshot::Sender<SyncResult>>,
}

#[derive(Debug, Clone)]
pub enum SyncType {
    Params,
    Degraded,
    All,
}

#[derive(Debug)]
pub struct SyncResult {
    pub request_id: String,
    pub results: Vec<MesSyncResult>,
    pub success: bool,
    pub message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AddManualLabelRequest {
    pub request_id: String,
    pub cabinet_id: u16,
    pub channel_id: u32,
    pub cycle_index: u16,
    pub corrected_mode: String,
    pub notes: String,
    pub operator: String,
    pub respond_to: Option<oneshot::Sender<bool>>,
}

pub enum ConnectorMessage {
    RecordParam(RecordParamRequest),
    RecordDegraded(RecordDegradedRequest),
    SyncRequest(SyncRequest),
    GetStatus {
        respond_to: oneshot::Sender<(bool, u32, usize, usize, usize, usize)>,
    },
    GetPendingCounts {
        respond_to: oneshot::Sender<(usize, usize)>,
    },
    GetSyncStatus {
        batch_id: String,
        respond_to: oneshot::Sender<Option<MesSyncResult>>,
    },
    AddManualLabel(AddManualLabelRequest),
    GetPendingConfirmations {
        respond_to: oneshot::Sender<Vec<(u16, u32, u16)>>,
    },
    UpdateConfig(ConnectorConfig),
    Shutdown,
}

impl fmt::Debug for ConnectorMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConnectorMessage::RecordParam(req) => write!(f, "RecordParam({})", req.request_id),
            ConnectorMessage::RecordDegraded(req) => write!(f, "RecordDegraded({})", req.request_id),
            ConnectorMessage::SyncRequest(req) => write!(f, "SyncRequest({:?}, {})", req.sync_type, req.request_id),
            ConnectorMessage::GetStatus { .. } => write!(f, "GetStatus"),
            ConnectorMessage::GetPendingCounts { .. } => write!(f, "GetPendingCounts"),
            ConnectorMessage::GetSyncStatus { batch_id, .. } => write!(f, "GetSyncStatus({})", batch_id),
            ConnectorMessage::AddManualLabel(req) => write!(f, "AddManualLabel({})", req.request_id),
            ConnectorMessage::GetPendingConfirmations { .. } => write!(f, "GetPendingConfirmations"),
            ConnectorMessage::UpdateConfig(_) => write!(f, "UpdateConfig"),
            ConnectorMessage::Shutdown => write!(f, "Shutdown"),
        }
    }
}

pub type ConnectorSender = mpsc::Sender<ConnectorMessage>;
pub type ConnectorReceiver = mpsc::Receiver<ConnectorMessage>;

#[derive(Clone)]
pub struct MesConnectorHandle {
    sender: ConnectorSender,
    config: Arc<Mutex<ConnectorConfig>>,
}

impl MesConnectorHandle {
    pub fn new(sender: ConnectorSender, config: ConnectorConfig) -> Self {
        Self {
            sender,
            config: Arc::new(Mutex::new(config)),
        }
    }

    pub async fn record_process_param(
        &self,
        request: RecordParamRequest,
    ) -> Result<oneshot::Receiver<RecordParamResult>, String> {
        let (tx, rx) = oneshot::channel();
        let message = ConnectorMessage::RecordParam(RecordParamRequest {
            respond_to: Some(tx),
            ..request
        });

        self.sender
            .send(message)
            .await
            .map_err(|e| format!("Failed to send record param request: {}", e))?;

        Ok(rx)
    }

    pub async fn record_degraded_cell(
        &self,
        request: RecordDegradedRequest,
    ) -> Result<oneshot::Receiver<RecordDegradedResult>, String> {
        let (tx, rx) = oneshot::channel();
        let message = ConnectorMessage::RecordDegraded(RecordDegradedRequest {
            respond_to: Some(tx),
            ..request
        });

        self.sender
            .send(message)
            .await
            .map_err(|e| format!("Failed to send record degraded request: {}", e))?;

        Ok(rx)
    }

    pub async fn sync_request(
        &self,
        request: SyncRequest,
    ) -> Result<oneshot::Receiver<SyncResult>, String> {
        let (tx, rx) = oneshot::channel();
        let message = ConnectorMessage::SyncRequest(SyncRequest {
            respond_to: Some(tx),
            ..request
        });

        self.sender
            .send(message)
            .await
            .map_err(|e| format!("Failed to send sync request: {}", e))?;

        Ok(rx)
    }

    pub async fn get_status(&self) -> Result<oneshot::Receiver<(bool, u32, usize, usize, usize, usize)>, String> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(ConnectorMessage::GetStatus { respond_to: tx })
            .await
            .map_err(|e| format!("Failed to send get status request: {}", e))?;

        Ok(rx)
    }

    pub async fn get_pending_counts(&self) -> Result<oneshot::Receiver<(usize, usize)>, String> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(ConnectorMessage::GetPendingCounts { respond_to: tx })
            .await
            .map_err(|e| format!("Failed to send get pending counts request: {}", e))?;

        Ok(rx)
    }

    pub async fn get_sync_status(&self, batch_id: String) -> Result<oneshot::Receiver<Option<MesSyncResult>>, String> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(ConnectorMessage::GetSyncStatus { batch_id, respond_to: tx })
            .await
            .map_err(|e| format!("Failed to send get sync status request: {}", e))?;

        Ok(rx)
    }

    pub async fn add_manual_label(
        &self,
        request: AddManualLabelRequest,
    ) -> Result<oneshot::Receiver<bool>, String> {
        let (tx, rx) = oneshot::channel();
        let message = ConnectorMessage::AddManualLabel(AddManualLabelRequest {
            respond_to: Some(tx),
            ..request
        });

        self.sender
            .send(message)
            .await
            .map_err(|e| format!("Failed to send add manual label request: {}", e))?;

        Ok(rx)
    }

    pub async fn get_pending_confirmations(&self) -> Result<oneshot::Receiver<Vec<(u16, u32, u16)>>, String> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(ConnectorMessage::GetPendingConfirmations { respond_to: tx })
            .await
            .map_err(|e| format!("Failed to send get pending confirmations request: {}", e))?;

        Ok(rx)
    }

    pub async fn update_config(&self, config: ConnectorConfig) -> Result<(), String> {
        *self.config.lock().await = config.clone();
        self.sender
            .send(ConnectorMessage::UpdateConfig(config))
            .await
            .map_err(|e| format!("Failed to send config update: {}", e))
    }

    pub async fn get_config(&self) -> ConnectorConfig {
        self.config.lock().await.clone()
    }

    pub async fn shutdown(&self) -> Result<(), String> {
        self.sender
            .send(ConnectorMessage::Shutdown)
            .await
            .map_err(|e| format!("Failed to send shutdown: {}", e))
    }
}

struct MesConnectorState {
    pending_params: Vec<ProcessParamRecord>,
    pending_degraded: Vec<DegradedCellRecord>,
    sync_history: HashMap<String, MesSyncResult>,
    batch_info_cache: HashMap<String, BatchInfo>,
    mes_available: bool,
    last_health_check: Option<DateTime<Utc>>,
    consecutive_failures: u32,
    current_retry_delay: u64,
    offline_cache_params: Vec<ProcessParamRecord>,
    offline_cache_degraded: Vec<DegradedCellRecord>,
    backpressure_active: bool,
    total_backlogged: u64,
    last_offline_flush: Option<DateTime<Utc>>,
    manual_labels: HashMap<(u16, u32, u16), ManualLabel>,
    pending_confirmation: Vec<(u16, u32, u16)>,
}

impl MesConnectorState {
    fn new(config: &ConnectorConfig) -> Self {
        let mut state = Self {
            pending_params: Vec::new(),
            pending_degraded: Vec::new(),
            sync_history: HashMap::new(),
            batch_info_cache: HashMap::new(),
            mes_available: true,
            last_health_check: None,
            consecutive_failures: 0,
            current_retry_delay: 0,
            offline_cache_params: Vec::new(),
            offline_cache_degraded: Vec::new(),
            backpressure_active: false,
            total_backlogged: 0,
            last_offline_flush: None,
            manual_labels: HashMap::new(),
            pending_confirmation: Vec::new(),
        };

        if config.enable_offline_cache {
            state.load_offline_cache(config);
        }

        state
    }
}

pub struct MesConnectorService {
    config: ConnectorConfig,
    receiver: ConnectorReceiver,
    state: MesConnectorState,
}

impl MesConnectorService {
    pub fn new(config: ConnectorConfig) -> (Self, MesConnectorHandle) {
        let (sender, receiver) = mpsc::channel(config.channel_buffer);
        let state = MesConnectorState::new(&config);
        let handle = MesConnectorHandle::new(sender, config.clone());
        (
            Self {
                config,
                receiver,
                state,
            },
            handle,
        )
    }

    pub async fn run(mut self) {
        tracing::info!("MesConnectorService started, waiting for requests");

        while let Some(message) = self.receiver.recv().await {
            match message {
                ConnectorMessage::RecordParam(request) => {
                    self.handle_record_param(request).await;
                }
                ConnectorMessage::RecordDegraded(request) => {
                    self.handle_record_degraded(request).await;
                }
                ConnectorMessage::SyncRequest(request) => {
                    self.handle_sync_request(request).await;
                }
                ConnectorMessage::GetStatus { respond_to } => {
                    let status = (
                        self.state.mes_available,
                        self.state.consecutive_failures,
                        self.state.pending_params.len(),
                        self.state.pending_degraded.len(),
                        self.state.offline_cache_params.len(),
                        self.state.offline_cache_degraded.len(),
                    );
                    let _ = respond_to.send(status);
                }
                ConnectorMessage::GetPendingCounts { respond_to } => {
                    let counts = (self.state.pending_params.len(), self.state.pending_degraded.len());
                    let _ = respond_to.send(counts);
                }
                ConnectorMessage::GetSyncStatus { batch_id, respond_to } => {
                    let result = self.state.sync_history.get(&batch_id).cloned();
                    let _ = respond_to.send(result);
                }
                ConnectorMessage::AddManualLabel(request) => {
                    self.handle_add_manual_label(request).await;
                }
                ConnectorMessage::GetPendingConfirmations { respond_to } => {
                    let pending = self.state.pending_confirmation.clone();
                    let _ = respond_to.send(pending);
                }
                ConnectorMessage::UpdateConfig(new_config) => {
                    self.config = new_config;
                    tracing::info!("Connector config updated");
                }
                ConnectorMessage::Shutdown => {
                    tracing::info!("MesConnectorService shutting down");
                    break;
                }
            }
        }

        tracing::info!("MesConnectorService stopped");
    }

    async fn handle_record_param(&mut self, request: RecordParamRequest) {
        let request_id = request.request_id.clone();
        let respond_to = request.respond_to;
        let mut record = request.record;

        self.check_health_and_recovery();

        let total_pending = self.state.pending_params.len() + self.state.offline_cache_params.len();

        if total_pending >= self.config.backpressure_threshold {
            self.state.backpressure_active = true;
        }

        let result = if !self.state.mes_available || self.state.backpressure_active {
            self.state.total_backlogged += 1;

            if self.config.enable_offline_cache {
                record.mes_sync_status = MesSyncStatus::CachedOffline;
                self.state.offline_cache_params.push(record);

                if self.state.offline_cache_params.len() % 1000 == 0 {
                    let _ = self.flush_offline_cache();
                }
            } else {
                self.state.pending_params.push(record);
            }
            RecordParamResult {
                request_id,
                success: true,
                message: Some("Record cached offline or backpressured".to_string()),
            }
        } else {
            if total_pending >= self.config.max_pending_records {
                self.discard_oldest_records(1000);
            }

            self.state.pending_params.push(record);

            if self.config.enable_automatic_sync && self.state.pending_params.len() >= self.config.batch_size {
                let _ = self.sync_process_params_sync();
            }

            RecordParamResult {
                request_id,
                success: true,
                message: None,
            }
        };

        if let Some(tx) = respond_to {
            let _ = tx.send(result);
        }
    }

    async fn handle_record_degraded(&mut self, request: RecordDegradedRequest) {
        let request_id = request.request_id.clone();
        let respond_to = request.respond_to;
        let mut record = request.record;

        self.check_health_and_recovery();

        let total_pending = self.state.pending_degraded.len() + self.state.offline_cache_degraded.len();

        let result = if !self.state.mes_available || self.state.backpressure_active {
            self.state.total_backlogged += 1;

            if self.config.enable_offline_cache {
                record.mes_sync_status = MesSyncStatus::CachedOffline;
                self.state.offline_cache_degraded.push(record);

                if self.state.offline_cache_degraded.len() % 500 == 0 {
                    let _ = self.flush_offline_cache();
                }
            } else {
                self.state.pending_degraded.push(record);
            }
            RecordDegradedResult {
                request_id,
                success: true,
                message: Some("Record cached offline or backpressured".to_string()),
            }
        } else {
            if total_pending >= self.config.max_pending_records / 2 {
                self.discard_oldest_degraded(500);
            }

            self.state.pending_degraded.push(record);

            if self.config.enable_automatic_sync && self.state.pending_degraded.len() >= self.config.batch_size / 2 {
                let _ = self.sync_degraded_cells_sync();
            }

            RecordDegradedResult {
                request_id,
                success: true,
                message: None,
            }
        };

        if let Some(tx) = respond_to {
            let _ = tx.send(result);
        }
    }

    async fn handle_sync_request(&mut self, request: SyncRequest) {
        let request_id = request.request_id.clone();
        let respond_to = request.respond_to;
        let sync_type = request.sync_type.clone();

        let config = self.config.clone();
        let state = Self::extract_sync_state(&mut self.state);

        let result = tokio::task::spawn_blocking(move || {
            sync_batch_sync(state, config, sync_type)
        })
        .await;

        let (sync_results, updated_state) = match result {
            Ok((results, state)) => (results, state),
            Err(e) => {
                tracing::error!("Sync task panicked: {}", e);
                (Vec::new(), self.state.clone_into())
            }
        };

        Self::apply_sync_state(&mut self.state, updated_state);

        let success = sync_results.iter().all(|r| r.failed_records == 0);
        let message = if success {
            None
        } else {
            let total_failed: u64 = sync_results.iter().map(|r| r.failed_records as u64).sum();
            Some(format!("Failed to sync {} records", total_failed))
        };

        let sync_result = SyncResult {
            request_id,
            results: sync_results,
            success,
            message,
        };

        if let Some(tx) = respond_to {
            let _ = tx.send(sync_result);
        }
    }

    async fn handle_add_manual_label(&mut self, request: AddManualLabelRequest) {
        let request_id = request.request_id.clone();
        let respond_to = request.respond_to;

        let key = (request.cabinet_id, request.channel_id, request.cycle_index);
        self.state.manual_labels.insert(key, ManualLabel {
            cabinet_id: request.cabinet_id,
            channel_id: request.channel_id,
            cycle_index: request.cycle_index,
            corrected_mode: request.corrected_mode,
            notes: request.notes,
            operator: request.operator,
            timestamp: Utc::now(),
        });

        self.state.pending_confirmation.retain(|&k| k != key);

        if let Some(tx) = respond_to {
            let _ = tx.send(true);
        }

        tracing::debug!("Added manual label for request: {}", request_id);
    }

    fn extract_sync_state(state: &mut MesConnectorState) -> SyncState {
        SyncState {
            pending_params: std::mem::take(&mut state.pending_params),
            pending_degraded: std::mem::take(&mut state.pending_degraded),
            offline_cache_params: std::mem::take(&mut state.offline_cache_params),
            offline_cache_degraded: std::mem::take(&mut state.offline_cache_degraded),
            sync_history: std::mem::take(&mut state.sync_history),
            mes_available: state.mes_available,
            consecutive_failures: state.consecutive_failures,
            current_retry_delay: state.current_retry_delay,
            backpressure_active: state.backpressure_active,
        }
    }

    fn apply_sync_state(state: &mut MesConnectorState, sync_state: SyncState) {
        state.pending_params = sync_state.pending_params;
        state.pending_degraded = sync_state.pending_degraded;
        state.offline_cache_params = sync_state.offline_cache_params;
        state.offline_cache_degraded = sync_state.offline_cache_degraded;
        state.sync_history = sync_state.sync_history;
        state.mes_available = sync_state.mes_available;
        state.consecutive_failures = sync_state.consecutive_failures;
        state.current_retry_delay = sync_state.current_retry_delay;
        state.backpressure_active = sync_state.backpressure_active;
    }

    fn check_health_and_recovery(&mut self) {
        let now = Utc::now();

        if let Some(last_check) = self.state.last_health_check {
            let elapsed = (now - last_check).num_seconds() as u64;
            if elapsed < self.config.health_check_interval {
                return;
            }
        }

        self.state.last_health_check = Some(now);

        if !self.state.mes_available && self.config.auto_recovery_enabled {
            if self.state.current_retry_delay == 0 {
                self.state.current_retry_delay = self.config.retry_interval_seconds;
            }

            let elapsed = self.state.last_health_check
                .map(|t| (now - t).num_seconds() as u64)
                .unwrap_or(0);

            if elapsed >= self.state.current_retry_delay {
                if self.ping_mes() {
                    self.state.mes_available = true;
                    self.state.consecutive_failures = 0;
                    self.state.current_retry_delay = 0;
                    let _ = self.flush_offline_cache();
                } else {
                    self.state.current_retry_delay = (self.state.current_retry_delay * 2)
                        .min(self.config.max_retry_delay_seconds);
                }
            }
        }

        if self.state.backpressure_active && self.state.mes_available {
            let total_pending = self.state.pending_params.len() + self.state.offline_cache_params.len()
                + self.state.pending_degraded.len() + self.state.offline_cache_degraded.len();
            if total_pending < self.config.backpressure_threshold / 2 {
                self.state.backpressure_active = false;
            }
        }
    }

    fn ping_mes(&self) -> bool {
        if self.config.mes_api_url.is_empty() {
            return true;
        }
        true
    }

    fn flush_offline_cache(&mut self) -> Result<(), String> {
        if !self.config.enable_offline_cache {
            return Ok(());
        }

        let path = std::path::Path::new(&self.config.offline_cache_path);
        if let Err(e) = std::fs::create_dir_all(path) {
            return Err(format!("Failed to create cache directory: {}", e));
        }

        let timestamp = Utc::now().format("%Y%m%d_%H%M%S").to_string();

        if !self.state.offline_cache_params.is_empty() {
            let file_path = path.join(format!("params_{}.json", timestamp));
            let header = OfflineCacheHeader {
                created_at: Utc::now(),
                record_count: self.state.offline_cache_params.len(),
                data_type: "params".to_string(),
            };
            if let Ok(contents) = serde_json::to_string(&(&header, &self.state.offline_cache_params)) {
                let _ = std::fs::write(file_path, contents);
            }
        }

        if !self.state.offline_cache_degraded.is_empty() {
            let file_path = path.join(format!("degraded_{}.json", timestamp));
            let header = OfflineCacheHeader {
                created_at: Utc::now(),
                record_count: self.state.offline_cache_degraded.len(),
                data_type: "degraded".to_string(),
            };
            if let Ok(contents) = serde_json::to_string(&(&header, &self.state.offline_cache_degraded)) {
                let _ = std::fs::write(file_path, contents);
            }
        }

        self.state.last_offline_flush = Some(Utc::now());
        Ok(())
    }

    fn load_offline_cache(&mut self, config: &ConnectorConfig) {
        let path = std::path::Path::new(&config.offline_cache_path);
        if !path.exists() {
            return;
        }

        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                let file_path = entry.path();
                if let Ok(contents) = std::fs::read_to_string(&file_path) {
                    if file_path.to_string_lossy().contains("params") {
                        if let Ok((_, records)) = serde_json::from_str::<(OfflineCacheHeader, Vec<ProcessParamRecord>)>(&contents) {
                            self.state.offline_cache_params.extend(records);
                        }
                    } else if file_path.to_string_lossy().contains("degraded") {
                        if let Ok((_, records)) = serde_json::from_str::<(OfflineCacheHeader, Vec<DegradedCellRecord>)>(&contents) {
                            self.state.offline_cache_degraded.extend(records);
                        }
                    }
                }
                let _ = std::fs::remove_file(file_path);
            }
        }
    }

    fn discard_oldest_records(&mut self, count: usize) {
        let discard = count.min(self.state.pending_params.len());
        if discard > 0 {
            self.state.pending_params.drain(0..discard);
        }

        let discard_offline = (count - discard).min(self.state.offline_cache_params.len());
        if discard_offline > 0 {
            self.state.offline_cache_params.drain(0..discard_offline);
        }
    }

    fn discard_oldest_degraded(&mut self, count: usize) {
        let discard = count.min(self.state.pending_degraded.len());
        if discard > 0 {
            self.state.pending_degraded.drain(0..discard);
        }

        let discard_offline = (count - discard).min(self.state.offline_cache_degraded.len());
        if discard_offline > 0 {
            self.state.offline_cache_degraded.drain(0..discard_offline);
        }
    }

    fn sync_process_params_sync(&mut self) -> Result<MesSyncResult, String> {
        if self.state.offline_cache_params.len() > 0 && self.state.mes_available && self.config.auto_recovery_enabled {
            let recovery_result = self.recover_from_offline_cache_sync();
            if let Err(e) = recovery_result {
                eprintln!("Warning: Failed to recover offline cache: {}", e);
            }
        }

        if self.state.pending_params.is_empty() && self.state.offline_cache_params.is_empty() {
            return Ok(MesSyncResult {
                batch_id: "NONE".to_string(),
                total_records: 0,
                synced_records: 0,
                failed_records: 0,
                error_messages: Vec::new(),
                sync_time_ms: 0,
            });
        }

        let mut records_to_sync: Vec<ProcessParamRecord> = Vec::new();
        let transfer_count = self.state.offline_cache_params.len().min(self.config.batch_size * self.config.max_batch_per_sync);
        if transfer_count > 0 {
            records_to_sync.extend(self.state.offline_cache_params.drain(0..transfer_count));
        }
        let live_count = self.state.pending_params.len().min(self.config.batch_size - records_to_sync.len());
        if live_count > 0 {
            records_to_sync.extend(self.state.pending_params.drain(0..live_count));
        }

        let start_time = std::time::Instant::now();
        let batch_id = records_to_sync.first().map(|p| p.batch_id.clone()).unwrap_or_default();
        let total_records = records_to_sync.len();

        let mut synced_records = 0;
        let mut failed_records = 0;
        let mut error_messages: Vec<String> = Vec::new();

        for chunk in records_to_sync.chunks(self.config.batch_size) {
            let mut chunk_records: Vec<ProcessParamRecord> = chunk.to_vec();
            match self.send_batch_to_mes(&chunk_records, "params") {
                Ok(_) => {
                    synced_records += chunk.len();
                    for r in &mut chunk_records {
                        r.mes_sync_status = MesSyncStatus::Synced;
                        r.mes_sync_time = Some(Utc::now());
                    }
                }
                Err(e) => {
                    failed_records += chunk.len();
                    error_messages.push(e.clone());

                    self.state.consecutive_failures += 1;
                    if self.state.consecutive_failures >= self.config.retry_count {
                        self.state.mes_available = false;
                        self.state.current_retry_delay = self.config.retry_interval_seconds
                            * 2u64.pow(self.state.consecutive_failures.min(5))
                            .min(self.config.max_retry_delay_seconds);

                        for r in chunk_records {
                            let mut record = r.clone();
                            record.mes_sync_status = MesSyncStatus::Failed;
                            record.mes_error_message = e.clone();
                            self.state.offline_cache_params.push(record);
                        }
                        break;
                    }

                    for r in chunk_records {
                        let mut record = r.clone();
                        record.mes_sync_status = MesSyncStatus::Failed;
                        record.mes_error_message = e.clone();
                        self.state.pending_params.push(record);
                    }
                }
            }
        }

        if synced_records > 0 {
            self.state.consecutive_failures = 0;
            self.state.mes_available = true;
            self.state.current_retry_delay = 0;

            if self.state.backpressure_active {
                let total_pending = self.state.pending_params.len() + self.state.offline_cache_params.len();
                if total_pending < self.config.backpressure_threshold / 2 {
                    self.state.backpressure_active = false;
                }
            }
        }

        let sync_time_ms = start_time.elapsed().as_millis() as u64;

        let result = MesSyncResult {
            batch_id: batch_id.clone(),
            total_records,
            synced_records,
            failed_records,
            error_messages,
            sync_time_ms,
        };

        self.state.sync_history.insert(format!("params_{}", batch_id), result.clone());
        self.state.last_health_check = Some(Utc::now());

        if failed_records > 0 {
            Err(format!("Failed to sync {} records", failed_records))
        } else {
            Ok(result)
        }
    }

    fn sync_degraded_cells_sync(&mut self) -> Result<MesSyncResult, String> {
        if self.state.pending_degraded.is_empty() && self.state.offline_cache_degraded.is_empty() {
            return Ok(MesSyncResult {
                batch_id: "NONE".to_string(),
                total_records: 0,
                synced_records: 0,
                failed_records: 0,
                error_messages: Vec::new(),
                sync_time_ms: 0,
            });
        }

        let mut records_to_sync: Vec<DegradedCellRecord> = Vec::new();
        let transfer_count = self.state.offline_cache_degraded.len().min(self.config.batch_size * self.config.max_batch_per_sync / 2);
        if transfer_count > 0 {
            records_to_sync.extend(self.state.offline_cache_degraded.drain(0..transfer_count));
        }
        let live_count = self.state.pending_degraded.len().min(self.config.batch_size / 2 - records_to_sync.len());
        if live_count > 0 {
            records_to_sync.extend(self.state.pending_degraded.drain(0..live_count));
        }

        let start_time = std::time::Instant::now();
        let batch_id = records_to_sync.first().map(|p| p.batch_id.clone()).unwrap_or_default();
        let total_records = records_to_sync.len();

        let mut synced_records = 0;
        let mut failed_records = 0;
        let mut error_messages: Vec<String> = Vec::new();

        for chunk in records_to_sync.chunks(self.config.batch_size / 2) {
            let mut chunk_records: Vec<DegradedCellRecord> = chunk.to_vec();
            match self.send_batch_to_mes(&chunk_records, "degraded") {
                Ok(_) => {
                    synced_records += chunk.len();
                    for r in &mut chunk_records {
                        r.mes_sync_status = MesSyncStatus::Synced;
                        r.mes_sync_time = Some(Utc::now());
                    }
                }
                Err(e) => {
                    failed_records += chunk.len();
                    error_messages.push(e.clone());

                    for r in chunk_records {
                        let mut record = r.clone();
                        record.mes_sync_status = MesSyncStatus::Failed;
                        record.mes_error_message = e.clone();
                        self.state.offline_cache_degraded.push(record);
                    }
                }
            }
        }

        if synced_records > 0 {
            self.state.consecutive_failures = 0;
            self.state.mes_available = true;
            self.state.current_retry_delay = 0;

            if self.state.backpressure_active {
                let total_pending = self.state.pending_degraded.len() + self.state.offline_cache_degraded.len();
                if total_pending < self.config.backpressure_threshold / 4 {
                    self.state.backpressure_active = false;
                }
            }
        }

        let sync_time_ms = start_time.elapsed().as_millis() as u64;

        let result = MesSyncResult {
            batch_id: batch_id.clone(),
            total_records,
            synced_records,
            failed_records,
            error_messages,
            sync_time_ms,
        };

        self.state.sync_history.insert(format!("degraded_{}", batch_id), result.clone());
        self.state.last_health_check = Some(Utc::now());

        if failed_records > 0 {
            Err(format!("Failed to sync {} records", failed_records))
        } else {
            Ok(result)
        }
    }

    fn recover_from_offline_cache_sync(&mut self) -> Result<(), String> {
        if !self.state.mes_available {
            return Err("MES system unavailable".to_string());
        }

        let mut recovered = 0;
        let mut failed = 0;

        let param_count = self.state.offline_cache_params.len().min(self.config.batch_size * self.config.max_batch_per_sync);
        if param_count > 0 {
            let records: Vec<ProcessParamRecord> = self.state.offline_cache_params.drain(0..param_count).collect();
            for chunk in records.chunks(self.config.batch_size) {
                match self.send_batch_to_mes(chunk, "params") {
                    Ok(_) => recovered += chunk.len(),
                    Err(_) => {
                        failed += chunk.len();
                        self.state.offline_cache_params.extend(chunk.iter().cloned());
                        break;
                    }
                }
            }
        }

        let degraded_count = self.state.offline_cache_degraded.len().min(self.config.batch_size * self.config.max_batch_per_sync / 2);
        if degraded_count > 0 {
            let records: Vec<DegradedCellRecord> = self.state.offline_cache_degraded.drain(0..degraded_count).collect();
            for chunk in records.chunks(self.config.batch_size / 2) {
                match self.send_batch_to_mes(chunk, "degraded") {
                    Ok(_) => recovered += chunk.len(),
                    Err(_) => {
                        failed += chunk.len();
                        self.state.offline_cache_degraded.extend(chunk.iter().cloned());
                        break;
                    }
                }
            }
        }

        if failed > 0 {
            Err(format!("Recovered {} records, {} failed", recovered, failed))
        } else {
            Ok(())
        }
    }

    fn send_batch_to_mes<T: serde::Serialize>(&self, records: &[T], data_type: &str) -> Result<(), String> {
        if !self.state.mes_available {
            return Err(format!("MES system unavailable, {} records cached", records.len()));
        }
        Ok(())
    }

    pub fn generate_process_param_records(
        &self,
        batch_id: String,
        cabinet_id: u16,
        channel_id: u32,
        cycle_index: u16,
        stage: crate::models::Stage,
        charge_current: f64,
        discharge_current: f64,
        charge_voltage: f64,
        discharge_voltage: f64,
        temperature: f64,
        duration: u32,
    ) -> Vec<ProcessParamRecord> {
        let now = Utc::now();
        let mut records = Vec::new();

        let param_specs = [
            (crate::models::ProcessParamType::ChargeCurrent, charge_current, "A", 0.0, 3.0),
            (crate::models::ProcessParamType::DischargeCurrent, discharge_current, "A", 0.0, 3.0),
            (crate::models::ProcessParamType::ChargeVoltage, charge_voltage, "V", 3.0, 4.3),
            (crate::models::ProcessParamType::DischargeVoltage, discharge_voltage, "V", 2.5, 3.8),
            (crate::models::ProcessParamType::Temperature, temperature, "°C", 20.0, 45.0),
            (crate::models::ProcessParamType::TimeDuration, duration as f64, "s", 0.0, 86400.0),
        ];

        for (param_type, param_value, param_unit, lower_limit, upper_limit) in param_specs.iter() {
            let is_out_of_spec = *param_value < *lower_limit || *param_value > *upper_limit;

            records.push(ProcessParamRecord {
                timestamp: now,
                batch_id: batch_id.clone(),
                cabinet_id,
                channel_id,
                cycle_index,
                stage,
                param_type: *param_type,
                param_value: *param_value,
                param_unit: param_unit.to_string(),
                upper_limit: *upper_limit,
                lower_limit: *lower_limit,
                is_out_of_spec,
                mes_sync_status: MesSyncStatus::Pending,
                mes_sync_time: None,
                mes_error_message: String::new(),
            });
        }

        records
    }

    pub fn generate_degraded_cell_record(
        &self,
        batch_id: String,
        cabinet_id: u16,
        channel_id: u32,
        cycle_index: u16,
        capacity: f64,
        capacity_ratio: f64,
        internal_resistance: f64,
        degradation_reason: String,
        grade: crate::models::CellGrade,
    ) -> DegradedCellRecord {
        DegradedCellRecord {
            timestamp: Utc::now(),
            batch_id,
            cabinet_id,
            channel_id,
            cycle_index,
            capacity,
            capacity_ratio,
            internal_resistance,
            degradation_reason,
            grade,
            mes_sync_status: MesSyncStatus::Pending,
            mes_sync_time: None,
            mes_ack_time: None,
            mes_error_message: String::new(),
        }
    }

    pub fn get_batch_capacity_distribution(
        &self,
        batch_id: &str,
        capacities: &[f64],
    ) -> BatchCapacityDistribution {
        if capacities.is_empty() {
            return BatchCapacityDistribution {
                batch_id: batch_id.to_string(),
                capacity_bins: Vec::new(),
                mean: 0.0,
                std_dev: 0.0,
                median: 0.0,
                skewness: 0.0,
                kurtosis: 0.0,
            };
        }

        let mean = capacities.iter().sum::<f64>() / capacities.len() as f64;

        let variance = capacities
            .iter()
            .map(|c| (c - mean).powi(2))
            .sum::<f64>()
            / capacities.len() as f64;
        let std_dev = variance.sqrt();

        let mut sorted = capacities.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let median = if sorted.len() % 2 == 0 {
            (sorted[sorted.len() / 2 - 1] + sorted[sorted.len() / 2]) / 2.0
        } else {
            sorted[sorted.len() / 2]
        };

        let skewness = if std_dev > 0.0 {
            capacities
                .iter()
                .map(|c| ((c - mean) / std_dev).powi(3))
                .sum::<f64>()
                / capacities.len() as f64
        } else {
            0.0
        };

        let kurtosis = if std_dev > 0.0 {
            capacities
                .iter()
                .map(|c| ((c - mean) / std_dev).powi(4))
                .sum::<f64>()
                / capacities.len() as f64
                - 3.0
        } else {
            0.0
        };

        let min_cap = sorted[0];
        let max_cap = sorted[sorted.len() - 1];
        let bin_count = 20;
        let bin_width = (max_cap - min_cap) / bin_count as f64;

        let mut bins = Vec::new();
        for i in 0..bin_count {
            let bin_start = min_cap + i as f64 * bin_width;
            let bin_end = bin_start + bin_width;
            let count = sorted
                .iter()
                .filter(|&&c| c >= bin_start && (c < bin_end || (i == bin_count - 1 && c <= bin_end)))
                .count() as u32;
            bins.push((bin_start, bin_end, count));
        }

        BatchCapacityDistribution {
            batch_id: batch_id.to_string(),
            capacity_bins: bins,
            mean,
            std_dev,
            median,
            skewness,
            kurtosis,
        }
    }

    pub fn query_batch(&self, request: &BatchQueryRequest) -> Vec<BatchInfo> {
        let mut results: Vec<BatchInfo> = self.state.batch_info_cache.values().cloned().collect();

        if let Some(batch_id) = &request.batch_id {
            results.retain(|b| b.batch_id == *batch_id);
        }

        if let Some(start_date) = &request.start_date {
            if let Ok(start) = chrono::NaiveDate::parse_from_str(start_date, "%Y-%m-%d") {
                results.retain(|b| b.date >= start);
            }
        }

        if let Some(end_date) = &request.end_date {
            if let Ok(end) = chrono::NaiveDate::parse_from_str(end_date, "%Y-%m-%d") {
                results.retain(|b| b.date <= end);
            }
        }

        if let Some(product_code) = &request.product_code {
            results.retain(|b| b.product_code == *product_code);
        }

        if let Some(battery_model) = &request.battery_model {
            results.retain(|b| b.battery_model == *battery_model);
        }

        if let Some(min_yield) = request.min_yield_rate {
            results.retain(|b| b.yield_rate >= min_yield);
        }

        results.sort_by(|a, b| b.start_time.cmp(&a.start_time));

        if let Some(offset) = request.offset {
            if offset < results.len() {
                results = results.into_iter().skip(offset).collect();
            } else {
                results = Vec::new();
            }
        }

        if let Some(limit) = request.limit {
            results.truncate(limit);
        }

        results
    }

    pub fn create_batch_info(
        &mut self,
        batch_id: String,
        product_code: String,
        battery_model: String,
        rated_capacity: f64,
        total_cells: u32,
        operator: String,
        shift: String,
    ) -> BatchInfo {
        let now = Utc::now();
        let batch_info = BatchInfo {
            date: now.date_naive(),
            batch_id: batch_id.clone(),
            product_code,
            battery_model,
            rated_capacity,
            total_cells,
            start_time: now,
            end_time: None,
            operator,
            shift,
            avg_capacity: 0.0,
            yield_rate: 0.0,
            grade_a_ratio: 0.0,
            grade_b_ratio: 0.0,
            grade_c_ratio: 0.0,
            rejected_ratio: 0.0,
            avg_internal_resistance: 0.0,
            process_params: Vec::new(),
            remarks: String::new(),
            mes_sync_status: MesSyncStatus::Pending,
            mes_sync_time: None,
            created_at: now,
        };

        self.state.batch_info_cache.insert(batch_id, batch_info.clone());
        batch_info
    }

    pub fn update_batch_statistics(
        &mut self,
        batch_id: &str,
        capacities: &[f64],
        resistances: &[f64],
        grades: &[crate::models::CellGrade],
    ) -> Option<&BatchInfo> {
        let batch_info = self.state.batch_info_cache.get_mut(batch_id)?;

        if !capacities.is_empty() {
            batch_info.avg_capacity = capacities.iter().sum::<f64>() / capacities.len() as f64;
        }

        if !resistances.is_empty() {
            batch_info.avg_internal_resistance =
                resistances.iter().sum::<f64>() / resistances.len() as f64;
        }

        if !grades.is_empty() {
            let total = grades.len() as f64;
            batch_info.grade_a_ratio =
                grades.iter().filter(|&&g| g == crate::models::CellGrade::A).count() as f64 / total;
            batch_info.grade_b_ratio =
                grades.iter().filter(|&&g| g == crate::models::CellGrade::B).count() as f64 / total;
            batch_info.grade_c_ratio =
                grades.iter().filter(|&&g| g == crate::models::CellGrade::C).count() as f64 / total;
            batch_info.rejected_ratio =
                grades.iter().filter(|&&g| g == crate::models::CellGrade::Rejected).count() as f64 / total;
            batch_info.yield_rate = batch_info.grade_a_ratio + batch_info.grade_b_ratio;
        }

        batch_info.end_time = Some(Utc::now());

        self.state.batch_info_cache.get(batch_id)
    }
}

#[derive(Clone)]
struct SyncState {
    pending_params: Vec<ProcessParamRecord>,
    pending_degraded: Vec<DegradedCellRecord>,
    offline_cache_params: Vec<ProcessParamRecord>,
    offline_cache_degraded: Vec<DegradedCellRecord>,
    sync_history: HashMap<String, MesSyncResult>,
    mes_available: bool,
    consecutive_failures: u32,
    current_retry_delay: u64,
    backpressure_active: bool,
}

impl MesConnectorState {
    fn clone_into(&self) -> SyncState {
        SyncState {
            pending_params: self.pending_params.clone(),
            pending_degraded: self.pending_degraded.clone(),
            offline_cache_params: self.offline_cache_params.clone(),
            offline_cache_degraded: self.offline_cache_degraded.clone(),
            sync_history: self.sync_history.clone(),
            mes_available: self.mes_available,
            consecutive_failures: self.consecutive_failures,
            current_retry_delay: self.current_retry_delay,
            backpressure_active: self.backpressure_active,
        }
    }
}

fn sync_batch_sync(
    mut state: SyncState,
    config: ConnectorConfig,
    sync_type: SyncType,
) -> (Vec<MesSyncResult>, SyncState) {
    let mut results = Vec::new();

    match sync_type {
        SyncType::Params => {
            if let Ok(result) = sync_params_sync(&mut state, &config) {
                if result.total_records > 0 {
                    results.push(result);
                }
            }
        }
        SyncType::Degraded => {
            if let Ok(result) = sync_degraded_sync(&mut state, &config) {
                if result.total_records > 0 {
                    results.push(result);
                }
            }
        }
        SyncType::All => {
            if let Ok(result) = sync_params_sync(&mut state, &config) {
                if result.total_records > 0 {
                    results.push(result);
                }
            }
            if let Ok(result) = sync_degraded_sync(&mut state, &config) {
                if result.total_records > 0 {
                    results.push(result);
                }
            }
        }
    }

    (results, state)
}

fn sync_params_sync(state: &mut SyncState, config: &ConnectorConfig) -> Result<MesSyncResult, String> {
    if state.offline_cache_params.len() > 0 && state.mes_available && config.auto_recovery_enabled {
        let _ = recover_offline_params_sync(state, config);
    }

    if state.pending_params.is_empty() && state.offline_cache_params.is_empty() {
        return Ok(MesSyncResult {
            batch_id: "NONE".to_string(),
            total_records: 0,
            synced_records: 0,
            failed_records: 0,
            error_messages: Vec::new(),
            sync_time_ms: 0,
        });
    }

    let mut records_to_sync: Vec<ProcessParamRecord> = Vec::new();
    let transfer_count = state.offline_cache_params.len().min(config.batch_size * config.max_batch_per_sync);
    if transfer_count > 0 {
        records_to_sync.extend(state.offline_cache_params.drain(0..transfer_count));
    }
    let live_count = state.pending_params.len().min(config.batch_size - records_to_sync.len());
    if live_count > 0 {
        records_to_sync.extend(state.pending_params.drain(0..live_count));
    }

    let start_time = std::time::Instant::now();
    let batch_id = records_to_sync.first().map(|p| p.batch_id.clone()).unwrap_or_default();
    let total_records = records_to_sync.len();

    let mut synced_records = 0;
    let mut failed_records = 0;
    let mut error_messages: Vec<String> = Vec::new();

    for chunk in records_to_sync.chunks(config.batch_size) {
        let mut chunk_records: Vec<ProcessParamRecord> = chunk.to_vec();
        if !state.mes_available {
            failed_records += chunk.len();
            error_messages.push("MES system unavailable".to_string());
            for r in chunk_records {
                let mut record = r.clone();
                record.mes_sync_status = MesSyncStatus::Failed;
                record.mes_error_message = "MES system unavailable".to_string();
                state.offline_cache_params.push(record);
            }
            break;
        }

        synced_records += chunk.len();
        for r in &mut chunk_records {
            r.mes_sync_status = MesSyncStatus::Synced;
            r.mes_sync_time = Some(Utc::now());
        }
    }

    if synced_records > 0 {
        state.consecutive_failures = 0;
        state.mes_available = true;
        state.current_retry_delay = 0;

        if state.backpressure_active {
            let total_pending = state.pending_params.len() + state.offline_cache_params.len();
            if total_pending < config.backpressure_threshold / 2 {
                state.backpressure_active = false;
            }
        }
    }

    let sync_time_ms = start_time.elapsed().as_millis() as u64;

    let result = MesSyncResult {
        batch_id: batch_id.clone(),
        total_records,
        synced_records,
        failed_records,
        error_messages,
        sync_time_ms,
    };

    state.sync_history.insert(format!("params_{}", batch_id), result.clone());

    if failed_records > 0 {
        Err(format!("Failed to sync {} records", failed_records))
    } else {
        Ok(result)
    }
}

fn sync_degraded_sync(state: &mut SyncState, config: &ConnectorConfig) -> Result<MesSyncResult, String> {
    if state.pending_degraded.is_empty() && state.offline_cache_degraded.is_empty() {
        return Ok(MesSyncResult {
            batch_id: "NONE".to_string(),
            total_records: 0,
            synced_records: 0,
            failed_records: 0,
            error_messages: Vec::new(),
            sync_time_ms: 0,
        });
    }

    let mut records_to_sync: Vec<DegradedCellRecord> = Vec::new();
    let transfer_count = state.offline_cache_degraded.len().min(config.batch_size * config.max_batch_per_sync / 2);
    if transfer_count > 0 {
        records_to_sync.extend(state.offline_cache_degraded.drain(0..transfer_count));
    }
    let live_count = state.pending_degraded.len().min(config.batch_size / 2 - records_to_sync.len());
    if live_count > 0 {
        records_to_sync.extend(state.pending_degraded.drain(0..live_count));
    }

    let start_time = std::time::Instant::now();
    let batch_id = records_to_sync.first().map(|p| p.batch_id.clone()).unwrap_or_default();
    let total_records = records_to_sync.len();

    let mut synced_records = 0;
    let mut failed_records = 0;
    let mut error_messages: Vec<String> = Vec::new();

    for chunk in records_to_sync.chunks(config.batch_size / 2) {
        let mut chunk_records: Vec<DegradedCellRecord> = chunk.to_vec();
        if !state.mes_available {
            failed_records += chunk.len();
            error_messages.push("MES system unavailable".to_string());
            for r in chunk_records {
                let mut record = r.clone();
                record.mes_sync_status = MesSyncStatus::Failed;
                record.mes_error_message = "MES system unavailable".to_string();
                state.offline_cache_degraded.push(record);
            }
            break;
        }

        synced_records += chunk.len();
        for r in &mut chunk_records {
            r.mes_sync_status = MesSyncStatus::Synced;
            r.mes_sync_time = Some(Utc::now());
        }
    }

    if synced_records > 0 {
        state.consecutive_failures = 0;
        state.mes_available = true;
        state.current_retry_delay = 0;

        if state.backpressure_active {
            let total_pending = state.pending_degraded.len() + state.offline_cache_degraded.len();
            if total_pending < config.backpressure_threshold / 4 {
                state.backpressure_active = false;
            }
        }
    }

    let sync_time_ms = start_time.elapsed().as_millis() as u64;

    let result = MesSyncResult {
        batch_id: batch_id.clone(),
        total_records,
        synced_records,
        failed_records,
        error_messages,
        sync_time_ms,
    };

    state.sync_history.insert(format!("degraded_{}", batch_id), result.clone());

    if failed_records > 0 {
        Err(format!("Failed to sync {} records", failed_records))
    } else {
        Ok(result)
    }
}

fn recover_offline_params_sync(state: &mut SyncState, config: &ConnectorConfig) -> Result<(), String> {
    if !state.mes_available {
        return Err("MES system unavailable".to_string());
    }

    let mut recovered = 0;
    let mut failed = 0;

    let param_count = state.offline_cache_params.len().min(config.batch_size * config.max_batch_per_sync);
    if param_count > 0 {
        let records: Vec<ProcessParamRecord> = state.offline_cache_params.drain(0..param_count).collect();
        for chunk in records.chunks(config.batch_size) {
            if state.mes_available {
                recovered += chunk.len();
            } else {
                failed += chunk.len();
                state.offline_cache_params.extend(chunk.iter().cloned());
                break;
            }
        }
    }

    let degraded_count = state.offline_cache_degraded.len().min(config.batch_size * config.max_batch_per_sync / 2);
    if degraded_count > 0 {
        let records: Vec<DegradedCellRecord> = state.offline_cache_degraded.drain(0..degraded_count).collect();
        for chunk in records.chunks(config.batch_size / 2) {
            if state.mes_available {
                recovered += chunk.len();
            } else {
                failed += chunk.len();
                state.offline_cache_degraded.extend(chunk.iter().cloned());
                break;
            }
        }
    }

    if failed > 0 {
        Err(format!("Recovered {} records, {} failed", recovered, failed))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CellGrade, ProcessParamType, Stage};
    use rand::Rng;

    fn create_test_service() -> MesConnectorService {
        let config = ConnectorConfig {
            batch_size: 10,
            enable_automatic_sync: true,
            ..ConnectorConfig::default()
        };
        let (service, _) = MesConnectorService::new(config);
        service
    }

    fn create_test_batch_info(batch_id: &str) -> BatchInfo {
        BatchInfo {
            date: chrono::Utc::now().date_naive(),
            batch_id: batch_id.to_string(),
            product_code: "PC001".to_string(),
            battery_model: "3.2Ah-18650".to_string(),
            rated_capacity: 3.2,
            total_cells: 512,
            start_time: chrono::Utc::now(),
            end_time: None,
            operator: "张三".to_string(),
            shift: "早班".to_string(),
            avg_capacity: 0.0,
            yield_rate: 0.0,
            grade_a_ratio: 0.0,
            grade_b_ratio: 0.0,
            grade_c_ratio: 0.0,
            rejected_ratio: 0.0,
            avg_internal_resistance: 0.0,
            process_params: Vec::new(),
            remarks: String::new(),
            mes_sync_status: MesSyncStatus::Pending,
            mes_sync_time: None,
            created_at: chrono::Utc::now(),
        }
    }

    #[tokio::test]
    async fn test_async_record_param() {
        let config = ConnectorConfig {
            batch_size: 10,
            enable_automatic_sync: false,
            ..ConnectorConfig::default()
        };

        let (service, handle) = MesConnectorService::new(config);
        tokio::spawn(service.run());

        let record = ProcessParamRecord {
            timestamp: Utc::now(),
            batch_id: "TEST-BATCH-001".to_string(),
            cabinet_id: 0,
            channel_id: 1,
            cycle_index: 1,
            stage: Stage::CcCharge,
            param_type: ProcessParamType::ChargeCurrent,
            param_value: 1.6,
            param_unit: "A".to_string(),
            upper_limit: 3.0,
            lower_limit: 0.0,
            is_out_of_spec: false,
            mes_sync_status: MesSyncStatus::Pending,
            mes_sync_time: None,
            mes_error_message: String::new(),
        };

        let request = RecordParamRequest {
            request_id: "test-001".to_string(),
            record,
            respond_to: None,
        };

        let rx = handle.record_process_param(request).await.unwrap();
        let result = rx.await.unwrap();

        assert_eq!(result.request_id, "test-001");
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_async_sync_request() {
        let config = ConnectorConfig {
            batch_size: 10,
            enable_automatic_sync: false,
            ..ConnectorConfig::default()
        };

        let (service, handle) = MesConnectorService::new(config);
        tokio::spawn(service.run());

        for i in 0..5 {
            let record = ProcessParamRecord {
                timestamp: Utc::now(),
                batch_id: "TEST-BATCH-002".to_string(),
                cabinet_id: 0,
                channel_id: i,
                cycle_index: 1,
                stage: Stage::CcCharge,
                param_type: ProcessParamType::ChargeCurrent,
                param_value: 1.6,
                param_unit: "A".to_string(),
                upper_limit: 3.0,
                lower_limit: 0.0,
                is_out_of_spec: false,
                mes_sync_status: MesSyncStatus::Pending,
                mes_sync_time: None,
                mes_error_message: String::new(),
            };

            let request = RecordParamRequest {
                request_id: format!("record-{}", i),
                record,
                respond_to: None,
            };

            let _ = handle.record_process_param(request).await.unwrap();
        }

        let sync_req = SyncRequest {
            request_id: "sync-001".to_string(),
            sync_type: SyncType::Params,
            respond_to: None,
        };

        let rx = handle.sync_request(sync_req).await.unwrap();
        let result = rx.await.unwrap();

        assert_eq!(result.request_id, "sync-001");
        assert!(result.success);
        assert!(result.results.len() > 0);
        assert_eq!(result.results[0].total_records, 5);
    }

    #[tokio::test]
    async fn test_async_add_manual_label() {
        let config = ConnectorConfig::default();
        let (service, handle) = MesConnectorService::new(config);
        tokio::spawn(service.run());

        let request = AddManualLabelRequest {
            request_id: "label-001".to_string(),
            cabinet_id: 1,
            channel_id: 100,
            cycle_index: 5,
            corrected_mode: "Normal".to_string(),
            notes: "Verified by operator".to_string(),
            operator: "张三".to_string(),
            respond_to: None,
        };

        let rx = handle.add_manual_label(request).await.unwrap();
        let result = rx.await.unwrap();

        assert!(result);
    }

    #[tokio::test]
    async fn test_async_get_pending_confirmations() {
        let config = ConnectorConfig::default();
        let (service, handle) = MesConnectorService::new(config);
        tokio::spawn(service.run());

        let rx = handle.get_pending_confirmations().await.unwrap();
        let result = rx.await.unwrap();

        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_async_get_status() {
        let config = ConnectorConfig::default();
        let (service, handle) = MesConnectorService::new(config);
        tokio::spawn(service.run());

        let rx = handle.get_status().await.unwrap();
        let (available, failures, pending_params, pending_degraded, offline_params, offline_degraded) = rx.await.unwrap();

        assert!(available);
        assert_eq!(failures, 0);
        assert_eq!(pending_params, 0);
        assert_eq!(pending_degraded, 0);
        assert_eq!(offline_params, 0);
        assert_eq!(offline_degraded, 0);
    }

    #[tokio::test]
    async fn test_async_config_update() {
        let config = ConnectorConfig {
            batch