//! Baseline security response headers for every HTTP response.
//!
//! Applied globally in [`crate::router::create_router`]. The header set is
//! deliberately conservative: the bundled Desk and portal pages use inline
//! scripts and styles, so the CSP keeps `'unsafe-inline'` for both while still
//! pinning every other fetch directive to `'self'`.
//!
//! Escape hatches for deployments that manage headers at the proxy layer:
//! - `ANRI_SECURITY_HEADERS=off` disables this middleware entirely.
//! - `ANRI_HSTS=1` adds `Strict-Transport-Security` (only meaningful when the
//!   app itself serves HTTPS; proxies usually set this instead).

use axum::{extract::Request, http::HeaderMap, middleware::Next, response::Response};

/// Whether the `sid` cookie should carry the `Secure` attribute.
///
/// - `ANRI_COOKIE_SECURE=1/true` forces it on; `=0/false` forces it off.
/// - Otherwise auto: requests whose `Host` is anything other than
///   localhost / 127.0.0.1 / ::1 get `Secure` (production deployments behind
///   a name), so the cookie never travels over cleartext HTTP by accident;
///   local dev over plain HTTP keeps working.
pub fn secure_cookie_flag(headers: &HeaderMap) -> bool {
    match std::env::var("ANRI_COOKIE_SECURE") {
        Ok(v) if v == "1" || v.eq_ignore_ascii_case("true") => true,
        Ok(v) if v == "0" || v.eq_ignore_ascii_case("false") => false,
        _ => {
            let host = headers
                .get(axum::http::header::HOST)
                .and_then(|h| h.to_str().ok())
                .unwrap_or("");
            let host = host
                .rsplit('@')
                .next()
                .unwrap_or("")
                .split(':')
                .next()
                .unwrap_or("");
            !(host.eq_ignore_ascii_case("localhost")
                || host == "127.0.0.1"
                || host == "::1"
                || host == "[::1]")
        }
    }
}

/// Build the full `Set-Cookie` value for the session cookie, applying the
/// [`secure_cookie_flag`] decision.
pub fn session_cookie(session_id: &str, max_age_secs: u64, headers: &HeaderMap) -> String {
    let secure = if secure_cookie_flag(headers) { "; Secure" } else { "" };
    format!(
        "sid={session_id}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age_secs}{secure}"
    )
}

/// CORS layer: same-origin by default.
///
/// No `Access-Control-Allow-Origin` is emitted at all unless the deployment
/// opts in with `ANRI_CORS_ORIGINS` (comma-separated origin list, e.g.
/// `https://app.example.com`). Cross-origin credentials are never allowed:
/// the session cookie is `SameSite=Lax` and no portal API is designed for
/// cross-site embedding, so a permissive wildcard is pure downside.
pub fn cors_layer() -> tower_http::cors::CorsLayer {
    let mut layer = tower_http::cors::CorsLayer::new();
    if let Ok(raw) = std::env::var("ANRI_CORS_ORIGINS") {
        let origins: Vec<axum::http::HeaderValue> = raw
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse().ok())
            .collect();
        if !origins.is_empty() {
            layer = layer.allow_origin(origins);
        }
    }
    layer
}

pub async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        axum::http::header::X_CONTENT_TYPE_OPTIONS,
        "nosniff".parse().unwrap(),
    );
    headers.insert(
        axum::http::header::X_FRAME_OPTIONS,
        "SAMEORIGIN".parse().unwrap(),
    );
    headers.insert(
        axum::http::header::REFERRER_POLICY,
        "strict-origin-when-cross-origin".parse().unwrap(),
    );
    headers.insert(
        axum::http::header::HeaderName::from_static("content-security-policy"),
        "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; \
         img-src 'self' data: blob:; font-src 'self' data:; connect-src 'self'; \
         frame-ancestors 'self'; base-uri 'self'"
            .parse()
            .unwrap(),
    );
    headers.insert(
        axum::http::header::HeaderName::from_static("permissions-policy"),
        "camera=(), microphone=(), geolocation=()".parse().unwrap(),
    );
    if std::env::var("ANRI_HSTS")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        headers.insert(
            axum::http::header::STRICT_TRANSPORT_SECURITY,
            "max-age=31536000; includeSubDomains".parse().unwrap(),
        );
    }
    response
}

pub fn enabled() -> bool {
    !matches!(std::env::var("ANRI_SECURITY_HEADERS"), Ok(v) if v == "off")
}
