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
        // Try to read from __auth table by username, falling back to email.
        for filter_col in ["name", "email"] {
            let rows = pool
                .execute_sql(
                    &format!(
                        r#"SELECT a.password FROM "__auth" a
                       JOIN "user" u ON u.name = a.name
                       WHERE u.{} = ? AND a.doctype = 'User' AND a.fieldname = 'password'"#,
                        filter_col
                    ),
                    vec![serde_json::Value::String(username.into())],
                )
                .await?;

            if let Some(row) = rows.into_iter().next() {
                if let Some(hash) = row.get("password").and_then(|v| v.as_str()) {
                    return Ok(hash.to_string());
                }
            }
        }

        Err(RuntimeError::Auth("user not found".into()))
    }

    pub async fn verify_password(&self, password: &str, hash: &str) -> Result<bool> {
        if hash.starts_with("$argon2") {
            let parsed_hash = PasswordHash::new(hash)
                .map_err(|e| RuntimeError::Auth(format!("invalid hash: {}", e)))?;
            let argon2 = Argon2::default();
            return Ok(argon2
                .verify_password(password.as_bytes(), &parsed_hash)
                .is_ok());
        }

        if hash.starts_with("$pbkdf2-sha256$") {
            return Ok(verify_pbkdf2_sha256(password, hash));
        }

        Err(RuntimeError::Auth("unsupported password hash".into()))
    }

    pub async fn verify_totp(&self, secret: &str, token: &str) -> Result<bool> {
        use totp_rs::{Algorithm, Secret, TOTP};
        let secret_bytes = Secret::Raw(secret.as_bytes().to_vec())
            .to_bytes()
            .map_err(|e| RuntimeError::Auth(format!("totp secret error: {}", e)))?;
        let totp = TOTP::new(Algorithm::SHA1, 6, 1, 30, secret_bytes)
            .map_err(|e| RuntimeError::Auth(format!("totp init error: {}", e)))?;
        Ok(totp.check_current(token).unwrap_or(false))
    }
}

fn verify_pbkdf2_sha256(password: &str, hash: &str) -> bool {
    // passlib format: $pbkdf2-sha256$<rounds>$<salt_ab64>$<hash_ab64>
    let parts: Vec<&str> = hash.split('$').collect();
    if parts.len() != 5 || parts[1] != "pbkdf2-sha256" {
        return false;
    }
    let rounds: u32 = match parts[2].parse() {
        Ok(r) => r,
        Err(_) => return false,
    };
    let salt = match ab64_decode(parts[3]) {
        Ok(s) => s,
        Err(_) => return false,
    };

    use pbkdf2::pbkdf2_hmac_array;
    use sha2::Sha256;

    let computed = pbkdf2_hmac_array::<Sha256, 32>(password.as_bytes(), &salt, rounds);
    let expected = match ab64_decode(parts[4]) {
        Ok(e) => e,
        Err(_) => return false,
    };

    computed.as_slice() == expected.as_slice()
}

fn ab64_decode(input: &str) -> std::result::Result<Vec<u8>, base64::DecodeError> {
    // passlib's "ab64" uses '.' instead of '+' and omits padding.
    let normalized = input.replace('.', "+");
    let padded = match normalized.len() % 4 {
        0 => normalized,
        2 => normalized + "==",
        3 => normalized + "=",
        _ => normalized, // invalid length, let base64 fail
    };
    base64::decode(padded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verify_pbkdf2_sha256() {
        // passlib pbkdf2_sha256 hash of "test-password"
        let hash = "$pbkdf2-sha256$29000$K8XYu1fKmbO29v7fG8O4dw$0ftJMwuU.DfgHUNRu5xXqQhBgFioIIq3bx0nmXhl7l0";
        assert!(verify_pbkdf2_sha256("test-password", hash));
        assert!(!verify_pbkdf2_sha256("wrong-password", hash));
    }

    #[tokio::test]
    async fn test_verify_argon2() {
        let auth = AuthService::new(SessionStore::new());
        // argon2 hash of "admin"
        let hash = "$argon2id$v=19$m=19456,t=2,p=1$UEWqTMicBrdEJXqPMhP4oA$bR1RecCR37Rw+Spup2ULPNKAZ7H6vZTX4VeqNAfvdkY";
        assert!(auth.verify_password("admin", hash).await.unwrap());
        assert!(!auth.verify_password("wrong", hash).await.unwrap());
    }
}
