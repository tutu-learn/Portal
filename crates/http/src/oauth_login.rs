//! Native Rust implementation of the Microsoft/Office365 OAuth2 login
//! callback (`frappe.integrations.oauth2_logins.login_via_office365`).
//!
//! Previously this callback ran entirely in the embedded Python (state
//! validation, token exchange, JWT decode, user lookup, session creation),
//! which meant every mismatch between this project's Rust-authored Python
//! shims (e.g. `_MetaProxy`) and the real Frappe code paths those shims stand
//! in for could break login. This handler moves everything except the final
//! `User.save()` into Rust: state validation, the authorization-code token
//! exchange, and id_token decoding are done natively (`kiff_core::oauth`),
//! and Python is invoked only for `frappe.utils.oauth.update_oauth_user`,
//! which owns creating/updating the `User` document (permissions, hooks,
//! default role, welcome-mail suppression) exactly as it does today.

use crate::social_login::site_url_from_headers;
use crate::AppState;
use axum::{
    http::{header::SET_COOKIE, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::warn;

const PROVIDER: &str = "office_365";

struct ProviderConfig {
    client_id: String,
    client_secret: String,
    access_token_url: String,
    redirect_uri: String,
}

/// Handle a GET/POST to `/api/method/frappe.integrations.oauth2_logins.login_via_office365`.
/// `params` is the same flattened query/body map `method_response` already
/// builds for every `/api/method/*` call.
pub async fn handle_office365_callback(
    state: &AppState,
    headers: &HeaderMap,
    params: &HashMap<String, Value>,
) -> Response {
    let code = match params.get("code").and_then(|v| v.as_str()) {
        Some(c) if !c.is_empty() => c.to_string(),
        _ => return error_page(StatusCode::BAD_REQUEST, "Invalid Request", "Missing 'code' parameter"),
    };
    let state_param = match params.get("state").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return error_page(StatusCode::BAD_REQUEST, "Invalid Request", "Missing 'state' parameter"),
    };

    let Some(oauth_state) = decode_state(&state_param) else {
        return error_page(
            StatusCode::EXPECTATION_FAILED,
            "Invalid Request",
            "Your login attempt is invalid or has expired",
        );
    };

    let (site, pool) = match crate::site::resolve_site_pool(state, headers) {
        Some(sp) => sp,
        None => {
            return error_page(
                StatusCode::SERVICE_UNAVAILABLE,
                "Service Unavailable",
                "No database pool for this site",
            )
        }
    };

    let provider = match load_provider_config(&pool, &site_url_from_headers(headers), &site.config.encryption_key).await {
        Ok(Some(p)) => p,
        Ok(None) => {
            return error_page(
                StatusCode::NOT_FOUND,
                "Not Found",
                "Office 365 login is not configured for this site",
            )
        }
        Err(e) => {
            warn!(error = %e, "failed to load Office 365 Social Login Key config");
            return error_page(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Server Error",
                "Failed to load Office 365 login configuration",
            );
        }
    };

    let token_response = match kiff_core::oauth::token_exchange(
        &provider.access_token_url,
        &code,
        &provider.redirect_uri,
        &provider.client_id,
        &provider.client_secret,
        None,
    )
    .await
    {
        Ok(v) => v,
        Err(e) => {
            warn!(error = %e, "Office 365 token exchange failed");
            return error_page(StatusCode::BAD_GATEWAY, "Login Failed", &e);
        }
    };

    let id_token = match token_response.get("id_token").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => {
            return error_page(
                StatusCode::BAD_GATEWAY,
                "Login Failed",
                "Office 365 did not return an id_token",
            )
        }
    };

    let claims = match kiff_core::oauth::decode_jwt_payload(id_token) {
        Ok(c) => c,
        Err(e) => {
            warn!(error = %e, "failed to decode Office 365 id_token");
            return error_page(StatusCode::BAD_GATEWAY, "Login Failed", &e);
        }
    };

    let email = claims
        .get("email")
        .and_then(|v| v.as_str())
        .or_else(|| claims.get("upn").and_then(|v| v.as_str()))
        .or_else(|| claims.get("unique_name").and_then(|v| v.as_str()))
        .map(|s| s.to_lowercase())
        .filter(|s| !s.is_empty());

    let Some(email) = email else {
        return error_page(
            StatusCode::BAD_REQUEST,
            "Invalid Request",
            "Please ensure that your profile has an email address",
        );
    };

    // Hand off to Python only for the User document lookup/create/save. This
    // is the one step that needs Frappe's real DocType controller: hooks,
    // permission-ignore flags, default role assignment, and welcome-mail
    // suppression on first sign-up. `update_oauth_user` is not
    // `@frappe.whitelist()`-decorated (it's an internal helper), so it goes
    // through `call_trusted_method`, which — unlike the HTTP method
    // dispatcher — is only ever invoked here with a fixed, Rust-authored
    // method path, never a caller-supplied string.
    let update_args = json!({
        "user": email,
        "data": claims,
        "provider": PROVIDER,
    });
    let update_result =
        kiff_core::call_trusted_method("frappe.utils.oauth.update_oauth_user", &update_args, Some(&email));

    if let Err(e) = pool.commit().await {
        warn!(error = %e, "failed to commit transaction after Office 365 login");
    }

    match update_result {
        Ok(result) => {
            // `update_oauth_user` returns `False` (never raises) when the
            // account is disabled, after already calling
            // `frappe.respond_as_web_page`; there is no document to log into.
            let message = result.get("message");
            if matches!(message, Some(Value::Bool(false))) {
                return error_page(
                    StatusCode::FORBIDDEN,
                    "Not Allowed",
                    &format!("User {} is disabled", email),
                );
            }
        }
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("SignupDisabledError") {
                return error_page(
                    StatusCode::FORBIDDEN,
                    "Signup is Disabled",
                    "Sorry. Signup from Website is disabled.",
                );
            }
            warn!(error = %msg, "update_oauth_user failed for Office 365 login");
            return error_page(StatusCode::INTERNAL_SERVER_ERROR, "Login Failed", &msg);
        }
    }

    let store = session::SessionStore::new();
    let session = match store.create(&pool, email.clone(), "localhost".to_string()).await {
        Ok(s) => s,
        Err(e) => {
            warn!(error = %e, "failed to create session after Office 365 login");
            return error_page(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Login Failed",
                "Could not create a session",
            );
        }
    };
    let cookie = format!(
        "sid={}; Path=/; HttpOnly; SameSite=Lax; Max-Age=86400",
        session.id
    );

    let redirect_to = oauth_state
        .redirect_to
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/app".to_string());

    let mut resp = Redirect::to(&redirect_to).into_response();
    if let Ok(v) = HeaderValue::from_str(&cookie) {
        resp.headers_mut().insert(SET_COOKIE, v);
    }
    resp
}

