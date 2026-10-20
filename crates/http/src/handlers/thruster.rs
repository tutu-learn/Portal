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
use std::time::{Duration, SystemTime};
use tracing::warn;

const THRUSTER_DIR: &str = "thruster";
const PLATFORMS: &[&str] = &["windows", "macos", "linux"];
/// Maximum size accepted by the non-chunked upload endpoint. Files larger than
/// this must use `thruster.upload_chunk` so a reverse proxy such as Cloudflare
/// (100 MB request limit on free/pro) never sees a huge body.
const MAX_SINGLE_UPLOAD_BYTES: usize = 50 * 1024 * 1024;

fn thruster_dir() -> PathBuf {
    StdPath::new(THRUSTER_DIR).to_path_buf()
}

fn is_valid_platform(name: &str) -> bool {
    PLATFORMS.contains(&name)
}

fn is_valid_filename(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains('\0')
        && !name.contains("..")
        && !name.ends_with(".sha256")
}

fn bad_request_json(msg: impl std::fmt::Display) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": msg.to_string() })),
    )
        .into_response()
}

fn internal_error_json(msg: impl std::fmt::Display) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": msg.to_string() })),
    )
        .into_response()
}

async fn require_thruster_uploader(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<String, Response> {
    let user = authenticate_request(state, headers)
        .await
        .map(|u| u.user)
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, "authentication required").into_response())?;

    let Some(pool) = state
        .pools
        .iter()
        .next()
        .map(|e| e.value().core().clone())
    else {
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
                        return bad_request_json(format!("invalid platform: {value}"));
                    }
                    platform = Some(value);
                }
            }
            "file" => {
                let filename = field.file_name().unwrap_or("binary").to_string();
                if !is_valid_filename(&filename) {
                    return bad_request_json("invalid file name");
                }

                let data = match field.bytes().await {
                    Ok(b) => b,
                    Err(e) => return bad_request_json(format!("failed to read upload: {e}")),
                };

                if data.len() > MAX_SINGLE_UPLOAD_BYTES {
                    return bad_request_json(format!(
                        "file is {} bytes; use /api/method/thruster.upload_chunk for files larger than {} bytes",
                        data.len(),
                        MAX_SINGLE_UPLOAD_BYTES
                    ));
                }

                let p = match platform.as_deref() {
                    Some(p) => p,
                    None => {
                        return bad_request_json("platform must be sent before file");
                    }
                };

                let dir = thruster_dir().join(p);
                if let Err(e) = tokio::fs::create_dir_all(&dir).await {
                    return internal_error_json(format!("failed to create directory: {e}"));
                }

                let path = dir.join(&filename);
                if let Err(e) = write_file_atomic(&path, &data).await {
                    return internal_error_json(format!("failed to write file: {e}"));
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
        None => bad_request_json("no file received"),
    }
}

