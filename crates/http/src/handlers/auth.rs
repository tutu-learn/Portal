use crate::AppState;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode, header::SET_COOKIE},
    response::IntoResponse,
    Json,
};

pub async fn login(
    State(state): State<AppState>,
    crate::extract::AnyBody(body): crate::extract::AnyBody,
) -> impl IntoResponse {
    let usr = body.get("usr").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let pwd = body.get("pwd").and_then(|v| v.as_str()).unwrap_or("").to_string();

    let pool = state.pools.iter().next().map(|e| e.value().clone());
    match pool {
        Some(pool) => {
            let auth = session::AuthService::new(session::SessionStore::new());
            match auth.login(&pool, &usr, &pwd, "localhost").await {
                Ok(session) => {
                    let cookie = format!(
                        "sid={}; Path=/; HttpOnly; SameSite=Lax; Max-Age=86400",
                        session.id
                    );
                    let mut res = Json(serde_json::json!({
                        "message": "Logged In",
                        "home_page": "/desk",
                        "full_name": usr,
                    })).into_response();
                    res.headers_mut().insert(SET_COOKIE, cookie.parse().unwrap());
                    res
                }
                Err(e) => {
                    let mut res = Json(serde_json::json!({ "error": format!("{}", e) })).into_response();
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

pub async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let pool = state.pools.iter().next().map(|e| e.value().clone());
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
            res.headers_mut().insert(SET_COOKIE, cookie.parse().unwrap());
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
