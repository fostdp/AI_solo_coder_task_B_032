use crate::alarm_sender::AlarmSender;
use crate::capacity_predictor::CapacityPredictor;
use crate::database::Database;
use crate::messages::PredictionRequest;
use crate::models::{
    CabinetStats, ChannelHistory, ChannelStatus, CHANNELS_PER_CABINET, NUM_CABINETS,
    RATED_CAPACITY,
};
use axum::{
    extract::{Path, Query},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub hours: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct CabinetQuery {
    pub cabinet_id: Option<u16>,
}

#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub message: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CabinetPanelData {
    pub cabinet_id: u16,
    pub channels: Vec<ChannelPixelData>,
    pub stats: Option<CabinetStats>,
}

#[derive(Debug, Serialize)]
pub struct ChannelPixelData {
    pub channel_id: u32,
    pub capacity_ratio: f64,
    pub is_abnormal: bool,
    pub is_paused: bool,
    pub stage: u8,
    pub color: String,
}

#[derive(Debug, Serialize)]
pub struct ChannelDetailResponse {
    pub status: ChannelStatus,
    pub history: ChannelHistory,
    pub capacity_trend: crate::models::CapacityTrend,
    pub stage_summaries: Vec<crate::models::StageSummary>,
    pub predictions: Vec<crate::models::PredictionResult>,
}

pub struct ApiState {
    pub db: Database,
    pub predictor: CapacityPredictor,
    pub alert_manager: AlarmSender,
}

pub fn create_router(state: Arc<ApiState>) -> Router {
    Router::new()
        .route("/api/health", get(health_check))
        .route("/api/cabinets", get(get_all_cabinets))
        .route("/api/cabinet/:id", get(get_cabinet_panel))
        .route("/api/cabinet/:id/status", get(get_cabinet_status))
        .route("/api/channel/:cabinet_id/:channel_id", get(get_channel_detail))
        .route("/api/channel/:cabinet_id/:channel_id/history", get(get_channel_history))
        .route("/api/channel/:cabinet_id/:channel_id/pause", post(pause_channel))
        .route("/api/channel/:cabinet_id/:channel_id/resume", post(resume_channel))
        .route("/api/predict/:cabinet_id/:channel_id", get(predict_capacity))
        .route("/api/alerts", get(get_alerts))
        .route("/api/alerts/:id/acknowledge", post(acknowledge_alert))
        .route("/api/anomalies", get(get_anomalies))
        .route("/api/stats/summary", get(get_system_summary))
        .with_state(state)
}

async fn health_check() -> impl IntoResponse {
    Json(ApiResponse {
        success: true,
        data: Some(serde_json::json!({
            "status": "ok",
            "timestamp": Utc::now().to_rfc3339(),
        })),
        message: None,
    })
}

async fn get_all_cabinets(
    axum::extract::State(state): axum::extract::State<Arc<ApiState>>,
) -> impl IntoResponse {
    match state.db.get_all_cabinet_statuses().await {
        Ok(statuses) => {
            let mut cabinets: HashMap<u16, Vec<ChannelStatus>> = HashMap::new();
            for status in statuses {
                cabinets.entry(status.cabinet_id).or_default().push(status);
            }

            let result: Vec<_> = (0..NUM_CABINETS as u16)
                .map(|id| {
                    let channels = cabinets.get(&id).cloned().unwrap_or_default();
                    let abnormal_count = channels.iter().filter(|c| c.is_abnormal).count();
                    serde_json::json!({
                        "cabinet_id": id,
                        "total_channels": CHANNELS_PER_CABINET,
                        "active_channels": channels.len(),
                        "abnormal_channels": abnormal_count,
                        "abnormal_ratio": if CHANNELS_PER_CABINET > 0 {
                            abnormal_count as f64 / CHANNELS_PER_CABINET as f64
                        } else { 0.0 },
                    })
                })
                .collect();

            Json(ApiResponse {
                success: true,
                data: Some(result),
                message: None,
            })
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::<()> {
                success: false,
                data: None,
                message: Some(format!("Failed to get cabinets: {}", e)),
            }),
        )
            .into_response(),
    }
}

