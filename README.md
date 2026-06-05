# 锂电池化成工艺监控与容量预测系统

## 📋 项目简介

基于Rust + ClickHouse + Vue构建的锂电池化成工艺实时监控与容量预测系统。

## 🏗️ 系统架构

```
┌─────────────────────────────────────────────────────────────────┐
│                     前端 (Nginx + Gzip + 缓存
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐
│  │ Cabinet   │  │  │  │ Channel │  │  │  Channel │
│  │ Panel   │  │  │  │ Detail  │  │  │ Detail  │
│  └──────┘  └────────┘  └────────┘  └─────────────┘
└─────────────────────────────────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│                   Rust 后端服务 (Modular Architecture)         │
│  ┌─────────────────┐      ┌─────────────────┐          │
│  │  MQTT Collector │────→│  Data Pipeline  │          │
│  │  (订阅/接收      │      │  阶段检测/循环跟踪  │          │
│  └─────────────────┘      └──────┬──────────┘          │
│                                       │                     │
│                                       ▼                     │
│                              ┌──────────────────┐        │
│                              │ Anomaly Detector │        │
│                              │ 3σ+绝对偏差检测 │        │
│                              └────────┬─────────┘        │
│                                       │                     │
│                                       ▼                     │
│                              ┌──────────────────┐    ┌───────────────┐    │
│                              │ Capacity    │    │  Alarm Sender │    │
│                              │ Predictor  │    │  告警评估/推送 │    │
│                              └──────────┘    └───────┬───────┘    │
└──────────────────────────────────────────────────────┼───────────────────┘
                                               │
                        ┌──────────────────────┘
                        ▼
┌────────────────────────────────────────────────────────────┐
│  MQTT Broker (Eclipse Mosquitto)          │
│  Topic: battery/cabinet/+/data            │
└────────────────────────────────────────────────────────────┘
                        ▲
                        │
┌────────────────────────────────────────────────────────────┐
│  化成柜模拟器 (20柜 × 512通道 = 10240通道  │
│  支持电压/电流/温度/容量 每10秒上报        │
└────────────────────────────────────────────────────────────┘
                        │
                        ▼
┌────────────────────────────────────────────────────────────┐
│  ClickHouse (列式存储数据库                     │
│  - 按月分区 PARTITION BY toYYYYMM(timestamp)     │
│  - TTL自动过期 (90天)                            │
│  - MergeTree引擎 + ReplacingMergeTree               │
└────────────────────────────────────────────────────────────┘
                        │
                        ▼
┌────────────────────────────────────────────────────────────┐
│  Prometheus + Grafana 监控面板           │
│  - Rust服务指标 /metrics 端点            │
│  - ClickHouse指标采集                 │
│  - 可视化仪表盘                     │
└────────────────────────────────────────────────────────────┘
```

## 📁 目录结构

```
.
├── backend/                    # Rust 后端服务
│   ├── src/
│   │   ├── main.rs           # 主入口，模块初始化
│   │   ├── mqtt_collector.rs  # MQTT订阅接收模块
│   │   ├── data_pipeline.rs   # 数据处理流水线
│   │   ├── anomaly_detector.rs # 异常检测模块
│   │   ├── capacity_predictor.rs  # 容量预测模块
│   │   ├── alarm_sender.rs    # 告警推送模块
│   │   ├── metrics.rs         # Prometheus指标模块
│   │   ├── api.rs           # HTTP API接口
│   │   ├── config.rs        # 配置管理
│   │   ├── database.rs      # ClickHouse数据库操作
│   │   ├── models.rs        # 数据模型
│   │   ├── messages.rs      # 消息类型定义
│   │   └── stage_detector.rs # 工艺阶段检测
│   ├── model_config.json  # 电池模型配置（特征权重）
│   ├── Cargo.toml
│   └── Dockerfile           # 多阶段构建
│
├── frontend/                   # 前端静态资源
│   ├── index.html
│   ├── cabinet_panel.js     # 化成柜面板组件
│   ├── channel_detail.js # 通道详情组件
│   ├── styles.css
│   └── app.js            # （已废弃，已拆分）
│
├── simulator/                  # 化成柜模拟器
│   ├── cabinet_simulator.py  # 模拟器主程序
│   ├── requirements.txt
│   └── Dockerfile
│
├── clickhouse/                 # ClickHouse配置
│   ├── init.sql          # 数据库初始化脚本
│   └── config.xml        # ClickHouse配置
│
├── prometheus/                 # Prometheus配置
│   └── prometheus.yml
│
├── grafana/                    # Grafana配置
│   ├── datasources/
│   └── dashboards/
│
├── docker-compose.yml       # Docker编排文件
├── nginx.conf           # Nginx配置（Gzip+缓存）
├── mosquitto.conf      # MQTT Broker配置
├── .env.example       # 环境变量示例
└── README.md
```

