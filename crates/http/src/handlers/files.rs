use crate::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use std::path::PathBuf;

pub async fn serve_public(
    State(state): State<AppState>,
    Path(filename): Path<String>,
) -> impl IntoResponse {
    // Use first site for now
    let site = state.site_manager.sites().values().next().cloned();
    match site {
        Some(site) => {
            let path = site.public.join("files").join(&filename);
            serve_file(&path).await
        }
        None => (StatusCode::NOT_FOUND, "site not found").into_response(),
    }
}

pub async fn serve_private(
    State(state): State<AppState>,
    Path(filename): Path<String>,
) -> impl IntoResponse {
    // TODO: check auth
    let site = state.site_manager.sites().values().next().cloned();
    match site {
        Some(site) => {
            let path = site.private.join("files").join(&filename);
            serve_file(&path).await
        }
        None => (StatusCode::NOT_FOUND, "site not found").into_response(),
    }
}

async fn serve_file(path: &PathBuf) -> Response {
    if !path.exists() {
        return (StatusCode::NOT_FOUND, "file not found").into_response();
    }
    match tokio::fs::read(path).await {
        Ok(bytes) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            (
                StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, mime.as_ref())],
                bytes,
            )
                .into_response()
        }
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "read error").into_response(),
    }
}

pub async fn upload_file(State(_state): State<AppState>) -> impl IntoResponse {
    // TODO: handle multipart upload
    (
        StatusCode::OK,
        Json(serde_json::json!({ "message": "uploaded" })),
    )
}
