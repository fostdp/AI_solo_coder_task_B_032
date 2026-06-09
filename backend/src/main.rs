mod alarm_sender;
mod anomaly_detector;
mod api;
mod capacity_predictor;
mod cell_grouping;
mod config;
mod data_pipeline;
mod database;
mod degradation_analyzer;
mod electrolyte_optimizer;
mod mes_integrator;
mod messages;
mod metrics;
mod models;
mod mqtt_collector;
mod stage_detector;

use crate::alarm_sender::AlarmSender;
use crate::anomaly_detector::AnomalyDetector;
use crate::api::{create_router, ApiState};
use crate::capacity_predictor::CapacityPredictor;
use crate::cell_grouping::CellGroupingService;
use crate::config::Config;
use crate::data_pipeline::DataPipeline;
use crate::database::Database;
use crate::degradation_analyzer::DegradationAnalysisService;
use crate::electrolyte_optimizer::ElectrolyteOptimizationService;
use crate::mes_integrator::MesIntegrationService;
use crate::messages::{
    AlertSender, AnomalyReceiver, AnomalySender, PauseReceiver, PauseSender, PredictionReceiver,
    PredictionSender, PredictionResultReceiver, PredictionResultSender,
};
use crate::mqtt_collector::{DataReceiver, DataSender};
use crate::mqtt_collector::MqttCollector;
use crate::stage_detector::StageDetector;
use anyhow::Result;
use metrics_exporter_prometheus::PrometheusBuilder;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::compression::CompressionLayer;
use tower_http::trace::TraceLayer;
use tracing::{error, info};
use tracing_subscriber::{fmt, EnvFilter};
use tracing_subscriber::fmt::format::FmtSpan;

