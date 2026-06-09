use crate::config::ClickHouseConfig;
use crate::models::{
    Alert, Anomaly, BatteryGroup, CabinetStats, CellInfo, ChannelData, ChannelHistory,
    ChannelStatus, CycleFeatures, DegradationAnalysis, DegradationDetail, DvDqPoint,
    ElectrolyteInjection, GasGenerationData, GroupingResult, InjectionOptimizationResult,
    PredictionResult, Stage, StageSummary, CapacityTrend, RATED_CAPACITY,
};
use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use clickhouse::Client;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error, info};

#[derive(Clone)]
pub struct Database {
    client: Client,
    config: ClickHouseConfig,
    insert_buffer: Arc<Mutex<VecDeque<ChannelData>>>,
}

impl Database {
    pub fn new(config: ClickHouseConfig) -> Result<Self> {
        let client = Client::default()
            .with_url(&config.url)
            .with_database(&config.database)
            .with_user(&config.user)
            .with_password(&config.password);

        Ok(Self {
            client,
            config,
            insert_buffer: Arc::new(Mutex::new(VecDeque::new())),
        })
    }

    pub async fn insert_data(&self, data: ChannelData) -> Result<()> {
        let mut buffer = self.insert_buffer.lock().await;
        buffer.push_back(data);

        if buffer.len() >= self.config.insert_batch_size {
            let batch: Vec<_> = buffer.drain(..).collect();
            drop(buffer);
            self.flush_batch(batch).await?;
        }

        Ok(())
    }

    pub async fn flush(&self) -> Result<()> {
        let mut buffer = self.insert_buffer.lock().await;
        if !buffer.is_empty() {
            let batch: Vec<_> = buffer.drain(..).collect();
            drop(buffer);
            self.flush_batch(batch).await?;
        }
        Ok(())
    }

    async fn flush_batch(&self, batch: Vec<ChannelData>) -> Result<()> {
        let mut inserter = self.client.insert("channel_data")?;

        for data in batch {
            inserter
                .write(&data)
                .await
                .context("Failed to write channel data")?;
        }

        inserter.end().await.context("Failed to end insert")?;
        debug!("Flushed {} records to ClickHouse", inserter.rows_written());

        Ok(())
    }

    pub async fn insert_cycle_features(&self, features: &CycleFeatures) -> Result<()> {
        self.client.insert("cycle_features")?.write(features).await?.end().await?;
        Ok(())
    }

    pub async fn insert_prediction(&self, prediction: &PredictionResult) -> Result<()> {
        self.client
            .insert("capacity_prediction")?
            .write(prediction)
            .await?
            .end()
            .await?;
        Ok(())
    }

    pub async fn insert_anomaly(&self, anomaly: &Anomaly) -> Result<()> {
        self.client.insert("anomalies")?.write(anomaly).await?.end().await?;
        Ok(())
    }

    pub async fn insert_alert(&self, alert: &Alert) -> Result<()> {
        self.client.insert("alerts")?.write(alert).await?.end().await?;
        Ok(())
    }

    pub async fn insert_cabinet_stats(&self, stats: &CabinetStats) -> Result<()> {
        self.client.insert("cabinet_stats")?.write(stats).await?.end().await?;
        Ok(())
    }

    pub async fn update_channel_status(&self, status: &ChannelStatus) -> Result<()> {
        self.client
            .insert("channel_status")?
            .write(status)
            .await?
            .end()
            .await?;
        Ok(())
    }

    pub async fn get_channel_status(&self, cabinet_id: u16, channel_id: u32) -> Result<Option<ChannelStatus>> {
        let query = format!(
            "SELECT * FROM channel_status WHERE cabinet_id = {} AND channel_id = {} ORDER BY last_update DESC LIMIT 1",
            cabinet_id, channel_id
        );

        let mut cursor = self.client.query(&query).fetch::<ChannelStatus>()?;
        Ok(cursor.next().await?)
    }

