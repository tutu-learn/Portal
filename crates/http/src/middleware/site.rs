use axum::{
    extract::Request,
    middleware::Next,
    response::Response,
};

pub async fn site_resolution_middleware(
    mut request: Request,
    next: Next,
) -> Response {
    if let Some(host) = request.headers().get("host").and_then(|h| h.to_str().ok()) {
        let site = host.split(':').next().unwrap_or(host).to_string();
        request.extensions_mut().insert(site);
    }
    next.run(request).await
}
