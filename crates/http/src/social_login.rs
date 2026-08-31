//! Social login (OAuth2) helpers shared by the core login page and Rust apps
//! that render their own login screens.
//!
//! The functions here were extracted from `handlers::desk` so app crates (e.g.
//! `sebrus_logger`) can build the same authorize URLs without duplicating the
//! provider-loading and OAuth quirks (Entra ID scope defaults, custom base
//! URLs, legacy vs. server-validated state formats).

use axum::http::HeaderMap;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use serde_json::{json, Value};
use std::collections::HashMap;

/// Enabled social login provider as stored in the Social Login Key doctype.
#[derive(Debug)]
pub struct SocialLoginProvider {
    pub name: String,
    pub provider_name: String,
    pub client_id: String,
    pub authorize_url: String,
    pub redirect_url: String,
    pub auth_url_data: Option<Value>,
    pub custom_base_url: bool,
    pub base_url: Option<String>,
    pub icon: Option<String>,
}

/// Build the absolute site URL from the request Host header.
pub fn site_url_from_headers(headers: &HeaderMap) -> String {
    let host = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("localhost:8000");
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("http");
    format!("{}://{}", scheme, host)
}

/// Load enabled Social Login Keys from the database.
pub async fn get_social_login_providers(pool: &orm::DatabasePool) -> Vec<SocialLoginProvider> {
    let sql = r#"SELECT name, client_id, base_url, provider_name, icon,
                        authorize_url, redirect_url, auth_url_data, custom_base_url
                 FROM "social_login_key"
                 WHERE enable_social_login = 1
                 ORDER BY name"#;

    let rows = match pool.execute_sql(sql, vec![]).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("failed to load social login keys: {}", e);
            return vec![];
        }
    };

    rows.into_iter()
        .filter_map(|mut row| {
            let client_id = row.remove("client_id")?.as_str()?.to_string();
            if client_id.is_empty() {
                return None;
            }
            let name = row.remove("name")?.as_str()?.to_string();
            let provider_name = row
                .remove("provider_name")
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_else(|| name.clone());
            let authorize_url = row.remove("authorize_url")?.as_str()?.to_string();
            let redirect_url = row.remove("redirect_url")?.as_str()?.to_string();
            let auth_url_data = row.remove("auth_url_data").filter(|v| !v.is_null());
            let custom_base_url = row
                .remove("custom_base_url")
                .and_then(|v| {
                    v.as_i64()
                        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                })
                .unwrap_or(0)
                == 1;
            let base_url = row
                .remove("base_url")
                .and_then(|v| v.as_str().map(String::from));
            let icon = row
                .remove("icon")
                .and_then(|v| v.as_str().map(String::from));

            Some(SocialLoginProvider {
                name,
                provider_name,
                client_id,
                authorize_url,
                redirect_url,
                auth_url_data,
                custom_base_url,
                base_url,
                icon,
            })
        })
        .collect()
}

/// Legacy OAuth state format expected by Frappe <= 16.25: a base64-encoded
/// JSON blob validated client-side by the callback (no server-side cache).
pub fn legacy_oauth_state(site_url: &str, redirect_to: Option<&str>) -> String {
    let token = uuid::Uuid::new_v4().simple().to_string();
    let state = json!({
        "site": site_url,
        "token": token,
        "redirect_to": redirect_to.unwrap_or(""),
    });
    BASE64.encode(state.to_string().as_bytes())
}

/// Build the OAuth2 authorization URL for a provider. `state` is created by
/// the caller: via `kiff_core::create_oauth_state` on Frappe versions
/// that validate state server-side, or [`legacy_oauth_state`] on older ones.
pub fn build_authorize_url(
    provider: &SocialLoginProvider,
    site_url: &str,
    state: &str,
) -> Option<String> {
    let authorize_url = if provider.custom_base_url {
        match &provider.base_url {
            Some(base) => build_oauth_url(base, &provider.authorize_url),
            None => return None,
        }
    } else {
        provider.authorize_url.clone()
    };

    let redirect_uri = if provider.redirect_url.starts_with("http://")
        || provider.redirect_url.starts_with("https://")
    {
        provider.redirect_url.clone()
    } else {
        format!(
            "{}{}",
            site_url.trim_end_matches('/'),
            provider.redirect_url
        )
    };

    let mut params: HashMap<String, String> = HashMap::new();
    params.insert("client_id".to_string(), provider.client_id.clone());
    params.insert("redirect_uri".to_string(), redirect_uri);
    params.insert("state".to_string(), state.to_string());

    if let Some(Value::Object(map)) = &provider.auth_url_data {
        for (k, v) in map {
            if let Some(s) = v.as_str() {
                params.insert(k.clone(), s.to_string());
            } else if !v.is_null() {
                params.insert(k.clone(), v.to_string());
            }
        }
    }

    // Default OAuth2 parameters if the provider config did not supply them.
    params
        .entry("response_type".to_string())
        .or_insert_with(|| "code".to_string());

    // Microsoft Entra ID (v2.0) requires a scope parameter on the authorize request.
    // If the provider config left it out, default to the standard OIDC scopes so login works.
    if authorize_url.contains("login.microsoftonline.com")
        && authorize_url.contains("/oauth2/v2.0/authorize")
    {
        params
            .entry("scope".to_string())
            .or_insert_with(|| "openid email profile".to_string());
    }

    let query = match serde_urlencoded::to_string(&params) {
        Ok(q) => q,
        Err(_) => return None,
    };

    Some(format!("{}?{}", authorize_url, query))
}

