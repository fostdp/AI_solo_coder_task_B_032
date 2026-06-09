use crate::models::{ChannelData, DegradationAnalysis, DegradationMode, DvDqPoint};
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex};

#[derive(Debug, Clone)]
pub struct ClassifierConfig {
    pub peak_detection_threshold: f64,
    pub min_peak_distance: f64,
    pub reference_cycle: u16,
    pub cathode_peak_range: (f64, f64),
    pub anode_peak_range: (f64, f64),
    pub sei_peak_range: (f64, f64),
    pub fading_rate_threshold: f64,
    pub resistance_growth_threshold: f64,
    pub enable_transfer_learning: bool,
    pub min_baseline_samples: usize,
    pub new_model_confidence_penalty: f64,
    pub transfer_learning_weight: f64,
    pub require_manual_confirmation_threshold: f64,
    pub max_transfer_distance: f64,
    pub channel_buffer: usize,
}

impl Default for ClassifierConfig {
    fn default() -> Self {
        Self {
            peak_detection_threshold: 0.01,
            min_peak_distance: 0.1,
            reference_cycle: 1,
            cathode_peak_range: (3.8, 4.2),
            anode_peak_range: (0.05, 0.3),
            sei_peak_range: (0.5, 1.5),
            fading_rate_threshold: 0.02,
            resistance_growth_threshold: 0.05,
            enable_transfer_learning: true,
            min_baseline_samples: 5,
            new_model_confidence_penalty: 0.3,
            transfer_learning_weight: 0.7,
            require_manual_confirmation_threshold: 0.6,
            max_transfer_distance: 0.3,
            channel_buffer: 100,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModelBaseline {
    pub model_name: String,
    pub avg_dvdq_curve: Vec<DvDqPoint>,
    pub peak_positions: Vec<f64>,
    pub sample_count: usize,
    pub cathode_peak_range: (f64, f64),
    pub anode_peak_range: (f64, f64),
    pub sei_peak_range: (f64, f64),
    pub model_features: Vec<f64>,
}

#[derive(Debug, Clone)]
pub struct ManualLabel {
    pub cabinet_id: u16,
    pub channel_id: u32,
    pub cycle_index: u16,
    pub corrected_mode: DegradationMode,
    pub notes: String,
    pub operator: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct AnalyzeRequest {
    pub request_id: String,
    pub cabinet_id: u16,
    pub channel_id: u32,
    pub cycle_index: u16,
    pub discharge_data: Vec<ChannelData>,
    pub historical_capacities: Vec<(u16, f64)>,
    pub historical_resistances: Vec<(u16, f64)>,
    pub battery_model: Option<String>,
    pub respond_to: Option<oneshot::Sender<AnalyzeResult>>,
}

#[derive(Debug)]
pub struct AnalyzeResult {
    pub request_id: String,
    pub analysis: DegradationAnalysis,
    pub dvdq_curve: Vec<DvDqPoint>,
}

#[derive(Debug, Clone)]
pub struct AddLabelRequest {
    pub request_id: String,
    pub cabinet_id: u16,
    pub channel_id: u32,
    pub cycle_index: u16,
    pub corrected_mode: DegradationMode,
    pub notes: String,
    pub operator: String,
    pub respond_to: Option<oneshot::Sender<bool>>,
}

#[derive(Debug, Clone)]
pub struct RegisterBaselineRequest {
    pub request_id: String,
    pub model_name: String,
    pub baseline: ModelBaseline,
    pub respond_to: Option<oneshot::Sender<bool>>,
}

pub enum ClassifierMessage {
    Analyze(AnalyzeRequest),
    AddManualLabel(AddLabelRequest),
    GetPendingConfirmations {
        respond_to: oneshot::Sender<Vec<(u16, u32, u16)>>,
    },
    GetKnownModels {
        respond_to: oneshot::Sender<Vec<String>>,
    },
    RegisterModelBaseline(RegisterBaselineRequest),
    UpdateConfig(ClassifierConfig),
    Shutdown,
}

impl fmt::Debug for ClassifierMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClassifierMessage::Analyze(req) => {
                write!(f, "Analyze({}, cab:{}, ch:{})", req.request_id, req.cabinet_id, req.channel_id)
            }
            ClassifierMessage::AddManualLabel(req) => write!(f, "AddManualLabel({})", req.request_id),
            ClassifierMessage::GetPendingConfirmations { .. } => write!(f, "GetPendingConfirmations"),
            ClassifierMessage::GetKnownModels { .. } => write!(f, "GetKnownModels"),
            ClassifierMessage::RegisterModelBaseline(req) => {
                write!(f, "RegisterModelBaseline({})", req.request_id)
            }
            ClassifierMessage::UpdateConfig(_) => write!(f, "UpdateConfig"),
            ClassifierMessage::Shutdown => write!(f, "Shutdown"),
        }
    }
}

pub type ClassifierSender = mpsc::Sender<ClassifierMessage>;
pub type ClassifierReceiver = mpsc::Receiver<ClassifierMessage>;

#[derive(Clone)]
pub struct AgingClassifierHandle {
    sender: ClassifierSender,
    config: Arc<Mutex<ClassifierConfig>>,
}

impl AgingClassifierHandle {
    pub fn new(sender: ClassifierSender, config: ClassifierConfig) -> Self {
        Self {
            sender,
            config: Arc::new(Mutex::new(config)),
        }
    }

    pub async fn analyze(
        &self,
        request: AnalyzeRequest,
    ) -> Result<oneshot::Receiver<AnalyzeResult>, String> {
        let (tx, rx) = oneshot::channel();
        let message = ClassifierMessage::Analyze(AnalyzeRequest {
            respond_to: Some(tx),
            ..request
        });

        self.sender
            .send(message)
            .await
            .map_err(|e| format!("Failed to send analyze request: {}", e))?;

        Ok(rx)
    }

    pub async fn add_manual_label(
        &self,
        request: AddLabelRequest,
    ) -> Result<oneshot::Receiver<bool>, String> {
        let (tx, rx) = oneshot::channel();
        let message = ClassifierMessage::AddManualLabel(AddLabelRequest {
            respond_to: Some(tx),
            ..request
        });

        self.sender
            .send(message)
            .await
            .map_err(|e| format!("Failed to send add label request: {}", e))?;

        Ok(rx)
    }

