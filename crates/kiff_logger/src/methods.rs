use std::collections::HashMap;

use log_engine::LogRecord;
use rust_apps_core::{AppContext, AppState};
use serde_json::{Map, Value};

use crate::handlers::QueryResponse;

fn require_logger(state: &AppState) -> error::Result<&log_engine::LogService> {
    state
        .logger
        .get()
        .ok_or_else(|| error::RuntimeError::Config("log engine is not initialized".into()))
}

pub async fn ingest_method(
    ctx: AppContext,
    params: HashMap<String, Value>,
) -> error::Result<Value> {
    let logger = require_logger(&ctx.state)?;

    let level = params
        .get("level")
        .and_then(|v| v.as_str())
        .unwrap_or("INFO")
        .to_string();
    let service = params
        .get("service")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let message = params
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let mut rec = LogRecord::new(&level, &service, &message);
    if let Some(Value::Object(fields)) = params.get("fields") {
        for (k, v) in fields {
            rec.fields.insert(k.clone(), v.clone());
        }
    }

    logger
        .ingest(rec)
        .await
        .map_err(|e| error::RuntimeError::Validation(format!("log ingest failed: {}", e)))?;
    Ok(Value::Object(Map::from_iter([(
        "ok".to_string(),
        Value::Bool(true),
    )])))
}

pub async fn query_method(
    ctx: AppContext,
    params: HashMap<String, Value>,
) -> error::Result<Value> {
    let logger = require_logger(&ctx.state)?;

    let q = params
        .get("q")
        .and_then(|v| v.as_str())
        .unwrap_or("*")
        .to_string();
    let limit = params
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(50) as usize;

    let records = logger
        .query(&q, limit)
        .await
        .map_err(|e| error::RuntimeError::Validation(format!("log query failed: {}", e)))?;
    let resp = QueryResponse {
        total: records.len(),
        records,
    };
    Ok(serde_json::to_value(resp)?)
}
