//! OAuth2 token exchange, done in Rust instead of the Python `rauth`
//! library.
//!
//! `rauth`'s `process_token_request` only knows how to look up a fixed key
//! (`access_token`) in the provider's JSON response and raises a bare
//! `KeyError` with the *entire* raw response crammed into the message when
//! that key is missing — which is exactly what happens on every OAuth error
//! response (Microsoft returns `{"error": "...", "error_description": "..."}`
//! with no `access_token` at all). That turned a normal, actionable OAuth
//! error into an opaque `KeyError` traceback. This module does the token
//! POST directly and raises a Python exception carrying the provider's own
//! `error`/`error_description` when the exchange fails.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use std::collections::HashMap;

use crate::{json_to_py, rt};

/// POST an OAuth2 authorization-code grant to `access_token_url` and return
/// the parsed JSON response as a Python dict.
///
/// Raises a `RuntimeError` (readable by the Python caller) describing the
/// provider's own error when the exchange fails, instead of surfacing a
/// generic HTTP/JSON error.
#[pyfunction]
#[pyo3(signature = (access_token_url, code, redirect_uri, client_id, client_secret, scope=None))]
pub fn oauth2_token_exchange(
    py: Python<'_>,
    access_token_url: String,
    code: String,
    redirect_uri: String,
    client_id: String,
    client_secret: String,
    scope: Option<String>,
) -> PyResult<PyObject> {
    // Diagnostic only -- never logs the secret or the full code (which is a
    // one-time credential), just enough to tell whether the code/redirect_uri
    // reaching this point look like what the browser actually received.
    tracing::warn!(
        access_token_url = %access_token_url,
        redirect_uri = %redirect_uri,
        client_id = %client_id,
        scope = ?scope,
        code_len = code.len(),
        code_prefix = %code.chars().take(12).collect::<String>(),
        code_suffix = %code.chars().rev().take(12).collect::<String>().chars().rev().collect::<String>(),
        code_has_plus = code.contains('+'),
        code_has_space = code.contains(' '),
        "oauth2_token_exchange request"
    );

    let mut form: HashMap<&str, String> = HashMap::new();
    form.insert("code", code);
    form.insert("redirect_uri", redirect_uri);
    form.insert("grant_type", "authorization_code".to_string());
    form.insert("client_id", client_id);
    form.insert("client_secret", client_secret);
    if let Some(scope) = scope {
        form.insert("scope", scope);
    }

    let body: serde_json::Value = py.allow_threads(|| {
        rt().block_on(async {
            let client = reqwest::Client::new();
            let resp = client
                .post(&access_token_url)
                .header("Accept", "application/json")
                .form(&form)
                .send()
                .await
                .map_err(|e| format!("OAuth token request to {access_token_url} failed: {e}"))?;

            let status = resp.status();
            let text = resp
                .text()
                .await
                .map_err(|e| format!("failed to read OAuth token response body: {e}"))?;

            parse_token_response(status, &text)
                .map_err(|e| format!("{e} (requested from {access_token_url})"))
        })
    })
    .map_err(PyRuntimeError::new_err)?;

    json_to_py(py, &body)
}

/// Parse a token-endpoint HTTP response into either the successful JSON body
/// or a human-readable error message describing what the provider actually
/// said. Split out from [`oauth2_token_exchange`] so the error-formatting
/// logic is unit-testable without a live network call.
fn parse_token_response(
    status: reqwest::StatusCode,
    text: &str,
) -> Result<serde_json::Value, String> {
    let parsed: serde_json::Value = serde_json::from_str(text).map_err(|e| {
        format!("OAuth token response was not valid JSON (status {status}): {e}. Raw response: {text}")
    })?;

    if !status.is_success() || parsed.get("error").is_some() {
        let error = parsed
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown_error");
        let description = parsed
            .get("error_description")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        return Err(format!(
            "OAuth token exchange failed (HTTP {status}): {error}: {description}"
        ));
    }

    Ok(parsed)
}

/// Decode a JWT's payload (the middle base64url segment) into a Python dict,
/// without verifying the signature — mirrors the previous
/// `jwt.decode(token, ..., options={"verify_signature": False})` behavior
/// used to read Microsoft's `id_token` claims.
#[pyfunction]
pub fn oauth2_decode_jwt_payload(py: Python<'_>, token: String) -> PyResult<PyObject> {
    let payload_segment = token
        .split('.')
        .nth(1)
        .ok_or_else(|| PyRuntimeError::new_err("id_token is not a JWT (missing payload segment)"))?;

    let decoded = URL_SAFE_NO_PAD
        .decode(payload_segment)
        .map_err(|e| PyRuntimeError::new_err(format!("failed to base64-decode id_token payload: {e}")))?;

    let claims: serde_json::Value = serde_json::from_slice(&decoded)
        .map_err(|e| PyRuntimeError::new_err(format!("id_token payload is not valid JSON: {e}")))?;

    json_to_py(py, &claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_token_response_surfaces_microsoft_error_description() {
        // The exact shape Microsoft returns for AADSTS70000 (invalid_grant) --
        // this is what rauth used to swallow into a bare `KeyError: 'access_token'`.
        let body = r#"{"error":"invalid_grant","error_description":"AADSTS70000: The request was denied because one or more scopes requested are unauthorized or expired."}"#;
        let err = parse_token_response(reqwest::StatusCode::BAD_REQUEST, body).unwrap_err();
        assert!(err.contains("invalid_grant"));
        assert!(err.contains("AADSTS70000"));
    }

    #[test]
    fn parse_token_response_accepts_successful_body() {
        let body = r#"{"access_token":"abc","id_token":"xyz","token_type":"Bearer"}"#;
        let parsed = parse_token_response(reqwest::StatusCode::OK, body).unwrap();
        assert_eq!(parsed["id_token"], "xyz");
    }

    #[test]
    fn parse_token_response_rejects_non_json_body() {
        let err = parse_token_response(reqwest::StatusCode::OK, "not json").unwrap_err();
        assert!(err.contains("not valid JSON"));
    }

    #[test]
    fn decode_jwt_payload_extracts_claims() {
        pyo3::prepare_freethreaded_python();
        Python::with_gil(|py| {
            // header.payload.signature, payload = base64url({"email":"a@b.com","sub":"123"})
            let payload = URL_SAFE_NO_PAD.encode(r#"{"email":"a@b.com","sub":"123"}"#);
            let token = format!("eyJhbGciOiJIUzI1NiJ9.{payload}.sig");
            let result = oauth2_decode_jwt_payload(py, token).unwrap();
            let bound = result.bind(py);
            let email: String = bound.get_item("email").unwrap().extract().unwrap();
            assert_eq!(email, "a@b.com");
        });
    }

    #[test]
    fn decode_jwt_payload_rejects_malformed_token() {
        pyo3::prepare_freethreaded_python();
        Python::with_gil(|py| {
            assert!(oauth2_decode_jwt_payload(py, "not-a-jwt".to_string()).is_err());
        });
    }
}
