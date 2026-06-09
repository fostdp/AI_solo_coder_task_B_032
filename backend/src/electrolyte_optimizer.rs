use crate::models::{ElectrolyteInjection, GasGenerationData, InjectionOptimizationResult, InjectionStatus};
use chrono::Utc;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct OptimizerConfig {
    pub nominal_injection_volume: f64,
    pub min_injection_volume: f64,
    pub max_injection_volume: f64,
    pub hard_min_injection_volume: f64,
    pub hard_max_injection_volume: f64,
    pub target_gas_volume: f64,
    pub max_gas_volume: f64,
    pub min_gas_volume: f64,
    pub gas_to_electrolyte_ratio: f64,
    pub learning_rate: f64,
    pub history_window_size: usize,
    pub min_pressure_data_coverage: f64,
    pub manual_confirmation_confidence_threshold: f64,
    pub max_adjustment_per_batch: f64,
    pub enable_fallback_to_nominal: bool,
    pub channel_buffer: usize,
}

impl Default for OptimizerConfig {
    fn default() -> Self {
        Self {
            nominal_injection_volume: 120.0,
            min_injection_volume: 100.0,
            max_injection_volume: 140.0,
            hard_min_injection_volume: 90.0,
            hard_max_injection_volume: 150.0,
            target_gas_volume: 50.0,
            max_gas_volume: 80.0,
            min_gas_volume: 20.0,
            gas_to_electrolyte_ratio: 0.8,
            learning_rate: 0.3,
            history_window_size: 100,
            min_pressure_data_coverage: 0.7,
            manual_confirmation_confidence_threshold: 0.6,
            max_adjustment_per_batch: 10.0,
            enable_fallback_to_nominal: true,
            channel_buffer: 100,
        }
    }
}

#[derive(Debug, Clone)]
pub struct OptimizeRequest {
    pub request_id: String,
    pub batch_id: String,
    pub gas_data: Vec<GasGenerationData>,
    pub respond_to: Option<oneshot::Sender<OptimizeResult>>,
}

#[derive(Debug)]
pub struct OptimizeResult {
    pub request_id: String,
    pub result: InjectionOptimizationResult,
    pub channel_suggestions: Vec<ElectrolyteInjection>,
}

#[derive(Debug, Clone)]
pub struct ConfirmRequest {
    pub request_id: String,
    pub injection_id: String,
    pub confirmed_volume: f64,
    pub notes: Option<String>,
    pub operator: String,
    pub respond_to: Option<oneshot::Sender<ConfirmResult>>,
}

#[derive(Debug)]
pub struct ConfirmResult {
    pub request_id: String,
    pub success: bool,
    pub updated_injection: Option<ElectrolyteInjection>,
    pub message: Option<String>,
}

pub enum OptimizerMessage {
    ProcessGas(GasGenerationData),
    OptimizeBatch(OptimizeRequest),
    ConfirmInjection(ConfirmRequest),
    GetPendingConfirmations {
        batch_id: String,
        respond_to: oneshot::Sender<Vec<ElectrolyteInjection>>,
    },
    UpdateConfig(OptimizerConfig),
    Shutdown,
}

impl fmt::Debug for OptimizerMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OptimizerMessage::ProcessGas(_) => write!(f, "ProcessGas"),
            OptimizerMessage::OptimizeBatch(req) => write!(f, "OptimizeBatch({})", req.request_id),
            OptimizerMessage::ConfirmInjection(req) => write!(f, "ConfirmInjection({})", req.request_id),
            OptimizerMessage::GetPendingConfirmations { batch_id, .. } => {
                write!(f, "GetPendingConfirmations({})", batch_id)
            }
            OptimizerMessage::UpdateConfig(_) => write!(f, "UpdateConfig"),
            OptimizerMessage::Shutdown => write!(f, "Shutdown"),
        }
    }
}

pub type OptimizerSender = mpsc::Sender<OptimizerMessage>;
pub type OptimizerReceiver = mpsc::Receiver<OptimizerMessage>;

#[derive(Clone)]
pub struct ElectrolyteOptimizerHandle {
    sender: OptimizerSender,
    config: Arc<Mutex<OptimizerConfig>>,
}

impl ElectrolyteOptimizerHandle {
    pub fn new(sender: OptimizerSender, config: OptimizerConfig) -> Self {
        Self {
            sender,
            config: Arc::new(Mutex::new(config)),
        }
    }

    pub async fn process_gas_data(&self, gas_data: GasGenerationData) -> Result<(), String> {
        self.sender
            .send(OptimizerMessage::ProcessGas(gas_data))
            .await
            .map_err(|e| format!("Failed to send gas data: {}", e))
    }

