use crate::AppState;
use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use axum::http::HeaderMap;

#[derive(Debug, Clone)]
pub struct SessionUser {
    pub user: String,
    pub sid: String,
}

fn extract_cookie_value(header: &str, name: &str) -> Option<String> {
    for pair in header.split(';') {
        let pair = pair.trim();
        if let Some((key, value)) = pair.split_once('=') {
            if key.trim() == name {
                return Some(value.trim().to_string());
            }
        }
    }
    None
}

/// Authenticate a request from its headers.
///
/// Returns a [`SessionUser`] when either a valid `sid` cookie or a valid
/// `Authorization: Bearer <kiff-logger-token>` header is present.  Credentials
/// are invalid or missing, returns `None` (Guest).
pub async fn authenticate_request(
    state: &AppState,
    headers: &HeaderMap,
) -> Option<SessionUser> {
    // 1. Cookie session.
    if let Some(cookie_header) = headers.get("cookie").and_then(|h| h.to_str().ok()) {
        if let Some(sid) = extract_cookie_value(cookie_header, "sid") {
            let pool = state.pools.iter().next().map(|e| e.value().clone());
            if let Some(pool) = pool {
                let store = session::SessionStore::new();
                match store.get(&pool, &sid).await {
                    Ok(Some(session)) if !session.is_expired() => {
                        return Some(SessionUser {
                            user: session.user.clone(),
                            sid: sid.clone(),
                        });
                    }
                    _ => {}
                }
            }
        }
    }

    // 2. Bearer token.
    if let Some(auth_header) = headers.get("authorization").and_then(|h| h.to_str().ok()) {
        if let Some(token) = auth_header.strip_prefix("Bearer ").map(str::trim) {
            let pool = state.pools.iter().next().map(|e| e.value().clone());
            if let Some(pool) = pool {
                match kiff_logger::verify_bearer_token(&pool, token).await {
                    Ok(Some(verified)) => {
                        let _ = kiff_logger::touch_token(&pool, &verified.name).await;
                        return Some(SessionUser {
                            user: format!("token:{}", verified.token_name),
                            sid: String::new(),
                        });
                    }
                    _ => {}
                }
            }
        }
    }

    None
}

/// Global auth middleware.
///
/// Attaches a [`SessionUser`] extension when credentials are present. Requests
/// without credentials continue as Guest; individual handlers decide whether
/// authentication is required.
pub async fn token_auth_middleware(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    if let Some(user) = authenticate_request(&state, request.headers()).await {
        request.extensions_mut().insert(user);
    }
    next.run(request).await
}
