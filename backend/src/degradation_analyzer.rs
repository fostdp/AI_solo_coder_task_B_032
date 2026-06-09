use crate::models::{ChannelData, DegradationAnalysis, DegradationMode, DvDqPoint};
use chrono::Utc;

#[derive(Debug, Clone)]
pub struct DegradationConfig {
    pub peak_detection_threshold: f64,
    pub min_peak_distance: f64,
    pub reference_cycle: u16,
    pub cathode_peak_range: (f64, f64),
    pub anode_peak_range: (f64, f64),
    pub sei_peak_range: (f64, f64),
    pub fading_rate_threshold: f64,
    pub resistance_growth_threshold: f64,
}

impl Default for DegradationConfig {
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
        }
    }
}

pub struct DegradationAnalysisService {
    config: DegradationConfig,
    baseline_data: std::collections::HashMap<(u16, u32), Vec<DvDqPoint>>,
    historical_analysis: std::collections::HashMap<(u16, u32), Vec<(u16, DegradationMode, f64)>>,
}

impl DegradationAnalysisService {
    pub fn new(config: DegradationConfig) -> Self {
        Self {
            config,
            baseline_data: std::collections::HashMap::new(),
            historical_analysis: std::collections::HashMap::new(),
        }
    }

    pub fn analyze_channel(
        &mut self,
        cabinet_id: u16,
        channel_id: u32,
        cycle_index: u16,
        discharge_data: &[ChannelData],
        historical_capacities: &[(u16, f64)],
        historical_resistances: &[(u16, f64)],
    ) -> (DegradationAnalysis, Vec<DvDqPoint>) {
        let dvdq_curve = self.calculate_dvdq_curve(discharge_data);

        let peaks = self.detect_peaks(&dvdq_curve);
        let peak_positions: Vec<f64> = peaks.iter().map(|(v, _)| *v).collect();
        let peak_heights: Vec<f64> = peaks.iter().map(|(_, h)| *h).collect();

        let (cathode_score, anode_score, electrolyte_score, sei_score) =
            self.calculate_degradation_scores(&dvdq_curve, &peaks, cabinet_id, channel_id, cycle_index);

        let capacity_fade_rate = self.calculate_fade_rate(historical_capacities);
        let resistance_growth_rate = self.calculate_resistance_growth_rate(historical_resistances);

        let (mode, confidence) = self.classify_degradation_mode(
            cathode_score,
            anode_score,
            electrolyte_score,
            sei_score,
            capacity_fade_rate,
            resistance_growth_rate,
        );

        let recommendations = self.generate_recommendations(mode, confidence, capacity_fade_rate);

        let analysis = DegradationAnalysis {
            timestamp: Utc::now(),
            cabinet_id,
            channel_id,
            cycle_index,
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
        };

        let key = (cabinet_id, channel_id);
        self.historical_analysis
            .entry(key)
            .or_insert_with(Vec::new)
            .push((cycle_index, mode, confidence));

        if cycle_index == self.config.reference_cycle {
            self.baseline_data.insert(key, dvdq_curve.clone());
        }

        (analysis, dvdq_curve)
    }

    fn calculate_dvdq_curve(&self, discharge_data: &[ChannelData]) -> Vec<DvDqPoint> {
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

        self.smooth_dvdq_curve(dvdq_points, 3)
    }

    fn smooth_dvdq_curve(&self, points: Vec<DvDqPoint>, window_size: usize) -> Vec<DvDqPoint> {
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

    fn detect_peaks(&self, points: &[DvDqPoint]) -> Vec<(f64, f64)> {
        let mut peaks = Vec::new();
        let min_height = self.config.peak_detection_threshold;

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
                    (curr.voltage - v).abs() < self.config.min_peak_distance
                });