    pub async fn optimize_batch(
        &self,
        request: OptimizeRequest,
    ) -> Result<oneshot::Receiver<OptimizeResult>, String> {
        let (tx, rx) = oneshot::channel();
        let message = OptimizerMessage::OptimizeBatch(OptimizeRequest {
            respond_to: Some(tx),
            ..request
        });

        self.sender
            .send(message)
            .await
            .map_err(|e| format!("Failed to send optimize request: {}", e))?;

        Ok(rx)
    }

    pub async fn confirm_injection(
        &self,
        request: ConfirmRequest,
    ) -> Result<oneshot::Receiver<ConfirmResult>, String> {
        let (tx, rx) = oneshot::channel();
        let message = OptimizerMessage::ConfirmInjection(ConfirmRequest {
            respond_to: Some(tx),
            ..request
        });

        self.sender
            .send(message)
            .await
            .map_err(|e| format!("Failed to send confirm request: {}", e))?;

        Ok(rx)
    }

    pub async fn get_pending_confirmations(
        &self,
        batch_id: String,
    ) -> Result<oneshot::Receiver<Vec<ElectrolyteInjection>>, String> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(OptimizerMessage::GetPendingConfirmations { batch_id, respond_to: tx })
            .await
            .map_err(|e| format!("Failed to send pending confirmations request: {}", e))?;

        Ok(rx)
    }

    pub async fn update_config(&self, config: OptimizerConfig) -> Result<(), String> {
        *self.config.lock().await = config.clone();
        self.sender
            .send(OptimizerMessage::UpdateConfig(config))
            .await
            .map_err(|e| format!("Failed to send config update: {}", e))
    }

    pub async fn get_config(&self) -> OptimizerConfig {
        self.config.lock().await.clone()
    }

    pub async fn shutdown(&self) -> Result<(), String> {
        self.sender
            .send(OptimizerMessage::Shutdown)
            .await
            .map_err(|e| format!("Failed to send shutdown: {}", e))
    }
}

pub struct ElectrolyteOptimizerService {
    config: OptimizerConfig,
    receiver: OptimizerReceiver,
    history_data: Vec<(f64, f64)>,
    pending_injections: HashMap<String, ElectrolyteInjection>,
    confirmed_injections: HashMap<String, ElectrolyteInjection>,
    active_requests: usize,
}

impl ElectrolyteOptimizerService {
    pub fn new(config: OptimizerConfig) -> (Self, ElectrolyteOptimizerHandle) {
        let (sender, receiver) = mpsc::channel(config.channel_buffer);
        let handle = ElectrolyteOptimizerHandle::new(sender, config.clone());
        (
            Self {
                config,
                receiver,
                history_data: Vec::new(),
                pending_injections: HashMap::new(),
                confirmed_injections: HashMap::new(),
                active_requests: 0,
            },
            handle,
        )
    }