async fn get_cabinet_panel(
    axum::extract::State(state): axum::extract::State<Arc<ApiState>>,
    Path(id): Path<u16>,
) -> impl IntoResponse {
    if id >= NUM_CABINETS as u16 {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::<()> {
                success: false,
                data: None,
                message: Some(format!(
                    "Invalid cabinet id: {}, max is {}",
                    id,
                    NUM_CABINETS - 1
                )),
            }),
        )
            .into_response();
    }

    let statuses = match state.db.get_cabinet_status(id).await {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<()> {
                    success: false,
                    data: None,
                    message: Some(format!("Failed to get cabinet status: {}", e)),
                }),
            )
                .into_response()
        }
    };

    let mut channel_map: HashMap<u32, ChannelStatus> = HashMap::new();
    for status in statuses {
        channel_map.insert(status.channel_id, status);
    }

    let channels: Vec<ChannelPixelData> = (0..CHANNELS_PER_CABINET as u32)
        .map(|channel_id| {
            let status = channel_map.get(&channel_id);
            let (capacity_ratio, is_abnormal, is_paused, stage) = match status {
                Some(s) => (
                    s.capacity_ratio,
                    s.is_abnormal,
                    s.is_paused,
                    s.current_stage as u8,
                ),
                None => (0.0, false, false, 0),
            };

            let color = get_channel_color(capacity_ratio, is_abnormal, is_paused);

            ChannelPixelData {
                channel_id,
                capacity_ratio,
                is_abnormal,
                is_paused,
                stage,
                color,
            }
        })
        .collect();

    let stats = state.alert_manager.get_cabinet_stats(id).await;

    Json(ApiResponse {
        success: true,
        data: Some(CabinetPanelData {
            cabinet_id: id,
            channels,
            stats,
        }),
        message: None,
    })
    .into_response()
}

fn get_channel_color(capacity_ratio: f64, is_abnormal: bool, is_paused: bool) -> String {
    if is_paused {
        return "#808080".to_string();
    }
    if is_abnormal {
        return "#FF0000".to_string();
    }
    if capacity_ratio >= 0.95 {
        "#00FF00".to_string()
    } else if capacity_ratio >= 0.90 {
        "#FFFF00".to_string()
    } else {
        "#FF6600".to_string()
    }
}

async fn get_cabinet_status(
    axum::extract::State(state): axum::extract::State<Arc<ApiState>>,
    Path(id): Path<u16>,
) -> impl IntoResponse {
    match state.db.get_cabinet_status(id).await {
        Ok(statuses) => Json(ApiResponse {
            success: true,
            data: Some(statuses),
            message: None,
        })
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::<()> {
                success: false,
                data: None,
                message: Some(format!("Failed to get cabinet status: {}", e)),
            }),
        )
            .into_response(),
    }
}

async fn get_channel_detail(
    axum::extract::State(state): axum::extract::State<Arc<ApiState>>,
    Path((cabinet_id, channel_id)): Path<(u16, u32)>,
) -> impl IntoResponse {
    let status = match state.db.get_channel_status(cabinet_id, channel_id).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiResponse::<()> {
                    success: false,
                    data: None,
                    message: Some("Channel not found".to_string()),
                }),
            )
                .into_response()
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<()> {
                    success: false,
                    data: None,
                    message: Some(format!("Failed to get channel status: {}", e)),
                }),
            )
                .into_response()
        }
    };

    let end_time = Utc::now();
    let start_time = end_time - Duration::hours(24);
    let history = state
        .db
        .get_channel_history(cabinet_id, channel_id, start_time, end_time)
        .await
        .unwrap_or(crate::models::ChannelHistory {
            timestamps: Vec::new(),
            voltages: Vec::new(),
            currents: Vec::new(),
            temperatures: Vec::new(),
            capacities: Vec::new(),
            stages: Vec::new(),
        });

    let capacity_trend = state
        .db
        .get_capacity_trend(cabinet_id, channel_id, 20)
        .await
        .unwrap_or(crate::models::CapacityTrend {
            cycle_indices: Vec::new(),
            charge_capacities: Vec::new(),
            discharge_capacities: Vec::new(),
            predicted_capacities: Vec::new(),
        });

    let cycle = state
        .db
        .get_channel_current_cycle(cabinet_id, channel_id)
        .await
        .unwrap_or(Some(0))
        .unwrap_or(0);

    let stage_summaries = state
        .db
        .get_stage_summaries(cabinet_id, channel_id, cycle)
        .await
        .unwrap_or_default();

    let response = ChannelDetailResponse {
        status,
        history,
        capacity_trend,
        stage_summaries,
        predictions: Vec::new(),
    };

    Json(ApiResponse {
        success: true,
        data: Some(response),
        message: None,
    })
    .into_response()
}

