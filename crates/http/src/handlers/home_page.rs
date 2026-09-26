//! Post-login landing page management.
//!
//! Priority order (see `user_home::get_effective_home_page`): a user's own
//! override, then the admin-set default, then the global `custom_home_path`
//! setting.

use crate::handlers::permissions::require_permission_manager;
use crate::middleware::auth::authenticate_request;
use crate::site::resolve_site_pool;
use crate::user_home::{normalize_stored_home_path, set_admin_home_page, set_user_home_page};
use crate::AppState;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json},
};
use serde_json::Value;

/// `POST /api/method/set_my_home_page` — let the signed-in user set (or, with
/// an empty value, clear) their own home-page override. Requires no special
/// role: a user may only ever touch their own record here.
pub async fn set_my_home_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let session = match authenticate_request(&state, &headers).await {
        Some(s) => s,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "authentication required" })),
            )
                .into_response()
        }
    };

    let raw = body.get("home_page").and_then(|v| v.as_str()).unwrap_or("");
    let home_page = normalize_stored_home_path(raw);
    if !raw.trim().is_empty() && home_page.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "invalid home_page path" })),
        )
            .into_response();
    }

    let pool = match resolve_site_pool(&state, &headers).map(|(_, p)| p) {
        Some(p) => p,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": "no database pool" })),
            )
                .into_response()
        }
    };

    match set_user_home_page(&pool, &session.user, &home_page).await {
        Ok(()) => {
            Json(serde_json::json!({ "message": "ok", "home_page": home_page })).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("{}", e) })),
        )
            .into_response(),
    }
}

/// `POST /api/method/set_user_home_page` — let an admin (`Administrator` or
/// `System Manager`) set the default home page for any user. Body:
/// `{"user": "<name>", "home_page": "<path>"}`.
pub async fn set_user_home_page_admin(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    if let Err(err) = require_permission_manager(&state, &headers).await {
        return err.into_response();
    }

    let target_user = body.get("user").and_then(|v| v.as_str()).unwrap_or("");
    if target_user.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "missing user" })),
        )
            .into_response();
    }

    let raw = body.get("home_page").and_then(|v| v.as_str()).unwrap_or("");
    let home_page = normalize_stored_home_path(raw);
    if !raw.trim().is_empty() && home_page.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "invalid home_page path" })),
        )
            .into_response();
    }

    let pool = match resolve_site_pool(&state, &headers).map(|(_, p)| p) {
        Some(p) => p,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": "no database pool" })),
            )
                .into_response()
        }
    };

    match set_admin_home_page(&pool, target_user, &home_page).await {
        Ok(()) => {
            Json(serde_json::json!({ "message": "ok", "home_page": home_page })).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("{}", e) })),
        )
            .into_response(),
    }
}
