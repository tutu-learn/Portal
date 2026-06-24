use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use log_engine::LogRecord;
use rust_apps_core::AppState;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::token;

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
    headers: HeaderMap,
    Json(req): Json<IngestRequest>,
) -> Result<Json<IngestResponse>, StatusCode> {
    let pool = state.pools.iter().next().map(|e| e.value().clone());
    let pool = pool.ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let token_info = authenticate_bearer(&pool, &headers).await?;

    let logger = state.logger.get().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let mut rec = LogRecord::new(&req.level, &req.service, &req.message);
    rec.fields
        .insert("token_name".into(), token_info.token_name.clone().into());
    if !token_info.external_app.is_empty() {
        rec.fields.insert(
            "external_app".into(),
            token_info.external_app.clone().into(),
        );
    }
    for (k, v) in req.fields {
        rec.fields.insert(k, v);
    }

    logger
        .ingest(rec)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(IngestResponse { ok: true }))
}

async fn authenticate_bearer(
    pool: &orm::DatabasePool,
    headers: &HeaderMap,
) -> Result<token::VerifiedToken, StatusCode> {
    let auth_header = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let token = auth_header
        .strip_prefix("Bearer ")
        .map(str::trim)
        .ok_or(StatusCode::UNAUTHORIZED)?;
    match token::verify_bearer_token(pool, token).await {
        Ok(Some(verified)) => {
            let _ = token::touch_token(pool, &verified.name).await;
            Ok(verified)
        }
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

pub async fn query_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<QueryParams>,
) -> Result<Json<QueryResponse>, StatusCode> {
    let pool = state.pools.iter().next().map(|e| e.value().clone());
    let pool = pool.ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let _token_info = authenticate_bearer(&pool, &headers).await?;

    let logger = state.logger.get().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let records = logger
        .query(&params.q, params.limit)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let total = records.len();
    Ok(Json(QueryResponse { records, total }))
}