struct OAuthState {
    redirect_to: Option<String>,
}

/// Decode the `state` param. Two shapes are accepted:
///
/// - the structured base64-JSON blob produced by
///   [`crate::social_login::legacy_oauth_state`] (`{"site", "token",
///   "redirect_to"}`), mirroring the check `frappe.utils.oauth.login_oauth_user`
///   performs (`state and state["token"]`) — this is the shape used by every
///   login page this project renders (`serve_login`, sebrus_logger's login
///   page), and it carries `redirect_to`.
/// - a bare opaque token (anything non-empty that isn't valid base64-JSON
///   with a `token` field). Frappe's own check never did more than confirm a
///   non-empty token was present (no server-side lookup in this codebase), so
///   treating a plain state string as already-validated is no weaker than
///   the check it replaces. This tolerates authorize URLs built by another
///   code path (e.g. an older cached login page, or a hand-built link) that
///   sent the raw token as `state` instead of the wrapped JSON blob; only
///   `redirect_to` is unavailable in that case.
fn decode_state(raw: &str) -> Option<OAuthState> {
    if raw.is_empty() {
        return None;
    }
    // Only fall back to treating `raw` as an opaque already-validated token
    // when it does not even decode as the structured JSON blob — a state
    // that *does* decode that way but is missing/empty `token` is explicitly
    // malformed, not just a different (opaque) format, and must still be
    // rejected.
    match decode_structured_state(raw) {
        Some(StructuredState::Valid(state)) => Some(state),
        Some(StructuredState::MissingToken) => None,
        None => Some(OAuthState { redirect_to: None }),
    }
}

enum StructuredState {
    Valid(OAuthState),
    MissingToken,
}