const CHANNEL_BUFFER_SIZE: usize = 1000;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env())
        .with(
            fmt::layer()
                .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
                .with_target(true)
                .with_thread_ids(true)
                .with_file(true)
                .with_line_number(true),
        )
        .init();

    info!("Starting Battery Monitor System v1.0.0 (Modular Architecture)...");

    crate::metrics::init_metrics();
    info!("Metrics system initialized");

    let prometheus_handle = PrometheusBuilder::new()
        .install_recorder()
        .expect("Failed to install Prometheus recorder");
    info!("Prometheus metrics recorder installed");

    let config = Config::load();
    info!("Configuration loaded");
    info!(
        "Using model: {}, rated capacity: {:.2}Ah",
        config.model.default_model,
        config
            .model
            .models
            .get(&config.model.default_model)
            .map(|m| m.rated_capacity)
            .unwrap_or(3.2)
    );

    let db = Database::new(config.clickhouse.clone())?;
    info!("Database connection established");

    let model_config = config
        .model
        .models
        .get(&config.model.default_model)
        .cloned()
        .unwrap_or_else(|| {
            error!(
                "Default model '{}' not found, using fallback config",
                config.model.default_model
            );
            config.model.models.values().next().cloned().unwrap()
        });
    let rated_capacity = model_config.rated_capacity;
    let min_cycles = model_config.min_cycles;
    info!(
        "Model config loaded: {}, min_cycles: {}",
        model_config.description, min_cycles
    );

    let (raw_data_sender, raw_data_receiver): (DataSender, DataReceiver) =
        tokio::sync::mpsc::channel(CHANNEL_BUFFER_SIZE);

    let (data_batch_sender, data_batch_receiver) =
        tokio::sync::mpsc::channel(CHANNEL_BUFFER_SIZE);

    let (prediction_sender, prediction_receiver): (PredictionSender, PredictionReceiver) =
        tokio::sync::mpsc::channel(CHANNEL_BUFFER_SIZE);

    let (prediction_result_sender, prediction_result_receiver): (
        PredictionResultSender,
        PredictionResultReceiver,
    ) = tokio::sync::mpsc::channel(CHANNEL_BUFFER_SIZE);

    let (anomaly_sender, anomaly_receiver): (AnomalySender, AnomalyReceiver) =
        tokio::sync::mpsc::channel(CHANNEL_BUFFER_SIZE);

    let (pause_sender, pause_receiver): (PauseSender, PauseReceiver) =
        tokio::sync::mpsc::channel(CHANNEL_BUFFER_SIZE);

    let (alert_sender, alert_receiver) = tokio::sync::mpsc::channel(CHANNEL_BUFFER_SIZE);

    info!("Inter-module channels created");

    let stage_detector = StageDetector::new();
    info!("Stage detector initialized");

    let mqtt_collector = MqttCollector::new(
        config.mqtt.clone(),
        raw_data_sender.clone(),
    )?;
    info!("MQTT collector initialized");

    let mut data_pipeline = DataPipeline::new(
        db.clone(),
        stage_detector,
        data_batch_sender.clone(),
        anomaly_sender.clone(),
        prediction_sender.clone(),
        min_cycles,
    );
    info!("Data pipeline initialized");

    let anomaly_detector = AnomalyDetector::new(config.detection.clone(), db.clone())
        .with_channels(anomaly_sender.clone(), pause_sender.clone());
    info!("Anomaly detector initialized");

    let capacity_predictor =
        CapacityPredictor::new(db.clone(), model_config.clone())
            .with_result_sender(prediction_result_sender.clone());
    info!("Capacity predictor initialized");

    capacity_predictor.train_with_historical_data().await?;
    info!("Prediction model trained with sample data");

    let alarm_sender = AlarmSender::new(
        config.alert.clone(),
        config.mqtt.clone(),
        db.clone(),
    )
    .await
    .with_rated_capacity(rated_capacity);
    info!("Alarm sender initialized");

    let grouping_service = Arc::new(std::sync::Mutex::new(
        CellGroupingService::default()
    ));
    info!("Cell grouping service initialized");

    let electrolyte_service = Arc::new(std::sync::Mutex::new(
        ElectrolyteOptimizationService::default()
    ));
    info!("Electrolyte optimization service initialized");

    let degradation_service = Arc::new(std::sync::Mutex::new(
        DegradationAnalysisService::default()
    ));
    info!("Degradation analysis service initialized");

    let mes_service = Arc::new(std::sync::Mutex::new(
        MesIntegrationService::new(db.clone())
    ));
    info!("MES integration service initialized");

    mqtt_collector.start().await?;
    info!("MQTT collector started");

    data_pipeline.start(raw_data_receiver).await?;
    info!("Data pipeline started");

    let anomaly_detector_clone = anomaly_detector.clone();
    tokio::spawn(async move {
        anomaly_detector_clone.start(data_batch_receiver).await;
    });
    info!("Anomaly detector task started");

    let predictor_clone = capacity_predictor.clone();
    tokio::spawn(async move {
        predictor_clone.start(prediction_receiver).await;
    });
    info!("Capacity predictor task started");

    let alarm_sender_clone = alarm_sender.clone();
    tokio::spawn(async move {
        alarm_sender_clone
            .start(
                anomaly_receiver,
                prediction_result_receiver,
                pause_receiver,
                alert_sender.clone(),
            )
            .await;
    });
    info!("Alarm sender task started");

    let api_state = Arc::new(ApiState {
        db: db.clone(),
        predictor: capacity_predictor,
        alert_manager: alarm_sender,
        prometheus_handle: Arc::new(prometheus_handle),
        grouping_service: grouping_service.clone(),
        electrolyte_service: electrolyte_service.clone(),
        degradation_service: degradation_service.clone(),
        mes_service: mes_service.clone(),
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let compression = CompressionLayer::new().gzip(true).deflate(true).br(true);

    let trace = TraceLayer::new_for_http()
        .make_span_with(tower_http::trace::DefaultMakeSpan::new().level(tracing::Level::INFO))
        .on_response(tower_http::trace::DefaultOnResponse::new().level(tracing::Level::INFO));

    let app = create_router(api_state)
        .layer(cors)
        .layer(compression)
        .layer(trace);

    let addr = format!("{}:{}", config.server.host, config.server.port);
    info!("Starting HTTP server on {}", addr);
    info!("System architecture (v1.1.0 - Extended):");
    info!("  ┌─────────────────┐      ┌─────────────────┐      ┌──────────────────┐");
    info!("  │  MQTT Collector │─────▶│  Data Pipeline  │─────▶│ Anomaly Detector │");
    info!("  └─────────────────┘      └─────────────────┘      └────────┬─────────┘");
    info!("                                 │                            │");
    info!("                                 ▼                            ▼");
    info!("                    ┌──────────────────────┐      ┌───────────────────┐");
    info!("                    │ Capacity Predictor   │      │   Alarm Sender    │");
    info!("                    └──────────────────────┘      └───────────────────┘");
    info!("");
    info!("  New Features (v1.1.0):");
    info!("  ┌───────────────────┐  ┌────────────────────┐  ┌────────────────────┐  ┌────────────┐");
    info!("  │  Cell Grouping    │  │ Electrolyte Opt    │  │ Degradation Analy  │  │  MES Sync  │");
    info!("  │ (GA + Greedy)     │  │ (Feedback Control) │  │ (dQ/dV Analysis)   │  │ (MES/ERP)  │");
    info!("  └───────────────────┘  └────────────────────┘  └────────────────────┘  └────────────┘");
    info!("");
    info!("Prometheus metrics available at /metrics endpoint");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app)
        .await
        .map_err(|e| anyhow::anyhow!("Server error: {}", e))?;

    Ok(())
}