    pub async fn get_pending_confirmations(
        &self,
    ) -> Result<oneshot::Receiver<Vec<(u16, u32, u16)>>, String> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(ClassifierMessage::GetPendingConfirmations { respond_to: tx })
            .await
            .map_err(|e| format!("Failed to send pending confirmations request: {}", e))?;

        Ok(rx)
    }

    pub async fn get_known_models(&self) -> Result<oneshot::Receiver<Vec<String>>, String> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(ClassifierMessage::GetKnownModels { respond_to: tx })
            .await
            .map_err(|e| format!("Failed to send get known models request: {}", e))?;

        Ok(rx)
    }

    pub async fn register_model_baseline(
        &self,
        request: RegisterBaselineRequest,
    ) -> Result<oneshot::Receiver<bool>, String> {
        let (tx, rx) = oneshot::channel();
        let message = ClassifierMessage::RegisterModelBaseline(RegisterBaselineRequest {
            respond_to: Some(tx),
            ..request
        });

        self.sender
            .send(message)
            .await
            .map_err(|e| format!("Failed to send register baseline request: {}", e))?;

        Ok(rx)
    }

    pub async fn update_config(&self, config: ClassifierConfig) -> Result<(), String> {
        *self.config.lock().await = config.clone();
        self.sender
            .send(ClassifierMessage::UpdateConfig(config))
            .await
            .map_err(|e| format!("Failed to send config update: {}", e))
    }

    pub async fn get_config(&self) -> ClassifierConfig {
        self.config.lock().await.clone()
    }

    pub async fn shutdown(&self) -> Result<(), String> {
        self.sender
            .send(ClassifierMessage::Shutdown)
            .await
            .map_err(|e| format!("Failed to send shutdown: {}", e))
    }
}

struct ClassifierState {
    baseline_data: HashMap<(u16, u32), Vec<DvDqPoint>>,
    historical_analysis: HashMap<(u16, u32), Vec<(u16, DegradationMode, f64)>>,
    model_baselines: HashMap<String, ModelBaseline>,
    manual_labels: HashMap<(u16, u32, u16), ManualLabel>,
    pending_confirmation: HashSet<(u16, u32, u16)>,
}

impl ClassifierState {
    fn new() -> Self {
        Self {
            baseline_data: HashMap::new(),
            historical_analysis: HashMap::new(),
            model_baselines: HashMap::new(),
            manual_labels: HashMap::new(),
            pending_confirmation: HashSet::new(),
        }
    }
}

pub struct AgingClassifierService {
    config: ClassifierConfig,
    receiver: ClassifierReceiver,
    state: ClassifierState,
    active_requests: usize,
}

impl AgingClassifierService {
    pub fn new(config: ClassifierConfig) -> (Self, AgingClassifierHandle) {
        let (sender, receiver) = mpsc::channel(config.channel_buffer);
        let state = ClassifierState::new();
        let handle = AgingClassifierHandle::new(sender, config.clone());
        (
            Self {
                config,
                receiver,
                state,
                active_requests: 0,
            },
            handle,
        )
    }

    pub async fn run(mut self) {
        tracing::info!("AgingClassifierService started, waiting for requests");

        while let Some(message) = self.receiver.recv().await {
            match message {
                ClassifierMessage::Analyze(request) => {
                    self.active_requests += 1;
                    tracing::debug!(
                        "Processing analyze request: {} (cab:{}, ch:{}, cycle:{})",
                        request.request_id,
                        request.cabinet_id,
                        request.channel_id,
                        request.cycle_index
                    );

                    let config = self.config.clone();
                    let state = self.extract_analyze_state();
                    let request_id = request.request_id.clone();
                    let respond_to = request.respond_to;

                    let result = tokio::task::spawn_blocking(move || {
                        process_analyze_sync(request, config, state)
                    })
                    .await;

                    let (analyze_result, updated_state) = match result {
                        Ok((result, state)) => (
                            AnalyzeResult {
                                request_id: request_id.clone(),
                                analysis: result.0,
                                dvdq_curve: result.1,
                            },
                            state,
                        ),
                        Err(e) => {
                            tracing::error!("Analyze task panicked: {}", e);
                            (
                                AnalyzeResult {
                                    request_id: request_id.clone(),
                                    analysis: DegradationAnalysis {
                                        timestamp: Utc::now(),
                                        cabinet_id: 0,
                                        channel_id: 0,
                                        cycle_index: 0,
                                        mode: DegradationMode::Normal,
                                        confidence: 0.0,
                                        cathode_score: 0.0,
                                        anode_score: 0.0,
                                        electrolyte_score: 0.0,
                                        sei_score: 0.0,
                                        peak_positions: Vec::new(),
                                        peak_heights: Vec::new(),
                                        capacity_fade_rate: 0.0,
                                        resistance_growth_rate: 0.0,
                                        recommendations: "Analysis error".to_string(),
                                        battery_model: None,
                                        used_transfer_learning: false,
                                        transfer_source_model: None,
                                        transfer_similarity: None,
                                        baseline_sample_count: 0,
                                        requires_manual_confirmation: true,
                                        is_new_model: false,
                                        manually_corrected_mode: None,
                                        correction_notes: None,
                                        corrected_by: None,
                                        corrected_at: None,
                                    },
                                    dvdq_curve: Vec::new(),
                                },
                                self.state.extract_sync(),
                            )
                        }
                    };

                    self.apply_analyze_state(updated_state);

                    if analyze_result.analysis.requires_manual_confirmation {
                        self.state.pending_confirmation.insert((
                            analyze_result.analysis.cabinet_id,
                            analyze_result.analysis.channel_id,
                            analyze_result.analysis.cycle_index,
                        ));
                    }

                    if let Some(tx) = respond_to {
                        let _ = tx.send(analyze_result);
                    }

                    self.active_requests -= 1;
                }
                ClassifierMessage::AddManualLabel(request) => {
                    let result = self.process_add_label(&request);
                    if let Some(tx) = request.respond_to {
                        let _ = tx.send(result);
                    }
                }
                ClassifierMessage::GetPendingConfirmations { respond_to } => {
                    let pending: Vec<(u16, u32, u16)> =
                        self.state.pending_confirmation.iter().cloned().collect();
                    let _ = respond_to.send(pending);
                }
                ClassifierMessage::GetKnownModels { respond_to } => {
                    let models: Vec<String> = self.state.model_baselines.keys().cloned().collect();
                    let _ = respond_to.send(models);
                }
                ClassifierMessage::RegisterModelBaseline(request) => {
                    self.state
                        .model_baselines
                        .insert(request.model_name.clone(), request.baseline);
                    if let Some(tx) = request.respond_to {
                        let _ = tx.send(true);
                    }
                }
                ClassifierMessage::UpdateConfig(new_config) => {
                    self.config = new_config;
                    tracing::info!("Classifier config updated");
                }
                ClassifierMessage::Shutdown => {
                    tracing::info!("AgingClassifierService shutting down");
                    break;
                }
            }
        }

        tracing::info!("AgingClassifierService stopped");
    }