    pub async fn get_cabinet_status(&self, cabinet_id: u16) -> Result<Vec<ChannelStatus>> {
        let query = format!(
            "SELECT * FROM channel_status WHERE cabinet_id = {} ORDER BY channel_id",
            cabinet_id
        );

        let statuses: Vec<ChannelStatus> = self.client.query(&query).fetch_all().await?;
        Ok(statuses)
    }

    pub async fn get_all_cabinet_statuses(&self) -> Result<Vec<ChannelStatus>> {
        let query = "SELECT * FROM channel_status ORDER BY cabinet_id, channel_id";
        let statuses: Vec<ChannelStatus> = self.client.query(query).fetch_all().await?;
        Ok(statuses)
    }

    pub async fn get_channel_history(
        &self,
        cabinet_id: u16,
        channel_id: u32,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
    ) -> Result<ChannelHistory> {
        let query = format!(
            "SELECT timestamp, voltage, current, temperature, capacity, stage \
             FROM channel_data \
             WHERE cabinet_id = {} AND channel_id = {} \
             AND timestamp BETWEEN '{}' AND '{}' \
             ORDER BY timestamp",
            cabinet_id, channel_id, start_time, end_time
        );

        #[derive(clickhouse::Row, serde::Deserialize)]
        struct HistoryRow {
            timestamp: DateTime<Utc>,
            voltage: f64,
            current: f64,
            temperature: f64,
            capacity: f64,
            stage: Stage,
        }

        let rows: Vec<HistoryRow> = self.client.query(&query).fetch_all().await?;

        let mut history = ChannelHistory {
            timestamps: Vec::with_capacity(rows.len()),
            voltages: Vec::with_capacity(rows.len()),
            currents: Vec::with_capacity(rows.len()),
            temperatures: Vec::with_capacity(rows.len()),
            capacities: Vec::with_capacity(rows.len()),
            stages: Vec::with_capacity(rows.len()),
        };

        for row in rows {
            history.timestamps.push(row.timestamp);
            history.voltages.push(row.voltage);
            history.currents.push(row.current);
            history.temperatures.push(row.temperature);
            history.capacities.push(row.capacity);
            history.stages.push(row.stage);
        }

        Ok(history)
    }

    pub async fn get_recent_cycle_features(
        &self,
        cabinet_id: u16,
        channel_id: u32,
        n_cycles: usize,
    ) -> Result<Vec<CycleFeatures>> {
        let query = format!(
            "SELECT * FROM cycle_features \
             WHERE cabinet_id = {} AND channel_id = {} \
             ORDER BY cycle_index DESC LIMIT {}",
            cabinet_id, channel_id, n_cycles
        );

        let mut features: Vec<CycleFeatures> = self.client.query(&query).fetch_all().await?;
        features.reverse();
        Ok(features)
    }

    pub async fn get_cabinet_voltage_stats(
        &self,
        cabinet_id: u16,
        timestamp: DateTime<Utc>,
    ) -> Result<(f64, f64)> {
        let window_start = timestamp - Duration::seconds(60);

        let query = format!(
            "SELECT AVG(voltage) as avg_voltage, STDDEV(voltage) as std_voltage \
             FROM channel_data \
             WHERE cabinet_id = {} \
             AND timestamp BETWEEN '{}' AND '{}'",
            cabinet_id, window_start, timestamp
        );

        #[derive(clickhouse::Row, serde::Deserialize)]
        struct StatsRow {
            avg_voltage: Option<f64>,
            std_voltage: Option<f64>,
        }

        let result: Option<StatsRow> = self.client.query(&query).fetch_one().await?;

        match result {
            Some(row) => Ok((
                row.avg_voltage.unwrap_or(0.0),
                row.std_voltage.unwrap_or(0.0),
            )),
            None => Ok((0.0, 0.0)),
        }
    }