    pub async fn run(mut self) {
        tracing::info!("ElectrolyteOptimizerService started, waiting for requests");

        while let Some(message) = self.receiver.recv().await {
            match message {
                OptimizerMessage::ProcessGas(gas_data) => {
                    self.process_single_gas_data(gas_data);
                }
                OptimizerMessage::OptimizeBatch(request) => {
                    self.active_requests += 1;
                    tracing::debug!("Processing batch optimization: {}", request.request_id);

                    let config = self.config.clone();
                    let history_data = self.history_data.clone();
                    let request_id = request.request_id.clone();
                    let respond_to = request.respond_to;

                    let result = tokio::task::spawn_blocking(move || {
                        process_optimize_batch_sync(request, &config, &history_data)
                    })
                    .await;

                    let optimize_result = match result {
                        Ok((result, channel_suggestions)) => OptimizeResult {
                            request_id: request_id.clone(),
                            result,
                            channel_suggestions,
                        },
                        Err(e) => {
                            tracing::error!("Optimize task panicked: {}", e);
                            OptimizeResult {
                                request_id: request_id.clone(),
                                result: InjectionOptimizationResult {
                                    batch_id: request_id,
                                    total_channels: 0,
                                    avg_nominal_volume: config.nominal_injection_volume,
                                    avg_suggested_volume: config.nominal_injection_volume,
                                    avg_adjustment: 0.0,
                                    over_injected_count: 0,
                                    under_injected_count: 0,
                                    estimated_gas_reduction: 0.0,
                                    estimated_capacity_improvement: 0.0,
                                    next_batch_suggestion: config.nominal_injection_volume,
                                    channels_with_missing_data: 0,
                                    channels_requiring_confirmation: 0,
                                    used_fallback_strategy: false,
                                    avg_data_completeness: 1.0,
                                    hard_limits_applied_count: 0,
                                    fallback_explanation: String::new(),
                                },
                                channel_suggestions: Vec::new(),
                            }
                        }
                    };

                    for injection in &optimize_result.channel_suggestions {
                        if injection.requires_manual_confirmation {
                            self.pending_injections
                                .insert(injection.injection_id.clone(), injection.clone());
                        }
                    }

                    if let Some(tx) = respond_to {
                        let _ = tx.send(optimize_result);
                    }

                    self.active_requests -= 1;
                }
                OptimizerMessage::ConfirmInjection(request) => {
                    let result = self.process_confirm_injection(&request);

                    if let Some(tx) = request.respond_to {
                        let _ = tx.send(ConfirmResult {
                            request_id: request.request_id,
                            success: result.0,
                            updated_injection: result.1,
                            message: result.2,
                        });
                    }
                }
                OptimizerMessage::GetPendingConfirmations { batch_id, respond_to } => {
                    let pending: Vec<ElectrolyteInjection> = self
                        .pending_injections
                        .values()
                        .filter(|inj| inj.batch_id == batch_id)
                        .cloned()
                        .collect();
                    let _ = respond_to.send(pending);
                }
                OptimizerMessage::UpdateConfig(new_config) => {
                    self.config = new_config;
                    tracing::info!("Optimizer config updated");
                }
                OptimizerMessage::Shutdown => {
                    tracing::info!("ElectrolyteOptimizerService shutting down");
                    break;
                }
            }
        }

        tracing::info!("ElectrolyteOptimizerService stopped");
    }

    fn process_single_gas_data(&mut self, gas_data: GasGenerationData) -> Option<ElectrolyteInjection> {
        if gas_data.stage != crate::models::Stage::Precharge
            && gas_data.stage != crate::models::Stage::CcCharge
        {
            return None;
        }

        let pressure_available = check_pressure_data_availability(&gas_data);
        let data_completeness = assess_data_completeness(&gas_data, &self.history_data);
        let mut used_fallback = false;
        let mut hard_limit_applied = false;

        let effective_gas_volume = if pressure_available
            && data_completeness >= self.config.min_pressure_data_coverage
        {
            gas_data.cumulative_gas
        } else {
            used_fallback = true;
            if self.config.enable_fallback_to_nominal {
                get_historical_avg_gas(&self.history_data, self.config.target_gas_volume)
            } else {
                return None;
            }
        };

        if effective_gas_volume < self.config.min_gas_volume * 0.5 {
            return None;
        }

        if !used_fallback {
            self.history_data
                .push((self.config.nominal_injection_volume, gas_data.cumulative_gas));
            if self.history_data.len() > self.config.history_window_size {
                self.history_data.remove(0);
            }
        }

        let mut suggested_volume =
            calculate_suggested_volume(&self.config, effective_gas_volume, &self.history_data);
        let adjustment = suggested_volume - self.config.nominal_injection_volume;

        if adjustment.abs() > self.config.max_adjustment_per_batch {
            suggested_volume = self.config.nominal_injection_volume
                + adjustment.signum() * self.config.max_adjustment_per_batch;
            hard_limit_applied = true;
        }

        if suggested_volume < self.config.hard_min_injection_volume {
            suggested_volume = self.config.hard_min_injection_volume;
            hard_limit_applied = true;
        }
        if suggested_volume > self.config.hard_max_injection_volume {
            suggested_volume = self.config.hard_max_injection_volume;
            hard_limit_applied = true;
        }

        if suggested_volume < self.config.min_injection_volume {
            suggested_volume = self.config.min_injection_volume;
            hard_limit_applied = true;
        }
        if suggested_volume > self.config.max_injection_volume {
            suggested_volume = self.config.max_injection_volume;
            hard_limit_applied = true;
        }

        let status = if effective_gas_volume > self.config.max_gas_volume {
            InjectionStatus::OverInjected
        } else if effective_gas_volume < self.config.min_gas_volume {
            InjectionStatus::UnderInjected
        } else {
            InjectionStatus::Normal
        };

        let mut confidence = calculate_confidence(
            effective_gas_volume,
            &self.history_data,
            self.config.history_window_size,
        );
        if used_fallback {
            confidence *= 0.5;
        }
        if data_completeness < self.config.min_pressure_data_coverage {
            confidence *= data_completeness;
        }

        let requires_manual_confirmation =
            confidence < self.config.manual_confirmation_confidence_threshold
                || used_fallback
                || hard_limit_applied;

        let injection_id = Uuid::new_v4().to_string();
        let batch_id = format!("BATCH_{}", Utc::now().format("%Y%m%d"));

        let injection = ElectrolyteInjection {
            date: Utc::now().date_naive(),
            batch_id,
            injection_id: injection_id.clone(),
            cabinet_id: gas_data.cabinet_id,
            channel_id: gas_data.channel_id,
            cycle_index: gas_data.cycle_index,
            nominal_volume: self.config.nominal_injection_volume,
            actual_volume: self.config.nominal_injection_volume,
            gas_volume: effective_gas_volume,
            suggested_volume,
            adjustment: suggested_volume - self.config.nominal_injection_volume,
            status,
            confidence,
            requires_manual_confirmation,
            used_fallback,
            data_completeness,
            hard_limit_applied,
            pressure_data_available: pressure_available,
            confirmation_notes: None,
            confirmed_by: None,
            confirmed_at: None,
        };

        if requires_manual_confirmation {
            self.pending_injections
                .insert(injection_id, injection.clone());
        }

        Some(injection)
    }