                if !too_close {
                    peaks.push((curr.voltage, curr.dq_dv));
                }
            }
        }

        peaks.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        peaks.into_iter().take(5).collect()
    }

    fn calculate_degradation_scores(
        &self,
        current_curve: &[DvDqPoint],
        current_peaks: &[(f64, f64)],
        cabinet_id: u16,
        channel_id: u32,
        cycle_index: u16,
    ) -> (f64, f64, f64, f64) {
        let key = (cabinet_id, channel_id);
        let baseline = self.baseline_data.get(&key);

        match baseline {
            None => (0.5, 0.5, 0.5, 0.5),
            Some(baseline_curve) => {
                let cathode_score = self.calculate_cathode_score(current_peaks, baseline_curve, cycle_index);
                let anode_score = self.calculate_anode_score(current_peaks, baseline_curve, cycle_index);
                let electrolyte_score = self.calculate_electrolyte_score(current_curve, baseline_curve);
                let sei_score = self.calculate_sei_score(current_peaks, cycle_index);

                (cathode_score, anode_score, electrolyte_score, sei_score)
            }
        }
    }

    fn calculate_cathode_score(
        &self,
        peaks: &[(f64, f64)],
        baseline: &[DvDqPoint],
        cycle_index: u16,
    ) -> f64 {
        let (low, high) = self.config.cathode_peak_range;

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

        let peak_shift = if cathode_peaks.len() >= 1 && baseline_cathode_peaks.len() >= 1 {
            let current_pos = cathode_peaks[0].0;
            let baseline_pos = baseline_cathode_peaks
                .iter()
                .max_by(|a, b| a.dq_dv.partial_cmp(&b.dq_dv).unwrap())
                .map(|p| p.voltage)
                .unwrap_or(current_pos);

            (current_pos - baseline_pos).abs()
        } else {
            0.0
        };

        let cycle_factor = (cycle_index as f64 / 100.0).min(1.0);
        let height_score = (1.0 - (1.0 - height_ratio).abs()).max(0.0);
        let shift_score = (1.0 - peak_shift * 5.0).max(0.0);

        (height_score * 0.6 + shift_score * 0.4 + cycle_factor * 0.2).min(1.0)
    }

    fn calculate_anode_score(
        &self,
        peaks: &[(f64, f64)],
        baseline: &[DvDqPoint],
        cycle_index: u16,
    ) -> f64 {
        let (low, high) = self.config.anode_peak_range;

        let anode_peaks: Vec<&(f64, f64)> = peaks
            .iter()
            .filter(|(v, _)| *v >= low && *v <= high)
            .collect();

        if anode_peaks.is_empty() {
            return 0.4;
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

        let cycle_factor = (cycle_index as f64 / 50.0).min(1.0);
        let height_score = (1.0 - (1.0 - height_ratio).abs() * 1.5).max(0.0);

        (height_score * 0.7 + cycle_factor * 0.3).min(1.0)
    }

    fn calculate_electrolyte_score(
        &self,
        current: &[DvDqPoint],
        baseline: &[DvDqPoint],
    ) -> f64 {
        if current.len() < 3 || baseline.len() < 3 {
            return 0.5;
        }

        let current_avg_dqdv: f64 = current.iter().map(|p| p.dq_dv).sum::<f64>() / current.len() as f64;
        let baseline_avg_dqdv: f64 = baseline.iter().map(|p| p.dq_dv).sum::<f64>() / baseline.len() as f64;

        let ratio = if baseline_avg_dqdv > 0.0 {
            current_avg_dqdv / baseline_avg_dqdv
        } else {
            1.0
        };

        let voltage_range_score = self.analyze_voltage_range(current, baseline);

        let score = ((ratio * 0.6 + voltage_range_score * 0.4) * 1.2).min(1.0);
        score.max(0.1)
    }

    fn analyze_voltage_range(&self, current: &[DvDqPoint], baseline: &[DvDqPoint]) -> f64 {
        let current_min_v = current.iter().map(|p| p.voltage).fold(f64::INFINITY, f64::min);
        let current_max_v = current.iter().map(|p| p.voltage).fold(f64::NEG_INFINITY, f64::max);
        let baseline_min_v = baseline.iter().map(|p| p.voltage).fold(f64::INFINITY, f64::min);
        let baseline_max_v = baseline.iter().map(|p| p.voltage).fold(f64::NEG_INFINITY, f64::max);

        let current_range = current_max_v - current_min_v;
        let baseline_range = baseline_max_v - baseline_min_v;

        if baseline_range <= 0.0 {
            return 0.5;
        }

        let range_ratio = current_range / baseline_range;
        (1.0 - (1.0 - range_ratio).abs()).max(0.0)
    }

    fn calculate_sei_score(&self, peaks: &[(f64, f64)], cycle_index: u16) -> f64 {
        let (low, high) = self.config.sei_peak_range;

        let sei_peaks: Vec<&(f64, f64)> = peaks
            .iter()
            .filter(|(v, _)| *v >= low && *v <= high)
            .collect();

        let cycle_factor = (cycle_index as f64 / 200.0).min(1.0);

        if sei_peaks.is_empty() {
            return 0.3 + cycle_factor * 0.2;
        }

        let total_height: f64 = sei_peaks.iter().map(|(_, h)| *h).sum();
        let peak_count = sei_peaks.len() as f64;

        let height_score = (total_height * 2.0).min(1.0);
        let count_score = (peak_count / 3.0).min(1.0);

        (height_score * 0.5 + count_score * 0.3 + cycle_factor * 0.2).min(1.0)
    }

    fn calculate_fade_rate(&self, historical_capacities: &[(u16, f64)]) -> f64 {
        if historical_capacities.len() < 3 {
            return 0.0;
        }

        let first = historical_capacities.first().unwrap();
        let last = historical_capacities.last().unwrap();

        let cycles = (last.0 - first.0) as f64;
        if cycles <= 0.0 || first.1 <= 0.0 {
            return 0.0;
        }

        let capacity_loss = first.1 - last.1;
        let fade_rate = (capacity_loss / first.1) / cycles * 100.0;

        fade_rate.max(0.0)
    }

    fn calculate_resistance_growth_rate(&self, historical_resistances: &[(u16, f64)]) -> f64 {
        if historical_resistances.len() < 3 {
            return 0.0;
        }

        let first = historical_resistances.first().unwrap();
        let last = historical_resistances.last().unwrap();

        let cycles = (last.0 - first.0) as f64;
        if cycles <= 0.0 || first.1 <= 0.0 {
            return 0.0;
        }

        let resistance_gain = last.1 - first.1;
        let growth_rate = (resistance_gain / first.1) / cycles * 100.0;

        growth_rate.max(0.0)
    }

    fn classify_degradation_mode(
        &self,
        cathode: f64,
        anode: f64,
        electrolyte: f64,
        sei: f64,
        fade_rate: f64,
        resistance_rate: f64,
    ) -> (DegradationMode, f64) {
        let threshold = 0.7;
        let high_scores: Vec<(DegradationMode, f64)> = vec![
            (DegradationMode::CathodeDegradation, cathode),
            (DegradationMode::AnodeDegradation, anode),
            (DegradationMode::ElectrolyteConsumption, electrolyte),
            (DegradationMode::SEIGrowth, sei),
        ]
        .into_iter()
        .filter(|(_, s)| *s >= threshold)
        .collect();

        if high_scores.len() >= 2 {
            let avg_confidence: f64 = high_scores.iter().map(|(_, s)| *s).sum::<f64>() / high_scores.len() as f64;
            return (DegradationMode::MixedDegradation, avg_confidence.min(0.95));
        }

        if fade_rate < self.config.fading_rate_threshold
            && resistance_rate < self.config.resistance_growth_threshold
            && cathode < 0.5
            && anode < 0.5
            && electrolyte < 0.5
            && sei < 0.5
        {
            return (DegradationMode::Normal, 0.8);
        }

        let max_score = cathode.max(anode).max(electrolyte).max(sei);

        let mode = if max_score == cathode {
            DegradationMode::CathodeDegradation
        } else if max_score == anode {
            DegradationMode::AnodeDegradation
        } else if max_score == electrolyte {
            DegradationMode::ElectrolyteConsumption
        } else {
            DegradationMode::SEIGrowth
        };

        let confidence = (0.5 + max_score * 0.5).min(0.98);

        (mode, confidence)
    }

    fn generate_recommendations(
        &self,
        mode: DegradationMode,
        confidence: f64,
        fade_rate: f64,
    ) -> String {
        if confidence < 0.5 {
            return "数据不足，建议继续监测更多循环后再进行分析".to_string();
        }

        match mode {
            DegradationMode::Normal => {
                if fade_rate > self.config.fading_rate_threshold * 0.5 {
                    format!(
                        "电池正常老化，容量衰减率{:.3}%/cycle。建议：继续正常监测，每50循环复检一次。",
                        fade_rate
                    )
                } else {
                    "电池状态良好，处于正常老化阶段。建议：保持当前化成工艺参数，定期抽检。".to_string()
                }
            }
            DegradationMode::CathodeDegradation => {
                "检测到正极衰减迹象，dQ/dV高电压区域峰值发生偏移。建议：1) 检查充电上限电压是否过高；2) 考虑降低充电截止电流；3) 优化化成工艺，减少高电压停留时间；4) 评估是否需要更换正极材料批次。".to_string()
            }
            DegradationMode::AnodeDegradation => {
                "检测到负极衰减迹象，低SOC区域dQ/dV峰值变化明显。建议：1) 检查负极材料是否存在锂 plating；2) 优化预充工艺，确保SEI膜形成良好；3) 降低放电截止电压，避免过放；4) 考虑提高注液量或优化电解液配方。".to_string()
            }
            DegradationMode::ElectrolyteConsumption => {
                "检测到电解液消耗迹象，dQ/dV曲线整体变平。建议：1) 检查注液量是否充足；2) 评估SEI膜稳定性，是否存在持续副反应；3) 优化化成温度曲线，减少电解液分解；4) 考虑更换电解液添加剂。".to_string()
            }
            DegradationMode::SEIGrowth => {
                "检测到SEI膜过度生长迹象，内阻上升加快。建议：1) 优化预充电流密度，避免SEI膜过厚；2) 检查电解液添加剂配比；3) 降低化成最高温度；4) 评估是否需要延长静置时间。".to_string()
            }
            DegradationMode::MixedDegradation => {
                "检测到多种衰减机制同时存在。建议：1) 进行全面的电池失效分析；2) 结合历史数据追溯根因；3) 考虑调整整个化成工艺参数；4) 与材料供应商沟通评估原材料质量。".to_string()
            }
        }
    }

    pub fn get_historical_modes(
        &self,
        cabinet_id: u16,
        channel_id: u32,
    ) -> Vec<(u16, DegradationMode, f64)> {
        let key = (cabinet_id, channel_id);
        self.historical_analysis
            .get(&key)
            .cloned()
            .unwrap_or_default()
    }
}
