use crate::models::{Stage, ChannelData, CycleFeatures};
use dashmap::DashMap;
use std::sync::Arc;
use chrono::Utc;

#[derive(Debug, Clone)]
struct ChannelState {
    current_stage: Stage,
    stage_start_time: chrono::DateTime<Utc>,
    stage_data_buffer: Vec<ChannelData>,
}

pub struct StageDetector {
    channel_states: Arc<DashMap<(u16, u32), ChannelState>>,
}

impl StageDetector {
    pub fn new() -> Self {
        Self {
            channel_states: Arc::new(DashMap::new()),
        }
    }

    pub fn detect_stage(&self, data: &ChannelData) -> (Stage, u32) {
        let key = (data.cabinet_id, data.channel_id);
        
        if let Some(mut state) = self.channel_states.get_mut(&key) {
            let new_stage = self.determine_stage(data, state.current_stage);
            
            if new_stage != state.current_stage {
                state.current_stage = new_stage;
                state.stage_start_time = data.timestamp;
                state.stage_data_buffer.clear();
            }
            
            state.stage_data_buffer.push(data.clone());
            
            let duration = (data.timestamp - state.stage_start_time).num_seconds() as u32;
            (new_stage, duration)
        } else {
            let initial_stage = self.determine_initial_stage(data);
            self.channel_states.insert(
                key,
                ChannelState {
                    current_stage: initial_stage,
                    stage_start_time: data.timestamp,
                    stage_data_buffer: vec![data.clone()],
                },
            );
            (initial_stage, 0)
        }
    }

    fn determine_stage(&self, data: &ChannelData, current_stage: Stage) -> Stage {
        let voltage = data.voltage;
        let current = data.current;
        let abs_current = current.abs();

        if abs_current < 0.02 {
            if matches!(current_stage, Stage::CcCharge | Stage::CvCharge) && voltage > 3.9 {
                return Stage::Rest;
            }
            if matches!(current_stage, Stage::Discharge) && voltage < 3.5 {
                return Stage::Rest;
            }
            return if current_stage == Stage::Rest {
                Stage::Rest
            } else {
                self.determine_initial_stage(data)
            };
        }

        if current > 0.0 {
            if voltage < 3.0 {
                return Stage::Precharge;
            }
            if voltage >= 4.15 && abs_current < 1.0 {
                return Stage::CvCharge;
            }
            if voltage < 4.2 && abs_current > 1.0 {
                return Stage::CcCharge;
            }
            return Stage::CcCharge;
        }

        if current < 0.0 {
            if voltage > 2.8 {
                return Stage::Discharge;
            }
        }

        current_stage
    }

    fn determine_initial_stage(&self, data: &ChannelData) -> Stage {
        let voltage = data.voltage;
        let current = data.current;
        let abs_current = current.abs();

        if abs_current < 0.02 {
            return Stage::Rest;
        }

        if current > 0.0 {
            if voltage < 3.0 {
                return Stage::Precharge;
            }
            if voltage >= 4.15 && abs_current < 1.0 {
                return Stage::CvCharge;
            }
            return Stage::CcCharge;
        }

        if current < 0.0 {
            return Stage::Discharge;
        }

        Stage::Rest
    }

    pub fn get_current_stage(&self, cabinet_id: u16, channel_id: u32) -> Option<Stage> {
        self.channel_states
            .get(&(cabinet_id, channel_id))
            .map(|s| s.current_stage)
    }

    pub fn get_stage_duration(&self, cabinet_id: u16, channel_id: u32) -> Option<u32> {
        self.channel_states
            .get(&(cabinet_id, channel_id))
            .map(|s| (Utc::now() - s.stage_start_time).num_seconds() as u32)
    }

    pub fn get_stage_data(&self, cabinet_id: u16, channel_id: u32) -> Option<Vec<ChannelData>> {
        self.channel_states
            .get(&(cabinet_id, channel_id))
            .map(|s| s.stage_data_buffer.clone())
    }

    pub fn extract_cycle_features(&self, cabinet_id: u16, channel_id: u32) -> Option<CycleFeatureAccumulator> {
        let state = self.channel_states.get(&(cabinet_id, channel_id))?;
        let data = &state.stage_data_buffer;
        
        let mut accumulator = CycleFeatureAccumulator::default();
        
        for d in data {
            match d.stage {
                Stage::CcCharge => {
                    accumulator.cc_charge_time += 10;
                    accumulator.max_charge_temp = accumulator.max_charge_temp.max(d.temperature);
                    if d.current > 0.0 {
                        accumulator.charge_capacity += d.current * 10.0 / 3600.0;
                    }
                }
                Stage::CvCharge => {
                    accumulator.cv_charge_time += 10;
                    accumulator.cv_end_current = d.current;
                    accumulator.max_charge_temp = accumulator.max_charge_temp.max(d.temperature);
                    if d.current > 0.0 {
                        accumulator.charge_capacity += d.current * 10.0 / 3600.0;
                    }
                }
                Stage::Discharge => {
                    accumulator.discharge_time += 10;
                    accumulator.max_discharge_temp = accumulator.max_discharge_temp.max(d.temperature);
                    if d.current < 0.0 {
                        accumulator.discharge_capacity += d.current.abs() * 10.0 / 3600.0;
                    }
                    if d.voltage > 3.2 && d.voltage < 3.6 {
                        accumulator.discharge_platform_voltage_samples.push(d.voltage);
                    }
                }
                _ => {}
            }
            
            if matches!(d.stage, Stage::CcCharge) && d.voltage >= 4.15 {
                accumulator.cc_end_voltage = d.voltage;
            }
        }
        
        accumulator.discharge_platform_voltage = if !accumulator.discharge_platform_voltage_samples.is_empty() {
            accumulator.discharge_platform_voltage_samples.iter().sum::<f64>() 
                / accumulator.discharge_platform_voltage_samples.len() as f64
        } else {
            0.0
        };
        
        Some(accumulator)
    }
}

#[derive(Debug, Default, Clone)]
pub struct CycleFeatureAccumulator {
    pub cc_charge_time: u32,
    pub cv_charge_time: u32,
    pub discharge_time: u32,
    pub discharge_platform_voltage: f64,
    pub discharge_platform_voltage_samples: Vec<f64>,
    pub cc_end_voltage: f64,
    pub cv_end_current: f64,
    pub max_charge_temp: f64,
    pub max_discharge_temp: f64,
    pub charge_capacity: f64,
    pub discharge_capacity: f64,
}

impl CycleFeatureAccumulator {
    pub fn efficiency(&self) -> f64 {
        if self.charge_capacity > 0.0 {
            self.discharge_capacity / self.charge_capacity
        } else {
            0.0
        }
    }
}