## 🚀 快速开始

### 环境要求

- Docker >= 24.0
- Docker Compose >= 2.20
- 内存 >= 8GB（建议16GB）
- 磁盘 >= 50GB

### 一键部署

1. **克隆项目**

```bash
git clone <repository-url>
cd AI_solo_coder_task_A_032
```

2. **配置环境变量**

```bash
cp .env.example .env
# 根据需要修改 .env 中的配置
```

3. **启动核心服务**

```bash
# 启动核心服务（后端 + 数据库 + MQTT + 前端
docker-compose up -d

# 启动所有服务（含模拟器 + 监控）
docker-compose --profile all up -d

# 只启动模拟器
docker-compose --profile simulator up -d simulator

# 只启动监控
docker-compose --profile monitoring up -d
```

4. **验证服务状态**

```bash
docker-compose ps
```

5. **访问系统**

| 服务 | 地址 | 说明 |
|------|------|------|
| 前端 | http://localhost | 监控面板 |
| API文档 | http://localhost/api/health | 健康检查 |
| Prometheus | http://localhost:9090 | 指标监控 |
| Grafana | http://localhost:3000 | 可视化仪表盘 |
| ClickHouse | http://localhost:8123 | 数据库接口 |
| MQTT | tcp://localhost:1883 | MQTT Broker |
| Metrics | http://localhost:8080/metrics | Rust服务指标 |

### 常用命令

```bash
# 查看日志
docker-compose logs -f backend          # 查看后端日志
docker-compose logs -f simulator    # 查看模拟器日志
docker-compose logs -f clickhouse   # 查看数据库日志

# 停止服务
docker-compose stop

# 停止并清理
docker-compose down

# 重新构建
docker-compose build --no-cache

# 重启服务
docker-compose restart backend
```

## 🎛️ 模拟器配置说明

### 环境变量配置

| 变量名 | 默认值 | 说明 |
|--------|--------|------|
| `NUM_CABINETS` | 20 | 化成柜数量 |
| `CHANNELS_PER_CABINET` | 512 | 每柜通道数 |
| `REPORT_INTERVAL` | 10 | 数据上报间隔（秒） |
| `ABNORMAL_RATIO` | 0.03 | 异常通道比例 |
| `ANOMALY_DURATION_MIN` | 60 | 异常持续最小时间（秒） |
| `ANOMALY_DURATION_MAX` | 300 | 异常持续最大时间（秒） |
| `RATED_CAPACITY` | 3.2 | 额定容量（Ah） |
| `CAPACITY_FACTOR_MIN` | 0.85 | 容量因子最小值 |
| `CAPACITY_FACTOR_MAX` | 1.05 | 容量因子最大值 |
| `BATCH_SIZE` | 64 | MQTT消息批量大小 |
| `MQTT_QOS` | 1 | MQTT QoS级别 |

### 异常类型

模拟器支持4种异常类型：

| 异常类型 | 说明 | 表现 |
|---------|------|------|
| `voltage_abnormal` | 电压异常 | 电压随机波动±0.3V |
| `current_abnormal` | 电流异常 | 电流随机波动±0.5A |
| `temperature_high` | 高温异常 | 温度升高5-15°C |
| `capacity_low` | 容量异常 | 容量降低30% |

### 自定义异常场景

