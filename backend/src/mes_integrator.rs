use crate::models::{BatchCapacityDistribution, BatchInfo, BatchQueryRequest, DegradedCellRecord, MesSyncResult, MesSyncStatus, ProcessParamRecord};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct MesConfig {
    pub mes_api_url: String,
    pub mes_api_key: String,
    pub sync_interval_seconds: u64,
    pub retry_count: u32,
    pub retry_interval_seconds: u64,
    pub batch_size: usize,
    pub enable_automatic_sync: bool,
}

impl Default for MesConfig {
    fn default() -> Self {
        Self {
            mes_api_url: "http://mes-server/api/v1".to_string(),
            mes_api_key: "".to_string(),
            sync_interval_seconds: 300,
            retry_count: 3,
            retry_interval_seconds: 10,
            batch_size: 100,
            enable_automatic_sync: true,
        }
    }
}

pub struct MesIntegrationService {
    config: MesConfig,
    pending_params: Vec<ProcessParamRecord>,
    pending_degraded: Vec<DegradedCellRecord>,
    sync_history: HashMap<String, MesSyncResult>,
    batch_info_cache: HashMap<String, BatchInfo>,
}

impl MesIntegrationService {
    pub fn new(config: MesConfig) -> Self {
        Self {
            config,
            pending_params: Vec::new(),
            pending_degraded: Vec::new(),
            sync_history: HashMap::new(),
            batch_info_cache: HashMap::new(),
        }
    }

    pub fn record_process_param(&mut self, record: ProcessParamRecord) {
        self.pending_params.push(record);

        if self.config.enable_automatic_sync && self.pending_params.len() >= self.config.batch_size {
            let _ = self.sync_process_params();
        }
    }

    pub fn record_degraded_cell(&mut self, record: DegradedCellRecord) {
        self.pending_degraded.push(record);

        if self.config.enable_automatic_sync && self.pending_degraded.len() >= self.config.batch_size / 2 {
            let _ = self.sync_degraded_cells();
        }
    }

    pub fn sync_process_params(&mut self) -> Result<MesSyncResult, String> {
        if self.pending_params.is_empty() {
            return Ok(MesSyncResult {
                batch_id: "NONE".to_string(),
                total_records: 0,
                synced_records: 0,
                failed_records: 0,
                error_messages: Vec::new(),
                sync_time_ms: 0,
            });
        }

        let start_time = std::time::Instant::now();
        let batch_id = self.pending_params.first().map(|p| p.batch_id.clone()).unwrap_or_default();
        let total_records = self.pending_params.len();

        let mut synced = 0;
        let mut failed = 0;
        let mut errors = Vec::new();

        for record in self.pending_params.iter_mut() {
            match self.send_param_to_mes(record) {
                Ok(_) => {
                    record.mes_sync_status = MesSyncStatus::Synced;
                    record.mes_sync_time = Some(Utc::now());
                    synced += 1;
                }
                Err(e) => {
                    record.mes_sync_status = MesSyncStatus::Failed;
                    errors.push(format!("通道{}: {}", record.channel_id, e));
                    failed += 1;
                }
            }
        }

        let result = MesSyncResult {
            batch_id: batch_id.clone(),
            total_records,
            synced_records: synced,
            failed_records: failed,
            error_messages: errors,
            sync_time_ms: start_time.elapsed().as_millis() as u64,
        };

        self.sync_history.insert(format!("params_{}", batch_id), result.clone());

        self.pending_params.retain(|r| r.mes_sync_status == MesSyncStatus::Failed);

        Ok(result)
    }

    pub fn sync_degraded_cells(&mut self) -> Result<MesSyncResult, String> {
        if self.pending_degraded.is_empty() {
            return Ok(MesSyncResult {
                batch_id: "NONE".to_string(),
                total_records: 0,
                synced_records: 0,
                failed_records: 0,
                error_messages: Vec::new(),
                sync_time_ms: 0,
            });
        }

        let start_time = std::time::Instant::now();
        let batch_id = self.pending_degraded.first().map(|p| p.batch_id.clone()).unwrap_or_default();
        let total_records = self.pending_degraded.len();

        let mut synced = 0;
        let mut failed = 0;
        let mut errors = Vec::new();

        for record in self.pending_degraded.iter_mut() {
            match self.send_degraded_cell_to_mes(record) {
                Ok(_) => {
                    record.mes_sync_status = MesSyncStatus::Synced;
                    record.mes_sync_time = Some(Utc::now());
                    synced += 1;
                }
                Err(e) => {
                    record.mes_sync_status = MesSyncStatus::Failed;
                    errors.push(format!("通道{}: {}", record.channel_id, e));
                    failed += 1;
                }
            }
        }

        let result = MesSyncResult {
            batch_id: batch_id.clone(),
            total_records,
            synced_records: synced,
            failed_records: failed,
            error_messages: errors,
            sync_time_ms: start_time.elapsed().as_millis() as u64,
        };

        self.sync_history.insert(format!("degraded_{}", batch_id), result.clone());

        self.pending_degraded.retain(|r| r.mes_sync_status == MesSyncStatus::Failed);

        Ok(result)
    }