    fn extract_analyze_state(&mut self) -> AnalyzeStateSnapshot {
        AnalyzeStateSnapshot {
            baseline_data: self.state.baseline_data.clone(),
            historical_analysis: self.state.historical_analysis.clone(),
            model_baselines: self.state.model_baselines.clone(),
            manual_labels: self.state.manual_labels.clone(),
        }
    }

    fn apply_analyze_state(&mut self, snapshot: AnalyzeStateSnapshot) {
        self.state.baseline_data = snapshot.baseline_data;
        self.state.historical_analysis = snapshot.historical_analysis;
        self.state.model_baselines = snapshot.model_baselines;
        self.state.manual_labels = snapshot.manual_labels;
    }

    fn process_add_label(&mut self, request: &AddLabelRequest) -> bool {
        let key = (request.cabinet_id, request.channel_id, request.cycle_index);
        self.state.manual_labels.insert(
            key,
            ManualLabel {
                cabinet_id: request.cabinet_id,
                channel_id: request.channel_id,
                cycle_index: request.cycle_index,
                corrected_mode: request.corrected_mode,
                notes: request.notes.clone(),
                operator: request.operator.clone(),
                timestamp: Utc::now(),
            },
        );
        self.state.pending_confirmation.remove(&key);
        true
    }

    pub fn active_requests(&self) -> usize {
        self.active_requests
    }
}

#[derive(Clone)]
struct AnalyzeStateSnapshot {
    baseline_data: HashMap<(u16, u32), Vec<DvDqPoint>>,
    historical_analysis: HashMap<(u16, u32), Vec<(u16, DegradationMode, f64)>>,
    model_baselines: HashMap<String, ModelBaseline>,
    manual_labels: HashMap<(u16, u32, u16), ManualLabel>,
}

impl ClassifierState {
    fn extract_sync(&self) -> AnalyzeStateSnapshot {
        AnalyzeStateSnapshot {
            baseline_data: self.baseline_data.clone(),
            historical_analysis: self.historical_analysis.clone(),
            model_baselines: self.model_baselines.clone(),
            manual_labels: self.manual_labels.clone(),
        }
    }
}

fn process_analyze_sync(
    request: AnalyzeRequest,
    config: ClassifierConfig,
    mut state: AnalyzeStateSnapshot,
) -> ((DegradationAnalysis, Vec<DvDqPoint>), AnalyzeStateSnapshot) {
    let dvdq_curve = calculate_dvdq_curve(&request.discharge_data, &config);
    let peaks = detect_peaks(&dvdq_curve, &config);
    let peak_positions: Vec<f64> = peaks.iter().map(|(v, _)| *v).collect();
    let peak_heights: Vec<f64> = peaks.iter().map(|(_, h)| *h).collect();

    let is_new_model = request
        .battery_model
        .as_ref()
        .map(|m| !state.model_baselines.contains_key(m))
        .unwrap_or(false);

    let mut used_transfer_learning = false;
    let mut transfer_source_model: Option<String> = None;
    let mut transfer_similarity: Option<f64> = None;
    let baseline_sample_count = request
        .battery_model
        .as_ref()
        .and_then(|m| state.model_baselines.get(m))
        .map(|b| b.sample_count)
        .unwrap_or(0);

    let effective_cathode_range = if is_new_model && config.enable_transfer_learning {
        if let Some(source) = find_similar_baseline(&peak_positions, &peak_heights, &state.model_baselines, &config) {
            used_transfer_learning = true;
            transfer_source_model = Some(source.model_name.clone());
            transfer_similarity = Some(calculate_model_similarity(&peaks, &source));
            Some(source.cathode_peak_range)
        } else {
            None
        }
    } else {
        None
    };

    let (cathode_score, anode_score, electrolyte_score, sei_score) = calculate_degradation_scores_with_transfer(
        &dvdq_curve,
        &peaks,
        request.cabinet_id,
        request.channel_id,
        request.cycle_index,
        effective_cathode_range,
        &request.battery_model,
        &config,
        &state,
    );

    let capacity_fade_rate = calculate_fade_rate(&request.historical_capacities, &config);
    let resistance_growth_rate = calculate_resistance_growth_rate(&request.historical_resistances, &config);

    let (mut mode, mut confidence) = classify_degradation_mode(
        cathode_score,
        anode_score,
        electrolyte_score,
        sei_score,
        capacity_fade_rate,
        resistance_growth_rate,
        &config,
    );

    if is_new_model {
        confidence *= 1.0 - config.new_model_confidence_penalty;
    }
    if used_transfer_learning {
        confidence = confidence * (1.0 - config.transfer_learning_weight)
            + config.transfer_learning_weight * transfer_similarity.unwrap_or(0.5);
    }

    let key_label = (request.cabinet_id, request.channel_id, request.cycle_index);
    if let Some(label) = state.manual_labels.get(&key_label) {
        mode = label.corrected_mode;
        confidence = 0.95;
    }

    let requires_manual_confirmation = confidence < config.require_manual_confirmation_threshold
        || is_new_model
        || (used_transfer_learning && transfer_similarity.unwrap_or(0.0) < 0.7);

    let recommendations = generate_recommendations_with_context(
        mode,
        confidence,
        capacity_fade_rate,
        is_new_model,
        used_transfer_learning,
        &config,
    );

    let analysis = DegradationAnalysis {
        timestamp: Utc::now(),
        cabinet_id: request.cabinet_id,
        channel_id: request.channel_id,
        cycle_index: request.cycle_index,
        mode,
        confidence,
        cathode_score,
        anode_score,
        electrolyte_score,
        sei_score,
        peak_positions: peak_positions.clone(),
        peak_heights: peak_heights.clone(),
        capacity_fade_rate,
        resistance_growth_rate,
        recommendations,
        battery_model: request.battery_model.clone(),
        used_transfer_learning,
        transfer_source_model,
        transfer_similarity,
        baseline_sample_count,
        requires_manual_confirmation,
        is_new_model,
        manually_corrected_mode: state.manual_labels.get(&key_label).map(|l| l.corrected_mode),
        correction_notes: state.manual_labels.get(&key_label).map(|l| l.notes.clone()),
        corrected_by: state.manual_labels.get(&key_label).map(|l| l.operator.clone()),
        corrected_at: state.manual_labels.get(&key_label).map(|l| l.timestamp),
    };

    let key = (request.cabinet_id, request.channel_id);
    state
        .historical_analysis
        .entry(key)
        .or_insert_with(Vec::new)
        .push((request.cycle_index, mode, confidence));

    if request.cycle_index == config.reference_cycle {
        state.baseline_data.insert(key, dvdq_curve.clone());
        if let Some(model) = &request.battery_model {
            update_model_baseline(model, &dvdq_curve, &peaks, &mut state.model_baselines, &config);
        }
    }

    ((analysis, dvdq_curve), state)
}

