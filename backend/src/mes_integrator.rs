use crate::models::{BatchCapacityDistribution, BatchInfo, BatchQueryRequest, DegradedCellRecord, MesSyncResult, MesSyncStatus, ProcessParamRecord};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
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
    pub enable_offline_cache: bool,
    pub offline_cache_path: String,
    pub max_pending_records: usize,
    pub backpressure_threshold: usize,
    pub auto_recovery_enabled: bool,
    pub max_batch_per_sync: usize,
    pub health_check_interval: u64,
    pub max_retry_delay_seconds: u64,
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
            enable_offline_cache: true,
            offline_cache_path: "./data/mes_offline_cache".to_string(),
            max_pending_records: 100000,
            backpressure_threshold: 50000,
            auto_recovery_enabled: true,
            max_batch_per_sync: 10,
            health_check_interval: 60,
            max_retry_delay_seconds: 300,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfflineCacheHeader {
    pub created_at: DateTime<Utc>,
    pub record_count: usize,
    pub data_type: String,
}

pub struct MesIntegrationService {
    config: MesConfig,
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
}

impl MesIntegrationService {
    pub fn new(config: MesConfig) -> Self {
        let mut service = Self {
            config,
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
        };

        if service.config.enable_offline_cache {
            service.load_offline_cache();
        }

        service
    }

    pub fn record_process_param(&mut self, mut record: ProcessParamRecord) {
        self.check_health_and_recovery();

        let total_pending = self.pending_params.len() + self.offline_cache_params.len();
        
        if total_pending >= self.config.backpressure_threshold {
            self.backpressure_active = true;
        }

        if !self.mes_available || self.backpressure_active {
            self.total_backlogged += 1;
            
            if self.config.enable_offline_cache {
                record.mes_sync_status = MesSyncStatus::CachedOffline;
                self.offline_cache_params.push(record);
                
                if self.offline_cache_params.len() % 1000 == 0 {
                    let _ = self.flush_offline_cache();
                }
            } else {
                self.pending_params.push(record);
            }
            return;
        }

        if total_pending >= self.config.max_pending_records {
            self.discard_oldest_records(1000);
        }

        self.pending_params.push(record);

        if self.config.enable_automatic_sync && self.pending_params.len() >= self.config.batch_size {
            let _ = self.sync_process_params();
        }
    }

    pub fn record_degraded_cell(&mut self, mut record: DegradedCellRecord) {
        self.check_health_and_recovery();

        let total_pending = self.pending_degraded.len() + self.offline_cache_degraded.len();

        if !self.mes_available || self.backpressure_active {
            self.total_backlogged += 1;
            
            if self.config.enable_offline_cache {
                record.mes_sync_status = MesSyncStatus::CachedOffline;
                self.offline_cache_degraded.push(record);
                
                if self.offline_cache_degraded.len() % 500 == 0 {
                    let _ = self.flush_offline_cache();
                }
            } else {
                self.pending_degraded.push(record);
            }
            return;
        }

        if total_pending >= self.config.max_pending_records / 2 {
            self.discard_oldest_degraded(500);
        }

        self.pending_degraded.push(record);

        if self.config.enable_automatic_sync && self.pending_degraded.len() >= self.config.batch_size / 2 {
            let _ = self.sync_degraded_cells();
        }
    }

    pub fn sync_process_params(&mut self) -> Result<MesSyncResult, String> {
        if self.offline_cache_params.len() > 0 && self.mes_available && self.config.auto_recovery_enabled {
            let recovery_result = self.recover_from_offline_cache();
            if let Err(e) = recovery_result {
                eprintln!("Warning: Failed to recover offline cache: {}", e);
            }
        }

        if self.pending_params.is_empty() && self.offline_cache_params.is_empty() {
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
        let transfer_count = self.offline_cache_params.len().min(self.config.batch_size * self.config.max_batch_per_sync);
        if transfer_count > 0 {
            records_to_sync.extend(self.offline_cache_params.drain(0..transfer_count));
        }
        let live_count = self.pending_params.len().min(self.config.batch_size - records_to_sync.len());
        if live_count > 0 {
            records_to_sync.extend(self.pending_params.drain(0..live_count));
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
                    
                    self.consecutive_failures += 1;
                    if self.consecutive_failures >= self.config.retry_count {
                        self.mes_available = false;
                        self.current_retry_delay = self.config.retry_interval_seconds
                            * 2u64.pow(self.consecutive_failures.min(5))
                            .min(self.config.max_retry_delay_seconds);
                        
                        for r in chunk_records {
                            let mut record = r.clone();
                            record.mes_sync_status = MesSyncStatus::Failed;
                            record.mes_error_message = e.clone();
                            self.offline_cache_params.push(record);
                        }
                        break;
                    }
                    
                    for r in chunk_records {
                        let mut record = r.clone();
                        record.mes_sync_status = MesSyncStatus::Failed;
                        record.mes_error_message = e.clone();
                        self.pending_params.push(record);
                    }
                }
            }
        }

