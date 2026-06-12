use crate::session::{Session, SessionStore};
use argon2::{Argon2, PasswordHash, PasswordVerifier};
use error::{Result, RuntimeError};
use orm::DatabasePool;

#[derive(Debug, Clone)]
pub struct AuthService {
    store: SessionStore,
}

impl AuthService {
    pub fn new(store: SessionStore) -> Self {
        Self { store }
    }

    pub async fn login(
        &self,
        pool: &DatabasePool,
        username: &str,
        password: &str,
        site: &str,
    ) -> Result<Session> {
        let hash = self.get_password_hash(pool, username).await?;
        if !self.verify_password(password, &hash).await? {
            return Err(RuntimeError::Auth("invalid password".into()));
        }
        self.store.create(pool, username.into(), site.into()).await
    }

    pub async fn logout(&self, pool: &DatabasePool, session_id: &str) -> Result<()> {
        self.store.delete(pool, session_id).await
    }

    async fn get_password_hash(&self, pool: &DatabasePool, username: &str) -> Result<String> {
        // Try to read from __auth table (Frappe-compatible)
        let rows = pool.execute_sql(
            r#"SELECT password FROM "__auth" WHERE name = ? AND doctype = 'User' AND fieldname = '_password'"#,
            vec![serde_json::Value::String(username.into())],
        ).await?;

        if let Some(row) = rows.into_iter().next() {
            if let Some(hash) = row.get("password").and_then(|v| v.as_str()) {
                return Ok(hash.to_string());
            }
        }

        Err(RuntimeError::Auth("user not found".into()))
    }

    pub async fn verify_password(&self, password: &str, hash: &str) -> Result<bool> {
        let parsed_hash = PasswordHash::new(hash)
            .map_err(|e| RuntimeError::Auth(format!("invalid hash: {}", e)))?;
        let argon2 = Argon2::default();
        Ok(argon2.verify_password(password.as_bytes(), &parsed_hash).is_ok())
    }

    pub async fn verify_totp(&self, secret: &str, token: &str) -> Result<bool> {
        use totp_rs::{Algorithm, TOTP, Secret};
        let secret_bytes = Secret::Raw(secret.as_bytes().to_vec())
            .to_bytes()
            .map_err(|e| RuntimeError::Auth(format!("totp secret error: {}", e)))?;
        let totp = TOTP::new(
            Algorithm::SHA1,
            6,
            1,
            30,
            secret_bytes,
        ).map_err(|e| RuntimeError::Auth(format!("totp init error: {}", e)))?;
        Ok(totp.check_current(token).unwrap_or(false))
    }
}
