use crate::models::{ElectrolyteInjection, GasGenerationData, InjectionOptimizationResult, InjectionStatus};
use chrono::Utc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ElectrolyteConfig {
    pub nominal_injection_volume: f64,
    pub min_injection_volume: f64,
    pub max_injection_volume: f64,
    pub target_gas_volume: f64,
    pub max_gas_volume: f64,
    pub min_gas_volume: f64,
    pub gas_to_electrolyte_ratio: f64,
    pub learning_rate: f64,
    pub history_window_size: usize,
}

impl Default for ElectrolyteConfig {
    fn default() -> Self {
        Self {
            nominal_injection_volume: 120.0,
            min_injection_volume: 100.0,
            max_injection_volume: 140.0,
            target_gas_volume: 50.0,
            max_gas_volume: 80.0,
            min_gas_volume: 20.0,
            gas_to_electrolyte_ratio: 0.8,
            learning_rate: 0.3,
            history_window_size: 100,
        }
    }
}

pub struct ElectrolyteOptimizationService {
    config: ElectrolyteConfig,
    history_data: Vec<(f64, f64)>,
}

impl ElectrolyteOptimizationService {
    pub fn new(config: ElectrolyteConfig) -> Self {
        Self {
            config,
            history_data: Vec::new(),
        }
    }

    pub fn process_gas_data(&mut self, gas_data: &GasGenerationData) -> Option<ElectrolyteInjection> {
        if gas_data.stage != crate::models::Stage::Precharge && gas_data.stage != crate::models::Stage::CcCharge {
            return None;
        }

        if gas_data.cumulative_gas < self.config.min_gas_volume * 0.5 {
            return None;
        }

        self.history_data.push((self.config.nominal_injection_volume, gas_data.cumulative_gas));
        if self.history_data.len() > self.config.history_window_size {
            self.history_data.remove(0);
        }

        let suggested_volume = self.calculate_suggested_volume(gas_data.cumulative_gas);
        let adjustment = suggested_volume - self.config.nominal_injection_volume;

        let status = if gas_data.cumulative_gas > self.config.max_gas_volume {
            InjectionStatus::OverInjected
        } else if gas_data.cumulative_gas < self.config.min_gas_volume {
            InjectionStatus::UnderInjected
        } else {
            InjectionStatus::Normal
        };

        let confidence = self.calculate_confidence(gas_data.cumulative_gas);

        let injection_id = Uuid::new_v4().to_string();
        let batch_id = format!("BATCH_{}", Utc::now().format("%Y%m%d"));

        Some(ElectrolyteInjection {
            date: Utc::now().date_naive(),
            batch_id,
            injection_id,
            cabinet_id: gas_data.cabinet_id,
            channel_id: gas_data.channel_id,
            cycle_index: gas_data.cycle_index,
            nominal_volume: self.config.nominal_injection_volume,
            actual_volume: self.config.nominal_injection_volume,
            gas_volume: gas_data.cumulative_gas,
            suggested_volume,
            adjustment,
            status,
            confidence,
        })
    }