    pub async fn get_capacity_trend(
        &self,
        cabinet_id: u16,
        channel_id: u32,
        max_cycles: u16,
    ) -> Result<CapacityTrend> {
        let query = format!(
            "SELECT cycle_index, charge_capacity, discharge_capacity \
             FROM cycle_features \
             WHERE cabinet_id = {} AND channel_id = {} \
             ORDER BY cycle_index DESC LIMIT {}",
            cabinet_id, channel_id, max_cycles
        );

        #[derive(clickhouse::Row, serde::Deserialize)]
        struct TrendRow {
            cycle_index: u16,
            charge_capacity: f64,
            discharge_capacity: f64,
        }

        let mut rows: Vec<TrendRow> = self.client.query(&query).fetch_all().await?;
        rows.reverse();

        let pred_query = format!(
            "SELECT cycle_index, predicted_capacity \
             FROM capacity_prediction \
             WHERE cabinet_id = {} AND channel_id = {} \
             ORDER BY cycle_index DESC LIMIT {}",
            cabinet_id, channel_id, max_cycles
        );

        #[derive(clickhouse::Row, serde::Deserialize)]
        struct PredRow {
            cycle_index: u16,
            predicted_capacity: f64,
        }

        let pred_rows: Vec<PredRow> = self.client.query(&pred_query).fetch_all().await?;

        let mut predicted_map = std::collections::HashMap::new();
        for row in pred_rows {
            predicted_map.insert(row.cycle_index, row.predicted_capacity);
        }

        let mut trend = CapacityTrend {
            cycle_indices: Vec::new(),
            charge_capacities: Vec::new(),
            discharge_capacities: Vec::new(),
            predicted_capacities: Vec::new(),
        };

        for row in rows {
            trend.cycle_indices.push(row.cycle_index);
            trend.charge_capacities.push(row.charge_capacity);
            trend.discharge_capacities.push(row.discharge_capacity);
            trend
                .predicted_capacities
                .push(predicted_map.get(&row.cycle_index).copied().unwrap_or(0.0));
        }

        Ok(trend)
    }

    pub async fn get_stage_summaries(
        &self,
        cabinet_id: u16,
        channel_id: u32,
        cycle_index: u16,
    ) -> Result<Vec<StageSummary>> {
        let query = format!(
            "SELECT stage, duration, start_voltage, end_voltage, avg_current, max_temperature, capacity_gain \
             FROM channel_stage_summary \
             WHERE cabinet_id = {} AND channel_id = {} AND cycle_index = {} \
             ORDER BY stage",
            cabinet_id, channel_id, cycle_index
        );

        let summaries: Vec<StageSummary> = self.client.query(&query).fetch_all().await?;
        Ok(summaries)
    }

    pub async fn get_recent_anomalies(
        &self,
        cabinet_id: Option<u16>,
        limit: usize,
    ) -> Result<Vec<Anomaly>> {
        let where_clause = match cabinet_id {
            Some(cid) => format!("WHERE cabinet_id = {}", cid),
            None => "".to_string(),
        };

        let query = format!(
            "SELECT * FROM anomalies {} ORDER BY timestamp DESC LIMIT {}",
            where_clause, limit
        );

        let anomalies: Vec<Anomaly> = self.client.query(&query).fetch_all().await?;
        Ok(anomalies)
    }

    pub async fn get_recent_alerts(&self, limit: usize) -> Result<Vec<Alert>> {
        let query = format!(
            "SELECT * FROM alerts ORDER BY timestamp DESC LIMIT {}",
            limit
        );

        let alerts: Vec<Alert> = self.client.query(&query).fetch_all().await?;
        Ok(alerts)
    }

    pub async fn get_channel_current_cycle(&self, cabinet_id: u16, channel_id: u32) -> Result<Option<u16>> {
        let query = format!(
            "SELECT cycle_index FROM channel_data \
             WHERE cabinet_id = {} AND channel_id = {} \
             ORDER BY timestamp DESC LIMIT 1",
            cabinet_id, channel_id
        );

        #[derive(clickhouse::Row, serde::Deserialize)]
        struct CycleRow {
            cycle_index: u16,
        }

        let result: Option<CycleRow> = self.client.query(&query).fetch_one().await?;
        Ok(result.map(|r| r.cycle_index))
    }

