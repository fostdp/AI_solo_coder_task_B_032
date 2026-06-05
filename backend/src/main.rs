mod alert_manager;
mod anomaly_detector;
mod api;
mod config;
mod database;
mod models;
mod mqtt_client;
mod prediction;
mod stage_detector;

use crate::alert_manager::AlertManager;
use crate::anomaly_detector::AnomalyDetector;
use crate::api::{create_router, ApiState};
use crate::config::Config;
use crate::database::Database;
use crate::mqtt_client::MqttDataClient;
use crate::prediction::CapacityPredictor;
use crate::stage_detector::StageDetector;
use anyhow::Result;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info};
use tracing_subscriber::{fmt, EnvFilter};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env())
        .with(fmt::layer())
        .init();

    info!("Starting Battery Monitor System...");

    let config = Config::default();
    info!("Configuration loaded");

    let db = Database::new(config.clickhouse.clone())?;
    info!("Database connection established");

    let stage_detector = StageDetector::new();
    info!("Stage detector initialized");

    let anomaly_detector = AnomalyDetector::new(config.detection.clone(), db.clone());
    info!("Anomaly detector initialized");

    let predictor = CapacityPredictor::new(db.clone());
    info!("Capacity predictor initialized");

    predictor.train_with_historical_data().await?;
    info!("Prediction model trained with sample data");

    let alert_manager = AlertManager::new(
        config.alert.clone(),
        config.mqtt.clone(),
        db.clone(),
    )
    .await;
    info!("Alert manager initialized");

    let mqtt_client = MqttDataClient::new(
        config.mqtt.clone(),
        db.clone(),
        stage_detector,
        anomaly_detector,
        alert_manager.clone(),
        predictor.clone(),
    )?;
    info!("MQTT client created");

    mqtt_client.start().await?;
    info!("MQTT client started");

    mqtt_client.periodic_tasks().await;
    info!("Periodic tasks scheduled");

    let api_state = Arc::new(ApiState {
        db: db.clone(),
        predictor,
        alert_manager,
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = create_router(api_state).layer(cors);

    let addr = format!("{}:{}", config.server.host, config.server.port);
    info!("Starting HTTP server on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app)
        .await
        .map_err(|e| anyhow::anyhow!("Server error: {}", e))?;

    Ok(())
}