```python
# 在模拟器启动后，可以通过设置环境变量模拟不同场景：

# 高异常比例场景（10%异常）
ABNORMAL_RATIO=0.10

# 无异常场景（用于基准测试）
ABNORMAL_RATIO=0.00

# 快速测试场景（1秒上报）
REPORT_INTERVAL=1

# 小容量场景（测试用）
NUM_CABINETS=2
CHANNELS_PER_CABINET=64
```

## 🔧 容量预测模型配置

### 模型配置文件 (`backend/model_config.json`)

支持多种电池型号，每种型号独立配置特征权重和模型参数。

```json
{
  "NMC_3Ah": {
    "description": "三元锂电池 3.0Ah 标准型号",
    "rated_capacity": 3.2,
    "feature_names": [
      "cc_charge_time", "cv_charge_time", "discharge_time",
      "discharge_platform_voltage", "cc_end_voltage", "cv_end_current",
      "max_charge_temp", "max_discharge_temp", "efficiency", "charge_capacity"
    ],
    "feature_weights": {
      "cc_charge_time": 1.0,
      "cv_charge_time": 1.2,
      "discharge_time": 1.0,
      "discharge_platform_voltage": 1.5,
      "cc_end_voltage": 1.1,
      "cv_end_current": 1.3,
      "max_charge_temp": 0.8,
      "max_discharge_temp": 0.8,
      "efficiency": 1.4,
      "charge_capacity": 1.6
    },
    "feature_ranges": {
      "cc_charge_time": [1800, 14400],
      "cv_charge_time": [600, 7200],
      "discharge_time": [1800, 10800],
      "discharge_platform_voltage": [3.2, 3.8],
      "cc_end_voltage": [3.8, 4.3],
      "cv_end_current": [0.05, 0.3],
      "max_charge_temp": [25, 55],
      "max_discharge_temp": [25, 55],
      "efficiency": [0.9, 0.99],
      "charge_capacity": [2.5, 3.5]
    },
    "model_params": {
      "num_trees": 100,
      "max_depth": 5,
      "learning_rate": 0.1,
      "min_samples_split": 10
    },
    "min_cycles": 3
  }
}
```

### 添加新电池型号

1. 在 `model_config.json` 中添加新的型号配置
2. 更新 `.env` 中的 `DEFAULT_MODEL`
3. 重启后端服务

## 📊 ClickHouse 数据存储配置

### 数据分区策略

```sql
-- 按月分区
PARTITION BY toYYYYMM(timestamp)

-- 主键排序
ORDER BY (cabinet_id, channel_id, timestamp)

-- TTL自动过期90天
TTL timestamp + INTERVAL 90 DAY
```

### 主要数据表

| 表名 | 说明 | 引擎 |
|------|------|------|
| `channel_data` | 原始通道数据 | MergeTree |
| `channel_stage_summary` | 阶段统计摘要 | MergeTree |
| `cycle_features` | 循环特征 | MergeTree |
| `capacity_prediction` | 容量预测结果 | MergeTree |
| `anomalies` | 异常记录 | MergeTree |
| `alerts` | 告警记录 | MergeTree |
| `channel_status` | 通道最新状态 | ReplacingMergeTree |
| `cabinet_stats` | 化成柜统计 | MergeTree |

### 关键配置参数

| 参数 | 值 | 说明 |
|------|-----|------|
| `index_granularity` | 8192 | 索引粒度 |
| `ttl_only_drop_parts` | 1 | 仅删除过期部分 |
| `compression` | LZ4/ZSTD | 压缩算法 |
| `max_memory_usage` | 10G | 最大内存使用 |

## 📈 Prometheus 监控指标

### Rust 服务指标

| 指标名 | 类型 | 说明 |
|--------|------|------|
| `mqtt_messages_received_total` | Counter | 接收的MQTT消息总数 |
| `mqtt_bytes_received_total` | Counter | 接收的MQTT字节总数 |
| `channel_data_inserted_total` | Counter | 插入的数据点总数 |
| `anomalies_detected_total` | Counter | 检测到的异常数（按类型） |
| `predictions_made_total` | Counter | 容量预测总数 |
| `alerts_generated_total` | Counter | 生成的告警数（按级别） |
| `active_channels` | Gauge | 活跃通道数 |
| `abnormal_channels` | Gauge | 异常通道数 |
| `paused_channels` | Gauge | 暂停通道数 |
| `system_capacity_ratio` | Gauge | 系统平均容量比 |
| `prediction_latency_seconds` | Histogram | 预测延迟 |
| `mqtt_processing_latency_ms` | Histogram | MQTT处理延迟 |
| `db_insert_latency_ms` | Histogram | 数据库插入延迟 |

