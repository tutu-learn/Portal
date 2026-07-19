use crate::site::resolve_site_pool;
use crate::AppState;
use axum::{
    extract::{ConnectInfo, State},
    http::{header::SET_COOKIE, HeaderMap, StatusCode},
    response::IntoResponse,
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
            let metadata = session::SessionMetadata {
                ip: extract_client_ip(&headers, addr.map(|a| a.0)),
                user_agent: extract_user_agent(&headers),
            };
            let auth = session::AuthService::new(session::SessionStore::new());
            match auth
                .login_with_metadata(&pool, &usr, &pwd, "localhost", metadata)
                .await
            {
                Ok(session) => {
                    let cookie = format!(
                        "sid={}; Path=/; HttpOnly; SameSite=Lax; Max-Age=86400",
                        session.id
                    );
                    let mut res = Json(serde_json::json!({
                        "message": "Logged In",
                        "home_page": "/desk",
                        "full_name": usr,
                    }))
                    .into_response();
                    res.headers_mut()
                        .insert(SET_COOKIE, cookie.parse().unwrap());
                    res
                }
                Err(e) => {
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
                    let auth = session::AuthService::new(session::SessionStore::new());
                    let _ = auth.logout(&pool, &sid).await;
                }
            }
            let cookie = "sid=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0";
            let mut res = Json(serde_json::json!({ "message": "Logged Out" })).into_response();
            res.headers_mut()
                .insert(SET_COOKIE, cookie.parse().unwrap());
            res
        }
        None => Json(serde_json::json!({ "message": "Logged Out" })).into_response(),
    }
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