    pub async fn pause_channel(&self, cabinet_id: u16, channel_id: u32) -> Result<()> {
        if let Some(mut status) = self.get_channel_status(cabinet_id, channel_id).await? {
            status.is_paused = true;
            self.update_channel_status(&status).await?;
            info!("Paused channel {}-{}", cabinet_id, channel_id);
        }
        Ok(())
    }

    pub async fn resume_channel(&self, cabinet_id: u16, channel_id: u32) -> Result<()> {
        if let Some(mut status) = self.get_channel_status(cabinet_id, channel_id).await? {
            status.is_paused = false;
            self.update_channel_status(&status).await?;
            info!("Resumed channel {}-{}", cabinet_id, channel_id);
        }
        Ok(())
    }

    pub async fn get_cabinet_abnormal_count(&self, cabinet_id: u16) -> Result<usize> {
        let query = format!(
            "SELECT count() as cnt FROM channel_status \
             WHERE cabinet_id = {} AND is_abnormal = 1",
            cabinet_id
        );

        #[derive(clickhouse::Row, serde::Deserialize)]
        struct CountRow {
            cnt: u64,
        }

        let result: Option<CountRow> = self.client.query(&query).fetch_one().await?;
        Ok(result.map(|r| r.cnt as usize).unwrap_or(0))
    }

    pub async fn mark_anomaly_resolved(&self, cabinet_id: u16, channel_id: u32) -> Result<()> {
        let query = format!(
            "ALTER TABLE anomalies UPDATE resolved = 1 \
             WHERE cabinet_id = {} AND channel_id = {} AND resolved = 0",
            cabinet_id, channel_id
        );

        self.client.query(&query).execute().await?;
        Ok(())
    }

    pub async fn mark_alert_notified_mes(&self, alert_id: uuid::Uuid) -> Result<()> {
        let query = format!(
            "ALTER TABLE alerts UPDATE notified_mes = 1 WHERE alert_id = '{}'",
            alert_id
        );
        self.client.query(&query).execute().await?;
        Ok(())
    }

    pub async fn mark_alert_notified_screen(&self, alert_id: uuid::Uuid) -> Result<()> {
        let query = format!(
            "ALTER TABLE alerts UPDATE notified_screen = 1 WHERE alert_id = '{}'",
            alert_id
        );
        self.client.query(&query).execute().await?;
        Ok(())
    }

    pub async fn acknowledge_alert(&self, alert_id: uuid::Uuid) -> Result<()> {
        let query = format!(
            "ALTER TABLE alerts UPDATE acknowledged = 1 WHERE alert_id = '{}'",
            alert_id
        );
        self.client.query(&query).execute().await?;
        Ok(())
    }

    pub async fn execute_query(&self, query: &str) -> Result<()> {
        self.client.query(query).execute().await?;
        Ok(())
    }

    // ============================================
    // 新增：分容配组数据库方法
    // ============================================

    pub async fn get_batch_cells(&self, batch_id: &str) -> Result<Vec<CellInfo>> {
        let query = format!(
            "SELECT * FROM cell_info WHERE batch_id = '{}' ORDER BY capacity_ratio DESC",
            batch_id
        );

        let cells: Vec<CellInfo> = self.client.query(&query).fetch_all().await?;
        Ok(cells)
    }

    pub async fn save_grouping_result(&self, result: &GroupingResult) -> Result<()> {
        for group in &result.groups {
            self.client
                .insert("battery_groups")?
                .write(group)
                .await?;
        }

        Ok(())
    }