### 访问指标

```bash
# 查看Rust服务指标
curl http://localhost:8080/metrics

# 查看Prometheus目标状态
open http://localhost:9090/targets
```

## 🔌 API 接口文档

### 健康检查

```http
GET /api/health
```

### 化成柜相关

```http
GET /api/cabinets                          # 获取所有化成柜列表
GET /api/cabinet/:id                         # 获取化成柜面板数据
GET /api/cabinet/:id/status                  # 获取化成柜状态
```

### 通道相关

```http
GET /api/channel/:cabinet_id/:channel_id         # 获取通道详情
GET /api/channel/:cabinet_id/:channel_id/history # 获取通道历史数据
POST /api/channel/:cabinet_id/:channel_id/pause  # 暂停通道
POST /api/channel/:cabinet_id/:channel_id/resume # 恢复通道
```

### 预测与告警

```http
GET /api/predict/:cabinet_id/:channel_id        # 触发容量预测
GET /api/alerts                             # 获取告警列表
POST /api/alerts/:id/acknowledge              # 确认告警
GET /api/anomalies                           # 获取异常列表
```

### 系统统计

```http
GET /api/stats/summary                      # 获取系统统计摘要
GET /metrics                              # Prometheus指标
```

## ⚠️ 告警规则

### 一级告警（单通道）

- 容量低于额定值90% → 降级品告警

### 二级告警（化成柜）

- 同一化成柜超过10%通道异常 → 设备告警

### 告警推送渠道

- MQTT Topic: `battery/alerts`
- 产线大屏（WebSocket）
- MES系统接口

## 🛡️ 安全配置

### Nginx安全头

```nginx
add_header X-Content-Type-Options nosniff;
add_header X-Frame-Options SAMEORIGIN;
add_header X-XSS-Protection "1; mode=block";
```

### 前端缓存策略

| 资源类型 | 缓存时间 | 说明 |
|----------|----------|------|
| JS/CSS/图片 | 1年 | 不可变资源，内容哈希命名 |
| HTML | 1小时 | 可重新验证 |
| API响应 | 1分钟 | 动态数据 |

## 📝 日志配置

### Rust服务日志

- 使用 `tracing` 框架
- 支持结构化JSON格式
- 环境变量控制日志级别

```bash
# 仅错误日志
RUST_LOG=error

# 调试模式
RUST_LOG=debug

# 模块级别
RUST_LOG=info,battery_monitor=debug
```

### 日志轮转

```json
{
  "driver": "json-file",
  "options": {
    "max-size": "50m",
    "max-file": "3"
  }
}
```

## 🔍 故障排查

### 常见问题

**1. 后端无法连接MQTT**

```bash
# 检查MQTT容器状态
docker-compose ps mqtt-broker

# 查看MQTT日志
docker-compose logs mqtt-broker

# 测试MQTT连接
docker run --rm -it eclipse-mosquitto mosquitto_pub -h mqtt-broker -t test -m "hello"
```

**2. ClickHouse连接失败**

```bash
# 检查ClickHouse状态
curl http://localhost:8123/ping

# 查看数据库日志
docker-compose logs clickhouse

# 手动连接
docker exec -it battery-clickhouse clickhouse-client
```

**3. 模拟器不发送数据**

```bash
# 查看模拟器日志
docker-compose logs simulator

# 检查MQTT主题
docker exec -it battery-mqtt mosquitto_sub -t "battery/cabinet/+/data" -C 1
```

**4. 前端无法访问**

```bash
# 检查Nginx配置
docker exec battery-frontend nginx -t

# 查看Nginx错误日志
docker logs battery-frontend
```

## 📄 License

MIT License

---

**注意事项：

1. 生产环境请修改默认密码
2. 建议配置HTTPS
3. 定期备份ClickHouse数据
4. 监控磁盘空间使用情况