/// Join a base URL with a relative or absolute OAuth URL.
pub fn build_oauth_url(base_url: &str, url: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") {
        return url.to_string();
    }
    format!("{}{}", base_url.trim_end_matches('/'), url)
}

/// Load enabled providers and pair each with its authorize URL, ready for
/// rendering as login buttons. `redirect_to` is embedded in the OAuth state
/// so the callback lands the user on that path after login.
pub async fn social_login_urls(
    pool: &orm::DatabasePool,
    site_url: &str,
    redirect_to: Option<&str>,
) -> Vec<(SocialLoginProvider, String)> {
    get_social_login_providers(pool)
        .await
        .into_iter()
        .filter_map(|p| {
            // The callback runs in the embedded Python and (on Frappe
            // versions newer than 16.25) validates `state` against a
            // cache entry written by create_oauth_state — so the state
            // must come from there when the API exists. Older Frappe
            // expects the legacy self-contained base64 state instead.
            let state = kiff_core::create_oauth_state(redirect_to)
                .unwrap_or_else(|| legacy_oauth_state(site_url, redirect_to));
            let url = build_authorize_url(&p, site_url, &state)?;
            Some((p, url))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_oauth_url_with_absolute_url() {
        assert_eq!(
            build_oauth_url(
                "https://example.com",
                "https://login.microsoftonline.com/common/oauth2/authorize"
            ),
            "https://login.microsoftonline.com/common/oauth2/authorize"
        );
    }

    #[test]
    fn test_build_oauth_url_with_relative_url() {
        assert_eq!(
            build_oauth_url("https://example.com", "/oauth2/authorize"),
            "https://example.com/oauth2/authorize"
        );
    }

    #[test]
    fn test_build_oauth_url_with_base_trailing_slash() {
        assert_eq!(
            build_oauth_url("https://example.com/", "/oauth2/authorize"),
            "https://example.com/oauth2/authorize"
        );
    }

    #[test]
    fn test_build_authorize_url_for_office365() {
        let provider = SocialLoginProvider {
            name: "office_365".to_string(),
            provider_name: "Office 365".to_string(),
            client_id: "test-client-id".to_string(),
            authorize_url: "https://login.microsoftonline.com/common/oauth2/authorize".to_string(),
            redirect_url: "/api/method/frappe.integrations.oauth2_logins.login_via_office365"
                .to_string(),
            auth_url_data: Some(json!({"response_type": "code", "scope": "openid"})),
            custom_base_url: false,
            base_url: None,
            icon: Some("/assets/frappe/icons/social/office_365.svg".to_string()),
        };

        let url = build_authorize_url(&provider, "http://localhost:8000", "test-state").unwrap();
        assert!(url.starts_with("https://login.microsoftonline.com/common/oauth2/authorize?"));
        assert!(url.contains("client_id=test-client-id"));
        assert!(url.contains("redirect_uri=http%3A%2F%2Flocalhost%3A8000%2Fapi%2Fmethod%2Ffrappe.integrations.oauth2_logins.login_via_office365"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("scope=openid"));
        assert!(url.contains("state=test-state"));
    }

    #[test]
    fn test_build_authorize_url_for_microsoft_entra_v2_defaults_scope() {
        let provider = SocialLoginProvider {
            name: "microsoft".to_string(),
            provider_name: "Microsoft".to_string(),
            client_id: "test-client-id".to_string(),
            authorize_url: "https://login.microsoftonline.com/1d6f2f1f-694e-4308-a2ba-bb00bb00fa46/oauth2/v2.0/authorize".to_string(),
            redirect_url: "/api/method/frappe.integrations.oauth2_logins.login_via_microsoft".to_string(),
            auth_url_data: Some(json!({"response_type": "code"})),
            custom_base_url: false,
            base_url: None,
            icon: Some("/assets/frappe/icons/social/office_365.svg".to_string()),
        };

        let url = build_authorize_url(
            &provider,
            "https://compliance-system.sebrus.dev",
            "test-state",
        )
        .unwrap();
        assert!(url.starts_with("https://login.microsoftonline.com/1d6f2f1f-694e-4308-a2ba-bb00bb00fa46/oauth2/v2.0/authorize?"));
        assert!(url.contains("client_id=test-client-id"));
        assert!(url.contains("redirect_uri=https%3A%2F%2Fcompliance-system.sebrus.dev%2Fapi%2Fmethod%2Ffrappe.integrations.oauth2_logins.login_via_microsoft"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("scope=openid+email+profile"));
        assert!(url.contains("state=test-state"));
    }

    #[test]
    fn test_legacy_oauth_state_is_base64_json() {
        let state = legacy_oauth_state("http://localhost:8000", Some("/desk"));
        let decoded: Value = serde_json::from_slice(&BASE64.decode(&state).unwrap()).unwrap();
        assert_eq!(decoded["site"], "http://localhost:8000");
        assert_eq!(decoded["redirect_to"], "/desk");
        assert!(decoded["token"].as_str().unwrap().len() >= 32);

        let state = legacy_oauth_state("http://localhost:8000", None);
        let decoded: Value = serde_json::from_slice(&BASE64.decode(&state).unwrap()).unwrap();
        assert_eq!(decoded["redirect_to"], "");
    }
}
