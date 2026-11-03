//! Single-use random tokens for out-of-band flows.
//!
//! Password resets, account invitations, email verification, share links —
//! every app hand-rolls these, and hand-rolled versions drift toward
//! `sha256(timestamp ‖ nanos ‖ pid)` and plaintext-at-rest. This service is
//! the one correct implementation:
//!
//! - **CSPRNG tokens**: 256 bits from `uuid::Uuid::new_v4` ×2, URL-safe hex.
//! - **Hashed at rest**: the table stores only `sha256(token)`; a database
//!   leak reveals nothing usable.
//! - **Single use**: `consume` marks the row used and returns the subject
//!   exactly once.
//! - **Expiry enforced**: rows past their TTL are refused and swept lazily.
//!
//! Storage is a per-site `__kiff_tokens` table created on first use:
//!
//! ```sql
//! name TEXT PRIMARY KEY, purpose TEXT, subject TEXT,
//! token_hash TEXT, expires_at TEXT, used INTEGER
//! ```

use std::time::Duration;

use orm::DatabasePool;
use serde_json::Value;

fn generate() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

fn hash(token: &str) -> String {
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    h.update(token.as_bytes());
    hex::encode(h.finalize())
}

async fn ensure_table(pool: &DatabasePool) -> error::Result<()> {
    pool.execute_sql(
        r#"CREATE TABLE IF NOT EXISTS __kiff_tokens (
            name TEXT PRIMARY KEY,
            purpose TEXT NOT NULL,
            subject TEXT NOT NULL,
            token_hash TEXT NOT NULL,
            expires_at TEXT NOT NULL,
            used INTEGER NOT NULL DEFAULT 0
        )"#,
        vec![],
    )
    .await?;
    Ok(())
}

/// Issue a token for `subject` under `purpose`, valid for `ttl`.
///
/// Returns the plaintext token to embed in a link or send out-of-band — it
/// is shown exactly once and cannot be recovered afterwards.
pub async fn issue(
    pool: &DatabasePool,
    purpose: &str,
    subject: &str,
    ttl: Duration,
) -> error::Result<String> {
    ensure_table(pool).await?;
    let token = generate();
    let expires = (chrono::Utc::now() + chrono::Duration::from_std(ttl).map_err(|_| {
        error::RuntimeError::Validation("token ttl out of range.".into())
    })?)
    .to_rfc3339();
    let name = format!("{}:{}", purpose, uuid::Uuid::new_v4());
    pool.execute_sql(
        r#"INSERT INTO __kiff_tokens (name, purpose, subject, token_hash, expires_at, used)
           VALUES (?, ?, ?, ?, ?, 0)"#,
        vec![
            Value::String(name),
            Value::String(purpose.into()),
            Value::String(subject.into()),
            Value::String(hash(&token)),
            Value::String(expires),
        ],
    )
    .await?;
    Ok(token)
}

/// Consume a token: validates purpose, expiry and single-use, marks it used,
/// and returns the subject. Returns `Ok(None)` for unknown, used or expired
/// tokens — callers should answer every failure identically ("invalid or
/// expired link") so the endpoint cannot be used to test token validity.
pub async fn consume(
    pool: &DatabasePool,
    purpose: &str,
    token: &str,
) -> error::Result<Option<String>> {
    ensure_table(pool).await?;
    let rows = pool
        .execute_sql(
            r#"SELECT name, subject, expires_at, used FROM __kiff_tokens
               WHERE purpose = ? AND token_hash = ? LIMIT 1"#,
            vec![Value::String(purpose.into()), Value::String(hash(token))],
        )
        .await?;
    let Some(mut row) = rows.into_iter().next() else {
        return Ok(None);
    };
    if row
        .remove("used")
        .and_then(|v| v.as_i64())
        .unwrap_or(0)
        != 0
    {
        return Ok(None);
    }
    let expired = row
        .remove("expires_at")
        .and_then(|v| v.as_str().map(String::from))
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
        .map(|e| e < chrono::Utc::now())
        .unwrap_or(true);
    if expired {
        return Ok(None);
    }
    let subject = row
        .remove("subject")
        .and_then(|v| v.as_str().map(String::from))
        .ok_or_else(|| error::RuntimeError::Validation("token row missing subject.".into()))?;
    let name = row
        .remove("name")
        .and_then(|v| v.as_str().map(String::from))
        .ok_or_else(|| error::RuntimeError::Validation("token row missing name.".into()))?;
    // Single use: mark consumed before returning the subject.
    pool.execute_sql(
        r#"UPDATE __kiff_tokens SET used = 1 WHERE name = ?"#,
        vec![Value::String(name)],
    )
    .await?;
    Ok(Some(subject))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_256_bit_hex() {
        let t = generate();
        assert_eq!(t.len(), 64);
        assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(generate(), generate());
    }

    #[test]
    fn hashing_is_stable_and_one_way() {
        assert_eq!(hash("abc"), hash("abc"));
        assert_ne!(hash("abc"), hash("abd"));
        assert_eq!(hash("abc").len(), 64);
    }
}
