use crate::handlers::desk::custom_login_target;
use crate::site::resolve_site_pool;
use crate::AppState;
use axum::{
    extract::{ConnectInfo, State},
    http::{header::SET_COOKIE, HeaderMap, StatusCode},
    response::{IntoResponse, Redirect},
    Json,
};
use std::net::SocketAddr;

fn extract_client_ip(headers: &HeaderMap, addr: Option<SocketAddr>) -> Option<String> {
    if let Some(forwarded) = headers.get("x-forwarded-for").and_then(|h| h.to_str().ok()) {
        return forwarded.split(',').next().map(|s| s.trim().to_string());
    }
    if let Some(addr) = addr {
        return Some(addr.ip().to_string());
    }
    None
}

fn extract_user_agent(headers: &HeaderMap) -> Option<String> {
    headers
        .get("user-agent")
        .and_then(|h| h.to_str().ok())
        .map(String::from)
}

pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    addr: Option<ConnectInfo<SocketAddr>>,
    crate::extract::AnyBody(body): crate::extract::AnyBody,
) -> impl IntoResponse {
    let usr = body
        .get("usr")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let pwd = body
        .get("pwd")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let pool = resolve_site_pool(&state, &headers).map(|(_, p)| p);
    match pool {
        Some(pool) => {
            // Throttle online brute force: the peer address (not the spoofable
            // X-Forwarded-For) keys the counter, combined with the account.
            let peer = addr
                .as_ref()
                .map(|a| a.0.ip().to_string())
                .or_else(|| extract_client_ip(&headers, None))
                .unwrap_or_else(|| "unknown".into());
            let ip = extract_client_ip(&headers, addr.map(|a| a.0));
            if let Some(retry_after) = state.login_limiter.retry_after(&usr, &peer) {
                let mut res = Json(serde_json::json!({
                    "error": "too many failed attempts — try again later"
                }))
                .into_response();
                *res.status_mut() = StatusCode::TOO_MANY_REQUESTS;
                res.headers_mut().insert(
                    axum::http::header::RETRY_AFTER,
                    retry_after.to_string().parse().unwrap(),
                );
                return res;
            }
            let user_agent = extract_user_agent(&headers);
            let metadata = session::SessionMetadata {
                ip: ip.clone(),
                user_agent: user_agent.clone(),
            };
            let auth = session::AuthService::new(session::SessionStore::new());
            match auth
                .login_with_metadata(&pool, &usr, &pwd, "localhost", metadata)
                .await
            {
                Ok(session) => {
                    state.login_limiter.clear(&usr, &peer);
                    kiff_logger::record_authentication_event(
                        &pool,
                        &usr,
                        kiff_logger::AuthenticationEvent::LoginSuccess,
                        ip.as_deref(),
                        user_agent.as_deref(),
                    )
                    .await;
                    let home_page = crate::user_home::get_effective_home_page(
                        &pool,
                        &usr,
                        state.config.auth.custom_home_path.as_deref(),
                    )
                    .await
                    .unwrap_or_else(|| "/desk".to_string());
                    // `Secure` follows secure_cookie_flag(): forced via
                    // ANRI_COOKIE_SECURE, otherwise auto (on for any
                    // non-local Host so the cookie never rides cleartext in
                    // production, off for localhost dev over plain HTTP).
                    let cookie = crate::middleware::security_headers::session_cookie(
                        &session.id,
                        86400,
                        &headers,
                    );
                    let mut res = Json(serde_json::json!({
                        "message": "Logged In",
                        "home_page": home_page,
                        "full_name": usr,
                    }))
                    .into_response();
                    res.headers_mut()
                        .insert(SET_COOKIE, cookie.parse().unwrap());
                    res
                }
                Err(e) => {
                    state.login_limiter.record_failure(&usr, &peer);
                    kiff_logger::record_authentication_event(
                        &pool,
                        &usr,
                        kiff_logger::AuthenticationEvent::LoginFailed,
                        ip.as_deref(),
                        user_agent.as_deref(),
                    )
                    .await;
                    let mut res =
                        Json(serde_json::json!({ "error": format!("{}", e) })).into_response();
                    *res.status_mut() = StatusCode::UNAUTHORIZED;
                    res
                }
            }
        }
        None => {
            let mut res = Json(serde_json::json!({ "error": "no database pool" })).into_response();
            *res.status_mut() = StatusCode::SERVICE_UNAVAILABLE;
            res
        }
    }
}

pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let pool = resolve_site_pool(&state, &headers).map(|(_, p)| p);
    match pool {
        Some(pool) => {
            // Extract the session id from the cookie and delete it server-side.
            if let Some(cookie_header) = headers.get("cookie").and_then(|h| h.to_str().ok()) {
                if let Some(sid) = extract_cookie_value(cookie_header, "sid") {
                    let store = session::SessionStore::new();
                    let user = session_user(&pool, &store, &sid).await;
                    let auth = session::AuthService::new(store);
                    let _ = auth.logout(&pool, &sid).await;
                    record_logout(&pool, &user, &headers).await;
                }
            }
            let cookie =
                crate::middleware::security_headers::session_cookie("", 0, &headers);
            let mut res = Json(serde_json::json!({ "message": "Logged Out" })).into_response();
            res.headers_mut()
                .insert(SET_COOKIE, cookie.parse().unwrap());
            res
        }
        None => Json(serde_json::json!({ "message": "Logged Out" })).into_response(),
    }
}

/// GET /logout — clear the session cookie and redirect to the login page.
pub async fn logout_redirect(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let pool = resolve_site_pool(&state, &headers).map(|(_, p)| p);
    if let Some(pool) = pool {
        if let Some(cookie_header) = headers.get("cookie").and_then(|h| h.to_str().ok()) {
            if let Some(sid) = extract_cookie_value(cookie_header, "sid") {
                let store = session::SessionStore::new();
                let user = session_user(&pool, &store, &sid).await;
                let auth = session::AuthService::new(store);
                let _ = auth.logout(&pool, &sid).await;
                record_logout(&pool, &user, &headers).await;
            }
        }
    }
    let cookie = crate::middleware::security_headers::session_cookie("", 0, &headers);
    let target = custom_login_target(&state.config, None).unwrap_or_else(|| "/login".into());
    let mut res = Redirect::temporary(&target).into_response();
    res.headers_mut()
        .insert(SET_COOKIE, cookie.parse().unwrap());
    res
}

/// Resolve the username behind a session id before the session is deleted.
/// Falls back to "Unknown" when the session is missing or expired.
async fn session_user(
    pool: &orm::DatabasePool,
    store: &session::SessionStore,
    sid: &str,
) -> String {
    store
        .get(pool, sid)
        .await
        .ok()
        .flatten()
        .map(|s| s.user)
        .unwrap_or_else(|| "Unknown".to_string())
}

async fn record_logout(pool: &orm::DatabasePool, user: &str, headers: &HeaderMap) {
    let ip = extract_client_ip(headers, None);
    let user_agent = extract_user_agent(headers);
    kiff_logger::record_authentication_event(
        pool,
        user,
        kiff_logger::AuthenticationEvent::Logout,
        ip.as_deref(),
        user_agent.as_deref(),
    )
    .await;
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
