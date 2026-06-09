-- ============================================
-- 锂电池化成工艺监控系统 - ClickHouse 初始化脚本
-- ============================================
-- 数据分区策略：按月分区 (toYYYYMM)
-- TTL自动过期：90天自动删除
-- 引擎：MergeTree (时序数据) / ReplacingMergeTree (状态数据)
-- ============================================

CREATE DATABASE IF NOT EXISTS battery_monitor;

USE battery_monitor;

-- ============================================
-- 1. 原始通道数据表 - 核心时序数据表
-- ============================================
-- 分区：按月
-- TTL：90天自动删除
-- 排序键：(cabinet_id, channel_id, timestamp) 便于按柜按通道查询
CREATE TABLE IF NOT EXISTS channel_data (
    timestamp DateTime64(3, 'Asia/Shanghai') COMMENT '数据时间戳',
    cabinet_id UInt16 COMMENT '化成柜ID (1-20)',
    channel_id UInt32 COMMENT '通道ID (1-512)',
    voltage Float64 COMMENT '电压 (V)',
    current Float64 COMMENT '电流 (A)',
    temperature Float64 COMMENT '温度 (°C)',
    capacity Float64 COMMENT '当前容量 (Ah)',
    cycle_index UInt16 COMMENT '循环次数',
    stage Enum8('precharge' = 1, 'cc_charge' = 2, 'cv_charge' = 3, 'rest' = 4, 'discharge' = 5) COMMENT '工艺阶段',
    stage_duration UInt32 COMMENT '当前阶段持续时间 (秒)'
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(timestamp)
ORDER BY (cabinet_id, channel_id, timestamp)
TTL timestamp + INTERVAL 90 DAY
SETTINGS 
    index_granularity = 8192,
    ttl_only_drop_parts = 1,
    merge_with_ttl_timeout = 3600;

-- ============================================
-- 2. 通道阶段统计表
-- ============================================
-- 分区：按日期
-- TTL：90天自动删除
CREATE TABLE IF NOT EXISTS channel_stage_summary (
    date Date COMMENT '统计日期',
    cabinet_id UInt16 COMMENT '化成柜ID',
    channel_id UInt32 COMMENT '通道ID',
    cycle_index UInt16 COMMENT '循环次数',
    stage Enum8('precharge' = 1, 'cc_charge' = 2, 'cv_charge' = 3, 'rest' = 4, 'discharge' = 5) COMMENT '工艺阶段',
    start_time DateTime64(3, 'Asia/Shanghai') COMMENT '阶段开始时间',
    end_time DateTime64(3, 'Asia/Shanghai') COMMENT '阶段结束时间',
    duration UInt32 COMMENT '阶段持续时间 (秒)',
    start_voltage Float64 COMMENT '阶段开始电压',
    end_voltage Float64 COMMENT '阶段结束电压',
    avg_current Float64 COMMENT '平均电流',
    max_temperature Float64 COMMENT '最高温度',
    capacity_gain Float64 COMMENT '容量增量'
) ENGINE = MergeTree()
PARTITION BY date
ORDER BY (cabinet_id, channel_id, cycle_index, stage)
TTL date + INTERVAL 90 DAY
SETTINGS 
    index_granularity = 8192,
    ttl_only_drop_parts = 1;

-- ============================================
-- 3. 容量预测结果表
-- ============================================
-- 分区：按月
-- TTL：90天自动删除
CREATE TABLE IF NOT EXISTS capacity_prediction (
    timestamp DateTime64(3, 'Asia/Shanghai') COMMENT '预测时间',
    cabinet_id UInt16 COMMENT '化成柜ID',
    channel_id UInt32 COMMENT '通道ID',
    cycle_index UInt16 COMMENT '预测时的循环次数',
    predicted_capacity Float64 COMMENT '预测容量 (Ah)',
    actual_capacity Float64 COMMENT '实际容量 (Ah)',
    rated_capacity Float64 COMMENT '额定容量 (Ah)',
    prediction_error Float64 COMMENT '预测误差 (%)',
    model_version String COMMENT '模型版本',
    features String COMMENT '特征向量 (JSON)'
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(timestamp)
ORDER BY (cabinet_id, channel_id, cycle_index)
TTL timestamp + INTERVAL 90 DAY
SETTINGS 
    index_granularity = 8192,
    ttl_only_drop_parts = 1;

-- ============================================
-- 4. 异常记录表
-- ============================================
-- 分区：按月
-- TTL：180天自动删除（异常记录保留更久）
CREATE TABLE IF NOT EXISTS anomalies (
    timestamp DateTime64(3, 'Asia/Shanghai') COMMENT '异常时间',
    cabinet_id UInt16 COMMENT '化成柜ID',
    channel_id UInt32 COMMENT '通道ID',
    anomaly_type Enum8('voltage_deviation' = 1, 'capacity_low' = 2, 'temperature_high' = 3, 'current_abnormal' = 4) COMMENT '异常类型',
    severity Enum8('warning' = 1, 'critical' = 2) COMMENT '严重程度',
    description String COMMENT '异常描述',
    value Float64 COMMENT '异常值',
    threshold Float64 COMMENT '阈值',
    sigma_deviation Float64 COMMENT 'σ偏离倍数',
    is_paused UInt8 DEFAULT 0 COMMENT '是否已暂停',
    resolved UInt8 DEFAULT 0 COMMENT '是否已解决',
    resolved_at DateTime64(3, 'Asia/Shanghai') COMMENT '解决时间'
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(timestamp)
ORDER BY (cabinet_id, channel_id, timestamp)
TTL timestamp + INTERVAL 180 DAY
SETTINGS 
    index_granularity = 8192,
    ttl_only_drop_parts = 1;

-- ============================================
-- 5. 告警记录表
-- ============================================
-- 分区：按月
-- TTL：180天自动删除（告警记录保留更久）
CREATE TABLE IF NOT EXISTS alerts (
    timestamp DateTime64(3, 'Asia/Shanghai') COMMENT '告警时间',
    alert_id UUID DEFAULT generateUUIDv4() COMMENT '告警ID',
    alert_level Enum8('level1' = 1, 'level2' = 2) COMMENT '告警级别 (1:单通道, 2:整柜)',
    alert_type String COMMENT '告警类型',
    cabinet_id UInt16 COMMENT '化成柜ID',
    channel_ids Array(UInt32) COMMENT '关联通道ID列表',
    message String COMMENT '告警消息',
    notified_mes UInt8 DEFAULT 0 COMMENT '是否已通知MES',
    notified_screen UInt8 DEFAULT 0 COMMENT '是否已通知产线大屏',
    acknowledged UInt8 DEFAULT 0 COMMENT '是否已确认',
    acknowledged_at DateTime64(3, 'Asia/Shanghai') COMMENT '确认时间',
    acknowledged_by String COMMENT '确认人'
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(timestamp)
ORDER BY (alert_level, timestamp)
TTL timestamp + INTERVAL 180 DAY
SETTINGS 
    index_granularity = 8192,
    ttl_only_drop_parts = 1;

-- ============================================
-- 6. 循环特征表
-- ============================================
-- 分区：按日期
-- TTL：90天自动删除
CREATE TABLE IF NOT EXISTS cycle_features (
    date Date COMMENT '循环完成日期',
    cabinet_id UInt16 COMMENT '化成柜ID',
    channel_id UInt32 COMMENT '通道ID',
    cycle_index UInt16 COMMENT '循环次数',
    cc_charge_time UInt32 COMMENT '恒流充电时间 (秒)',
    cv_charge_time UInt32 COMMENT '恒压充电时间 (秒)',
    discharge_time UInt32 COMMENT '放电时间 (秒)',
    discharge_platform_voltage Float64 COMMENT '放电平台电压',
    cc_end_voltage Float64 COMMENT '恒流结束电压',
    cv_end_current Float64 COMMENT '恒压结束电流',
    max_charge_temp Float64 COMMENT '充电最高温度',
    max_discharge_temp Float64 COMMENT '放电最高温度',
    charge_capacity Float64 COMMENT '充电容量',
    discharge_capacity Float64 COMMENT '放电容量',
    efficiency Float64 COMMENT '充放电效率'
) ENGINE = MergeTree()
PARTITION BY date
ORDER BY (cabinet_id, channel_id, cycle_index)
TTL date + INTERVAL 90 DAY
SETTINGS 
    index_granularity = 8192,
    ttl_only_drop_parts = 1;

-- ============================================
-- 7. 化成柜统计表
-- ============================================
-- 分区：按月
-- TTL：90天自动删除
CREATE TABLE IF NOT EXISTS cabinet_stats (
    timestamp DateTime64(3, 'Asia/Shanghai') COMMENT '统计时间',
    cabinet_id UInt16 COMMENT '化成柜ID',
    avg_voltage Float64 COMMENT '平均电压',
    std_voltage Float64 COMMENT '电压标准差',
    avg_current Float64 COMMENT '平均电流',
    avg_temperature Float64 COMMENT '平均温度',
    abnormal_channel_count UInt16 COMMENT '异常通道数',
    total_channels UInt16 COMMENT '总通道数',
    abnormal_ratio Float64 COMMENT '异常比例',
    stage_distribution Array(UInt32) COMMENT '各阶段通道数分布'
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(timestamp)
ORDER BY (cabinet_id, timestamp)
TTL timestamp + INTERVAL 90 DAY
SETTINGS 
    index_granularity = 8192,
    ttl_only_drop_parts = 1;

-- ============================================
-- 8. 通道状态表 (ReplacingMergeTree - 只保留最新状态)
-- ============================================
-- 无分区 (ReplacingMergeTree按主键去重)
-- 无TTL (状态表永久保留最新状态)
-- 排序键：(cabinet_id, channel_id)
CREATE TABLE IF NOT EXISTS channel_status (
    cabinet_id UInt16 COMMENT '化成柜ID',
    channel_id UInt32 COMMENT '通道ID',
    last_update DateTime64(3, 'Asia/Shanghai') COMMENT '最后更新时间',
    current_stage Enum8('precharge' = 1, 'cc_charge' = 2, 'cv_charge' = 3, 'rest' = 4, 'discharge' = 5) COMMENT '当前阶段',
    current_voltage Float64 COMMENT '当前电压',
    current_current Float64 COMMENT '当前电流',
    current_temperature Float64 COMMENT '当前温度',
    current_capacity Float64 COMMENT '当前容量',
    cycle_index UInt16 COMMENT '当前循环次数',
    is_abnormal UInt8 COMMENT '是否异常',
    is_paused UInt8 COMMENT '是否暂停',
    capacity_ratio Float64 COMMENT '容量比',
    predicted_capacity Float64 COMMENT '预测容量',
    battery_model String DEFAULT 'NMC_3Ah' COMMENT '电池型号'
) ENGINE = ReplacingMergeTree(last_update)
ORDER BY (cabinet_id, channel_id)
SETTINGS 
    index_granularity = 8192;

-- ============================================
-- 9. 控制命令表
-- ============================================
CREATE TABLE IF NOT EXISTS control_commands (
    timestamp DateTime64(3, 'Asia/Shanghai') COMMENT '命令时间',
    command_id UUID DEFAULT generateUUIDv4() COMMENT '命令ID',
    cabinet_id UInt16 COMMENT '化成柜ID',
    channel_id UInt32 COMMENT '通道ID',
    command Enum8('pause' = 1, 'resume' = 2, 'stop' = 3, 'restart' = 4) COMMENT '命令类型',
    operator String COMMENT '操作员',
    reason String COMMENT '操作原因',
    executed UInt8 DEFAULT 0 COMMENT '是否已执行',
    executed_at DateTime64(3, 'Asia/Shanghai') COMMENT '执行时间'
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(timestamp)
ORDER BY (cabinet_id, channel_id, timestamp)
TTL timestamp + INTERVAL 90 DAY
SETTINGS 
    index_granularity = 8192,
    ttl_only_drop_parts = 1;

-- ============================================
-- 物化视图：实时更新通道状态
-- ============================================
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
    0.0 AS predicted_capacity,
    'NMC_3Ah' AS battery_model
FROM channel_data
WHERE (cabinet_id, channel_id, timestamp) IN (
    SELECT cabinet_id, channel_id, MAX(timestamp)
    FROM channel_data
    GROUP BY cabinet_id, channel_id
);

-- ============================================
-- 创建索引提升查询性能
-- ============================================

-- 通道数据跳数索引
ALTER TABLE channel_data 
ADD INDEX IF NOT EXISTS idx_voltage voltage TYPE minmax GRANULARITY 4;

ALTER TABLE channel_data 
ADD INDEX IF NOT EXISTS idx_temperature temperature TYPE minmax GRANULARITY 4;

ALTER TABLE channel_data 
ADD INDEX IF NOT EXISTS idx_cycle cycle_index TYPE set(256) GRANULARITY 4;

ALTER TABLE channel_data 
ADD INDEX IF NOT EXISTS idx_stage stage TYPE set(8) GRANULARITY 4;

-- 异常表索引
ALTER TABLE anomalies 
ADD INDEX IF NOT EXISTS idx_severity severity TYPE set(8) GRANULARITY 4;

ALTER TABLE anomalies 
ADD INDEX IF NOT EXISTS idx_anomaly_type anomaly_type TYPE set(16) GRANULARITY 4;

-- 告警表索引
ALTER TABLE alerts 
ADD INDEX IF NOT EXISTS idx_alert_level alert_level TYPE set(8) GRANULARITY 4;

-- ============================================
-- 新增：10. 电池单体信息表（分容配组用）
-- ============================================
CREATE TABLE IF NOT EXISTS cell_info (
    date Date COMMENT '日期',
    batch_id String COMMENT '批次ID',
    cabinet_id UInt16 COMMENT '化成柜ID',
    channel_id UInt32 COMMENT '通道ID',
    predicted_capacity Float64 COMMENT '预测容量 (Ah)',
    measured_capacity Float64 COMMENT '实测容量 (Ah)',
    internal_resistance Float64 COMMENT '内阻 (mΩ)',
    capacity_ratio Float64 COMMENT '容量比',
    grade Enum8('A' = 1, 'B' = 2, 'C' = 3, 'Rejected' = 4) COMMENT '电池等级',
    cycle_index UInt16 COMMENT '循环次数',
    group_id String DEFAULT '' COMMENT '所属电池组ID'
) ENGINE = MergeTree()
PARTITION BY date
ORDER BY (batch_id, cabinet_id, channel_id)
TTL date + INTERVAL 365 DAY
SETTINGS 
    index_granularity = 8192,
    ttl_only_drop_parts = 1;

-- ============================================
-- 新增：11. 电池配组方案表
-- ============================================
CREATE TABLE IF NOT EXISTS battery_groups (
    date Date COMMENT '日期',
    group_id String COMMENT '电池组ID',
    batch_id String COMMENT '批次ID',
    group_number UInt32 COMMENT '组号',
    algorithm Enum8('greedy' = 1, 'genetic' = 2) COMMENT '配组算法',
    cell_count UInt16 COMMENT '电池数量',
    avg_capacity Float64 COMMENT '平均容量 (Ah)',
    capacity_std Float64 COMMENT '容量标准差',
    capacity_max_diff Float64 COMMENT '容量最大差异',
    avg_resistance Float64 COMMENT '平均内阻 (mΩ)',
    resistance_std Float64 COMMENT '内阻标准差',
    resistance_max_diff Float64 COMMENT '内阻最大差异',
    consistency_score Float64 COMMENT '一致性评分 (0-100)',
    cell_cabinet_ids Array(UInt16) COMMENT '电池所属化成柜ID列表',
    cell_channel_ids Array(UInt32) COMMENT '电池通道ID列表',
    created_at DateTime64(3, 'Asia/Shanghai') DEFAULT now() COMMENT '创建时间'
) ENGINE = MergeTree()
PARTITION BY date
ORDER BY (batch_id, group_id)
TTL date + INTERVAL 365 DAY
SETTINGS 
    index_granularity = 8192,
    ttl_only_drop_parts = 1;

-- ============================================
-- 新增：12. 气体产生数据表
-- ============================================
CREATE TABLE IF NOT EXISTS gas_generation_data (
    timestamp DateTime64(3, 'Asia/Shanghai') COMMENT '时间戳',
    cabinet_id UInt16 COMMENT '化成柜ID',
    channel_id UInt32 COMMENT '通道ID',
    cycle_index UInt16 COMMENT '循环次数',
    stage Enum8('precharge' = 1, 'cc_charge' = 2, 'cv_charge' = 3, 'rest' = 4, 'discharge' = 5) COMMENT '工艺阶段',
    pressure Float64 COMMENT '压力 (kPa)',
    temperature Float64 COMMENT '温度 (°C)',
    gas_volume Float64 COMMENT '产气体积 (mL)',
    gas_generation_rate Float64 COMMENT '产气速率 (mL/min)',
    cumulative_gas Float64 COMMENT '累计产气量 (mL)'
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(timestamp)
ORDER BY (cabinet_id, channel_id, timestamp)
TTL timestamp + INTERVAL 180 DAY
SETTINGS 
    index_granularity = 8192,
    ttl_only_drop_parts = 1;

-- ============================================
-- 新增：13. 电解液注液记录表
-- ============================================
CREATE TABLE IF NOT EXISTS electrolyte_injection (
    date Date COMMENT '日期',
    batch_id String COMMENT '批次ID',
    injection_id String COMMENT '注液记录ID',
    cabinet_id UInt16 COMMENT '化成柜ID',
    channel_id UInt32 COMMENT '通道ID',
    cycle_index UInt16 COMMENT '循环次数',
    nominal_volume Float64 COMMENT '标称注液量 (g)',
    actual_volume Float64 COMMENT '实际注液量 (g)',
    gas_volume Float64 COMMENT '产气体积 (mL)',
    suggested_volume Float64 COMMENT '建议注液量 (g)',
    adjustment Float64 COMMENT '调整量 (g)',
    status Enum8('normal' = 1, 'over_injected' = 2, 'under_injected' = 3, 'optimized' = 4) COMMENT '注液状态',
    confidence Float64 COMMENT '优化建议置信度',
    created_at DateTime64(3, 'Asia/Shanghai') DEFAULT now() COMMENT '创建时间'
) ENGINE = MergeTree()
PARTITION BY date
ORDER BY (batch_id, cabinet_id, channel_id)
TTL date + INTERVAL 365 DAY
SETTINGS 
    index_granularity = 8192,
    ttl_only_drop_parts = 1;

-- ============================================
-- 新增：14. dQ/dV分析数据表
-- ============================================
CREATE TABLE IF NOT EXISTS dvdq_analysis (
    timestamp DateTime64(3, 'Asia/Shanghai') COMMENT '时间戳',
    cabinet_id UInt16 COMMENT '化成柜ID',
    channel_id UInt32 COMMENT '通道ID',
    cycle_index UInt16 COMMENT '循环次数',
    voltage Array(Float64) COMMENT '电压序列 (V)',
    dq_dv Array(Float64) COMMENT 'dQ/dV序列 (Ah/V)',
    capacity Array(Float64) COMMENT '容量序列 (Ah)',
    peak_positions Array(Float64) COMMENT '峰值位置 (V)',
    peak_heights Array(Float64) COMMENT '峰值高度 (Ah/V)'
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(timestamp)
ORDER BY (cabinet_id, channel_id, cycle_index)
TTL timestamp + INTERVAL 365 DAY
SETTINGS 
    index_granularity = 8192,
    ttl_only_drop_parts = 1;

-- ============================================
-- 新增：15. 老化模式分析表
-- ============================================
CREATE TABLE IF NOT EXISTS degradation_analysis (
    timestamp DateTime64(3, 'Asia/Shanghai') COMMENT '分析时间',
    cabinet_id UInt16 COMMENT '化成柜ID',
    channel_id UInt32 COMMENT '通道ID',
    cycle_index UInt16 COMMENT '循环次数',
    mode Enum8('normal' = 0, 'cathode' = 1, 'anode' = 2, 'electrolyte' = 3, 'sei' = 4, 'mixed' = 5) COMMENT '衰减模式',
    confidence Float64 COMMENT '置信度',
    cathode_score Float64 COMMENT '正极衰减评分',
    anode_score Float64 COMMENT '负极衰减评分',
    electrolyte_score Float64 COMMENT '电解液消耗评分',
    sei_score Float64 COMMENT 'SEI膜生长评分',
    capacity_fade_rate Float64 COMMENT '容量衰减率 (%/cycle)',
    resistance_growth_rate Float64 COMMENT '内阻增长率 (%/cycle)',
    recommendations String COMMENT '处理建议'
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(timestamp)
ORDER BY (cabinet_id, channel_id, cycle_index)
TTL timestamp + INTERVAL 365 DAY
SETTINGS 
    index_granularity = 8192,
    ttl_only_drop_parts = 1;

-- ============================================
-- 新增：16. 工艺参数记录表（MES同步用）
-- ============================================
CREATE TABLE IF NOT EXISTS process_params (
    timestamp DateTime64(3, 'Asia/Shanghai') COMMENT '时间戳',
    batch_id String COMMENT '批次ID',
    cabinet_id UInt16 COMMENT '化成柜ID',
    channel_id UInt32 COMMENT '通道ID',
    cycle_index UInt16 COMMENT '循环次数',
    stage Enum8('precharge' = 1, 'cc_charge' = 2, 'cv_charge' = 3, 'rest' = 4, 'discharge' = 5) COMMENT '工艺阶段',
    param_type Enum8('charge_current' = 1, 'discharge_current' = 2, 'charge_voltage' = 3, 'discharge_voltage' = 4, 'temperature' = 5, 'time_duration' = 6) COMMENT '参数类型',
    param_value Float64 COMMENT '参数值',
    param_unit String COMMENT '参数单位',
    upper_limit Float64 COMMENT '上限',
    lower_limit Float64 COMMENT '下限',
    is_out_of_spec UInt8 COMMENT '是否超差',
    mes_sync_status Enum8('pending' = 0, 'synced' = 1, 'failed' = 2, 'acked' = 3) DEFAULT 'pending' COMMENT 'MES同步状态',
    mes_sync_time DateTime64(3, 'Asia/Shanghai') COMMENT 'MES同步时间',
    mes_error_message String DEFAULT '' COMMENT 'MES同步错误信息'
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(timestamp)
ORDER BY (batch_id, cabinet_id, channel_id, timestamp)
TTL timestamp + INTERVAL 365 DAY
SETTINGS 
    index_granularity = 8192,
    ttl_only_drop_parts = 1;

-- ============================================
-- 新增：17. 降级电池记录表（MES同步用）
-- ============================================
CREATE TABLE IF NOT EXISTS degraded_cells (
    timestamp DateTime64(3, 'Asia/Shanghai') COMMENT '时间戳',
    batch_id String COMMENT '批次ID',
    cabinet_id UInt16 COMMENT '化成柜ID',
    channel_id UInt32 COMMENT '通道ID',
    cycle_index UInt16 COMMENT '循环次数',
    capacity Float64 COMMENT '容量 (Ah)',
    capacity_ratio Float64 COMMENT '容量比',
    internal_resistance Float64 COMMENT '内阻 (mΩ)',
    degradation_reason String COMMENT '降级原因',
    grade Enum8('A' = 1, 'B' = 2, 'C' = 3, 'Rejected' = 4) COMMENT '电池等级',
    mes_sync_status Enum8('pending' = 0, 'synced' = 1, 'failed' = 2, 'acked' = 3) DEFAULT 'pending' COMMENT 'MES同步状态',
    mes_sync_time DateTime64(3, 'Asia/Shanghai') COMMENT 'MES同步时间',
    mes_ack_time DateTime64(3, 'Asia/Shanghai') COMMENT 'MES确认时间',
    mes_error_message String DEFAULT '' COMMENT 'MES同步错误信息'
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(timestamp)
ORDER BY (batch_id, cabinet_id, channel_id)
TTL timestamp + INTERVAL 365 DAY
SETTINGS 
    index_granularity = 8192,
    ttl_only_drop_parts = 1;

-- ============================================
-- 新增：18. 批次信息表（MES追溯用）
-- ============================================
CREATE TABLE IF NOT EXISTS batch_info (
    date Date COMMENT '日期',
    batch_id String COMMENT '批次ID',
    product_code String COMMENT '产品编码',
    battery_model String COMMENT '电池型号',
    rated_capacity Float64 COMMENT '额定容量 (Ah)',
    total_cells UInt32 COMMENT '电池总数',
    start_time DateTime64(3, 'Asia/Shanghai') COMMENT '开始时间',
    end_time DateTime64(3, 'Asia/Shanghai') COMMENT '结束时间',
    operator String COMMENT '操作员',
    shift String COMMENT '班次',
    avg_capacity Float64 COMMENT '平均容量 (Ah)',
    yield_rate Float64 COMMENT '良率 (%)',
    grade_a_ratio Float64 COMMENT 'A级品比例',
    grade_b_ratio Float64 COMMENT 'B级品比例',
    grade_c_ratio Float64 COMMENT 'C级品比例',
    rejected_ratio Float64 COMMENT '不合格品比例',
    avg_internal_resistance Float64 COMMENT '平均内阻 (mΩ)',
    remarks String COMMENT '备注',
    mes_sync_status Enum8('pending' = 0, 'synced' = 1, 'failed' = 2, 'acked' = 3) DEFAULT 'pending' COMMENT 'MES同步状态',
    mes_sync_time DateTime64(3, 'Asia/Shanghai') COMMENT 'MES同步时间',
    created_at DateTime64(3, 'Asia/Shanghai') DEFAULT now() COMMENT '创建时间'
) ENGINE = ReplacingMergeTree(created_at)
PARTITION BY date
ORDER BY (batch_id)
TTL date + INTERVAL 365 DAY
SETTINGS 
    index_granularity = 8192;

-- ============================================
-- 新增索引
-- ============================================
ALTER TABLE cell_info 
ADD INDEX IF NOT EXISTS idx_grade grade TYPE set(8) GRANULARITY 4;

ALTER TABLE battery_groups 
ADD INDEX IF NOT EXISTS idx_algorithm algorithm TYPE set(8) GRANULARITY 4;

ALTER TABLE gas_generation_data 
ADD INDEX IF NOT EXISTS idx_pressure pressure TYPE minmax GRANULARITY 4;

ALTER TABLE degradation_analysis 
ADD INDEX IF NOT EXISTS idx_mode mode TYPE set(8) GRANULARITY 4;

ALTER TABLE process_params 
ADD INDEX IF NOT EXISTS idx_mes_sync mes_sync_status TYPE set(8) GRANULARITY 4;

ALTER TABLE degraded_cells 
ADD INDEX IF NOT EXISTS idx_mes_sync mes_sync_status TYPE set(8) GRANULARITY 4;

-- ============================================
-- 数据库说明
-- ============================================
-- 1. 时序数据表 (channel_data, cabinet_stats) 按月分区，TTL 90天
-- 2. 异常和告警表 TTL 180天，保留更长时间用于追溯
-- 3. 状态表使用 ReplacingMergeTree，按 (cabinet_id, channel_id) 去重，自动保留最新
-- 4. 所有表使用 MergeTree 引擎，适合高吞吐写入和快速查询
-- 5. 跳数索引用于加速常用过滤条件的查询
-- 6. ttl_only_drop_parts = 1 确保只删除整个过期分区，提高效率
-- 7. 新增8张表支持分容配组、注液优化、老化识别、MES对接四大功能