fn calculate_dvdq_curve(discharge_data: &[ChannelData], config: &ClassifierConfig) -> Vec<DvDqPoint> {
    if discharge_data.len() < 3 {
        return Vec::new();
    }

    let mut sorted_data: Vec<&ChannelData> = discharge_data
        .iter()
        .filter(|d| d.stage == crate::models::Stage::Discharge)
        .collect();

    sorted_data.sort_by(|a, b| a.voltage.partial_cmp(&b.voltage).unwrap());

    let mut dvdq_points = Vec::new();

    for i in 1..sorted_data.len().saturating_sub(1) {
        let prev = sorted_data[i - 1];
        let curr = sorted_data[i];
        let next = sorted_data[i + 1];

        let dq = next.capacity - prev.capacity;
        let dv = next.voltage - prev.voltage;

        if dv.abs() > 1e-6 && dq > 0.0 {
            let dq_dv = dq / dv;

            if dq_dv.is_finite() && dq_dv >= 0.0 {
                dvdq_points.push(DvDqPoint {
                    voltage: curr.voltage,
                    dq_dv,
                    capacity: curr.capacity,
                });
            }
        }
    }

    smooth_dvdq_curve(dvdq_points, 3, config)
}

fn smooth_dvdq_curve(
    points: Vec<DvDqPoint>,
    window_size: usize,
    _config: &ClassifierConfig,
) -> Vec<DvDqPoint> {
    if points.len() < window_size * 2 + 1 {
        return points;
    }

    let mut smoothed = Vec::with_capacity(points.len());

    for i in 0..points.len() {
        let start = i.saturating_sub(window_size);
        let end = (i + window_size + 1).min(points.len());
        let slice = &points[start..end];

        let avg_dq_dv: f64 = slice.iter().map(|p| p.dq_dv).sum::<f64>() / slice.len() as f64;

        smoothed.push(DvDqPoint {
            voltage: points[i].voltage,
            dq_dv: avg_dq_dv,
            capacity: points[i].capacity,
        });
    }

    smoothed
}

fn detect_peaks(points: &[DvDqPoint], config: &ClassifierConfig) -> Vec<(f64, f64)> {
    let mut peaks = Vec::new();
    let min_height = config.peak_detection_threshold;

    for i in 2..points.len().saturating_sub(2) {
        let curr = &points[i];

        if curr.dq_dv < min_height {
            continue;
        }

        let is_peak = curr.dq_dv > points[i - 1].dq_dv
            && curr.dq_dv > points[i - 2].dq_dv
            && curr.dq_dv > points[i + 1].dq_dv
            && curr.dq_dv > points[i + 2].dq_dv;

        if is_peak {
            let too_close = peaks.iter().any(|(v, _)| {
                (curr.voltage - v).abs() < config.min_peak_distance
            });

            if !too_close {
                peaks.push((curr.voltage, curr.dq_dv));
            }
        }
    }

    peaks.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    peaks.into_iter().take(5).collect()
}

fn calculate_degradation_scores_with_transfer(
    current_curve: &[DvDqPoint],
    current_peaks: &[(f64, f64)],
    cabinet_id: u16,
    channel_id: u32,
    cycle_index: u16,
    transfer_cathode_range: Option<(f64, f64)>,
    battery_model: &Option<String>,
    config: &ClassifierConfig,
    state: &AnalyzeStateSnapshot,
) -> (f64, f64, f64, f64) {
    let key = (cabinet_id, channel_id);
    let baseline = state.baseline_data.get(&key);
    let model_baseline = battery_model.as_ref().and_then(|m| state.model_baselines.get(m));

    let cathode_range = transfer_cathode_range
        .or_else(|| model_baseline.map(|b| b.cathode_peak_range))
        .unwrap_or(config.cathode_peak_range);

    let anode_range = model_baseline
        .map(|b| b.anode_peak_range)
        .unwrap_or(config.anode_peak_range);

    let sei_range = model_baseline
        .map(|b| b.sei_peak_range)
        .unwrap_or(config.sei_peak_range);

    match (baseline, model_baseline) {
        (None, None) => (0.5, 0.5, 0.5, 0.5),
        (Some(baseline_curve), _) => {
            let cathode_score = calculate_cathode_score_with_range(
                current_peaks, baseline_curve, cycle_index, cathode_range, config);
            let anode_score = calculate_anode_score_with_range(
                current_peaks, baseline_curve, cycle_index, anode_range, config);
            let electrolyte_score = calculate_electrolyte_score(current_curve, baseline_curve, config);
            let sei_score = calculate_sei_score_with_range(
                current_peaks, cycle_index, sei_range, config);

            (cathode_score, anode_score, electrolyte_score, sei_score)
        }
        (None, Some(model_bl)) => {
            let cathode_score = calculate_cathode_score_with_range(
                current_peaks, &model_bl.avg_dvdq_curve, cycle_index, cathode_range, config);
            let anode_score = calculate_anode_score_with_range(
                current_peaks, &model_bl.avg_dvdq_curve, cycle_index, anode_range, config);
            let electrolyte_score = calculate_electrolyte_score(current_curve, &model_bl.avg_dvdq_curve, config);
            let sei_score = calculate_sei_score_with_range(
                current_peaks, cycle_index, sei_range, config);

            (cathode_score, anode_score, electrolyte_score, sei_score)
        }
    }
}