    fn process_confirm_injection(
        &mut self,
        request: &ConfirmRequest,
    ) -> (bool, Option<ElectrolyteInjection>, Option<String>) {
        if let Some(mut injection) = self.pending_injections.remove(&request.injection_id) {
            if request.confirmed_volume < self.config.hard_min_injection_volume
                || request.confirmed_volume > self.config.hard_max_injection_volume
            {
                self.pending_injections
                    .insert(request.injection_id.clone(), injection);
                return (
                    false,
                    None,
                    Some(format!(
                        "Confirmed volume {:.2} out of safety bounds [{:.2}, {:.2}]",
                        request.confirmed_volume,
                        self.config.hard_min_injection_volume,
                        self.config.hard_max_injection_volume
                    )),
                );
            }

            injection.actual_volume = request.confirmed_volume;
            injection.confirmed_by = Some(request.operator.clone());
            injection.confirmed_at = Some(Utc::now());
            injection.confirmation_notes = request.notes.clone();
            injection.requires_manual_confirmation = false;

            self.confirmed_injections
                .insert(injection.injection_id.clone(), injection.clone());

            (true, Some(injection), None)
        } else if let Some(injection) = self.confirmed_injections.get(&request.injection_id) {
            (false, Some(injection.clone()), Some("Injection already confirmed".to_string()))
        } else {
            (false, None, Some("Injection not found".to_string()))
        }
    }

    pub fn active_requests(&self) -> usize {
        self.active_requests
    }

    pub fn pending_confirmation_count(&self) -> usize {
        self.pending_injections.len()
    }
}

fn check_pressure_data_availability(gas_data: &GasGenerationData) -> bool {
    if gas_data.pressure <= 0.0 || gas_data.pressure > 200.0 {
        return false;
    }
    if gas_data.temperature <= -50.0 || gas_data.temperature > 150.0 {
        return false;
    }
    if gas_data.cumulative_gas < 0.0 || gas_data.gas_generation_rate < 0.0 {
        return false;
    }
    true
}

fn assess_data_completeness(gas_data: &GasGenerationData, history_data: &[(f64, f64)]) -> f64 {
    let mut score = 1.0;

    if gas_data.pressure <= 0.0 {
        score -= 0.4;
    }
    if gas_data.temperature <= -50.0 {
        score -= 0.2;
    }
    if gas_data.cumulative_gas <= 0.0 {
        score -= 0.3;
    }
    if gas_data.gas_generation_rate <= 0.0 {
        score -= 0.1;
    }

    if history_data.len() < 10 {
        score *= 0.8;
    }

    score.max(0.0).min(1.0)
}

fn get_historical_avg_gas(history_data: &[(f64, f64)], target_gas: f64) -> f64 {
    if history_data.is_empty() {
        return target_gas;
    }
    let sum: f64 = history_data.iter().map(|(_, gas)| *gas).sum();
    sum / history_data.len() as f64
}

fn calculate_suggested_volume(
    config: &OptimizerConfig,
    gas_volume: f64,
    history_data: &[(f64, f64)],
) -> f64 {
    let gas_deviation = gas_volume - config.target_gas_volume;
    let base_adjustment =
        -gas_deviation * config.gas_to_electrolyte_ratio * config.learning_rate;

    let trend_adjustment = if history_data.len() >= 10 {
        let recent_avg_gas: f64 = history_data
            .iter()
            .rev()
            .take(10)
            .map(|(_, g)| *g)
            .sum::<f64>()
            / 10.0;
        let trend = recent_avg_gas - config.target_gas_volume;
        -trend * config.gas_to_electrolyte_ratio * 0.1
    } else {
        0.0
    };

    let suggested = config.nominal_injection_volume + base_adjustment + trend_adjustment;

    suggested.clamp(config.min_injection_volume, config.max_injection_volume)
}

