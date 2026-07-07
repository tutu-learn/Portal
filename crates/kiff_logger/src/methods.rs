use std::collections::HashMap;

use log_engine::LogRecord;
use rust_apps_core::{AppContext, AppState};
use serde_json::{Map, Value};

use crate::handlers::QueryResponse;
use crate::token;

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
    if ctx.user.is_none() {
        return Err(error::RuntimeError::Auth(
            "Authentication required. Provide a valid Kiff Logger bearer token.".into(),
        ));
    }
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

pub async fn query_method(ctx: AppContext, params: HashMap<String, Value>) -> error::Result<Value> {
    if ctx.user.is_none() {
        return Err(error::RuntimeError::Auth(
            "Authentication required. Provide a valid Kiff Logger bearer token.".into(),
        ));
    }
    let logger = require_logger(&ctx.state)?;

    let q = params
        .get("q")
        .and_then(|v| v.as_str())
        .unwrap_or("*")
        .to_string();
    let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;

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

pub async fn create_token_method(
    ctx: AppContext,
    params: HashMap<String, Value>,
) -> error::Result<Value> {
    let pool = ctx
        .state
        .pools
        .iter()
        .next()
        .map(|e| e.value().clone())
        .ok_or_else(|| error::RuntimeError::Config("no database pool available".into()))?;

    let caller = ctx.user.as_deref().unwrap_or("Guest");
    if !has_role(&pool, caller, "Kiff Logs Admin").await? {
        return Err(error::RuntimeError::Auth(
            "Kiff Logs Admin role is required to create tokens.".into(),
        ));
    }

    let token_name = params
        .get("token_name")
        .and_then(|v| v.as_str())
        .unwrap_or("External App Token")
        .to_string();
    let external_app = params.get("external_app").and_then(|v| v.as_str());
    let role = params
        .get("role")
        .and_then(|v| v.as_str())
        .unwrap_or("Kiff Logs")
        .to_string();
    let description = params.get("description").and_then(|v| v.as_str());

    let raw_token =
        token::create_token(&pool, &token_name, external_app, &role, description).await?;

    Ok(Value::Object(Map::from_iter([
        ("ok".to_string(), Value::Bool(true)),
        ("token".to_string(), Value::String(raw_token)),
        (
            "message".to_string(),
            Value::String("Copy this token now — it will not be shown again.".into()),
        ),
    ])))
}

pub async fn revoke_token_method(
    ctx: AppContext,
    params: HashMap<String, Value>,
) -> error::Result<Value> {
    let pool = ctx
        .state
        .pools
        .iter()
        .next()
        .map(|e| e.value().clone())
        .ok_or_else(|| error::RuntimeError::Config("no database pool available".into()))?;

    let caller = ctx.user.as_deref().unwrap_or("Guest");
    if !has_role(&pool, caller, "Kiff Logs Admin").await? {
        return Err(error::RuntimeError::Auth(
            "Kiff Logs Admin role is required to revoke tokens.".into(),
        ));
    }

    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| error::RuntimeError::Validation("name is required".into()))?;

    let revoked = token::revoke_token(&pool, name, caller).await?;

    Ok(Value::Object(Map::from_iter([
        ("ok".to_string(), Value::Bool(revoked)),
        (
            "message".to_string(),
            Value::String(if revoked {
                "Token revoked.".into()
            } else {
                "Token not found or already revoked.".into()
            }),
        ),
    ])))
}

async fn has_role(pool: &orm::DatabasePool, user: &str, role: &str) -> error::Result<bool> {
    let pm = permissions::PermissionEngine::new();
    let roles = pm.get_roles(pool, user).await?;
    Ok(roles.iter().any(|r| r == role))
}
