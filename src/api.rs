use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::supabase::{SessionRow, SignalQuery, SignalRow};
use crate::types::AppState;

/// Response for signals list
#[derive(Serialize)]
pub struct SignalsResponse {
    pub signals: Vec<SignalRow>,
    pub total: u32,
}

/// Response for sessions list
#[derive(Serialize)]
pub struct SessionsResponse {
    pub sessions: Vec<SessionRow>,
}

/// Query params for signals endpoint
#[derive(Debug, Deserialize)]
pub struct SignalsQueryParams {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub signal_type: Option<String>,
    pub direction: Option<String>,
    pub outcome: Option<String>,
    /// Start date filter (ISO 8601 format, e.g., "2024-12-20T09:30:00Z")
    pub start_date: Option<String>,
    /// End date filter (ISO 8601 format)
    pub end_date: Option<String>,
}

/// Query params for sessions endpoint
#[derive(Debug, Deserialize)]
pub struct SessionsQueryParams {
    pub limit: Option<u32>,
}

/// GET /api/signals - List signals with filtering
pub async fn get_signals(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SignalsQueryParams>,
) -> impl IntoResponse {
    let Some(ref supabase) = state.supabase else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "Supabase not configured"})),
        );
    };

    let query = SignalQuery {
        limit: params.limit,
        offset: params.offset,
        signal_type: params.signal_type.clone(),
        direction: params.direction.clone(),
        outcome: params.outcome.clone(),
        start_date: params.start_date.clone(),
        end_date: params.end_date.clone(),
    };

    // Get signals and count in parallel
    let (signals_result, count_result) = tokio::join!(
        supabase.query_signals(&query),
        supabase.count_signals(&query)
    );

    match (signals_result, count_result) {
        (Ok(signals), Ok(total)) => (
            StatusCode::OK,
            Json(serde_json::json!(SignalsResponse { signals, total })),
        ),
        (Err(e), _) | (_, Err(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        ),
    }
}

/// GET /api/sessions - List sessions
pub async fn get_sessions(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SessionsQueryParams>,
) -> impl IntoResponse {
    let Some(ref supabase) = state.supabase else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "Supabase not configured"})),
        );
    };

    let limit = params.limit.unwrap_or(20);

    match supabase.query_sessions(limit).await {
        Ok(sessions) => (
            StatusCode::OK,
            Json(serde_json::json!(SessionsResponse { sessions })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        ),
    }
}

/// GET /api/stats - Aggregate stats
pub async fn get_stats(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let Some(ref supabase) = state.supabase else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "Supabase not configured"})),
        );
    };

    match supabase.get_aggregate_stats().await {
        Ok(stats) => (StatusCode::OK, Json(serde_json::json!(stats))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        ),
    }
}

/// Query params for export endpoint
#[derive(Debug, Deserialize)]
pub struct ExportQueryParams {
    pub signal_type: Option<String>,
    pub direction: Option<String>,
    pub outcome: Option<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    /// Export format: "csv" or "json" (default: json)
    pub format: Option<String>,
}

/// GET /api/signals/export - Export signals as CSV or JSON
pub async fn export_signals(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ExportQueryParams>,
) -> impl IntoResponse {
    let Some(ref supabase) = state.supabase else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            [(header::CONTENT_TYPE, "application/json")],
            r#"{"error": "Supabase not configured"}"#.to_string(),
        );
    };

    let query = SignalQuery {
        limit: Some(10000), // Export up to 10k signals
        offset: None,
        signal_type: params.signal_type.clone(),
        direction: params.direction.clone(),
        outcome: params.outcome.clone(),
        start_date: params.start_date.clone(),
        end_date: params.end_date.clone(),
    };

    let signals = match supabase.query_signals(&query).await {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                [(header::CONTENT_TYPE, "application/json")],
                format!(r#"{{"error": "{}"}}"#, e),
            );
        }
    };

    let format = params.format.as_deref().unwrap_or("json");

    if format == "csv" {
        let csv = signals_to_csv(&signals);
        (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/csv; charset=utf-8")],
            csv,
        )
    } else {
        let json = serde_json::to_string(&signals).unwrap_or_else(|_| "[]".to_string());
        (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            json,
        )
    }
}

/// Convert signals to CSV format
fn signals_to_csv(signals: &[SignalRow]) -> String {
    let mut csv = String::from("id,session_id,timestamp,signal_type,direction,price,price_after_1m,price_after_5m,outcome,created_at\n");

    for signal in signals {
        csv.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{}\n",
            signal.id,
            signal.session_id.map(|u| u.to_string()).unwrap_or_default(),
            signal.timestamp,
            signal.signal_type,
            signal.direction,
            signal.price,
            signal.price_after_1m.map(|p| p.to_string()).unwrap_or_default(),
            signal.price_after_5m.map(|p| p.to_string()).unwrap_or_default(),
            signal.outcome.as_deref().unwrap_or(""),
            signal.created_at,
        ));
    }

    csv
}