fn calculate_confidence(gas_volume: f64, history_data: &[(f64, f64)], history_window: usize) -> f64 {
    let data_sufficiency = (history_data.len() as f64 / history_window as f64).min(1.0);

    let gas_clarity = if gas_volume < 20.0 * 0.8 {
        0.6
    } else if gas_volume > 80.0 * 1.2 {
        0.9
    } else {
        0.75
    };

    let stability = if history_data.len() >= 20 {
        let recent_gas: Vec<f64> = history_data
            .iter()
            .rev()
            .take(20)
            .map(|(_, g)| *g)
            .collect();
        let mean = recent_gas.iter().sum::<f64>() / recent_gas.len() as f64;
        let variance: f64 = recent_gas
            .iter()
            .map(|g| (g - mean).powi(2))
            .sum::<f64>()
            / recent_gas.len() as f64;
        let std_dev = variance.sqrt();
        let cv = std_dev / mean;
        (1.0 - cv * 2.0).max(0.5)
    } else {
        0.6
    };

    (data_sufficiency * 0.4 + gas_clarity * 0.3 + stability * 0.3).clamp(0.5, 0.99)
}

fn calculate_next_batch_suggestion(config: &OptimizerConfig, avg_gas_volume: f64) -> f64 {
    let deviation_ratio = (avg_gas_volume - config.target_gas_volume) / config.target_gas_volume;

    let adjustment = if deviation_ratio > 0.1 {
        -config.nominal_injection_volume * 0.05
    } else if deviation_ratio < -0.1 {
        config.nominal_injection_volume * 0.03
    } else {
        -config.nominal_injection_volume * deviation_ratio * 0.3
    };

    let suggested = config.nominal_injection_volume + adjustment;
    suggested.clamp(config.min_injection_volume, config.max_injection_volume)
}

