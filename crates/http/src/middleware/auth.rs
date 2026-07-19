use crate::site::resolve_site_pool;
use crate::AppState;
use axum::http::HeaderMap;
use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};

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

fn extract_client_ip(headers: &HeaderMap) -> Option<String> {
    if let Some(forwarded) = headers.get("x-forwarded-for").and_then(|h| h.to_str().ok()) {
        return forwarded.split(',').next().map(|s| s.trim().to_string());
    }
    None
}

fn extract_user_agent(headers: &HeaderMap) -> Option<String> {
    headers
        .get("user-agent")
        .and_then(|h| h.to_str().ok())
        .map(String::from)
}

/// Authenticate a request from its headers.
///
/// Returns a [`SessionUser`] when either a valid `sid` cookie or a valid
/// `Authorization: Bearer <kiff-logger-token>` header is present.  Credentials
/// are invalid or missing, returns `None` (Guest).
pub async fn authenticate_request(state: &AppState, headers: &HeaderMap) -> Option<SessionUser> {
    // 1. Cookie session.
    if let Some(cookie_header) = headers.get("cookie").and_then(|h| h.to_str().ok()) {
        if let Some(sid) = extract_cookie_value(cookie_header, "sid") {
            let pool = resolve_site_pool(state, headers).map(|(_, p)| p);
            if let Some(pool) = pool {
                let store = session::SessionStore::new();
                match store.get(&pool, &sid).await {
                    Ok(Some(session)) if !session.is_expired() => {
                        // Refresh session metadata on every authenticated
                        // request so `last_updated` reflects real activity.
                        let metadata = session::SessionMetadata {
                            ip: extract_client_ip(headers).or_else(|| {
                                session
                                    .data
                                    .get("session_ip")
                                    .and_then(|v| v.as_str().map(String::from))
                            }),
                            user_agent: extract_user_agent(headers).or_else(|| {
                                session
                                    .data
                                    .get("user_agent")
                                    .and_then(|v| v.as_str().map(String::from))
                            }),
                        };
                        let _ = store.refresh_metadata(&pool, &sid, metadata).await;
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
            let pool = resolve_site_pool(state, headers).map(|(_, p)| p);
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