fn decode_structured_state(raw: &str) -> Option<StructuredState> {
    let decoded = BASE64.decode(raw).ok()?;
    let parsed: Value = serde_json::from_slice(&decoded).ok()?;
    let obj = parsed.as_object()?;
    let token = obj.get("token").and_then(|v| v.as_str());
    if token.map(|t| t.is_empty()).unwrap_or(true) {
        return Some(StructuredState::MissingToken);
    }
    let redirect_to = obj
        .get("redirect_to")
        .and_then(|v| v.as_str())
        .map(String::from);
    Some(StructuredState::Valid(OAuthState { redirect_to }))
}

/// The Social Login Key document's `name` is *not* reliably `"office_365"`.
/// Real Frappe names these records via a custom `SocialLoginKey.autoname()`
/// Python override (`self.name = frappe.scrub(self.provider_name)`), but this
/// project's native Rust `insert_doc` path (tried before falling back to
/// Python for desk-form saves) does not know about that per-DocType Python
/// override — it only honors a plain `autoname = "field:<x>"` DocType JSON
/// rule — so a Social Login Key record created/recreated through that path
/// ends up named with a random UUID instead. `python/frappe/__init__.py`'s
/// `_find_social_login_key`/`_oauth_provider_slugs` already solved exactly
/// this for the Python OAuth flow (this project has hit and fixed it before,
/// per that file's history) by matching on the *type* the row is configured
/// as, not its name; this mirrors that same resolution logic in Rust:
///   1. `frappe.scrub(social_login_provider)` (lowercase, spaces/dashes to
///      `_`) is one of the aliases for the requested provider slug.
///   2. Fallback: any enabled row whose authorize/access-token URL points at
///      `login.microsoftonline.com`, for rows where the Select field wasn't
///      set to the exact "Office 365"/"Microsoft" label.
fn oauth_provider_aliases(slug: &str) -> &'static [&'static str] {
    match slug {
        "office_365" | "microsoft" => &["office_365", "microsoft"],
        _ => &[],
    }
}

/// `frappe.scrub`: lowercase, spaces and dashes to underscores.
fn scrub(text: &str) -> String {
    text.replace(' ', "_").replace('-', "_").to_lowercase()
}

struct SocialLoginKeyRow {
    name: String,
    client_id: Option<String>,
    social_login_provider: Option<String>,
    authorize_url: String,
    access_token_url: String,
    redirect_url: String,
    custom_base_url: bool,
    base_url: Option<String>,
}

async fn find_social_login_key(
    pool: &orm::DatabasePool,
    provider_slug: &str,
) -> error::Result<Option<SocialLoginKeyRow>> {
    let sql = r#"SELECT name, client_id, social_login_provider, authorize_url,
                        access_token_url, redirect_url, custom_base_url, base_url
                 FROM "social_login_key"
                 WHERE enable_social_login = 1"#;
    let rows = pool.execute_sql(sql, vec![]).await?;

    let parsed: Vec<SocialLoginKeyRow> = rows
        .into_iter()
        .map(|mut r| SocialLoginKeyRow {
            name: r
                .remove("name")
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default(),
            client_id: r
                .remove("client_id")
                .and_then(|v| v.as_str().map(String::from))
                .filter(|s| !s.is_empty()),
            social_login_provider: r
                .remove("social_login_provider")
                .and_then(|v| v.as_str().map(String::from)),
            authorize_url: r
                .remove("authorize_url")
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default(),
            access_token_url: r
                .remove("access_token_url")
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default(),
            redirect_url: r
                .remove("redirect_url")
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default(),
            custom_base_url: r
                .remove("custom_base_url")
                .and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
                .unwrap_or(0)
                == 1,
            base_url: r.remove("base_url").and_then(|v| v.as_str().map(String::from)),
        })
        .collect();

    let aliases = oauth_provider_aliases(provider_slug);

    if let Some(row) = parsed
        .iter()
        .find(|r| r.social_login_provider.as_deref().is_some_and(|p| aliases.contains(&scrub(p).as_str())))
    {
        return Ok(Some(clone_row(row)));
    }

    // Fallback: any enabled key pointed at Microsoft's OAuth endpoints,
    // for a row whose Select field wasn't set to the exact label.
    if !aliases.is_empty() {
        if let Some(row) = parsed.iter().find(|r| {
            r.authorize_url.contains("login.microsoftonline.com")
                || r.access_token_url.contains("login.microsoftonline.com")
        }) {
            return Ok(Some(clone_row(row)));
        }
    }

    Ok(None)
}

