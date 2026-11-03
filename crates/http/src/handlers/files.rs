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
    axum::extract::RawQuery(raw_query): axum::extract::RawQuery,
) -> impl IntoResponse {
    let session = match authenticate_request(&state, &headers).await {
        Some(session) => session,
        None => return (StatusCode::UNAUTHORIZED, "authentication required").into_response(),
    };
    // Sidecar metadata (`<id>.key`, `<id>.owner`) must never be served: it
    // contains the access keys for the blob tree.
    if filename.ends_with(".key") || filename.ends_with(".owner") {
        return (StatusCode::NOT_FOUND, "file not found").into_response();
    }
    // Apps store blobs under the shared private tree but keep their own
    // permission models; the framework cannot enforce them itself. Without a
    // registered gate every authenticated user could read every app's private
    // files (proof-of-payment scans, signed agreements, personal vault
    // documents…) through this route, bypassing app-level gates entirely.
    // Refuse outright when no gate is registered, and let the gate decide
    // otherwise. 404 (not 403) so existence information does not leak.
    let gate = match state.private_file_gate.as_ref() {
        Some(gate) => gate,
        None => {
            tracing::warn!(
                "refusing private file request: no PrivateFileGate registered"
            );
            return (StatusCode::NOT_FOUND, "file not found").into_response();
        }
    };
    if !gate.can_read(&session.user, &filename).await {
        return (StatusCode::NOT_FOUND, "file not found").into_response();
    }
    // Framework-managed blobs (stored via rust_apps_core::blob) additionally
    // require the unguessable key issued at upload time, supplied as `?key=`.
    // Membership in an authorized document is not proof of possession —
    // without this check, anyone who can reference a blob ID can read it.
    if rust_apps_core::blob::blob_exists(&state, &filename) {
        let key = raw_query
            .as_deref()
            .unwrap_or("")
            .split('&')
            .find_map(|pair| pair.split_once('='))
            .filter(|(k, _)| *k == "key")
            .map(|(_, v)| v.to_string());
        let authorized = match key {
            Some(key) => rust_apps_core::blob::open_blob(&state, &filename, &key).is_ok(),
            None => false,
        };
        if !authorized {
            return (StatusCode::NOT_FOUND, "file not found").into_response();
        }
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

/// Gate implementation that preserves the pre-gate behavior for apps that
/// genuinely want any authenticated user to reach their private files.
/// Register explicitly; it is intentionally not the default.
pub struct AllowAllPrivateFiles;

#[async_trait::async_trait]
impl rust_apps_core::PrivateFileGate for AllowAllPrivateFiles {
    async fn can_read(&self, _user: &str, _path: &str) -> bool {
        true
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
