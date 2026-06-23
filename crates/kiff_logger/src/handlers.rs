use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use log_engine::LogRecord;
use rust_apps_core::AppState;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub struct IngestRequest {
    pub level: String,
    pub service: String,
    pub message: String,
    #[serde(default)]
    pub fields: serde_json::Map<String, Value>,
}

#[derive(Debug, Serialize)]
pub struct IngestResponse {
    pub ok: bool,
}

#[derive(Debug, Deserialize)]
pub struct QueryParams {
    pub q: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    50
}

#[derive(Debug, Serialize)]
pub struct QueryResponse {
    pub records: Vec<LogRecord>,
    pub total: usize,
}

pub async fn ingest_handler(
    State(state): State<AppState>,
    Json(req): Json<IngestRequest>,
) -> Result<Json<IngestResponse>, StatusCode> {
    let logger = state.logger.get().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let mut rec = LogRecord::new(&req.level, &req.service, &req.message);
    for (k, v) in req.fields {
        rec.fields.insert(k, v);
    }

    logger
        .ingest(rec)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(IngestResponse { ok: true }))
}

pub async fn query_handler(
    State(state): State<AppState>,
    Query(params): Query<QueryParams>,
) -> Result<Json<QueryResponse>, StatusCode> {
    let logger = state.logger.get().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let records = logger
        .query(&params.q, params.limit)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let total = records.len();
    Ok(Json(QueryResponse { records, total }))
}
