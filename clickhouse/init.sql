CREATE DATABASE IF NOT EXISTS battery_monitor;

USE battery_monitor;

CREATE TABLE IF NOT EXISTS channel_data (
    timestamp DateTime64(3, 'Asia/Shanghai'),
    cabinet_id UInt16,
    channel_id UInt32,
    voltage Float64,
    current Float64,
    temperature Float64,
    capacity Float64,
    cycle_index UInt16,
    stage Enum8('precharge' = 1, 'cc_charge' = 2, 'cv_charge' = 3, 'rest' = 4, 'discharge' = 5),
    stage_duration UInt32
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(timestamp)
ORDER BY (cabinet_id, channel_id, timestamp)
TTL timestamp + INTERVAL 90 DAY
SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS channel_stage_summary (
    date Date,
    cabinet_id UInt16,
    channel_id UInt32,
    cycle_index UInt16,
    stage Enum8('precharge' = 1, 'cc_charge' = 2, 'cv_charge' = 3, 'rest' = 4, 'discharge' = 5),
    start_time DateTime64(3, 'Asia/Shanghai'),
    end_time DateTime64(3, 'Asia/Shanghai'),
    duration UInt32,
    start_voltage Float64,
    end_voltage Float64,
    avg_current Float64,
    max_temperature Float64,
    capacity_gain Float64
) ENGINE = MergeTree()
PARTITION BY date
ORDER BY (cabinet_id, channel_id, cycle_index, stage)
SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS capacity_prediction (
    timestamp DateTime64(3, 'Asia/Shanghai'),
    cabinet_id UInt16,
    channel_id UInt32,
    cycle_index UInt16,
    predicted_capacity Float64,
    actual_capacity Float64,
    rated_capacity Float64,
    prediction_error Float64,
    model_version String,
    features String
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(timestamp)
ORDER BY (cabinet_id, channel_id, cycle_index)
SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS anomalies (
    timestamp DateTime64(3, 'Asia/Shanghai'),
    cabinet_id UInt16,
    channel_id UInt32,
    anomaly_type Enum8('voltage_deviation' = 1, 'capacity_low' = 2, 'temperature_high' = 3, 'current_abnormal' = 4),
    severity Enum8('warning' = 1, 'critical' = 2),
    description String,
    value Float64,
    threshold Float64,
    is_paused UInt8 DEFAULT 0,
    resolved UInt8 DEFAULT 0
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(timestamp)
ORDER BY (cabinet_id, channel_id, timestamp)
SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS alerts (
    timestamp DateTime64(3, 'Asia/Shanghai'),
    alert_id UUID DEFAULT generateUUIDv4(),
    alert_level Enum8('level1' = 1, 'level2' = 2),
    alert_type String,
    cabinet_id UInt16,
    channel_ids Array(UInt32),
    message String,
    notified_mes UInt8 DEFAULT 0,
    notified_screen UInt8 DEFAULT 0,
    acknowledged UInt8 DEFAULT 0
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(timestamp)
ORDER BY (alert_level, timestamp)
SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS cycle_features (
    date Date,
    cabinet_id UInt16,
    channel_id UInt32,
    cycle_index UInt16,
    cc_charge_time UInt32,
    cv_charge_time UInt32,
    discharge_time UInt32,
    discharge_platform_voltage Float64,
    cc_end_voltage Float64,
    cv_end_current Float64,
    max_charge_temp Float64,
    max_discharge_temp Float64,
    charge_capacity Float64,
    discharge_capacity Float64,
    efficiency Float64
) ENGINE = MergeTree()
PARTITION BY date
ORDER BY (cabinet_id, channel_id, cycle_index)
SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS cabinet_stats (
    timestamp DateTime64(3, 'Asia/Shanghai'),
    cabinet_id UInt16,
    avg_voltage Float64,
    std_voltage Float64,
    avg_current Float64,
    avg_temperature Float64,
    abnormal_channel_count UInt16,
    total_channels UInt16,
    abnormal_ratio Float64
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(timestamp)
ORDER BY (cabinet_id, timestamp)
SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS channel_status (
    cabinet_id UInt16,
    channel_id UInt32,
    last_update DateTime64(3, 'Asia/Shanghai'),
    current_stage Enum8('precharge' = 1, 'cc_charge' = 2, 'cv_charge' = 3, 'rest' = 4, 'discharge' = 5),
    current_voltage Float64,
    current_current Float64,
    current_temperature Float64,
    current_capacity Float64,
    cycle_index UInt16,
    is_abnormal UInt8,
    is_paused UInt8,
    capacity_ratio Float64,
    predicted_capacity Float64
) ENGINE = ReplacingMergeTree(last_update)
ORDER BY (cabinet_id, channel_id)
SETTINGS index_granularity = 8192;

CREATE MATERIALIZED VIEW IF NOT EXISTS channel_status_mv
TO channel_status
AS SELECT
    cabinet_id,
    channel_id,
    timestamp AS last_update,
    stage AS current_stage,
    voltage AS current_voltage,
    current AS current_current,
    temperature AS current_temperature,
    capacity AS current_capacity,
    cycle_index,
    0 AS is_abnormal,
    0 AS is_paused,
    capacity / 3.2 AS capacity_ratio,
    0.0 AS predicted_capacity
FROM channel_data
WHERE (cabinet_id, channel_id, timestamp) IN (
    SELECT cabinet_id, channel_id, MAX(timestamp)
    FROM channel_data
    GROUP BY cabinet_id, channel_id
);