    pub async fn get_grouping_result(&self, batch_id: &str) -> Result<Option<GroupingResult>> {
        let query = format!(
            "SELECT * FROM battery_groups WHERE batch_id = '{}' ORDER BY group_number",
            batch_id
        );

        let groups: Vec<BatteryGroup> = self.client.query(&query).fetch_all().await?;

        if groups.is_empty() {
            return Ok(None);
        }

        let total_cells: usize = groups.iter().map(|g| g.cell_count as usize).sum();
        let avg_consistency = if groups.is_empty() {
            0.0
        } else {
            groups.iter().map(|g| g.consistency_score).sum::<f64>() / groups.len() as f64
        };

        Ok(Some(GroupingResult {
            batch_id: batch_id.to_string(),
            algorithm: groups[0].algorithm,
            total_cells,
            rejected_cells: 0,
            group_count: groups.len(),
            cells_per_group: groups[0].cell_count as usize,
            groups,
            avg_consistency_score: avg_consistency,
            processing_time_ms: 0,
        }))
    }

    pub async fn list_grouping_results(&self, limit: usize) -> Result<Vec<GroupingResult>> {
        let query = format!(
            "SELECT DISTINCT batch_id, algorithm FROM battery_groups ORDER BY date DESC LIMIT {}",
            limit
        );

        #[derive(clickhouse::Row, serde::Deserialize)]
        struct BatchRow {
            batch_id: String,
            algorithm: crate::models::GroupingAlgorithm,
        }

        let rows: Vec<BatchRow> = self.client.query(&query).fetch_all().await?;

        let mut results = Vec::new();
        for row in rows {
            if let Some(result) = self.get_grouping_result(&row.batch_id).await? {
                results.push(result);
            }
        }

        Ok(results)
    }

    pub async fn insert_cell_info(&self, cell: &CellInfo) -> Result<()> {
        self.client.insert("cell_info")?.write(cell).await?.end().await?;
        Ok(())
    }

    // ============================================
    // 新增：电解液注液优化数据库方法
    // ============================================

    pub async fn get_batch_gas_data(&self, batch_id: &str) -> Result<Vec<GasGenerationData>> {
        let query = format!(
            "SELECT g.* FROM gas_generation_data g
             INNER JOIN (
                 SELECT cabinet_id, channel_id, MAX(timestamp) as ts
                 FROM gas_generation_data
                 WHERE batch_id = '{}'
                 GROUP BY cabinet_id, channel_id
             ) m ON g.cabinet_id = m.cabinet_id AND g.channel_id = m.channel_id AND g.timestamp = m.ts",
            batch_id
        );

        let data: Vec<GasGenerationData> = self.client.query(&query).fetch_all().await?;
        Ok(data)
    }

    pub async fn save_electrolyte_optimization(
        &self,
        result: &InjectionOptimizationResult,
    ) -> Result<()> {
        let injection = ElectrolyteInjection {
            date: Utc::now().date_naive(),
            batch_id: result.batch_id.clone(),
            injection_id: uuid::Uuid::new_v4().to_string(),
            cabinet_id: 0,
            channel_id: 0,
            cycle_index: 0,
            nominal_volume: result.avg_nominal_volume,
            actual_volume: result.avg_suggested_volume,
            gas_volume: 0.0,
            suggested_volume: result.next_batch_suggestion,
            adjustment: result.avg_adjustment,
            status: crate::models::InjectionStatus::Optimized,
            confidence: 0.9,
        };

        self.client
            .insert("electrolyte_injection")?
            .write(&injection)
            .await?
            .end()
            .await?;
        Ok(())
    }