fn calculate_cathode_score_with_range(
    peaks: &[(f64, f64)],
    baseline: &[DvDqPoint],
    cycle_index: u16,
    range: (f64, f64),
    _config: &ClassifierConfig,
) -> f64 {
    let (low, high) = range;

    let cathode_peaks: Vec<&(f64, f64)> = peaks
        .iter()
        .filter(|(v, _)| *v >= low && *v <= high)
        .collect();

    if cathode_peaks.is_empty() {
        return 0.3;
    }

    let baseline_cathode_peaks: Vec<&DvDqPoint> = baseline
        .iter()
        .filter(|p| p.voltage >= low && p.voltage <= high)
        .collect();

    if baseline_cathode_peaks.is_empty() {
        return 0.5;
    }

    let baseline_avg_height: f64 = baseline_cathode_peaks.iter().map(|p| p.dq_dv).sum::<f64>()
        / baseline_cathode_peaks.len() as f64;

    let current_avg_height: f64 = cathode_peaks.iter().map(|(_, h)| *h).sum::<f64>()
        / cathode_peaks.len() as f64;

    let height_ratio = if baseline_avg_height > 0.0 {
        current_avg_height / baseline_avg_height
    } else {
        1.0
    };

    let cycle_factor = (cycle_index as f64 / 100.0).min(1.0);
    let attenuation_factor = (1.0 - height_ratio).abs();

    1.0 - (attenuation_factor * cycle_factor).max(0.2)
}

fn calculate_anode_score_with_range(
    peaks: &[(f64, f64)],
    baseline: &[DvDqPoint],
    cycle_index: u16,
    range: (f64, f64),
    _config: &ClassifierConfig,
) -> f64 {
    let (low, high) = range;
    let anode_peaks: Vec<&(f64, f64)> = peaks
        .iter()
        .filter(|(v, _)| *v >= low && *v <= high)
        .collect();

    if anode_peaks.is_empty() {
        return 0.3;
    }

    let baseline_anode_peaks: Vec<&DvDqPoint> = baseline
        .iter()
        .filter(|p| p.voltage >= low && p.voltage <= high)
        .collect();

    if baseline_anode_peaks.is_empty() {
        return 0.5;
    }

    let baseline_avg_height: f64 = baseline_anode_peaks.iter().map(|p| p.dq_dv).sum::<f64>()
        / baseline_anode_peaks.len() as f64;

    let current_avg_height: f64 = anode_peaks.iter().map(|(_, h)| *h).sum::<f64>()
        / anode_peaks.len() as f64;

    let height_ratio = if baseline_avg_height > 0.0 {
        current_avg_height / baseline_avg_height
    } else {
        1.0
    };

    let cycle_factor = (cycle_index as f64 / 100.0).min(1.0);
    let attenuation_factor = (1.0 - height_ratio).abs();

    1.0 - (attenuation_factor * cycle_factor).max(0.2)
}

fn calculate_electrolyte_score(
    current_curve: &[DvDqPoint],
    baseline_curve: &[DvDqPoint],
    _config: &ClassifierConfig,
) -> f64 {
    if current_curve.len() < 10 || baseline_curve.len() < 10 {
        return 0.5;
    }

    let current_total_area: f64 = current_curve
        .windows(2)
        .map(|w| {
            let dv = w[1].voltage - w[0].voltage;
            let avg_h = (w[0].dq_dv + w[1].dq_dv) / 2.0;
            dv * avg_h
        })
        .sum();

    let baseline_total_area: f64 = baseline_curve
        .windows(2)
        .map(|w| {
            let dv = w[1].voltage - w[0].voltage;
            let avg_h = (w[0].dq_dv + w[1].dq_dv) / 2.0;
            dv * avg_h
        })
        .sum();

    if baseline_total_area > 0.0 {
        let ratio = current_total_area / baseline_total_area;
        (0.5 + (ratio - 1.0).abs() * 0.5).min(0.95)
    } else {
        0.5
    }
}

fn calculate_sei_score_with_range(
    peaks: &[(f64, f64)],
    cycle_index: u16,
    range: (f64, f64),
    _config: &ClassifierConfig,
) -> f64 {
    let (low, high) = range;
    let sei_peaks: Vec<&(f64, f64)> = peaks
        .iter()
        .filter(|(v, _)| *v >= low && *v <= high)
        .collect();

    if sei_peaks.is_empty() || cycle_index < 10 {
        return 0.2;
    }

    let max_height: f64 = sei_peaks.iter().map(|(_, h)| *h).fold(f64::NEG_INFINITY, f64::max);
    let cycle_factor = (cycle_index as f64 / 50.0).min(1.0);
    let growth_factor = (max_height - 0.5).max(0.0) * cycle_factor;

    (0.5 + growth_factor).min(0.95)
}

fn find_similar_baseline(
    peak_positions: &[f64],
    peak_heights: &[f64],
    model_baselines: &HashMap<String, ModelBaseline>,
    config: &ClassifierConfig,
) -> Option<&ModelBaseline> {
    if model_baselines.is_empty() {
        return None;
    }

    let mut best_match: Option<&ModelBaseline> = None;
    let mut best_similarity = 0.0;

    for baseline in model_baselines.values() {
        let similarity = calculate_peak_similarity(peak_positions, peak_heights, &baseline.peak_positions);

        if similarity > best_similarity && similarity >= config.max_transfer_distance {
            best_similarity = similarity;
            best_match = Some(baseline);
        }
    }

    best_match
}