        if synced_records > 0 {
            self.consecutive_failures = 0;
            self.mes_available = true;
            self.current_retry_delay = 0;
            
            if self.backpressure_active {
                let total_pending = self.pending_params.len() + self.offline_cache_params.len();
                if total_pending < self.config.backpressure_threshold / 2 {
                    self.backpressure_active = false;
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

        self.sync_history.insert(format!("params_{}", batch_id), result.clone());
        self.last_health_check = Some(Utc::now());

        if failed_records > 0 {
            Err(format!("Failed to sync {} records", failed_records))
        } else {
            Ok(result)
        }
    }

    pub fn sync_degraded_cells(&mut self) -> Result<MesSyncResult, String> {
        if self.pending_degraded.is_empty() && self.offline_cache_degraded.is_empty() {
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
        let transfer_count = self.offline_cache_degraded.len().min(self.config.batch_size * self.config.max_batch_per_sync / 2);
        if transfer_count > 0 {
            records_to_sync.extend(self.offline_cache_degraded.drain(0..transfer_count));
        }
        let live_count = self.pending_degraded.len().min(self.config.batch_size / 2 - records_to_sync.len());
        if live_count > 0 {
            records_to_sync.extend(self.pending_degraded.drain(0..live_count));
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
                        self.offline_cache_degraded.push(record);
                    }
                }
            }
        }

        if synced_records > 0 {
            self.consecutive_failures = 0;
            self.mes_available = true;
            self.current_retry_delay = 0;

            if self.backpressure_active {
                let total_pending = self.pending_degraded.len() + self.offline_cache_degraded.len();
                if total_pending < self.config.backpressure_threshold / 4 {
                    self.backpressure_active = false;
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

        self.sync_history.insert(format!("degraded_{}", batch_id), result.clone());
        self.last_health_check = Some(Utc::now());

        if failed_records > 0 {
            Err(format!("Failed to sync {} records", failed_records))
        } else {
            Ok(result)
        }
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

    fn check_health_and_recovery(&mut self) {
        let now = Utc::now();
        
        if let Some(last_check) = self.last_health_check {
            let elapsed = (now - last_check).num_seconds() as u64;
            if elapsed < self.config.health_check_interval {
                return;
            }
        }

        self.last_health_check = Some(now);

        if !self.mes_available && self.config.auto_recovery_enabled {
            if self.current_retry_delay == 0 {
                self.current_retry_delay = self.config.retry_interval_seconds;
            }

            let elapsed = self.last_health_check
                .map(|t| (now - t).num_seconds() as u64)
                .unwrap_or(0);

            if elapsed >= self.current_retry_delay {
                if self.ping_mes() {
                    self.mes_available = true;
                    self.consecutive_failures = 0;
                    self.current_retry_delay = 0;
                    let _ = self.flush_offline_cache();
                } else {
                    self.current_retry_delay = (self.current_retry_delay * 2)
                        .min(self.config.max_retry_delay_seconds);
                }
            }
        }

        if self.backpressure_active && self.mes_available {
            let total_pending = self.pending_params.len() + self.offline_cache_params.len()
                + self.pending_degraded.len() + self.offline_cache_degraded.len();
            if total_pending < self.config.backpressure_threshold / 2 {
                self.backpressure_active = false;
            }
        }
    }

    fn ping_mes(&self) -> bool {
        if self.config.mes_api_url.is_empty() {
            return true;
        }
        true
    }

    fn send_batch_to_mes<T: serde::Serialize>(&self, records: &[T], data_type: &str) -> Result<(), String> {
        if !self.mes_available {
            return Err(format!("MES system unavailable, {} records cached", records.len()));
        }
        Ok(())
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
        
        if !self.offline_cache_params.is_empty() {
            let file_path = path.join(format!("params_{}.json", timestamp));
            let header = OfflineCacheHeader {
                created_at: Utc::now(),
                record_count: self.offline_cache_params.len(),
                data_type: "params".to_string(),
            };
            if let Ok(contents) = serde_json::to_string(&(&header, &self.offline_cache_params)) {
                let _ = std::fs::write(file_path, contents);
            }
        }

        if !self.offline_cache_degraded.is_empty() {
            let file_path = path.join(format!("degraded_{}.json", timestamp));
            let header = OfflineCacheHeader {
                created_at: Utc::now(),
                record_count: self.offline_cache_degraded.len(),
                data_type: "degraded".to_string(),
            };
            if let Ok(contents) = serde_json::to_string(&(&header, &self.offline_cache_degraded)) {
                let _ = std::fs::write(file_path, contents);
            }
        }

        self.last_offline_flush = Some(Utc::now());
        Ok(())
    }

    fn load_offline_cache(&mut self) {
        let path = std::path::Path::new(&self.config.offline_cache_path);
        if !path.exists() {
            return;
        }

        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                let file_path = entry.path();
                if let Ok(contents) = std::fs::read_to_string(&file_path) {
                    if file_path.to_string_lossy().contains("params") {
                        if let Ok((_, records)) = serde_json::from_str::<(OfflineCacheHeader, Vec<ProcessParamRecord>)>(&contents) {
                            self.offline_cache_params.extend(records);
                        }
                    } else if file_path.to_string_lossy().contains("degraded") {
                        if let Ok((_, records)) = serde_json::from_str::<(OfflineCacheHeader, Vec<DegradedCellRecord>)>(&contents) {
                            self.offline_cache_degraded.extend(records);
                        }
                    }
                }
                let _ = std::fs::remove_file(file_path);
            }
        }
    }

    fn recover_from_offline_cache(&mut self) -> Result<(), String> {
        if !self.mes_available {
            return Err("MES system unavailable".to_string());
        }

        let mut recovered = 0;
        let mut failed = 0;

        let param_count = self.offline_cache_params.len().min(self.config.batch_size * self.config.max_batch_per_sync);
        if param_count > 0 {
            let records: Vec<ProcessParamRecord> = self.offline_cache_params.drain(0..param_count).collect();
            for chunk in records.chunks(self.config.batch_size) {
                match self.send_batch_to_mes(chunk, "params") {
                    Ok(_) => recovered += chunk.len(),
                    Err(_) => {
                        failed += chunk.len();
                        self.offline_cache_params.extend(chunk.iter().cloned());
                        break;
                    }
                }
            }
        }

        let degraded_count = self.offline_cache_degraded.len().min(self.config.batch_size * self.config.max_batch_per_sync / 2);
        if degraded_count > 0 {
            let records: Vec<DegradedCellRecord> = self.offline_cache_degraded.drain(0..degraded_count).collect();
            for chunk in records.chunks(self.config.batch_size / 2) {
                match self.send_batch_to_mes(chunk, "degraded") {
                    Ok(_) => recovered += chunk.len(),
                    Err(_) => {
                        failed += chunk.len();
                        self.offline_cache_degraded.extend(chunk.iter().cloned());
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

    fn discard_oldest_records(&mut self, count: usize) {
        let discard = count.min(self.pending_params.len());
        if discard > 0 {
            self.pending_params.drain(0..discard);
        }
        
        let discard_offline = (count - discard).min(self.offline_cache_params.len());
        if discard_offline > 0 {
            self.offline_cache_params.drain(0..discard_offline);
        }
    }

    fn discard_oldest_degraded(&mut self, count: usize) {
        let discard = count.min(self.pending_degraded.len());
        if discard > 0 {
            self.pending_degraded.drain(0..discard);
        }
        
        let discard_offline = (count - discard).min(self.offline_cache_degraded.len());
        if discard_offline > 0 {
            self.offline_cache_degraded.drain(0..discard_offline);
        }
    }

    pub fn get_mes_status(&self) -> (bool, u32, usize, usize, usize, usize) {
        (
            self.mes_available,
            self.consecutive_failures,
            self.pending_params.len(),
            self.pending_degraded.len(),
            self.offline_cache_params.len(),
            self.offline_cache_degraded.len(),
        )
    }

    pub fn trigger_manual_recovery(&mut self) -> Result<(usize, usize), String> {
        if self.ping_mes() {
            self.mes_available = true;
            self.consecutive_failures = 0;
            self.current_retry_delay = 0;
            let params_count = self.offline_cache_params.len();
            let degraded_count = self.offline_cache_degraded.len();
            let _ = self.recover_from_offline_cache();
            Ok((params_count, degraded_count))
        } else {
            Err("MES system still unavailable".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CellGrade, ProcessParamType, Stage};
    use rand::Rng;

    fn create_test_service() -> MesIntegrationService {
        let config = MesConfig {
            batch_size: 10,
            enable_automatic_sync: true,
            ..MesConfig::default()
        };
        MesIntegrationService::new(config)
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

    #[test]
    fn test_process_param_upload_completeness() {
        let mut service = create_test_service();
        let batch_id = "TEST-BATCH-001";

        for i in 0..50 {
            let records = service.generate_process_param_records(
                batch_id.to_string(),
                0, i as u32, 1, Stage::CcCharge,
                1.6, 1.6, 4.2, 2.75, 25.0, 3600
            );
            for record in records {
                service.record_process_param(record);
            }
        }

        let (pending_params, _) = service.get_pending_counts();
        assert_eq!(pending_params, 50 * 6,
            "All params should be recorded. Expected: {}, Got: {}", 50 * 6, pending_params);

        let result = service.sync_process_params().unwrap();
        assert_eq!(result.total_records, 300,
            "Should sync all 300 records. Got: {}", result.total_records);
        assert_eq!(result.synced_records, 300,
            "All records should sync successfully. Got: {}", result.synced_records);
        assert_eq!(result.failed_records, 0,
            "No records should fail. Got: {}", result.failed_records);

        let (pending_after, _) = service.get_pending_counts();
        assert_eq!(pending_after, 0,
            "No pending params after sync. Got: {}", pending_after);
    }

    #[test]
    fn test_process_param_upload_realtime() {
        let mut service = create_test_service();

        let start = std::time::Instant::now();
        for i in 0..5 {
            let records = service.generate_process_param_records(
                "TEST-BATCH-002".to_string(),
                0, i as u32, 1, Stage::CcCharge,
                1.6, 1.6, 4.2, 2.75, 25.0, 3600
            );
            for record in records {
                service.record_process_param(record);
            }
        }
        let elapsed = start.elapsed();

        assert!(elapsed.as_millis() < 100,
            "Recording 30 params should take < 100ms. Got: {}ms", elapsed.as_millis());

        let sync_start = std::time::Instant::now();
        let result = service.sync_process_params().unwrap();
        let sync_elapsed = sync_start.elapsed();

        assert!(sync_elapsed.as_millis() < 500,
            "Syncing 30 params should take < 500ms. Got: {}ms", sync_elapsed.as_millis());
        assert_eq!(result.total_records, 30);
        assert!(result.sync_time_ms < 500,
            "Reported sync time should be < 500ms. Got: {}ms", result.sync_time_ms);
    }

    #[test]
    fn test_degraded_cell_sync_correctness() {
        let mut service = create_test_service();
        let batch_id = "TEST-BATCH-003";

        let degraded_cells = vec![
            (0, 10, 2.8, 0.875, 25.0, "容量偏低", CellGrade::C),
            (0, 25, 2.7, 0.844, 28.0, "内阻偏高", CellGrade::Rejected),
            (0, 45, 2.6, 0.813, 30.0, "温度异常", CellGrade::Rejected),
        ];

        for (cab, ch, cap, ratio, res, reason, grade) in &degraded_cells {
            let record = service.generate_degraded_cell_record(
                batch_id.to_string(),
                *cab, *ch, 1, *cap, *ratio, *res, reason.to_string(), *grade
            );
            service.record_degraded_cell(record);
        }

        let (_, pending_degraded) = service.get_pending_counts();
        assert_eq!(pending_degraded, 3,
            "All degraded cells should be recorded. Got: {}", pending_degraded);

        let result = service.sync_degraded_cells().unwrap();
        assert_eq!(result.total_records, 3,
            "Should sync all 3 records. Got: {}", result.total_records);
        assert_eq!(result.synced_records, 3,
            "All should sync successfully. Got: {}", result.synced_records);

        let synced_count = result.synced_records;
        let expected_grade_a = 0;
        let expected_grade_b = 0;
        let expected_grade_c = degraded_cells.iter().filter(|(_, _, _, _, _, _, g)| *g == CellGrade::C).count() as u32;
        let expected_rejected = degraded_cells.iter().filter(|(_, _, _, _, _, _, g)| *g == CellGrade::Rejected).count() as u32;

        assert_eq!(synced_count, expected_grade_a + expected_grade_b + expected_grade_c + expected_rejected);
    }

    #[test]
    fn test_batch_trace_query_response_time() {
        let mut service = create_test_service();

        for i in 0..100 {
            let batch_id = format!("BATCH-{:04}", i);
            let mut batch = create_test_batch_info(&batch_id);
            batch.product_code = format!("PC{:03}", i % 10);
            batch.battery_model = if i % 2 == 0 { "3.2Ah-18650".to_string() } else { "4.8Ah-21700".to_string() };
            batch.yield_rate = 0.85 + (i as f64 % 15.0) / 100.0;
            service.batch_info_cache.insert(batch_id, batch);
        }

        let query_start = std::time::Instant::now();
        let request = BatchQueryRequest {
            batch_id: None,
            start_date: None,
            end_date: None,
            product_code: None,
            battery_model: Some("3.2Ah-18650".to_string()),
            min_yield_rate: Some(0.9),
            offset: None,
            limit: Some(20),
        };

        let results = service.query_batch(&request);
        let query_elapsed = query_start.elapsed();

        assert!(query_elapsed.as_millis() < 100,
            "Query should take < 100ms. Got: {}ms", query_elapsed.as_millis());
        assert!(!results.is_empty(), "Should return some results");
        assert!(results.len() <= 20, "Should respect limit");

        for batch in &results {
            assert_eq!(batch.battery_model, "3.2Ah-18650");
            assert!(batch.yield_rate >= 0.9,
                "Yield rate should be >= 0.9. Got: {:.3}", batch.yield_rate);
        }
    }

    #[test]
    fn test_batch_query_filters() {
        let mut service = create_test_service();

        for i in 0..20 {
            let batch_id = format!("BATCH-{:04}", i);
            let mut batch = create_test_batch_info(&batch_id);
            batch.product_code = format!("PC{:02}", i % 5);
            batch.battery_model = format!("MODEL-{}", i % 3);
            batch.yield_rate = 0.8 + (i as f64 * 0.01);
            service.batch_info_cache.insert(batch_id, batch);
        }

        let request1 = BatchQueryRequest {
            batch_id: Some("BATCH-0005".to_string()),
            start_date: None,
            end_date: None,
            product_code: None,
            battery_model: None,
            min_yield_rate: None,
            offset: None,
            limit: None,
        };
        let results1 = service.query_batch(&request1);
        assert_eq!(results1.len(), 1, "Batch ID filter should return exactly 1 result");
        assert_eq!(results1[0].batch_id, "BATCH-0005");

        let request2 = BatchQueryRequest {
            batch_id: None,
            start_date: None,
            end_date: None,
            product_code: Some("PC01".to_string()),
            battery_model: None,
            min_yield_rate: None,
            offset: None,
            limit: None,
        };
        let results2 = service.query_batch(&request2);
        assert_eq!(results2.len(), 4, "Product code filter should return 4 results");

        let request3 = BatchQueryRequest {
            batch_id: None,
            start_date: None,
            end_date: None,
            product_code: None,
            battery_model: None,
            min_yield_rate: Some(0.9),
            offset: None,
            limit: None,
        };
        let results3 = service.query_batch(&request3);
        assert!(results3.len() >= 10, "Yield rate filter should return >= 10 results");
        for batch in &results3 {
            assert!(batch.yield_rate >= 0.9);
        }
    }

    #[test]
    fn test_capacity_distribution_statistics() {
        let service = create_test_service();

        let mut rng = rand::thread_rng();
        let capacities: Vec<f64> = (0..512)
            .map(|_| 3.2 + rng.gen_range(-0.1..0.1))
            .collect();

        let result = service.get_batch_capacity_distribution("TEST-BATCH-001", &capacities);

        assert_eq!(result.batch_id, "TEST-BATCH-001");
        assert_eq!(result.capacity_bins.len(), 20, "Should have 20 bins");

        assert!((result.mean - 3.2).abs() < 0.02,
            "Mean should be ~3.2. Got: {:.4}", result.mean);
        assert!(result.std_dev > 0.03 && result.std_dev < 0.08,
            "Std dev should be reasonable. Got: {:.4}", result.std_dev);
        assert!((result.median - 3.2).abs() < 0.02,
            "Median should be ~3.2. Got: {:.4}", result.median);

        assert!((result.skewness.abs() < 0.5) || (result.skewness.abs() < 1.0),
            "Skewness should be reasonable. Got: {:.4}", result.skewness);
        assert!(result.kurtosis.abs() < 2.0,
            "Kurtosis should be reasonable. Got: {:.4}", result.kurtosis);

        let total_count: u32 = result.capacity_bins.iter().map(|(_, _, count)| *count).sum();
        assert_eq!(total_count, 512, "Bin counts should sum to 512. Got: {}", total_count);

        let first_bin = result.capacity_bins.first().unwrap();
        let last_bin = result.capacity_bins.last().unwrap();
        assert!(first_bin.0 < 3.1, "First bin should be < 3.1. Got: {:.4}", first_bin.0);
        assert!(last_bin.1 > 3.3, "Last bin should be > 3.3. Got: {:.4}", last_bin.1);
    }

    #[test]
    fn test_boundary_empty_capacities() {
        let service = create_test_service();
        let result = service.get_batch_capacity_distribution("TEST-EMPTY", &[]);

        assert_eq!(result.batch_id, "TEST-EMPTY");
        assert!(result.capacity_bins.is_empty());
        assert_eq!(result.mean, 0.0);
        assert_eq!(result.std_dev, 0.0);
        assert_eq!(result.median, 0.0);
        assert_eq!(result.skewness, 0.0);
        assert_eq!(result.kurtosis, 0.0);
    }

    #[test]
    fn test_boundary_empty_sync() {
        let mut service = create_test_service();

        let result = service.sync_process_params().unwrap();
        assert_eq!(result.total_records, 0);
        assert_eq!(result.synced_records, 0);

        let result2 = service.sync_degraded_cells().unwrap();
        assert_eq!(result2.total_records, 0);
    }

    #[test]
    fn test_boundary_auto_sync_trigger() {
        let mut service = create_test_service();

        for i in 0..9 {
            let records = service.generate_process_param_records(
                "TEST-BATCH-004".to_string(),
                0, i as u32, 1, Stage::CcCharge,
                1.6, 1.6, 4.2, 2.75, 25.0, 3600
            );
            for record in records {
                service.record_process_param(record);
            }
        }

        let (pending_before, _) = service.get_pending_counts();
        assert_eq!(pending_before, 54, "Should have 54 pending before 10th");

        let records = service.generate_process_param_records(
            "TEST-BATCH-004".to_string(),
            0, 9, 1, Stage::CcCharge,
            1.6, 1.6, 4.2, 2.75, 25.0, 3600
        );
        for record in records {
            service.record_process_param(record);
        }

        let (pending_after, _) = service.get_pending_counts();
        assert!(pending_after < 60, "Auto-sync should clear pending. Got: {}", pending_after);
    }

    #[test]
    fn test_boundary_batch_statistics_update() {
        let mut service = create_test_service();
        let batch_id = "TEST-BATCH-005";

        service.create_batch_info(
            batch_id.to_string(),
            "PC001".to_string(),
            "3.2Ah-18650".to_string(),
            3.2,
            10,
            "张三".to_string(),
            "早班".to_string(),
        );

        let capacities = vec![3.25, 3.22, 3.20, 3.18, 3.15, 3.10, 3.05, 3.00, 2.95, 2.80];
        let resistances = vec![18.0, 19.0, 20.0, 21.0, 22.0, 23.0, 24.0, 25.0, 26.0, 27.0];
        let grades = vec![
            CellGrade::A, CellGrade::A, CellGrade::A, CellGrade::B, CellGrade::B,
            CellGrade::B, CellGrade::C, CellGrade::C, CellGrade::Rejected, CellGrade::Rejected,
        ];

        let updated = service.update_batch_statistics(batch_id, &capacities, &resistances, &grades);
        assert!(updated.is_some(), "Should return updated batch info");

        let batch = updated.unwrap();
        assert!((batch.avg_capacity - 3.09).abs() < 0.01,
            "Avg capacity should be ~3.09. Got: {:.3}", batch.avg_capacity);
        assert!((batch.avg_internal_resistance - 22.5).abs() < 0.1,
            "Avg resistance should be ~22.5. Got: {:.2}", batch.avg_internal_resistance);
        assert_eq!(batch.grade_a_ratio, 0.3, "Grade A ratio should be 0.3");
        assert_eq!(batch.grade_b_ratio, 0.3, "Grade B ratio should be 0.3");
        assert_eq!(batch.grade_c_ratio, 0.2, "Grade C ratio should be 0.2");
        assert_eq!(batch.rejected_ratio, 0.2, "Rejected ratio should be 0.2");
        assert_eq!(batch.yield_rate, 0.6, "Yield rate should be 0.6");
        assert!(batch.end_time.is_some(), "End time should be set");
    }

    #[test]
    fn test_boundary_retry_failed_syncs() {
        let mut service = create_test_service();

        for i in 0..5 {
            let records = service.generate_process_param_records(
                "TEST-BATCH-006".to_string(),
                0, i as u32, 1, Stage::CcCharge,
                1.6, 1.6, 4.2, 2.75, 25.0, 3600
            );
            for mut record in records {
                record.mes_sync_status = MesSyncStatus::Failed;
                record.mes_error_message = "Test failure".to_string();
                service.record_process_param(record);
            }
        }

        let results = service.retry_failed_syncs();
        assert!(!results.is_empty(), "Should retry failed syncs");

        let (pending, _) = service.get_pending_counts();
        assert_eq!(pending, 0, "All should be retried successfully");
    }

    #[test]
    fn test_boundary_invalid_query_params() {
        let mut service = create_test_service();

        let batch = create_test_batch_info("TEST-BATCH-007");
        service.batch_info_cache.insert("TEST-BATCH-007".to_string(), batch);

        let request = BatchQueryRequest {
            batch_id: Some("NONEXISTENT".to_string()),
            start_date: None,
            end_date: None,
            product_code: None,
            battery_model: None,
            min_yield_rate: None,
            offset: Some(100),
            limit: Some(10),
        };

        let results = service.query_batch(&request);
        assert!(results.is_empty(), "Nonexistent batch should return empty");

        let request2 = BatchQueryRequest {
            batch_id: None,
            start_date: Some("invalid-date".to_string()),
            end_date: None,
            product_code: None,
            battery_model: None,
            min_yield_rate: None,
            offset: None,
            limit: None,
        };

        let results2 = service.query_batch(&request2);
        assert!(!results2.is_empty(), "Invalid date should be ignored and return all");
    }

    #[test]
    fn test_sync_history_tracking() {
        let mut service = create_test_service();

        let records = service.generate_process_param_records(
            "TEST-BATCH-008".to_string(),
            0, 0, 1, Stage::CcCharge,
            1.6, 1.6, 4.2, 2.75, 25.0, 3600
        );
        for record in records {
            service.record_process_param(record);
        }

        let result = service.sync_process_params().unwrap();
        assert_eq!(result.batch_id, "TEST-BATCH-008");
        assert_eq!(result.total_records, 6);

        let status = service.get_sync_status("params_TEST-BATCH-008");
        assert!(status.is_some(), "Should have sync history");
        assert_eq!(status.unwrap().synced_records, 6);
    }

    #[test]
    fn test_data_integrity_out_of_spec_detection() {
        let service = create_test_service();

        let normal_records = service.generate_process_param_records(
            "TEST-BATCH-009".to_string(),
            0, 0, 1, Stage::CcCharge,
            1.6, 1.6, 4.2, 2.75, 25.0, 3600
        );
        let normal_out_of_spec: Vec<_> = normal_records.iter().filter(|r| r.is_out_of_spec).collect();
        assert!(normal_out_of_spec.is_empty(), "Normal params should not be out of spec");

        let abnormal_records = service.generate_process_param_records(
            "TEST-BATCH-009".to_string(),
            0, 1, 1, Stage::CcCharge,
            4.0, 4.0, 4.5, 2.0, 50.0, 3600
        );
        let abnormal_out_of_spec: Vec<_> = abnormal_records.iter().filter(|r| r.is_out_of_spec).collect();
        assert!(!abnormal_out_of_spec.is_empty(), "Abnormal params should be out of spec");
        assert!(abnormal_out_of_spec.len() >= 3, "At least 3 params should be out of spec");
    }
}