fn process_optimize_batch_sync(
    request: OptimizeRequest,
    config: &OptimizerConfig,
    history_data: &[(f64, f64)],
) -> (InjectionOptimizationResult, Vec<ElectrolyteInjection>) {
    let batch_gas_data = &request.gas_data;
    let batch_id = request.batch_id;

    if batch_gas_data.is_empty() {
        return (
            InjectionOptimizationResult {
                batch_id,
                total_channels: 0,
                avg_nominal_volume: config.nominal_injection_volume,
                avg_suggested_volume: config.nominal_injection_volume,
                avg_adjustment: 0.0,
                over_injected_count: 0,
                under_injected_count: 0,
                estimated_gas_reduction: 0.0,
                estimated_capacity_improvement: 0.0,
                next_batch_suggestion: config.nominal_injection_volume,
                channels_with_missing_data: 0,
                channels_requiring_confirmation: 0,
                used_fallback_strategy: false,
                avg_data_completeness: 1.0,
                hard_limits_applied_count: 0,
                fallback_explanation: String::new(),
            },
            Vec::new(),
        );
    }

    let total_channels = batch_gas_data.len();
    let mut valid_gas_values: Vec<f64> = Vec::new();
    let mut over_injected_count = 0;
    let mut under_injected_count = 0;
    let mut suggestions: Vec<f64> = Vec::new();
    let mut channels_with_missing_data = 0;
    let mut channels_requiring_confirmation = 0;
    let mut hard_limits_applied_count = 0;
    let mut completeness_scores: Vec<f64> = Vec::new();
    let mut channel_suggestions: Vec<ElectrolyteInjection> = Vec::new();

    for gas_data in batch_gas_data {
        let pressure_ok = check_pressure_data_availability(gas_data);
        let completeness = assess_data_completeness(gas_data, history_data);
        completeness_scores.push(completeness);

        if !pressure_ok || completeness < config.min_pressure_data_coverage {
            channels_with_missing_data += 1;
            channels_requiring_confirmation += 1;
        }

        let effective_gas = if pressure_ok && completeness >= config.min_pressure_data_coverage {
            gas_data.cumulative_gas
        } else {
            get_historical_avg_gas(history_data, config.target_gas_volume)
        };

        valid_gas_values.push(effective_gas);

        let mut suggested = calculate_suggested_volume(config, effective_gas, history_data);
        let adjustment = suggested - config.nominal_injection_volume;

        let mut hard_limit_applied = false;
        if adjustment.abs() > config.max_adjustment_per_batch {
            suggested = config.nominal_injection_volume
                + adjustment.signum() * config.max_adjustment_per_batch;
            hard_limit_applied = true;
            hard_limits_applied_count += 1;
        }
        if suggested < config.hard_min_injection_volume {
            suggested = config.hard_min_injection_volume;
            hard_limit_applied = true;
            hard_limits_applied_count += 1;
        }
        if suggested > config.hard_max_injection_volume {
            suggested = config.hard_max_injection_volume;
            hard_limit_applied = true;
            hard_limits_applied_count += 1;
        }

        suggestions.push(suggested);

        if effective_gas > config.max_gas_volume {
            over_injected_count += 1;
        } else if effective_gas < config.min_gas_volume {
            under_injected_count += 1;
        }

        let status = if effective_gas > config.max_gas_volume {
            InjectionStatus::OverInjected
        } else if effective_gas < config.min_gas_volume {
            InjectionStatus::UnderInjected
        } else {
            InjectionStatus::Normal
        };

        let mut confidence = calculate_confidence(effective_gas, history_data, config.history_window_size);
        if !pressure_ok || completeness < config.min_pressure_data_coverage {
            confidence *= 0.5;
        }

        let requires_manual =
            confidence < config.manual_confirmation_confidence_threshold || hard_limit_applied;

        if requires_manual {
            channels_requiring_confirmation += 1;
        }

        channel_suggestions.push(ElectrolyteInjection {
            date: Utc::now().date_naive(),
            batch_id: batch_id.clone(),
            injection_id: Uuid::new_v4().to_string(),
            cabinet_id: gas_data.cabinet_id,
            channel_id: gas_data.channel_id,
            cycle_index: gas_data.cycle_index,
            nominal_volume: config.nominal_injection_volume,
            actual_volume: config.nominal_injection_volume,
            gas_volume: effective_gas,
            suggested_volume: suggested,
            adjustment: suggested - config.nominal_injection_volume,
            status,
            confidence,
            requires_manual_confirmation: requires_manual,
            used_fallback: !pressure_ok || completeness < config.min_pressure_data_coverage,
            data_completeness: completeness,
            hard_limit_applied,
            pressure_data_available: pressure_ok,
            confirmation_notes: None,
            confirmed_by: None,
            confirmed_at: None,
        });
    }

    let avg_gas_volume = if valid_gas_values.is_empty() {
        config.target_gas_volume
    } else {
        valid_gas_values.iter().sum::<f64>() / valid_gas_values.len() as f64
    };

    let avg_suggested_volume = if suggestions.is_empty() {
        config.nominal_injection_volume
    } else {
        suggestions.iter().sum::<f64>() / suggestions.len() as f64
    };

    let avg_adjustment = avg_suggested_volume - config.nominal_injection_volume;

    let next_batch_suggestion = calculate_next_batch_suggestion(config, avg_gas_volume);

    let estimated_gas_reduction = if avg_gas_volume > config.target_gas_volume {
        (avg_gas_volume - config.target_gas_volume) * config.gas_to_electrolyte_ratio
    } else {
        0.0
    };

    let estimated_capacity_improvement = if avg_gas_volume > config.max_gas_volume * 0.8 {
        let over_ratio = (avg_gas_volume - config.target_gas_volume) / config.target_gas_volume;
        over_ratio * 0.05 * 100.0
    } else {
        0.0
    };

    let avg_data_completeness = if completeness_scores.is_empty() {
        1.0
    } else {
        completeness_scores.iter().sum::<f64>() / completeness_scores.len() as f64
    };

    let used_fallback_strategy = channels_with_missing_data > 0;
    let mut fallback_explanation = String::new();
    if used_fallback_strategy {
        fallback_explanation = format!(
            "{}个通道压力数据不足（覆盖率{:.1}%，阈值{:.0}%），已使用历史平均值作为fallback",
            channels_with_missing_data,
            avg_data_completeness * 100.0,
            config.min_pressure_data_coverage * 100.0
        );
    }

    if hard_limits_applied_count > 0 {
        if !fallback_explanation.is_empty() {
            fallback_explanation.push_str("；");
        }
        fallback_explanation.push_str(&format!(
            "{}个通道的注液量建议被安全限值约束",
            hard_limits_applied_count
        ));
    }

    (
        InjectionOptimizationResult {
            batch_id: batch_id.clone(),
            total_channels,
            avg_nominal_volume: config.nominal_injection_volume,
            avg_suggested_volume,
            avg_adjustment,
            over_injected_count,
            under_injected_count,
            estimated_gas_reduction,
            estimated_capacity_improvement,
            next_batch_suggestion,
            channels_with_missing_data,
            channels_requiring_confirmation,
            used_fallback_strategy,
            avg_data_completeness,
            hard_limits_applied_count,
            fallback_explanation,
        },
        channel_suggestions,
    )
}