fn calculate_peak_similarity(
    positions1: &[f64],
    heights1: &[f64],
    positions2: &[f64],
) -> f64 {
    if positions1.is_empty() || positions2.is_empty() {
        return 0.0;
    }

    let mut matches = 0;
    for &p1 in positions1 {
        for &p2 in positions2 {
            if (p1 - p2).abs() < 0.2 {
                matches += 1;
                break;
            }
        }
    }

    let position_score = matches as f64 / positions1.len() as f64;

    let avg_h1: f64 = heights1.iter().sum::<f64>() / heights1.len() as f64;
    let avg_h2: f64 = if !positions2.is_empty() {
        0.5
    } else {
        0.5
    };

    let height_score = 1.0 - (avg_h1 - avg_h2).abs().min(1.0);

    position_score * 0.7 + height_score * 0.3
}

fn calculate_model_similarity(
    peaks: &[(f64, f64)],
    baseline: &ModelBaseline,
) -> f64 {
    let peak_positions: Vec<f64> = peaks.iter().map(|(v, _)| *v).collect();
    let peak_heights: Vec<f64> = peaks.iter().map(|(_, h)| *h).collect();
    calculate_peak_similarity(&peak_positions, &peak_heights, &baseline.peak_positions)
}

fn update_model_baseline(
    model_name: &str,
    dvdq_curve: &[DvDqPoint],
    peaks: &[(f64, f64)],
    model_baselines: &mut HashMap<String, ModelBaseline>,
    config: &ClassifierConfig,
) {
    let peak_positions: Vec<f64> = peaks.iter().map(|(v, _)| *v).collect();

    let entry = model_baselines
        .entry(model_name.to_string())
        .or_insert_with(|| ModelBaseline {
            model_name: model_name.to_string(),
            avg_dvdq_curve: Vec::new(),
            peak_positions: Vec::new(),
            sample_count: 0,
            cathode_peak_range: config.cathode_peak_range,
            anode_peak_range: config.anode_peak_range,
            sei_peak_range: config.sei_peak_range,
            model_features: Vec::new(),
        });

    entry.sample_count += 1;
    entry.peak_positions = if entry.peak_positions.is_empty() {
        peak_positions.clone()
    } else {
        let mut combined = entry.peak_positions.clone();
        combined.extend(peak_positions.iter().cloned());
        combined.sort_by(|a, b| a.partial_cmp(b).unwrap());
        combined.dedup_by(|a, b| (*a - *b).abs() < 0.1);
        combined
    };

    if entry.sample_count >= config.min_baseline_samples {
        if let Some(max_peak) = entry.peak_positions.iter().cloned().fold(f64::NEG_INFINITY, f64::max) {
            if max_peak.is_finite() {
                entry.cathode_peak_range = (
                    (max_peak - 0.2).max(3.6),
                    (max_peak + 0.2).min(4.2),
                );
            }
        }
    }
}

fn generate_recommendations_with_context(
    mode: DegradationMode,
    confidence: f64,
    fade_rate: f64,
    is_new_model: bool,
    used_transfer: bool,
    config: &ClassifierConfig,
) -> String {
    let mut recs = generate_recommendations(mode, confidence, fade_rate, config);

    if is_new_model {
        recs.push_str("\n⚠️ 新电池型号，基线数据不足，建议加强监测");
    }
    if used_transfer {
        recs.push_str("\n🔄 使用迁移学习基线，请关注结果准确性");
    }
    if confidence < config.require_manual_confirmation_threshold {
        recs.push_str("\n👤 建议人工审核确认分类结果");
    }

    recs
}

fn generate_recommendations(
    mode: DegradationMode,
    confidence: f64,
    fade_rate: f64,
    config: &ClassifierConfig,
) -> String {
    let mut recs = String::new();

    match mode {
        DegradationMode::Normal => {
            recs.push_str("✅ 电池状态正常，继续常规监测");
            if confidence < 0.7 {
                recs.push_str("（置信度较低，建议关注后续循环）");
            }
        }
        DegradationMode::CathodeDegradation => {
            recs.push_str("🔴 正极衰减：建议检查充电上限电压，考虑降低截止电压");
            if fade_rate > config.fading_rate_threshold {
                recs.push_str("，容量衰减速率较高，建议缩短化成时间");
            }
        }
        DegradationMode::AnodeDegradation => {
            recs.push_str("🔴 负极衰减：建议优化SEI形成工艺，提高预锂化程度");
            if fade_rate > config.fading_rate_threshold {
                recs.push_str("，容量衰减速率较高，建议增加化成时间");
            }
        }
        DegradationMode::ElectrolyteDepletion => {
            recs.push_str("🔴 电解液消耗：建议增加注液量，优化电解液浸润工艺");
        }
        DegradationMode::SEIGrowth => {
            recs.push_str("🟡 SEI膜过度生长：建议优化化成温度曲线，调整预充电流");
            if confidence < 0.7 {
                recs.push_str("（置信度较低，建议结合拆解分析确认）");
            }
        }
        DegradationMode::Mixed => {
            recs.push_str("🟠 混合衰减模式：存在多种衰减机制，建议综合优化工艺参数");
            if fade_rate > config.fading_rate_threshold {
                recs.push_str("，整体衰减速率较高，建议全面评估");
            }
        }
        DegradationMode::Unknown => {
            recs.push_str("❓ 衰减模式不明确，建议积累更多循环数据后重新分析");
        }
    }

    recs
}

fn calculate_fade_rate(
    historical_capacities: &[(u16, f64)],
    _config: &ClassifierConfig,
) -> f64 {
    if historical_capacities.len() < 2 {
        return 0.0;
    }

    let first_cap = historical_capacities.first().map(|(_, c)| *c).unwrap_or(0.0);
    let last_cap = historical_capacities.last().map(|(_, c)| *c).unwrap_or(0.0);
    let first_cycle = historical_capacities.first().map(|(c, _)| *c).unwrap_or(0);
    let last_cycle = historical_capacities.last().map(|(c, _)| *c).unwrap_or(0);

    if first_cap <= 0.0 || last_cycle <= first_cycle {
        return 0.0;
    }

    let capacity_loss = (first_cap - last_cap) / first_cap;
    let cycle_diff = (last_cycle - first_cycle) as f64;

    if cycle_diff > 0.0 {
        capacity_loss / cycle_diff * 100.0
    } else {
        0.0
    }
}

