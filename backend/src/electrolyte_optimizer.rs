use crate::models::{ElectrolyteInjection, GasGenerationData, InjectionOptimizationResult, InjectionStatus};
use chrono::Utc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ElectrolyteConfig {
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
}

impl Default for ElectrolyteConfig {
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

        let pressure_available = self.check_pressure_data_availability(gas_data);
        let data_completeness = self.assess_data_completeness(gas_data);
        let mut used_fallback = false;
        let mut hard_limit_applied = false;

        let effective_gas_volume = if pressure_available && data_completeness >= self.config.min_pressure_data_coverage {
            gas_data.cumulative_gas
        } else {
            used_fallback = true;
            if self.config.enable_fallback_to_nominal {
                self.get_historical_avg_gas()
            } else {
                return None;
            }
        };

        if effective_gas_volume < self.config.min_gas_volume * 0.5 {
            return None;
        }

        if !used_fallback {
            self.history_data.push((self.config.nominal_injection_volume, gas_data.cumulative_gas));
            if self.history_data.len() > self.config.history_window_size {
                self.history_data.remove(0);
            }
        }

        let mut suggested_volume = self.calculate_suggested_volume(effective_gas_volume);
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

        let mut confidence = self.calculate_confidence(effective_gas_volume);
        if used_fallback {
            confidence *= 0.5;
        }
        if data_completeness < self.config.min_pressure_data_coverage {
            confidence *= data_completeness;
        }

