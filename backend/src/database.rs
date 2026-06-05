use crate::config::ClickHouseConfig;
use crate::models::{
    Alert, Anomaly, CabinetStats, ChannelData, ChannelHistory, ChannelStatus, CycleFeatures,
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
}
