use crate::AppState;
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

pub async fn session_validation_middleware(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let user = if let Some(cookie_header) = request
        .headers()
        .get("cookie")
        .and_then(|h| h.to_str().ok())
    {
        if let Some(sid) = extract_cookie_value(cookie_header, "sid") {
            let pool = state.pools.iter().next().map(|e| e.value().clone());
            if let Some(pool) = pool {
                let store = session::SessionStore::new();
                match store.get(&pool, &sid).await {
                    Ok(Some(session)) if !session.is_expired() => {
                        request.extensions_mut().insert(SessionUser {
                            user: session.user.clone(),
                            sid: sid.clone(),
                        });
                        Some(session.user)
                    }
                    _ => None,
                }
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    // Always continue — Frappe itself checks allow_guest on each method.
    // If no valid session, the user remains None (Guest).
    let _ = user;
    next.run(request).await
}