        let requires_manual_confirmation = confidence < self.config.manual_confirmation_confidence_threshold
            || used_fallback
            || hard_limit_applied;

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
        })
    }

    fn check_pressure_data_availability(&self, gas_data: &GasGenerationData) -> bool {
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

    fn assess_data_completeness(&self, gas_data: &GasGenerationData) -> f64 {
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
        
        if self.history_data.len() < 10 {
            score *= 0.8;
        }
        
        score.max(0.0).min(1.0)
    }

    fn get_historical_avg_gas(&self) -> f64 {
        if self.history_data.is_empty() {
            return self.config.target_gas_volume;
        }
        let sum: f64 = self.history_data.iter().map(|(_, gas)| *gas).sum();
        sum / self.history_data.len() as f64
    }

    pub fn confirm_injection(
        &mut self,
        injection_id: &str,
        confirmed_volume: f64,
        notes: Option<String>,
        operator: String,
    ) -> Option<ElectrolyteInjection> {
        None
    }

    pub fn get_channels_requiring_confirmation(
        &self,
        batch_id: &str,
    ) -> Vec<&ElectrolyteInjection> {
        Vec::new()
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
                channels_with_missing_data: 0,
                channels_requiring_confirmation: 0,
                used_fallback_strategy: false,
                avg_data_completeness: 1.0,
                hard_limits_applied_count: 0,
                fallback_explanation: String::new(),
            };
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

        for gas_data in batch_gas_data {
            let pressure_ok = self.check_pressure_data_availability(gas_data);
            let completeness = self.assess_data_completeness(gas_data);
            completeness_scores.push(completeness);

            if !pressure_ok || completeness < self.config.min_pressure_data_coverage {
                channels_with_missing_data += 1;
                channels_requiring_confirmation += 1;
            }

            let effective_gas = if pressure_ok && completeness >= self.config.min_pressure_data_coverage {
                gas_data.cumulative_gas
            } else {
                self.get_historical_avg_gas()
            };

            valid_gas_values.push(effective_gas);

            let mut suggested = self.calculate_suggested_volume(effective_gas);
            let adjustment = suggested - self.config.nominal_injection_volume;

            if adjustment.abs() > self.config.max_adjustment_per_batch {
                suggested = self.config.nominal_injection_volume 
                    + adjustment.signum() * self.config.max_adjustment_per_batch;
                hard_limits_applied_count += 1;
            }
            if suggested < self.config.hard_min_injection_volume {
                suggested = self.config.hard_min_injection_volume;
                hard_limits_applied_count += 1;
            }
            if suggested > self.config.hard_max_injection_volume {
                suggested = self.config.hard_max_injection_volume;
                hard_limits_applied_count += 1;
            }

            suggestions.push(suggested);

            if effective_gas > self.config.max_gas_volume {
                over_injected_count += 1;
            } else if effective_gas < self.config.min_gas_volume {
                under_injected_count += 1;
            }
        }

        let avg_gas_volume = if valid_gas_values.is_empty() {
            self.config.target_gas_volume
        } else {
            valid_gas_values.iter().sum::<f64>() / valid_gas_values.len() as f64
        };

        let avg_suggested_volume = if suggestions.is_empty() {
            self.config.nominal_injection_volume
        } else {
            suggestions.iter().sum::<f64>() / suggestions.len() as f64
        };

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
                self.config.min_pressure_data_coverage * 100.0
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
            channels_with_missing_data,
            channels_requiring_confirmation,
            used_fallback_strategy,
            avg_data_completeness,
            hard_limits_applied_count,
            fallback_explanation,
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
                ElectrolyteOptimizationService::generate_gas_data(
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

    #[test]
    fn test_feedback_control_convergence() {
        let config = ElectrolyteConfig {
            learning_rate: 0.3,
            target_gas_volume: 50.0,
            ..ElectrolyteConfig::default()
        };

        let mut service = ElectrolyteOptimizationService::new(config);

        let initial_gas = 70.0;
        let mut current_gas = initial_gas;
        let mut suggestions = Vec::new();

        for i in 0..20 {
            let gas_data = ElectrolyteOptimizationService::generate_gas_data(
                0, 0, i as u16, Stage::CcCharge,
                101.325 + (current_gas - 50.0) * 0.5,
                25.0, 101.325, current_gas, 3600.0
            );

            let result = service.process_gas_data(&gas_data);
            if let Some(injection) = result {
                suggestions.push(injection.suggested_volume);
                let adjustment = injection.suggested_volume - 120.0;
                current_gas = 50.0 + (current_gas - 50.0) * 0.7 - adjustment * 0.5;
            }
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

            assert!((last_half_avg - 120.0).abs() < 5.0,
                "Suggestions should converge near nominal volume. Avg: {:.2}", last_half_avg);
        }
    }

    #[test]
    fn test_injection_reduction_reduces_gas_bloating() {
        let config = ElectrolyteConfig {
            target_gas_volume: 50.0,
            max_gas_volume: 80.0,
            ..ElectrolyteConfig::default()
        };

        let service = ElectrolyteOptimizationService::new(config);

        let high_gas_data = generate_batch_gas_data(100, 85.0, 5.0);
        let high_result = service.optimize_batch(&high_gas_data, "TEST-HIGH".to_string());

        assert!(high_result.avg_adjustment < 0.0,
            "High gas should result in negative adjustment (reduce injection). Adjustment: {:.2}",
            high_result.avg_adjustment);

        assert!(high_result.over_injected_count > 50,
            "Should detect many over-injected channels. Count: {}",
            high_result.over_injected_count);

        assert!(high_result.estimated_gas_reduction > 20.0,
            "Should estimate significant gas reduction. Estimate: {:.2}",
            high_result.estimated_gas_reduction);
    }

    #[test]
    fn test_injection_increase_reduces_capacity_deficit() {
        let config = ElectrolyteConfig {
            target_gas_volume: 50.0,
            min_gas_volume: 20.0,
            ..ElectrolyteConfig::default()
        };

        let service = ElectrolyteOptimizationService::new(config);

        let low_gas_data = generate_batch_gas_data(100, 15.0, 3.0);
        let low_result = service.optimize_batch(&low_gas_data, "TEST-LOW".to_string());

        assert!(low_result.avg_adjustment > 0.0,
            "Low gas should result in positive adjustment (increase injection). Adjustment: {:.2}",
            low_result.avg_adjustment);

        assert!(low_result.under_injected_count > 50,
            "Should detect many under-injected channels. Count: {}",
            low_result.under_injected_count);

        assert!(low_result.avg_suggested_volume > low_result.avg_nominal_volume,
            "Suggested volume should be higher than nominal. Suggested: {:.2}, Nominal: {:.2}",
            low_result.avg_suggested_volume, low_result.avg_nominal_volume);
    }

    #[test]
    fn test_different_battery_models_adaptation() {
        let models = vec![
            ("18650-2.5Ah", 90.0, 35.0),
            ("21700-4.8Ah", 150.0, 60.0),
            ("26650-5.0Ah", 180.0, 70.0),
            ("32650-6.0Ah", 220.0, 85.0),
        ];

        for (model_name, nominal_vol, target_gas) in models {
            let config = ElectrolyteConfig {
                nominal_injection_volume: nominal_vol,
                min_injection_volume: nominal_vol * 0.85,
                max_injection_volume: nominal_vol * 1.15,
                target_gas_volume: target_gas,
                max_gas_volume: target_gas * 1.6,
                min_gas_volume: target_gas * 0.4,
                ..ElectrolyteConfig::default()
            };

            let service = ElectrolyteOptimizationService::new(config);

            let normal_data = generate_batch_gas_data(50, target_gas, target_gas * 0.1);
            let result = service.optimize_batch(&normal_data, format!("TEST-{}", model_name));

            assert_eq!(result.avg_nominal_volume, nominal_vol,
                "Model {} should use correct nominal volume", model_name);

            assert!((result.avg_adjustment).abs() < nominal_vol * 0.05,
                "Model {} normal data should have small adjustment. Adjustment: {:.2}",
                model_name, result.avg_adjustment);

            assert!(result.over_injected_count < 10,
                "Model {} normal data should have few over-injected. Count: {}",
                model_name, result.over_injected_count);

            let high_gas_data = generate_batch_gas_data(50, target_gas * 1.5, target_gas * 0.1);
            let high_result = service.optimize_batch(&high_gas_data, format!("TEST-HIGH-{}", model_name));

            assert!(high_result.avg_adjustment < 0.0,
                "Model {} high gas should reduce injection. Adjustment: {:.2}",
                model_name, high_result.avg_adjustment);

            assert!(high_result.avg_suggested_volume >= config.min_injection_volume,
                "Model {} suggested volume should not go below min. Suggested: {:.2}, Min: {:.2}",
                model_name, high_result.avg_suggested_volume, config.min_injection_volume);
        }
    }

    #[test]
    fn test_gas_from_pressure_calculation() {
        let (gas_volume, gas_mass) = ElectrolyteOptimizationService::calculate_gas_from_pressure(
            105.0,
            25.0,
            101.325,
            0.001,
        );

        assert!(gas_volume > 0.0, "Gas volume should be positive");
        assert!(gas_mass > 0.0, "Gas mass should be positive");

        assert!((gas_volume - 13.5).abs() < 2.0,
            "Gas volume should be ~13.5 mL for 3.675 kPa delta. Got: {:.2}", gas_volume);

        let (gas_volume2, _) = ElectrolyteOptimizationService::calculate_gas_from_pressure(
            110.0,
            25.0,
            101.325,
            0.001,
        );

        assert!(gas_volume2 > gas_volume,
            "Higher pressure delta should produce more gas. {} > {}", gas_volume2, gas_volume);

        let (gas_volume3, _) = ElectrolyteOptimizationService::calculate_gas_from_pressure(
            105.0,
            45.0,
            101.325,
            0.001,
        );

        assert!(gas_volume3 < gas_volume,
            "Higher temperature should produce less volume. {} < {}", gas_volume3, gas_volume);
    }

    #[test]
    fn test_confidence_scoring() {
        let mut service = ElectrolyteOptimizationService::new(ElectrolyteConfig::default());

        let gas_data = ElectrolyteOptimizationService::generate_gas_data(
            0, 0, 1, Stage::CcCharge,
            105.0, 25.0, 101.325, 60.0, 3600.0
        );

        let result1 = service.process_gas_data(&gas_data);
        assert!(result1.as_ref().unwrap().confidence >= 0.5);
        assert!(result1.as_ref().unwrap().confidence <= 0.99);

        for i in 0..100 {
            let gas = ElectrolyteOptimizationService::generate_gas_data(
                0, 0, i as u16, Stage::CcCharge,
                105.0, 25.0, 101.325, 55.0 + (i as f64) * 0.1, 3600.0
            );
            service.process_gas_data(&gas);
        }

        let final_data = ElectrolyteOptimizationService::generate_gas_data(
            0, 0, 100, Stage::CcCharge,
            105.0, 25.0, 101.325, 60.0, 3600.0
        );

        let result_final = service.process_gas_data(&final_data);
        assert!(result_final.as_ref().unwrap().confidence > 0.7,
            "More data should increase confidence. Got: {:.2}",
            result_final.as_ref().unwrap().confidence);
    }

    #[test]
    fn test_boundary_empty_batch() {
        let service = ElectrolyteOptimizationService::new(ElectrolyteConfig::default());
        let result = service.optimize_batch(&[], "TEST-EMPTY".to_string());

        assert_eq!(result.total_channels, 0);
        assert_eq!(result.over_injected_count, 0);
        assert_eq!(result.under_injected_count, 0);
        assert_eq!(result.avg_adjustment, 0.0);
        assert_eq!(result.avg_suggested_volume, service.config.nominal_injection_volume);
    }

    #[test]
    fn test_boundary_extreme_gas_values() {
        let service = ElectrolyteOptimizationService::new(ElectrolyteConfig::default());

        let extreme_high = generate_batch_gas_data(10, 200.0, 0.0);
        let result_high = service.optimize_batch(&extreme_high, "TEST-EXTREME-HIGH".to_string());

        assert_eq!(result_high.over_injected_count, 10);
        assert!(result_high.avg_suggested_volume == service.config.min_injection_volume,
            "Extreme high gas should clamp to min volume. Got: {:.2}",
            result_high.avg_suggested_volume);

        let extreme_low = generate_batch_gas_data(10, 5.0, 0.0);
        let result_low = service.optimize_batch(&extreme_low, "TEST-EXTREME-LOW".to_string());

        assert_eq!(result_low.under_injected_count, 10);
        assert!(result_low.avg_suggested_volume == service.config.max_injection_volume,
            "Extreme low gas should clamp to max volume. Got: {:.2}",
            result_low.avg_suggested_volume);
    }

    #[test]
    fn test_boundary_ignored_stages() {
        let mut service = ElectrolyteOptimizationService::new(ElectrolyteConfig::default());

        let ignored_stages = vec![
            Stage::Rest,
            Stage::CvCharge,
            Stage::Discharge,
            Stage::Idle,
        ];

        for stage in ignored_stages {
            let gas_data = ElectrolyteOptimizationService::generate_gas_data(
                0, 0, 1, stage, 105.0, 25.0, 101.325, 60.0, 3600.0
            );
            let result = service.process_gas_data(&gas_data);
            assert!(result.is_none(), "Stage {:?} should be ignored", stage);
        }

        let valid_stages = vec![Stage::Precharge, Stage::CcCharge];
        for stage in valid_stages {
            let gas_data = ElectrolyteOptimizationService::generate_gas_data(
                0, 0, 1, stage, 105.0, 25.0, 101.325, 60.0, 3600.0
            );
            let result = service.process_gas_data(&gas_data);
            assert!(result.is_some(), "Stage {:?} should be processed", stage);
        }
    }

    #[test]
    fn test_next_batch_suggestion_logic() {
        let config = ElectrolyteConfig {
            nominal_injection_volume: 120.0,
            target_gas_volume: 50.0,
            ..ElectrolyteConfig::default()
        };
        let service = ElectrolyteOptimizationService::new(config);

        let data_high = generate_batch_gas_data(10, 60.0, 2.0);
        let result_high = service.optimize_batch(&data_high, "TEST".to_string());
        assert!(result_high.next_batch_suggestion < 120.0,
            "High gas should suggest lower next batch volume. Got: {:.2}",
            result_high.next_batch_suggestion);

        let data_low = generate_batch_gas_data(10, 40.0, 2.0);
        let result_low = service.optimize_batch(&data_low, "TEST".to_string());
        assert!(result_low.next_batch_suggestion > 120.0,
            "Low gas should suggest higher next batch volume. Got: {:.2}",
            result_low.next_batch_suggestion);

        let data_normal = generate_batch_gas_data(10, 51.0, 1.0);
        let result_normal = service.optimize_batch(&data_normal, "TEST".to_string());
        assert!((result_normal.next_batch_suggestion - 120.0).abs() < 3.0,
            "Normal gas should suggest near nominal. Got: {:.2}",
            result_normal.next_batch_suggestion);
    }

    #[test]
    fn test_gas_bloating_rate_correlation() {
        let config = ElectrolyteConfig::default();
        let service = ElectrolyteOptimizationService::new(config);

        let test_cases = vec![
            (90.0, 0.85),
            (75.0, 0.60),
            (50.0, 0.0),
            (35.0, 0.0),
        ];

        for (gas_vol, expected_ratio) in test_cases {
            let data = generate_batch_gas_data(50, gas_vol, 2.0);
            let result = service.optimize_batch(&data, "TEST".to_string());

            let bloat_rate = result.over_injected_count as f64 / result.total_channels as f64;

            if expected_ratio > 0.0 {
                assert!(bloat_rate >= expected_ratio * 0.8,
                    "Gas {} should have bloat rate >= {:.2}. Got: {:.2}",
                    gas_vol, expected_ratio * 0.8, bloat_rate);
            }
        }
    }
}
