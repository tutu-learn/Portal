//! Local file storage for Thruster binaries.
//!
//! The Thruster release itself is managed through the Sebrus Store Release
//! DocType (see the Thruster tab in /sebrus_apps/store). This module only
//! handles uploading and serving the raw files from:
//!   thruster/windows/<file>
//!   thruster/macos/<file>
//!   thruster/linux/<file>

use crate::middleware::auth::authenticate_request;
use crate::AppState;
use axum::{
    body::Bytes,
    extract::{Multipart, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
};
use serde_json::json;
use std::path::{Path as StdPath, PathBuf};
use tracing::warn;

const THRUSTER_DIR: &str = "thruster";
const PLATFORMS: &[&str] = &["windows", "macos", "linux"];

fn thruster_dir() -> PathBuf {
    StdPath::new(THRUSTER_DIR).to_path_buf()
}

fn is_valid_platform(name: &str) -> bool {
    PLATFORMS.contains(&name)
}

fn is_hidden_or_sidecar(name: &str) -> bool {
    name.starts_with('.') || name.ends_with(".sha256")
}

async fn require_thruster_uploader(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<String, Response> {
    let user = authenticate_request(state, headers)
        .await
        .map(|u| u.user)
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, "authentication required").into_response())?;

    let Some(pool) = state.pools.iter().next().map(|e| e.value().clone()) else {
        return Err((StatusCode::SERVICE_UNAVAILABLE, "no database pool").into_response());
    };

    let roles = match permissions::PermissionEngine::new()
        .get_roles(&pool, &user)
        .await
    {
        Ok(r) => r,
        Err(_) => {
            return Err((StatusCode::FORBIDDEN, "failed to load roles").into_response());
        }
    };

    let allowed = roles.iter().any(|r| {
        matches!(
            r.as_str(),
            "Administrator" | "System Manager" | "Sebrus Admin" | "Sebrus Release Manager"
        )
    });
    if !allowed {
        return Err((
            StatusCode::FORBIDDEN,
            "Thruster upload requires Sebrus Admin / Release Manager / System Manager",
        )
            .into_response());
    }

    Ok(user)
}

pub async fn serve_binary(Path((platform, filename)): Path<(String, String)>) -> Response {
    if !is_valid_platform(&platform) {
        return (StatusCode::NOT_FOUND, "unknown platform").into_response();
    }
    if filename.is_empty()
        || filename.starts_with('/')
        || filename.contains("..")
        || filename.contains('\\')
        || filename.contains('\0')
    {
        return (StatusCode::NOT_FOUND, "invalid filename").into_response();
    }

    let path = thruster_dir().join(&platform).join(&filename);
    if !path.exists() {
        return (StatusCode::NOT_FOUND, "binary not found").into_response();
    }

    match tokio::fs::read(&path).await {
        Ok(bytes) => {
            let mime = mime_guess::from_path(&path).first_or_octet_stream();
            (
                StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, mime.as_ref())],
                bytes,
            )
                .into_response()
        }
        Err(e) => {
            warn!("failed to read {}: {e}", path.display());
            (StatusCode::INTERNAL_SERVER_ERROR, "read error").into_response()
        }
    }
}

pub async fn upload_binary(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Response {
    if let Err(resp) = require_thruster_uploader(&state, &headers).await {
        return resp;
    }

    let mut platform: Option<String> = None;
    let mut saved_file: Option<String> = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "platform" => {
                if let Ok(value) = field.text().await {
                    if !is_valid_platform(&value) {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(json!({ "error": format!("invalid platform: {value}") })),
                        )
                            .into_response();
                    }
                    platform = Some(value);
                }
            }
            "file" => {
                let filename = field.file_name().unwrap_or("binary").to_string();
                if filename.is_empty() || is_hidden_or_sidecar(&filename) {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": "invalid file name" })),
                    )
                        .into_response();
                }

                let data = match field.bytes().await {
                    Ok(b) => b,
                    Err(e) => {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(json!({ "error": format!("failed to read upload: {e}") })),
                        )
                            .into_response();
                    }
                };

                let p = match platform.as_deref() {
                    Some(p) => p,
                    None => {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(json!({ "error": "platform must be sent before file" })),
                        )
                            .into_response();
                    }
                };

                let dir = thruster_dir().join(p);
                if let Err(e) = tokio::fs::create_dir_all(&dir).await {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({ "error": format!("failed to create directory: {e}") })),
                    )
                        .into_response();
                }

                let path = dir.join(&filename);
                if let Err(e) = write_file_atomic(&path, &data).await {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({ "error": format!("failed to write file: {e}") })),
                    )
                        .into_response();
                }
                saved_file = Some(filename);
            }
            _ => {}
        }
    }

    match saved_file {
        Some(file) => (
            StatusCode::OK,
            Json(json!({
                "message": "uploaded",
                "platform": platform,
                "file": file,
            })),
        )
            .into_response(),
        None => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "no file received" })),
        )
            .into_response(),
    }
}

async fn write_file_atomic(path: &PathBuf, data: &Bytes) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    tokio::fs::write(&tmp, data).await?;
    tokio::fs::rename(&tmp, path).await
}
