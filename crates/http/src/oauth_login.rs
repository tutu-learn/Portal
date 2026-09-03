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

/// Decode the base64-JSON `state` param produced by
/// [`crate::social_login::legacy_oauth_state`], mirroring the check
/// `frappe.utils.oauth.login_oauth_user` performs (`state and state["token"]`).
fn decode_state(raw: &str) -> Option<OAuthState> {
    let decoded = BASE64.decode(raw).ok()?;
    let parsed: Value = serde_json::from_slice(&decoded).ok()?;
    let token = parsed.get("token").and_then(|v| v.as_str());
    if token.map(|t| t.is_empty()).unwrap_or(true) {
        return None;
    }
    let redirect_to = parsed
        .get("redirect_to")
        .and_then(|v| v.as_str())
        .map(String::from);
    Some(OAuthState { redirect_to })
}

async fn load_provider_config(
    pool: &orm::DatabasePool,
    site_url: &str,
    encryption_key: &str,
) -> error::Result<Option<ProviderConfig>> {
    let sql = r#"SELECT client_id, access_token_url, redirect_url, custom_base_url, base_url
                 FROM "social_login_key"
                 WHERE name = ? AND enable_social_login = 1"#;
    let mut rows = pool
        .execute_sql(sql, vec![Value::String(PROVIDER.into())])
        .await?;
    let Some(mut row) = rows.pop() else {
        return Ok(None);
    };

    let client_id = row
        .remove("client_id")
        .and_then(|v| v.as_str().map(String::from))
        .filter(|s| !s.is_empty());
    let Some(client_id) = client_id else {
        return Ok(None);
    };

    let raw_access_token_url = row
        .remove("access_token_url")
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default();
    let redirect_url = row
        .remove("redirect_url")
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default();
    let custom_base_url = row
        .remove("custom_base_url")
        .and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
        .unwrap_or(0)
        == 1;
    let base_url = row.remove("base_url").and_then(|v| v.as_str().map(String::from));

    let access_token_url = if custom_base_url {
        match &base_url {
            Some(base) => crate::social_login::build_oauth_url(base, &raw_access_token_url),
            None => raw_access_token_url,
        }
    } else {
        raw_access_token_url
    };

    let redirect_uri = if redirect_url.starts_with("http://") || redirect_url.starts_with("https://") {
        redirect_url
    } else {
        format!("{}{}", site_url.trim_end_matches('/'), redirect_url)
    };

    let client_secret = orm::password::get_decrypted_password(
        pool,
        "Social Login Key",
        PROVIDER,
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
    fn decode_state_rejects_garbage() {
        assert!(decode_state("not-base64-json!!!").is_none());
    }

    #[test]
    fn decode_state_rejects_empty_token() {
        let raw = BASE64.encode(json!({"token": ""}).to_string());
        assert!(decode_state(&raw).is_none());
    }
}
