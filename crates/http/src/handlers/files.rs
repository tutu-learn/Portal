use crate::middleware::auth::authenticate_request;
use crate::site::resolve_site_name;
use crate::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use std::path::{Path as StdPath, PathBuf};

pub async fn serve_public(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(filename): Path<String>,
) -> impl IntoResponse {
    let site = resolve_site_name(&state, &headers)
        .and_then(|name| state.site_manager.sites().get(&name).cloned());
    match site {
        Some(site) => {
            let base = site.public.join("files");
            match resolve_under_base(&base, &filename).await {
                Some(path) => serve_file(&path).await,
                None => (StatusCode::NOT_FOUND, "file not found").into_response(),
            }
        }
        None => (StatusCode::NOT_FOUND, "site not found").into_response(),
    }
}

pub async fn serve_private(
    State(state): State<AppState>,
    Path(filename): Path<String>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    if authenticate_request(&state, &headers).await.is_none() {
        return (StatusCode::UNAUTHORIZED, "authentication required").into_response();
    }
    let site = resolve_site_name(&state, &headers)
        .and_then(|name| state.site_manager.sites().get(&name).cloned());
    match site {
        Some(site) => {
            let base = site.private.join("files");
            match resolve_under_base(&base, &filename).await {
                Some(path) => serve_file(&path).await,
                None => (StatusCode::NOT_FOUND, "file not found").into_response(),
            }
        }
        None => (StatusCode::NOT_FOUND, "site not found").into_response(),
    }
}

/// Resolve `filename` inside `base`, refusing anything that would escape the
/// base directory. Returns `None` for traversal attempts or unresolvable
/// paths; the caller maps that to a 404 so no existence information leaks.
async fn resolve_under_base(base: &StdPath, filename: &str) -> Option<PathBuf> {
    // Cheap lexical rejects first: parent traversal, Windows separators,
    // NUL bytes, and absolute paths (which `PathBuf::join` would let
    // replace the base entirely).
    if filename.is_empty()
        || filename.starts_with('/')
        || filename.contains("..")
        || filename.contains('\\')
        || filename.contains('\0')
    {
        return None;
    }

    let canonical_base = tokio::fs::canonicalize(base).await.ok()?;
    let joined = canonical_base.join(filename);

    // The final component may not exist, so canonicalize the parent and
    // re-attach the file name, then verify the result stays under the base.
    let parent = joined.parent()?;
    let canonical_parent = tokio::fs::canonicalize(parent).await.ok()?;
    if !canonical_parent.starts_with(&canonical_base) {
        return None;
    }
    let file_name = joined.file_name()?;
    Some(canonical_parent.join(file_name))
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