/// Chunked upload for large Thruster binaries.
///
/// Clients split a file into chunks and POST each chunk here with:
///   platform, filename, upload_id, chunk_index, total_chunks, file
/// When all chunks for an upload_id are present they are concatenated
/// and atomically moved to `thruster/<platform>/<filename>`.
pub async fn upload_binary_chunk(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Response {
    if let Err(resp) = require_thruster_uploader(&state, &headers).await {
        return resp;
    }

    let mut platform: Option<String> = None;
    let mut filename: Option<String> = None;
    let mut upload_id: Option<String> = None;
    let mut chunk_index: Option<usize> = None;
    let mut total_chunks: Option<usize> = None;
    let mut chunk_data: Option<Bytes> = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "platform" => {
                if let Ok(value) = field.text().await {
                    if !is_valid_platform(&value) {
                        return bad_request_json(format!("invalid platform: {value}"));
                    }
                    platform = Some(value);
                }
            }
            "filename" => {
                if let Ok(value) = field.text().await {
                    if !is_valid_filename(&value) {
                        return bad_request_json("invalid filename");
                    }
                    filename = Some(value);
                }
            }
            "upload_id" => {
                if let Ok(value) = field.text().await {
                    if value.is_empty()
                        || value.contains('/')
                        || value.contains('\\')
                        || value.contains('\0')
                    {
                        return bad_request_json("invalid upload_id");
                    }
                    upload_id = Some(value);
                }
            }
            "chunk_index" => {
                if let Ok(value) = field.text().await {
                    match value.parse::<usize>() {
                        Ok(n) => chunk_index = Some(n),
                        Err(_) => return bad_request_json("chunk_index must be an integer"),
                    }
                }
            }
            "total_chunks" => {
                if let Ok(value) = field.text().await {
                    match value.parse::<usize>() {
                        Ok(n) if n > 0 => total_chunks = Some(n),
                        _ => return bad_request_json("total_chunks must be a positive integer"),
                    }
                }
            }
            "file" => {
                match field.bytes().await {
                    Ok(b) => chunk_data = Some(b),
                    Err(e) => return bad_request_json(format!("failed to read chunk: {e}")),
                };
            }
            _ => {}
        }
    }

    let platform = match platform {
        Some(v) => v,
        None => return bad_request_json("platform is required"),
    };
    let filename = match filename {
        Some(v) => v,
        None => return bad_request_json("filename is required"),
    };
    let upload_id = match upload_id {
        Some(v) => v,
        None => return bad_request_json("upload_id is required"),
    };
    let chunk_index = match chunk_index {
        Some(v) => v,
        None => return bad_request_json("chunk_index is required"),
    };
    let total_chunks = match total_chunks {
        Some(v) => v,
        None => return bad_request_json("total_chunks is required"),
    };
    let chunk_data = match chunk_data {
        Some(v) => v,
        None => return bad_request_json("no chunk data received"),
    };

    if chunk_index >= total_chunks {
        return bad_request_json("chunk_index out of range");
    }

    // Clean up uploads abandoned more than a day ago before writing new chunks.
    cleanup_old_tmp_dirs(Duration::from_secs(24 * 60 * 60)).await;

    let tmp_dir = thruster_dir().join(".tmp").join(&upload_id);
    if let Err(e) = tokio::fs::create_dir_all(&tmp_dir).await {
        return internal_error_json(format!("failed to create temp directory: {e}"));
    }

    let chunk_path = tmp_dir.join(chunk_index.to_string());
    if let Err(e) = tokio::fs::write(&chunk_path, &chunk_data).await {
        return internal_error_json(format!("failed to write chunk: {e}"));
    }

    // Treat the upload as complete when every expected chunk exists.
    let all_present = (0..total_chunks).all(|i| tmp_dir.join(i.to_string()).exists());

    if !all_present {
        return (
            StatusCode::OK,
            Json(json!({
                "message": "chunk received",
                "completed": false,
                "chunk_index": chunk_index,
                "total_chunks": total_chunks,
            })),
        )
            .into_response();
    }

    let platform_dir = thruster_dir().join(&platform);
    if let Err(e) = tokio::fs::create_dir_all(&platform_dir).await {
        return internal_error_json(format!("failed to create directory: {e}"));
    }

    let final_path = platform_dir.join(&filename);

    // Another request may have already finalized this upload.
    if final_path.exists() {
        let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
        return (
            StatusCode::OK,
            Json(json!({
                "message": "uploaded",
                "completed": true,
                "platform": platform,
                "file": filename,
            })),
        )
            .into_response();
    }

    let part_path = platform_dir.join(format!("{filename}.part"));
    let mut part_file = match tokio::fs::File::create(&part_path).await {
        Ok(f) => f,
        Err(e) => return internal_error_json(format!("failed to create part file: {e}")),
    };

    for i in 0..total_chunks {
        let chunk_path = tmp_dir.join(i.to_string());
        let mut chunk_file = match tokio::fs::File::open(&chunk_path).await {
            Ok(f) => f,
            Err(e) => {
                let _ = tokio::fs::remove_file(&part_path).await;
                return internal_error_json(format!("failed to open chunk {i}: {e}"));
            }
        };
        if let Err(e) = tokio::io::copy(&mut chunk_file, &mut part_file).await {
            let _ = tokio::fs::remove_file(&part_path).await;
            return internal_error_json(format!("failed to assemble chunk {i}: {e}"));
        }
    }

    drop(part_file);

    if let Err(e) = tokio::fs::rename(&part_path, &final_path).await {
        if final_path.exists() {
            let _ = tokio::fs::remove_file(&part_path).await;
        } else {
            let _ = tokio::fs::remove_file(&part_path).await;
            return internal_error_json(format!("failed to finalize file: {e}"));
        }
    }

    let _ = tokio::fs::remove_dir_all(&tmp_dir).await;

    (
        StatusCode::OK,
        Json(json!({
            "message": "uploaded",
            "completed": true,
            "platform": platform,
            "file": filename,
        })),
    )
        .into_response()
}

/// Remove incomplete upload temp directories older than `max_age`. Runs best-
/// effort: failures are logged but not returned so they cannot block uploads.
async fn cleanup_old_tmp_dirs(max_age: Duration) {
    let tmp_root = thruster_dir().join(".tmp");
    let Ok(mut entries) = tokio::fs::read_dir(&tmp_root).await else {
        return;
    };
    let now = SystemTime::now();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let remove = match entry.metadata().await {
            Ok(meta) => match meta.modified() {
                Ok(mtime) => now.duration_since(mtime).unwrap_or_default() > max_age,
                Err(_) => false,
            },
            Err(_) => false,
        };
        if remove {
            if let Err(e) = tokio::fs::remove_dir_all(&path).await {
                warn!("failed to remove stale tmp dir {}: {e}", path.display());
            }
        }
    }
}

async fn write_file_atomic(path: &PathBuf, data: &Bytes) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    tokio::fs::write(&tmp, data).await?;
    tokio::fs::rename(&tmp, path).await
}