pub fn calculate_gas_from_pressure(
    pressure: f64,
    temperature: f64,
    initial_pressure: f64,
    headspace_volume: f64,
) -> (f64, f64) {
    const GAS_CONSTANT: f64 = 8.314;
    const MOLAR_MASS: f64 = 0.02897;

    let pressure_diff = pressure - initial_pressure;
    let temp_kelvin = temperature + 273.15;

    let gas_moles = (pressure_diff * headspace_volume) / (GAS_CONSTANT * temp_kelvin);
    let gas_mass = gas_moles * MOLAR_MASS * 1000.0;

    let reference_pressure = 101.325;
    let reference_temp = 273.15;
    let gas_volume =
        (pressure_diff * headspace_volume * reference_temp) / (reference_pressure * temp_kelvin) * 1000.0;

    (gas_volume, gas_mass)
}

pub fn generate_gas_data(
    cabinet_id: u16,
    channel_id: u32,
    cycle_index: u16,
    stage: crate::models::Stage,
    pressure: f64,
    temperature: f64,
    initial_pressure: f64,
    cumulative_gas: f64,
    time_delta_seconds: f64,
) -> GasGenerationData {
    let (gas_volume, _) = calculate_gas_from_pressure(pressure, temperature, initial_pressure, 0.001);

    let gas_generation_rate = if time_delta_seconds > 0.0 {
        gas_volume / time_delta_seconds * 60.0
    } else {
        0.0
    };

    GasGenerationData {
        timestamp: Utc::now(),
        cabinet_id,
        channel_id,
        cycle_index,
        stage,
        pressure,
        temperature,
        gas_volume,
        gas_generation_rate,
        cumulative_gas: cumulative_gas + gas_volume,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Stage;
    use rand::Rng;

    fn generate_batch_gas_data(count: usize, avg_gas: f64, std_dev: f64) -> Vec<GasGenerationData> {
        let mut rng = rand::thread_rng();
        (0..count)
            .map(|i| {
                let cumulative_gas = avg_gas + rng.gen_range(-std_dev..std_dev);
                generate_gas_data(
                    0,
                    i as u32,
                    1,
                    Stage::CcCharge,
                    105.0 + (cumulative_gas - 50.0) * 0.5,
                    25.0,
                    101.325,
                    cumulative_gas,
                    3600.0,
                )
            })
            .collect()
    }

    #[tokio::test]
    async fn test_async_optimize_batch() {
        let config = OptimizerConfig::default();
        let (service, handle) = ElectrolyteOptimizerService::new(config);
        tokio::spawn(service.run());

        let gas_data = generate_batch_gas_data(50, 55.0, 5.0);
        let request = OptimizeRequest {
            request_id: "test-001".to_string(),
            batch_id: "TEST-BATCH-001".to_string(),
            gas_data,
            respond_to: None,
        };

        let rx = handle.optimize_batch(request).await.unwrap();
        let result = rx.await.unwrap();

        assert_eq!(result.request_id, "test-001");
        assert_eq!(result.result.batch_id, "TEST-BATCH-001");
        assert_eq!(result.result.total_channels, 50);
        assert!(result.result.avg_suggested_volume > 0.0);
        assert!(!result.channel_suggestions.is_empty());
    }

    #[tokio::test]
    async fn test_async_confirm_injection() {
        let config = OptimizerConfig {
            manual_confirmation_confidence_threshold: 0.9,
            ..OptimizerConfig::default()
        };
        let (service, handle) = ElectrolyteOptimizerService::new(config);
        tokio::spawn(service.run());

        let gas_data = generate_batch_gas_data(10, 85.0, 5.0);
        let opt_request = OptimizeRequest {
            request_id: "opt-001".to_string(),
            batch_id: "TEST-BATCH-001".to_string(),
            gas_data,
            respond_to: None,
        };

        let rx = handle.optimize_batch(opt_request).await.unwrap();
        let opt_result = rx.await.unwrap();

        let injection_id = opt_result.channel_suggestions[0].injection_id.clone();

        let confirm_request = ConfirmRequest {
            request_id: "confirm-001".to_string(),
            injection_id,
            confirmed_volume: 115.0,
            notes: Some("看起来合理".to_string()),
            operator: "张三".to_string(),
            respond_to: None,
        };

        let rx = handle.confirm_injection(confirm_request).await.unwrap();
        let result = rx.await.unwrap();

        assert!(result.success);
        assert!(result.updated_injection.is_some());
        assert_eq!(result.updated_injection.unwrap().confirmed_by, Some("张三".to_string()));
    }

    #[tokio::test]
    async fn test_async_process_gas_data() {
        let config = OptimizerConfig::default();
        let (service, handle) = ElectrolyteOptimizerService::new(config);
        tokio::spawn(service.run());

        let gas_data = generate_gas_data(
            0, 0, 1, Stage::CcCharge,
            105.0, 25.0, 101.325, 60.0, 3600.0
        );

        let result = handle.process_gas_data(gas_data).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_get_pending_confirmations() {
        let config = OptimizerConfig {
            manual_confirmation_confidence_threshold: 0.9,
            ..OptimizerConfig::default()
        };
        let (service, handle) = ElectrolyteOptimizerService::new(config);
        tokio::spawn(service.run());

        let gas_data = generate_batch_gas_data(10, 85.0, 5.0);
        let opt_request = OptimizeRequest {
            request_id: "opt-001".to_string(),
            batch_id: "TEST-BATCH-001".to_string(),
            gas_data,
            respond_to: None,
        };

        let rx = handle.optimize_batch(opt_request).await.unwrap();
        let _ = rx.await.unwrap();

        let rx = handle
            .get_pending_confirmations("TEST-BATCH-001".to_string())
            .await
            .unwrap();
        let pending = rx.await.unwrap();

        assert!(!pending.is_empty());
    }

    #[test]
    fn test_feedback_control_convergence_sync() {
        let config = OptimizerConfig {
            learning_rate: 0.3,
            target_gas_volume: 50.0,
            ..OptimizerConfig::default()
        };

        let mut history_data = Vec::new();
        let initial_gas = 70.0;
        let mut current_gas = initial_gas;
        let mut suggestions = Vec::new();

        for i in 0..20 {
            let gas_data = generate_gas_data(
                0, 0, i as u16, Stage::CcCharge,
                101.325 + (current_gas - 50.0) * 0.5,
                25.0, 101.325, current_gas, 3600.0
            );

            let pressure_ok = check_pressure_data_availability(&gas_data);
            let completeness = assess_data_completeness(&gas_data, &history_data);

            let effective_gas = if pressure_ok && completeness >= config.min_pressure_data_coverage {
                gas_data.cumulative_gas
            } else {
                get_historical_avg_gas(&history_data, config.target_gas_volume)
            };

            if pressure_ok {
                history_data.push((config.nominal_injection_volume, gas_data.cumulative_gas));
                if history_data.len() > config.history_window_size {
                    history_data.remove(0);
                }
            }

            let suggested = calculate_suggested_volume(&config, effective_gas, &history_data);
            suggestions.push(suggested);
            let adjustment = suggested - 120.0;
            current_gas = 50.0 + (current_gas - 50.0) * 0.7 - adjustment * 0.5;
        }

        assert!(suggestions.len() >= 10, "Should have enough suggestions");

        if suggestions.len() >= 10 {
            let first_half_avg = suggestions.iter().take(5).sum::<f64>() / 5.0;
            let last_half_avg = suggestions.iter().skip(suggestions.len() - 5).sum::<f64>() / 5.0;

            let first_half_var: f64 = suggestions.iter().take(5)
                .map(|&s| (s - first_half_avg).powi(2)).sum::<f64>() / 5.0;
            let last_half_var: f64 = suggestions.iter().skip(suggestions.len() - 5)
                .map(|&s| (s - last_half_avg).powi(2)).sum::<f64>() / 5.0;

            assert!(last_half_var < first_half_var * 0.5,
                "Later suggestions should have lower variance (converging). First var: {:.4}, Last var: {:.4}",
                first_half_var, last_half_var);
        }
    }

    #[test]
    fn test_gas_from_pressure_calculation() {
        let (gas_volume, gas_mass) = calculate_gas_from_pressure(
            105.0,
            25.0,
            101.325,
            0.001,
        );

        assert!(gas_volume > 0.0, "Gas volume should be positive");
        assert!(gas_mass > 0.0, "Gas mass should be positive");
        assert!((gas_volume - 13.5).abs() < 2.0,
            "Gas volume should be ~13.5 mL for 3.675 kPa delta. Got: {:.2}", gas_volume);
    }
}