    pub async fn get_electrolyte_optimization(
        &self,
        batch_id: &str,
    ) -> Result<Option<InjectionOptimizationResult>> {
        let query = format!(
            "SELECT * FROM electrolyte_injection WHERE batch_id = '{}' ORDER BY date DESC LIMIT 1",
            batch_id
        );

        let result: Option<ElectrolyteInjection> = match self.client.query(&query).fetch_one().await {
            Ok(Some(r)) => r,
            _ => return Ok(None),
        };

        Ok(Some(InjectionOptimizationResult {
            batch_id: result.batch_id.clone(),
            total_channels: 0,
            avg_nominal_volume: result.nominal_volume,
            avg_suggested_volume: result.suggested_volume,
            avg_adjustment: result.adjustment,
            over_injected_count: 0,
            under_injected_count: 0,
            estimated_gas_reduction: 0.0,
            estimated_capacity_improvement: 0.0,
            next_batch_suggestion: result.suggested_volume,
        }))
    }

    pub async fn insert_gas_data(&self, data: &GasGenerationData) -> Result<()> {
        self.client
            .insert("gas_generation_data")?
            .write(data)
            .await?
            .end()
            .await?;
        Ok(())
    }

    pub async fn insert_electrolyte_injection(&self, injection: &ElectrolyteInjection) -> Result<()> {
        self.client
            .insert("electrolyte_injection")?
            .write(injection)
            .await?
            .end()
            .await?;
        Ok(())
    }

    // ============================================
    // 新增：老化模式识别数据库方法
    // ============================================

    pub async fn save_degradation_analysis(&self, analysis: &DegradationAnalysis) -> Result<()> {
        self.client
            .insert("degradation_analysis")?
            .write(analysis)
            .await?
            .end()
            .await?;

        #[derive(clickhouse::Row, serde::Serialize)]
        struct DvDqRow {
            timestamp: DateTime<Utc>,
            cabinet_id: u16,
            channel_id: u32,
            cycle_index: u16,
            voltage: Vec<f64>,
            dq_dv: Vec<f64>,
            capacity: Vec<f64>,
            peak_positions: Vec<f64>,
            peak_heights: Vec<f64>,
        }

        let row = DvDqRow {
            timestamp: analysis.timestamp,
            cabinet_id: analysis.cabinet_id,
            channel_id: analysis.channel_id,
            cycle_index: analysis.cycle_index,
            voltage: Vec::new(),
            dq_dv: Vec::new(),
            capacity: Vec::new(),
            peak_positions: analysis.peak_positions.clone(),
            peak_heights: analysis.peak_heights.clone(),
        };

        self.client
            .insert("dvdq_analysis")?
            .write(&row)
            .await?
            .end()
            .await?;

        Ok(())
    }

    pub async fn get_degradation_analysis(
        &self,
        cabinet_id: u16,
        channel_id: u32,
        limit: usize,
    ) -> Result<Option<DegradationDetail>> {
        let query = format!(
            "SELECT * FROM degradation_analysis \
             WHERE cabinet_id = {} AND channel_id = {} \
             ORDER BY cycle_index DESC LIMIT {}",
            cabinet_id, channel_id, limit
        );

        let analyses: Vec<DegradationAnalysis> = self.client.query(&query).fetch_all().await?;

        if analyses.is_empty() {
            return Ok(None);
        }

        let latest = analyses.into_iter().next().unwrap();

        let dvdq_query = format!(
            "SELECT voltage, dq_dv, capacity, peak_positions, peak_heights \
             FROM dvdq_analysis \
             WHERE cabinet_id = {} AND channel_id = {} \
             ORDER BY timestamp DESC LIMIT 1",
            cabinet_id, channel_id
        );

        #[derive(clickhouse::Row, serde::Deserialize)]
        struct DvDqRow {
            voltage: Vec<f64>,
            dq_dv: Vec<f64>,
            capacity: Vec<f64>,
            peak_positions: Vec<f64>,
            peak_heights: Vec<f64>,
        }

        let dvdq_row: Option<DvDqRow> = self.client.query(&dvdq_query).fetch_one().await?;

        let mut dvdq_curve = Vec::new();
        if let Some(row) = dvdq_row {
            for i in 0..row.voltage.len() {
                dvdq_curve.push(DvDqPoint {
                    voltage: row.voltage[i],
                    dq_dv: row.dq_dv[i],
                    capacity: row.capacity[i],
                });
            }
        }

        let historical_query = format!(
            "SELECT cycle_index, mode, confidence \
             FROM degradation_analysis \
             WHERE cabinet_id = {} AND channel_id = {} \
             ORDER BY cycle_index DESC LIMIT 20",
            cabinet_id, channel_id
        );

        #[derive(clickhouse::Row, serde::Deserialize)]
        struct HistoryRow {
            cycle_index: u16,
            mode: crate::models::DegradationMode,
            confidence: f64,
        }

        let history_rows: Vec<HistoryRow> = self.client.query(&historical_query).fetch_all().await?;

        let historical_modes: Vec<(u16, crate::models::DegradationMode, f64)> = history_rows
            .into_iter()
            .map(|r| (r.cycle_index, r.mode, r.confidence))
            .collect();

        Ok(Some(DegradationDetail {
            analysis: latest,
            dvdq_curve,
            historical_modes,
        }))
    }