async fn get_channel_history(
    axum::extract::State(state): axum::extract::State<Arc<ApiState>>,
    Path((cabinet_id, channel_id)): Path<(u16, u32)>,
    Query(query): Query<HistoryQuery>,
) -> impl IntoResponse {
    let end_time = match query.end_time.as_deref() {
        Some(s) => DateTime::parse_from_rfc3339(s)
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        None => Utc::now(),
    };

    let hours = query.hours.unwrap_or(24);
    let start_time = match query.start_time.as_deref() {
        Some(s) => DateTime::parse_from_rfc3339(s)
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| end_time - Duration::hours(hours)),
        None => end_time - Duration::hours(hours),
    };

    match state
        .db
        .get_channel_history(cabinet_id, channel_id, start_time, end_time)
        .await
    {
        Ok(history) => Json(ApiResponse {
            success: true,
            data: Some(history),
            message: None,
        })
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::<()> {
                success: false,
                data: None,
                message: Some(format!("Failed to get channel history: {}", e)),
            }),
        )
            .into_response(),
    }
}

async fn pause_channel(
    axum::extract::State(state): axum::extract::State<Arc<ApiState>>,
    Path((cabinet_id, channel_id)): Path<(u16, u32)>,
) -> impl IntoResponse {
    match state.db.pause_channel(cabinet_id, channel_id).await {
        Ok(_) => {
            state
                .alert_manager
                .send_pause_command(cabinet_id, channel_id)
                .await;
            info!("Channel paused: {}-{}", cabinet_id, channel_id);
            Json(ApiResponse {
                success: true,
                data: Some(serde_json::json!({
                    "cabinet_id": cabinet_id,
                    "channel_id": channel_id,
                    "paused": true,
                })),
                message: None,
            })
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::<()> {
                success: false,
                data: None,
                message: Some(format!("Failed to pause channel: {}", e)),
            }),
        ),
    }
}

async fn resume_channel(
    axum::extract::State(state): axum::extract::State<Arc<ApiState>>,
    Path((cabinet_id, channel_id)): Path<(u16, u32)>,
) -> impl IntoResponse {
    match state.db.resume_channel(cabinet_id, channel_id).await {
        Ok(_) => {
            state
                .alert_manager
                .send_resume_command(cabinet_id, channel_id)
                .await;
            info!("Channel resumed: {}-{}", cabinet_id, channel_id);
            Json(ApiResponse {
                success: true,
                data: Some(serde_json::json!({
                    "cabinet_id": cabinet_id,
                    "channel_id": channel_id,
                    "paused": false,
                })),
                message: None,
            })
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::<()> {
                success: false,
                data: None,
                message: Some(format!("Failed to resume channel: {}", e)),
            }),
        ),
    }
}

async fn predict_capacity(
    axum::extract::State(state): axum::extract::State<Arc<ApiState>>,
    Path((cabinet_id, channel_id)): Path<(u16, u32)>,
) -> impl IntoResponse {
    let request = PredictionRequest {
        cabinet_id,
        channel_id,
        n_cycles: 3,
    };

    match state.predictor.predict_capacity(request).await {
        Some(prediction) => Json(ApiResponse {
            success: true,
            data: Some(prediction),
            message: None,
        })
        .into_response(),
        None => (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::<()> {
                success: false,
                data: None,
                message: Some("Not enough data for prediction".to_string()),
            }),
        )
            .into_response(),
    }
}