fn clone_row(r: &SocialLoginKeyRow) -> SocialLoginKeyRow {
    SocialLoginKeyRow {
        name: r.name.clone(),
        client_id: r.client_id.clone(),
        social_login_provider: r.social_login_provider.clone(),
        authorize_url: r.authorize_url.clone(),
        access_token_url: r.access_token_url.clone(),
        redirect_url: r.redirect_url.clone(),
        custom_base_url: r.custom_base_url,
        base_url: r.base_url.clone(),
    }
}

async fn load_provider_config(
    pool: &orm::DatabasePool,
    site_url: &str,
    encryption_key: &str,
) -> error::Result<Option<ProviderConfig>> {
    let Some(mut row) = find_social_login_key(pool, PROVIDER).await? else {
        return Ok(None);
    };

    // The `__auth` table (holding the decrypted client_secret) is keyed by
    // the document's actual name, which may not be `PROVIDER` — see
    // `find_social_login_key`'s doc comment.
    let doc_name = std::mem::take(&mut row.name);

    let Some(client_id) = row.client_id else {
        return Ok(None);
    };

    let access_token_url = if row.custom_base_url {
        match &row.base_url {
            Some(base) => crate::social_login::build_oauth_url(base, &row.access_token_url),
            None => row.access_token_url,
        }
    } else {
        row.access_token_url
    };

    let redirect_uri = if row.redirect_url.starts_with("http://") || row.redirect_url.starts_with("https://") {
        row.redirect_url
    } else {
        format!("{}{}", site_url.trim_end_matches('/'), row.redirect_url)
    };

    let client_secret = orm::password::get_decrypted_password(
        pool,
        "Social Login Key",
        &doc_name,
        "client_secret",
        encryption_key,
    )
    .await?
    .unwrap_or_default();

    Ok(Some(ProviderConfig {
        client_id,
        client_secret,
        access_token_url,
        redirect_uri,
    }))
}

fn error_page(status: StatusCode, title: &str, message: &str) -> Response {
    (
        status,
        axum::response::Html(format!(
            "<h4>{}</h4><p>{}</p>",
            html_escape(title),
            html_escape(message)
        )),
    )
        .into_response()
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrub_matches_frappe_scrub() {
        // frappe.scrub("Office 365") == "office_365"
        assert_eq!(scrub("Office 365"), "office_365");
        assert_eq!(scrub("Micro-Soft"), "micro_soft");
        assert_eq!(scrub("GitHub"), "github");
    }

    #[test]
    fn oauth_provider_aliases_treats_office_365_and_microsoft_as_equivalent() {
        // A Social Login Key row can be labeled "Microsoft" instead of the
        // exact "Office 365" select option and must still resolve.
        assert!(oauth_provider_aliases("office_365").contains(&"microsoft"));
        assert!(oauth_provider_aliases("microsoft").contains(&"office_365"));
        assert!(oauth_provider_aliases("google").is_empty());
    }

    #[test]
    fn decode_state_accepts_legacy_base64_json_with_token() {
        let raw = crate::social_login::legacy_oauth_state("http://localhost:8000", Some("/desk"));
        let state = decode_state(&raw).expect("valid state should decode");
        assert_eq!(state.redirect_to.as_deref(), Some("/desk"));
    }

    #[test]
    fn decode_state_rejects_missing_token() {
        let raw = BASE64.encode(json!({"site": "http://localhost:8000"}).to_string());
        assert!(decode_state(&raw).is_none());
    }

    #[test]
    fn decode_state_rejects_empty_token() {
        let raw = BASE64.encode(json!({"token": ""}).to_string());
        assert!(decode_state(&raw).is_none());
    }

    #[test]
    fn decode_state_rejects_empty_string() {
        assert!(decode_state("").is_none());
    }

    /// A provider that redirects back with a bare opaque token as `state`
    /// (not the structured base64-JSON blob this project's own login pages
    /// send) must still be accepted with no `redirect_to`, not rejected --
    /// e.g. a 32-char hex token, exactly the shape Microsoft echoed back in
    /// a real callback that a stricter check once rejected with a 417.
    #[test]
    fn decode_state_accepts_bare_opaque_token() {
        let state = decode_state("0cc7184fb6ab100b57d732a7e9c1a466").expect("opaque token should be accepted");
        assert_eq!(state.redirect_to, None);
    }

    #[test]
    fn decode_state_accepts_non_base64_opaque_token() {
        let state = decode_state("not-base64-json!!!").expect("opaque token should be accepted");
        assert_eq!(state.redirect_to, None);
    }
}