fn calculate_resistance_growth_rate(
    historical_resistances: &[(u16, f64)],
    _config: &ClassifierConfig,
) -> f64 {
    if historical_resistances.len() < 2 {
        return 0.0;
    }

    let first_res = historical_resistances.first().map(|(_, r)| *r).unwrap_or(0.0);
    let last_res = historical_resistances.last().map(|(_, r)| *r).unwrap_or(0.0);
    let first_cycle = historical_resistances.first().map(|(c, _)| *c).unwrap_or(0);
    let last_cycle = historical_resistances.last().map(|(c, _)| *c).unwrap_or(0);

    if first_res <= 0.0 || last_cycle <= first_cycle {
        return 0.0;
    }

    let resistance_growth = (last_res - first_res) / first_res;
    let cycle_diff = (last_cycle - first_cycle) as f64;

    if cycle_diff > 0.0 {
        resistance_growth / cycle_diff * 100.0
    } else {
        0.0
    }
}

fn classify_degradation_mode(
    cathode_score: f64,
    anode_score: f64,
    electrolyte_score: f64,
    sei_score: f64,
    fade_rate: f64,
    resistance_growth: f64,
    config: &ClassifierConfig,
) -> (DegradationMode, f64) {
    let threshold = 0.7;
    let high_fade = fade_rate > config.fading_rate_threshold;
    let high_resistance = resistance_growth > config.resistance_growth_threshold;

    let mut scores = vec![
        (DegradationMode::CathodeDegradation, cathode_score),
        (DegradationMode::AnodeDegradation, anode_score),
        (DegradationMode::ElectrolyteDepletion, electrolyte_score),
        (DegradationMode::SEIGrowth, sei_score),
    ];

    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    let top_score = scores[0].1;
    let second_score = scores[1].1;

    let significant_diff = (top_score - second_score) > 0.1;
    let top_above_threshold = top_score >= threshold;
    let multiple_high = scores.iter().filter(|(_, s)| *s >= threshold).count() >= 2;

    if top_score < 0.5 {
        (DegradationMode::Normal, 0.5)
    } else if multiple_high && !significant_diff {
        let confidence = (top_score + second_score) / 2.0;
        (DegradationMode::Mixed, confidence.min(0.85))
    } else if top_above_threshold && significant_diff {
        let confidence = top_score;
        let mode = if high_fade && high_resistance && top_score < 0.85 {
            DegradationMode::Mixed
        } else {
            scores[0].0
        };
        (mode, confidence.min(0.95))
    } else if top_score >= 0.55 {
        (scores[0].0, top_score.min(0.7))
    } else {
        (DegradationMode::Normal, 0.55)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ChannelData, Stage};
    use rand::Rng;

    fn generate_discharge_data(capacity: f64, num_points: usize) -> Vec<ChannelData> {
        let mut rng = rand::thread_rng();
        let mut data = Vec::new();

        for i in 0..num_points {
            let voltage = 4.2 - (i as f64 / num_points as f64) * 3.0;
            let cap = capacity * (1.0 - i as f64 / num_points as f64);
            data.push(ChannelData {
                timestamp: Utc::now(),
                cabinet_id: 0,
                channel_id: 0,
                cycle_index: 1,
                stage: Stage::Discharge,
                step_index: i as u32,
                voltage,
                current: -1.0,
                capacity: cap,
                temperature: 25.0,
                internal_resistance: 20.0 + rng.gen_range(-1.0..1.0),
            });
        }

        data
    }

    fn generate_discharge_data_with_peaks(
        cycle: u16,
        cathode_attenuation: f64,
        anode_attenuation: f64,
    ) -> Vec<ChannelData> {
        let mut rng = rand::thread_rng();
        let mut data = Vec::new();
        let num_points = 100;

        for i in 0..num_points {
            let voltage = 4.2 - (i as f64 / num_points as f64) * 3.95;
            let mut dq_dv_base = 0.1;

            if voltage >= 3.8 && voltage <= 4.2 {
                let center = 3.95;
                let width = 0.08;
                let peak = 0.3 * (-(voltage - center).powi(2) / (2.0 * width.powi(2))).exp()
                    * (1.0 - cathode_attenuation * (cycle as f64 / 100.0).min(1.0));
                dq_dv_base += peak;
            }

            if voltage >= 0.05 && voltage <= 0.3 {
                let center = 0.15;
                let width = 0.03;
                let peak = 0.4 * (-(voltage - center).powi(2) / (2.0 * width.powi(2))).exp()
                    * (1.0 - anode_attenuation * (cycle as f64 / 100.0).min(1.0));
                dq_dv_base += peak;
            }

            let dq = dq_dv_base * (3.95 / num_points as f64);
            let cap = 3.0 - (i as f64 / num_points as f64) * 3.0 - dq;

            data.push(ChannelData {
                timestamp: Utc::now(),
                cabinet_id: 0,
                channel_id: 0,
                cycle_index: cycle,
                stage: Stage::Discharge,
                step_index: i as u32,
                voltage,
                current: -1.0,
                capacity: cap.max(0.0),
                temperature: 25.0,
                internal_resistance: 20.0 + rng.gen_range(-1.0..1.0),
            });
        }

        data
    }

    #[tokio::test]
    async fn test_async_analyze_request() {
        let config = ClassifierConfig::default();
        let (service, handle) = AgingClassifierService::new(config);
        tokio::spawn(service.run());

        let discharge_data = generate_discharge_data(3.0, 100);
        let historical_caps = vec![(1, 3.0), (50, 2.85)];
        let historical_res = vec![(1, 20.0), (50, 22.0)];

        let request = AnalyzeRequest {
            request_id: "test-001".to_string(),
            cabinet_id: 0,
            channel_id: 0,
            cycle_index: 1,
            discharge_data,
            historical_capacities: historical_caps,
            historical_resistances: historical_res,
            battery_model: Some("NCM523".to_string()),
            respond_to: None,
        };

        let rx = handle.analyze(request).await.unwrap();
        let result = rx.await.unwrap();

        assert_eq!(result.request_id, "test-001");
        assert!(result.analysis.confidence > 0.0);
        assert!(!result.analysis.peak_positions.is_empty());
    }

    #[tokio::test]
    async fn test_async_manual_label() {
        let config = ClassifierConfig::default();
        let (service, handle) = AgingClassifierService::new(config);
        tokio::spawn(service.run());

        let label_request = AddLabelRequest {
            request_id: "label-001".to_string(),
            cabinet_id: 0,
            channel_id: 1,
            cycle_index: 50,
            corrected_mode: DegradationMode::CathodeDegradation,
            notes: "经拆解确认".to_string(),
            operator: "王工".to_string(),
            respond_to: None,
        };

        let rx = handle.add_manual_label(label_request).await.unwrap();
        let result = rx.await.unwrap();

        assert!(result);

        let pending_rx = handle.get_pending_confirmations().await.unwrap();
        let pending = pending_rx.await.unwrap();
        assert!(!pending.contains(&(0, 1, 50)));
    }

    #[tokio::test]
    async fn test_transfer_learning_new_model() {
        let config = ClassifierConfig {
            enable_transfer_learning: true,
            ..ClassifierConfig::default()
        };
        let (service, handle) = AgingClassifierService::new(config);

        let baseline_req = RegisterBaselineRequest {
            request_id: "reg-001".to_string(),
            model_name: "NCM523".to_string(),
            baseline: ModelBaseline {
                model_name: "NCM523".to_string(),
                avg_dvdq_curve: Vec::new(),
                peak_positions: vec![3.95, 0.15],
                sample_count: 50,
                cathode_peak_range: (3.8, 4.2),
                anode_peak_range: (0.05, 0.3),
                sei_peak_range: (0.5, 1.5),
                model_features: Vec::new(),
            },
            respond_to: None,
        };

        let rx = handle.register_model_baseline(baseline_req).await.unwrap();
        assert!(rx.await.unwrap());

        tokio::spawn(service.run());

        let discharge_data = generate_discharge_data_with_peaks(30, 0.3, 0.0);
        let request = AnalyzeRequest {
            request_id: "transfer-001".to_string(),
            cabinet_id: 0,
            channel_id: 0,
            cycle_index: 30,
            discharge_data,
            historical_capacities: vec![(1, 3.0), (30, 2.75)],
            historical_resistances: vec![(1, 20.0), (30, 23.0)],
            battery_model: Some("NCM622".to_string()),
            respond_to: None,
        };

        let rx = handle.analyze(request).await.unwrap();
        let result = rx.await.unwrap();

        assert!(result.analysis.is_new_model);
        assert!(result.analysis.used_transfer_learning);
        assert_eq!(result.analysis.transfer_source_model, Some("NCM523".to_string()));
        assert!(result.analysis.requires_manual_confirmation);
    }

    #[tokio::test]
    async fn test_concurrent_analyze_requests() {
        let config = ClassifierConfig::default();
        let (service, handle) = AgingClassifierService::new(config);
        tokio::spawn(service.run());

        let discharge_data1 = generate_discharge_data(3.0, 50);
        let discharge_data2 = generate_discharge_data(3.1, 50);

        let request1 = AnalyzeRequest {
            request_id: "conc-1".to_string(),
            cabinet_id: 0,
            channel_id: 0,
            cycle_index: 1,
            discharge_data: discharge_data1,
            historical_capacities: vec![(1, 3.0)],
            historical_resistances: vec![(1, 20.0)],
            battery_model: None,
            respond_to: None,
        };

        let request2 = AnalyzeRequest {
            request_id: "conc-2".to_string(),
            cabinet_id: 0,
            channel_id: 1,
            cycle_index: 1,
            discharge_data: discharge_data2,
            historical_capacities: vec![(1, 3.1)],
            historical_resistances: vec![(1, 21.0)],
            battery_model: None,
            respond_to: None,
        };

        let rx1 = handle.analyze(request1).await.unwrap();
        let rx2 = handle.analyze(request2).await.unwrap();

        let (result1, result2) = tokio::join!(rx1, rx2);

        assert!(result1.is_ok());
        assert!(result2.is_ok());
        assert_eq!(result1.unwrap().request_id, "conc-1");
        assert_eq!(result2.unwrap().request_id, "conc-2");
    }

    #[test]
    fn test_dqdv_analysis_accuracy_sync() {
        let config = ClassifierConfig::default();
        let state = AnalyzeStateSnapshot {
            baseline_data: HashMap::new(),
            historical_analysis: HashMap::new(),
            model_baselines: HashMap::new(),
            manual_labels: HashMap::new(),
        };

        let discharge_data = generate_discharge_data_with_peaks(50, 0.0, 0.0);
        let baseline_curve = calculate_dvdq_curve(&discharge_data, &config);

        let mut state2 = state.clone();
        state2.baseline_data.insert((0, 0), baseline_curve);

        let aged_data = generate_discharge_data_with_peaks(50, 0.5, 0.0);

        let request = AnalyzeRequest {
            request_id: "sync-001".to_string(),
            cabinet_id: 0,
            channel_id: 0,
            cycle_index: 50,
            discharge_data: aged_data,
            historical_capacities: vec![(1, 3.0), (50, 2.6)],
            historical_resistances: vec![(1, 20.0), (50, 24.0)],
            battery_model: None,
            respond_to: None,
        };

        let ((analysis, _), _) = process_analyze_sync(request, config, state2);

        assert_eq!(analysis.mode, DegradationMode::CathodeDegradation);
        assert!(analysis.cathode_score < analysis.anode_score);
        assert!(analysis.capacity_fade_rate > 0.005);
        assert!(analysis.confidence >= 0.6);
    }

    #[test]
    fn test_peak_similarity_matching_sync() {
        let config = ClassifierConfig::default();

        let baseline_peaks = vec![3.95, 0.15];
        let new_peaks = vec![3.93, 0.14];
        let new_heights = vec![0.3, 0.4];

        let similarity = calculate_peak_similarity(&new_peaks, &new_heights, &baseline_peaks);
        assert!(similarity > 0.5, "Similar peaks should have high similarity, got {}", similarity);

        let different_peaks = vec![3.5, 0.5];
        let similarity2 = calculate_peak_similarity(&different_peaks, &new_heights, &baseline_peaks);
        assert!(similarity2 < 0.3, "Different peaks should have low similarity, got {}", similarity2);
    }
}
