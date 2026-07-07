//! Bearer-token lifecycle for external log ingestion.

use argon2::{
    password_hash::{rand_core::OsRng, SaltString},
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
};
use error::{Result, RuntimeError};
use orm::DatabasePool;
use rand::RngCore;
use serde_json::Value;

/// Length of the raw token in bytes (64 hex characters).
const TOKEN_BYTES: usize = 32;
/// Length of the token prefix used as the record name / lookup key.
const PREFIX_LEN: usize = 16;

/// Information returned after a successful token verification.
#[derive(Debug, Clone)]
pub struct VerifiedToken {
    pub name: String,
    pub token_name: String,
    pub external_app: String,
    pub role: String,
}

/// Generate a new random raw token.
pub fn generate_raw_token() -> String {
    let mut bytes = vec![0u8; TOKEN_BYTES];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Split a raw token into its public prefix and the secret remainder.
pub fn token_prefix(token: &str) -> String {
    token.chars().take(PREFIX_LEN).collect()
}

/// Hash a raw token for storage.
pub fn hash_token(token: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    argon2
        .hash_password(token.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| RuntimeError::Validation(format!("failed to hash token: {}", e)))
}

/// Verify a raw token against a stored argon2 hash.
pub fn verify_token(token: &str, hash: &str) -> bool {
    let parsed = match PasswordHash::new(hash) {
        Ok(h) => h,
        Err(_) => return false,
    };
    Argon2::default()
        .verify_password(token.as_bytes(), &parsed)
        .is_ok()
}

/// Create a token record and return the raw token that must be shown once.
pub async fn create_token(
    pool: &DatabasePool,
    token_name: &str,
    external_app: Option<&str>,
    role: &str,
    description: Option<&str>,
) -> Result<String> {
    let raw = generate_raw_token();
    let prefix = token_prefix(&raw);
    let hash = hash_token(&raw)?;

    let now = chrono::Utc::now().to_rfc3339();
    let external_app = external_app.unwrap_or("");
    let description = description.unwrap_or("");

    pool.execute_sql(
        r#"INSERT INTO kiff_logger_token
           (name, token_name, token_hash, external_app, role, enabled, description,
            creation, modified, modified_by, owner, docstatus)
           VALUES (?, ?, ?, ?, ?, 1, ?, ?, ?, 'Administrator', 'Administrator', 0)"#,
        vec![
            Value::String(prefix),
            Value::String(token_name.into()),
            Value::String(hash),
            Value::String(external_app.into()),
            Value::String(role.into()),
            Value::String(description.into()),
            Value::String(now.clone()),
            Value::String(now),
        ],
    )
    .await?;

    Ok(raw)
}

/// Look up a token by its prefix and verify the full secret.
pub async fn verify_bearer_token(
    pool: &DatabasePool,
    token: &str,
) -> Result<Option<VerifiedToken>> {
    if token.len() <= PREFIX_LEN {
        return Ok(None);
    }
    let prefix = token_prefix(token);

    let rows = pool
        .execute_sql(
            r#"SELECT name, token_name, external_app, role, token_hash
               FROM kiff_logger_token
               WHERE name = ? AND enabled = 1"#,
            vec![Value::String(prefix)],
        )
        .await?;

    for row in rows {
        let stored_hash = row.get("token_hash").and_then(|v| v.as_str()).unwrap_or("");
        if !verify_token(token, stored_hash) {
            continue;
        }
        return Ok(Some(VerifiedToken {
            name: row
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            token_name: row
                .get("token_name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            external_app: row
                .get("external_app")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            role: row
                .get("role")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        }));
    }

    Ok(None)
}

/// Update the `last_used_at` timestamp for a token.
pub async fn touch_token(pool: &DatabasePool, name: &str) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    pool.execute_sql(
        r#"UPDATE kiff_logger_token SET last_used_at = ?, modified = ? WHERE name = ?"#,
        vec![
            Value::String(now.clone()),
            Value::String(now),
            Value::String(name.into()),
        ],
    )
    .await?;
    Ok(())
}

/// Revoke (blacklist) a token by its public name/prefix.
pub async fn revoke_token(pool: &DatabasePool, name: &str, revoked_by: &str) -> Result<bool> {
    let now = chrono::Utc::now().to_rfc3339();
    let rows = pool
        .execute_sql(
            r#"UPDATE kiff_logger_token
               SET enabled = 0, revoked_at = ?, revoked_by = ?, modified = ?
               WHERE name = ? AND enabled = 1
               RETURNING name"#,
            vec![
                Value::String(now.clone()),
                Value::String(revoked_by.into()),
                Value::String(now),
                Value::String(name.into()),
            ],
        )
        .await?;
    Ok(!rows.is_empty())
}
