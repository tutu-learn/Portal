use axum::{
    extract::Request,
    middleware::Next,
    response::Response,
};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct AuthContext {
    pub user: String,
    pub site: String,
    pub session_id: Option<String>,
}

pub async fn auth_middleware(
    _request: Request,
    next: Next,
) -> Response {
    // TODO: extract session cookie/header, validate, attach AuthContext to extensions
    next.run(_request).await
}