    // ============================================
    // 新增：MES系统对接数据库方法
    // ============================================

    pub async fn get_batch_detail(&self, batch_id: &str) -> Result<Option<serde_json::Value>> {
        let param_query = format!(
            "SELECT * FROM process_params WHERE batch_id = '{}' ORDER BY timestamp DESC LIMIT 100",
            batch_id
        );

        let params: Vec<crate::models::ProcessParamRecord> =
            self.client.query(&param_query).fetch_all().await?;

        let degraded_query = format!(
            "SELECT * FROM degraded_cells WHERE batch_id = '{}' ORDER BY timestamp DESC LIMIT 100",
            batch_id
        );

        let degraded: Vec<crate::models::DegradedCellRecord> =
            self.client.query(&degraded_query).fetch_all().await?;

        let batch_query = format!(
            "SELECT * FROM batch_info WHERE batch_id = '{}' ORDER BY date DESC LIMIT 1",
            batch_id
        );

        let batch_info: Option<crate::models::BatchInfo> = self.client.query(&batch_query).fetch_one().await?;

        Ok(Some(serde_json::json!({
            "process_params": params,
            "degraded_cells": degraded,
            "batch_info": batch_info,
        })))
    }

    pub async fn get_batch_capacities(&self, batch_id: &str) -> Result<Vec<f64>> {
        let query = format!(
            "SELECT measured_capacity FROM cell_info WHERE batch_id = '{}'",
            batch_id
        );

        #[derive(clickhouse::Row, serde::Deserialize)]
        struct CapRow {
            measured_capacity: f64,
        }

        let rows: Vec<CapRow> = self.client.query(&query).fetch_all().await?;
        Ok(rows.into_iter().map(|r| r.measured_capacity).collect())
    }

    pub async fn insert_process_param(&self, param: &crate::models::ProcessParamRecord) -> Result<()> {
        self.client
            .insert("process_params")?
            .write(param)
            .await?
            .end()
            .await?;
        Ok(())
    }

    pub async fn insert_degraded_cell(&self, cell: &crate::models::DegradedCellRecord) -> Result<()> {
        self.client
            .insert("degraded_cells")?
            .write(cell)
            .await?
            .end()
            .await?;
        Ok(())
    }

    pub async fn insert_batch_info(&self, batch: &crate::models::BatchInfo) -> Result<()> {
        self.client
            .insert("batch_info")?
            .write(batch)
            .await?
            .end()
            .await?;
        Ok(())
    }

    pub async fn get_batch_info(&self, batch_id: &str) -> Result<Option<crate::models::BatchInfo>> {
        let query = format!(
            "SELECT * FROM batch_info WHERE batch_id = '{}' ORDER BY date DESC LIMIT 1",
            batch_id
        );

        let result: Option<crate::models::BatchInfo> = self.client.query(&query).fetch_one().await?;
        Ok(result)
    }
}