    pub fn optimize_batch(&self, batch_gas_data: &[GasGenerationData], batch_id: String) -> InjectionOptimizationResult {
        if batch_gas_data.is_empty() {
            return InjectionOptimizationResult {
                batch_id,
                total_channels: 0,
                avg_nominal_volume: self.config.nominal_injection_volume,
                avg_suggested_volume: self.config.nominal_injection_volume,
                avg_adjustment: 0.0,
                over_injected_count: 0,
                under_injected_count: 0,
                estimated_gas_reduction: 0.0,
                estimated_capacity_improvement: 0.0,
                next_batch_suggestion: self.config.nominal_injection_volume,
            };
        }

        let total_channels = batch_gas_data.len();
        let avg_gas_volume: f64 = batch_gas_data.iter().map(|g| g.cumulative_gas).sum::<f64>() / total_channels as f64;

        let mut over_injected_count = 0;
        let mut under_injected_count = 0;
        let mut suggestions: Vec<f64> = Vec::new();

        for gas_data in batch_gas_data {
            let suggested = self.calculate_suggested_volume(gas_data.cumulative_gas);
            suggestions.push(suggested);

            if gas_data.cumulative_gas > self.config.max_gas_volume {
                over_injected_count += 1;
            } else if gas_data.cumulative_gas < self.config.min_gas_volume {
                under_injected_count += 1;
            }
        }

        let avg_suggested_volume = suggestions.iter().sum::<f64>() / suggestions.len() as f64;
        let avg_adjustment = avg_suggested_volume - self.config.nominal_injection_volume;

        let next_batch_suggestion = self.calculate_next_batch_suggestion(avg_gas_volume);

        let estimated_gas_reduction = if avg_gas_volume > self.config.target_gas_volume {
            (avg_gas_volume - self.config.target_gas_volume) * self.config.gas_to_electrolyte_ratio
        } else {
            0.0
        };

        let estimated_capacity_improvement = if avg_gas_volume > self.config.max_gas_volume * 0.8 {
            let over_ratio = (avg_gas_volume - self.config.target_gas_volume) / self.config.target_gas_volume;
            over_ratio * 0.05 * 100.0
        } else {
            0.0
        };

        InjectionOptimizationResult {
            batch_id,
            total_channels,
            avg_nominal_volume: self.config.nominal_injection_volume,
            avg_suggested_volume,
            avg_adjustment,
            over_injected_count,
            under_injected_count,
            estimated_gas_reduction,
            estimated_capacity_improvement,
            next_batch_suggestion,
        }
    }

    fn calculate_suggested_volume(&self, gas_volume: f64) -> f64 {
        let gas_deviation = gas_volume - self.config.target_gas_volume;
        let base_adjustment = -gas_deviation * self.config.gas_to_electrolyte_ratio * self.config.learning_rate;

        let trend_adjustment = if self.history_data.len() >= 10 {
            let recent_avg_gas: f64 = self.history_data.iter().rev().take(10).map(|(_, g)| *g).sum::<f64>() / 10.0;
            let trend = recent_avg_gas - self.config.target_gas_volume;
            -trend * self.config.gas_to_electrolyte_ratio * 0.1
        } else {
            0.0
        };

        let suggested = self.config.nominal_injection_volume + base_adjustment + trend_adjustment;

        suggested.clamp(self.config.min_injection_volume, self.config.max_injection_volume)
    }

    fn calculate_next_batch_suggestion(&self, avg_gas_volume: f64) -> f64 {
        let deviation_ratio = (avg_gas_volume - self.config.target_gas_volume) / self.config.target_gas_volume;

        let adjustment = if deviation_ratio > 0.1 {
            -self.config.nominal_injection_volume * 0.05
        } else if deviation_ratio < -0.1 {
            self.config.nominal_injection_volume * 0.03
        } else {
            -self.config.nominal_injection_volume * deviation_ratio * 0.3
        };

        let suggested = self.config.nominal_injection_volume + adjustment;
        suggested.clamp(self.config.min_injection_volume, self.config.max_injection_volume)
    }

    fn calculate_confidence(&self, gas_volume: f64) -> f64 {
        let data_sufficiency = (self.history_data.len() as f64 / self.config.history_window_size as f64).min(1.0);

        let gas_clarity = if gas_volume < self.config.min_gas_volume * 0.8 {
            0.6
        } else if gas_volume > self.config.max_gas_volume * 1.2 {
            0.9
        } else {
            0.75
        };

        let stability = if self.history_data.len() >= 20 {
            let recent_gas: Vec<f64> = self.history_data.iter().rev().take(20).map(|(_, g)| *g).collect();
            let mean = recent_gas.iter().sum::<f64>() / recent_gas.len() as f64;
            let variance: f64 = recent_gas.iter().map(|g| (g - mean).powi(2)).sum::<f64>() / recent_gas.len() as f64;
            let std_dev = variance.sqrt();
            let cv = std_dev / mean;
            (1.0 - cv * 2.0).max(0.5)
        } else {
            0.6
        };

        (data_sufficiency * 0.4 + gas_clarity * 0.3 + stability * 0.3).clamp(0.5, 0.99)
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
        let gas_volume = (pressure_diff * headspace_volume * reference_temp) / (reference_pressure * temp_kelvin) * 1000.0;

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
        let (gas_volume, _) = Self::calculate_gas_from_pressure(
            pressure,
            temperature,
            initial_pressure,
            0.001,
        );

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
}