    pub fn sync_batch_summary(&mut self, batch_info: &BatchInfo) -> Result<MesSyncResult, String> {
        let start_time = std::time::Instant::now();
        let mut errors = Vec::new();

        let mut batch_info = batch_info.clone();

        match self.send_batch_summary_to_mes(&batch_info) {
            Ok(_) => {
                batch_info.mes_sync_status = MesSyncStatus::Synced;
                batch_info.mes_sync_time = Some(Utc::now());
            }
            Err(e) => {
                batch_info.mes_sync_status = MesSyncStatus::Failed;
                errors.push(e);
            }
        }

        self.batch_info_cache.insert(batch_info.batch_id.clone(), batch_info.clone());

        let result = MesSyncResult {
            batch_id: batch_info.batch_id.clone(),
            total_records: 1,
            synced_records: if batch_info.mes_sync_status == MesSyncStatus::Synced { 1 } else { 0 },
            failed_records: if batch_info.mes_sync_status == MesSyncStatus::Failed { 1 } else { 0 },
            error_messages: errors,
            sync_time_ms: start_time.elapsed().as_millis() as u64,
        };

        self.sync_history.insert(format!("batch_{}", batch_info.batch_id), result.clone());

        Ok(result)
    }

    pub fn query_batch(&self, request: &BatchQueryRequest) -> Vec<BatchInfo> {
        let mut results: Vec<BatchInfo> = self.batch_info_cache.values().cloned().collect();

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

    pub fn get_sync_status(&self, batch_id: &str) -> Option<&MesSyncResult> {
        self.sync_history.get(batch_id)
    }

    pub fn get_pending_counts(&self) -> (usize, usize) {
        (self.pending_params.len(), self.pending_degraded.len())
    }

    fn send_param_to_mes(&self, record: &ProcessParamRecord) -> Result<(), String> {
        if self.config.mes_api_key.is_empty() {
            return Ok(());
        }

        let _payload = serde_json::json!({
            "batch_id": record.batch_id,
            "cabinet_id": record.cabinet_id,
            "channel_id": record.channel_id,
            "cycle_index": record.cycle_index,
            "stage": record.stage as u8,
            "param_type": record.param_type as u8,
            "param_value": record.param_value,
            "param_unit": record.param_unit,
            "upper_limit": record.upper_limit,
            "lower_limit": record.lower_limit,
            "is_out_of_spec": record.is_out_of_spec,
            "timestamp": record.timestamp.to_rfc3339(),
        });

        Ok(())
    }

    fn send_degraded_cell_to_mes(&self, record: &DegradedCellRecord) -> Result<(), String> {
        if self.config.mes_api_key.is_empty() {
            return Ok(());
        }

        let _payload = serde_json::json!({
            "batch_id": record.batch_id,
            "cabinet_id": record.cabinet_id,
            "channel_id": record.channel_id,
            "cycle_index": record.cycle_index,
            "capacity": record.capacity,
            "capacity_ratio": record.capacity_ratio,
            "internal_resistance": record.internal_resistance,
            "degradation_reason": record.degradation_reason,
            "grade": record.grade as u8,
            "timestamp": record.timestamp.to_rfc3339(),
        });

        Ok(())
    }

    fn send_batch_summary_to_mes(&self, batch_info: &BatchInfo) -> Result<(), String> {
        if self.config.mes_api_key.is_empty() {
            return Ok(());
        }

        let _payload = serde_json::json!({
            "batch_id": batch_info.batch_id,
            "product_code": batch_info.product_code,
            "battery_model": batch_info.battery_model,
            "rated_capacity": batch_info.rated_capacity,
            "total_cells": batch_info.total_cells,
            "start_time": batch_info.start_time.to_rfc3339(),
            "end_time": batch_info.end_time.map(|t| t.to_rfc3339()),
            "operator": batch_info.operator,
            "shift": batch_info.shift,
            "avg_capacity": batch_info.avg_capacity,
            "yield_rate": batch_info.yield_rate,
            "grade_a_ratio": batch_info.grade_a_ratio,
            "grade_b_ratio": batch_info.grade_b_ratio,
            "grade_c_ratio": batch_info.grade_c_ratio,
            "rejected_ratio": batch_info.rejected_ratio,
            "avg_internal_resistance": batch_info.avg_internal_resistance,
            "remarks": batch_info.remarks,
        });

        Ok(())
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

        self.batch_info_cache.insert(batch_id, batch_info.clone());
        batch_info
    }

    pub fn update_batch_statistics(
        &mut self,
        batch_id: &str,
        capacities: &[f64],
        resistances: &[f64],
        grades: &[crate::models::CellGrade],
    ) -> Option<&BatchInfo> {
        let batch_info = self.batch_info_cache.get_mut(batch_id)?;

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

        self.batch_info_cache.get(batch_id)
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

    pub fn retry_failed_syncs(&mut self) -> Vec<MesSyncResult> {
        let mut results = Vec::new();

        if !self.pending_params.is_empty() {
            if let Ok(result) = self.sync_process_params() {
                if result.total_records > 0 {
                    results.push(result);
                }
            }
        }

        if !self.pending_degraded.is_empty() {
            if let Ok(result) = self.sync_degraded_cells() {
                if result.total_records > 0 {
                    results.push(result);
                }
            }
        }

        results
    }

    pub fn get_all_batches(&self) -> Vec<&BatchInfo> {
        let mut batches: Vec<&BatchInfo> = self.batch_info_cache.values().collect();
        batches.sort_by(|a, b| b.start_time.cmp(&a.start_time));
        batches
    }
}