async fn get_alerts(
    axum::extract::State(state): axum::extract::State<Arc<ApiState>>,
    Query(query): Query<CabinetQuery>,
) -> impl IntoResponse {
    let limit = 100;
    match state.db.get_recent_alerts(limit).await {
        Ok(alerts) => {
            let filtered: Vec<_> = match query.cabinet_id {
                Some(id) => alerts.into_iter().filter(|a| a.cabinet_id == id).collect(),
                None => alerts,
            };

            Json(ApiResponse {
                success: true,
                data: Some(filtered),
                message: None,
            })
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::<()> {
                success: false,
                data: None,
                message: Some(format!("Failed to get alerts: {}", e)),
            }),
        ),
    }
}

async fn acknowledge_alert(
    axum::extract::State(state): axum::extract::State<Arc<ApiState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let alert_id = match Uuid::parse_str(&id) {
        Ok(u) => u,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::<()> {
                    success: false,
                    data: None,
                    message: Some(format!("Invalid alert ID: {}", e)),
                }),
            );
        }
    };

    match state.alert_manager.acknowledge_alert(alert_id).await {
        Ok(_) => Json(ApiResponse {
            success: true,
            data: Some(serde_json::json!({
                "alert_id": alert_id.to_string(),
                "acknowledged": true,
            })),
            message: None,
        }),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::<()> {
                success: false,
                data: None,
                message: Some(format!("Failed to acknowledge alert: {}", e)),
            }),
        ),
    }
}

async fn get_anomalies(
    axum::extract::State(state): axum::extract::State<Arc<ApiState>>,
    Query(query): Query<CabinetQuery>,
) -> impl IntoResponse {
    let limit = 100;
    match state
        .db
        .get_recent_anomalies(query.cabinet_id, limit)
        .await
    {
        Ok(anomalies) => Json(ApiResponse {
            success: true,
            data: Some(anomalies),
            message: None,
        }),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::<()> {
                success: false,
                data: None,
                message: Some(format!("Failed to get anomalies: {}", e)),
            }),
        ),
    }
}

async fn get_system_summary(
    axum::extract::State(state): axum::extract::State<Arc<ApiState>>,
) -> impl IntoResponse {
    match state.db.get_all_cabinet_statuses().await {
        Ok(statuses) => {
            let total_channels = statuses.len();
            let abnormal_channels = statuses.iter().filter(|s| s.is_abnormal).count();
            let paused_channels = statuses.iter().filter(|s| s.is_paused).count();
            let avg_capacity_ratio = if total_channels > 0 {
                statuses.iter().map(|s| s.capacity_ratio).sum::<f64>() / total_channels as f64
            } else {
                0.0
            };

            let cabinets: HashMap<u16, Vec<_>> =
                statuses.into_iter().fold(HashMap::new(), |mut acc, s| {
                    acc.entry(s.cabinet_id).or_default().push(s);
                    acc
                });

            let cabinet_summaries: Vec<_> = (0..NUM_CABINETS as u16)
                .map(|id| {
                    let channels = cabinets.get(&id).cloned().unwrap_or_default();
                    let abnormal = channels.iter().filter(|c| c.is_abnormal).count();
                    let avg_cap = if !channels.is_empty() {
                        channels.iter().map(|c| c.capacity_ratio).sum::<f64>() / channels.len() as f64
                    } else {
                        0.0
                    };
                    serde_json::json!({
                        "cabinet_id": id,
                        "total_channels": CHANNELS_PER_CABINET,
                        "active_channels": channels.len(),
                        "abnormal_channels": abnormal,
                        "abnormal_ratio": if CHANNELS_PER_CABINET > 0 {
                            abnormal as f64 / CHANNELS_PER_CABINET as f64
                        } else { 0.0 },
                        "avg_capacity_ratio": avg_cap,
                    })
                })
                .collect();

            let summary = serde_json::json!({
                "total_cabinets": NUM_CABINETS,
                "total_channels": NUM_CABINETS * CHANNELS_PER_CABINET,
                "active_channels": total_channels,
                "abnormal_channels": abnormal_channels,
                "paused_channels": paused_channels,
                "avg_capacity_ratio": avg_capacity_ratio,
                "rated_capacity": RATED_CAPACITY,
                "cabinets": cabinet_summaries,
                "timestamp": Utc::now().to_rfc3339(),
            });

            Json(ApiResponse {
                success: true,
                data: Some(summary),
                message: None,
            })
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::<()> {
                success: false,
                data: None,
                message: Some(format!("Failed to get system summary: {}", e)),
            }),
        ),
    }
}
