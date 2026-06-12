use chrono::{DateTime, Utc};
use error::{Result, RuntimeError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub user: String,
    pub site: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    #[serde(default)]
    pub data: HashMap<String, serde_json::Value>,
}

impl Session {
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }
}

#[derive(Debug, Clone)]
pub struct SessionStore;

impl SessionStore {
    pub fn new() -> Self {
        Self
    }

    pub async fn create(
        &self,
        pool: &orm::DatabasePool,
        user: String,
        site: String,
    ) -> Result<Session> {
        let now = Utc::now();
        let expires = now + chrono::Duration::hours(24);
        let session = Session {
            id: uuid::Uuid::new_v4().to_string(),
            user: user.clone(),
            site: site.clone(),
            created_at: now,
            expires_at: expires,
            data: HashMap::new(),
        };

        let data_json = serde_json::to_string(&session.data)?;
        let sql = match pool.dialect() {
            "postgres" => r#"
                INSERT INTO __kiff_sessions (id, "user", site, created_at, expires_at, data)
                VALUES ($1, $2, $3, $4, $5, $6)
            "#,
            _ => r#"
                INSERT INTO __kiff_sessions (id, user, site, created_at, expires_at, data)
                VALUES (?, ?, ?, ?, ?, ?)
            "#,
        };
        pool.execute_sql(sql, vec![
            serde_json::Value::String(session.id.clone()),
            serde_json::Value::String(user),
            serde_json::Value::String(site),
            serde_json::Value::String(now.to_rfc3339()),
            serde_json::Value::String(expires.to_rfc3339()),
            serde_json::Value::String(data_json),
        ]).await?;

        Ok(session)
    }

    pub async fn get(&self, pool: &orm::DatabasePool, session_id: &str) -> Result<Option<Session>> {
        let sql = match pool.dialect() {
            "postgres" => "SELECT * FROM __kiff_sessions WHERE id = $1 LIMIT 1",
            _ => "SELECT * FROM __kiff_sessions WHERE id = ? LIMIT 1",
        };
        let rows = pool.execute_sql(sql, vec![serde_json::Value::String(session_id.into())]).await?;
        let mut row = match rows.into_iter().next() {
            Some(r) => r,
            None => return Ok(None),
        };

        let expires_str = row.remove("expires_at")
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default();
        let expires: DateTime<Utc> = expires_str.parse()
            .map_err(|e| RuntimeError::Validation(format!("invalid expires_at: {}", e)))?;

        if Utc::now() > expires {
            // Clean up expired session
            let _ = self.delete(pool, session_id).await;
            return Ok(None);
        }

        let data_json = row.remove("data")
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_else(|| "{}".into());
        let data: HashMap<String, serde_json::Value> = serde_json::from_str(&data_json)
            .unwrap_or_default();

        Ok(Some(Session {
            id: row.remove("id").and_then(|v| v.as_str().map(String::from)).unwrap_or_default(),
            user: row.remove("user").and_then(|v| v.as_str().map(String::from)).unwrap_or_default(),
            site: row.remove("site").and_then(|v| v.as_str().map(String::from)).unwrap_or_default(),
            created_at: row.remove("created_at")
                .and_then(|v| v.as_str().and_then(|s| s.parse().ok()))
                .unwrap_or_else(Utc::now),
            expires_at: expires,
            data,
        }))
    }

    pub async fn delete(&self, pool: &orm::DatabasePool, session_id: &str) -> Result<()> {
        let sql = match pool.dialect() {
            "postgres" => "DELETE FROM __kiff_sessions WHERE id = $1",
            _ => "DELETE FROM __kiff_sessions WHERE id = ?",
        };
        pool.execute_sql(sql, vec![serde_json::Value::String(session_id.into())]).await?;
        Ok(())
    }

    pub async fn update_data(
        &self,
        pool: &orm::DatabasePool,
        session_id: &str,
        data: &HashMap<String, serde_json::Value>,
    ) -> Result<()> {
        let data_json = serde_json::to_string(data)?;
        let sql = match pool.dialect() {
            "postgres" => "UPDATE __kiff_sessions SET data = $1 WHERE id = $2",
            _ => "UPDATE __kiff_sessions SET data = ? WHERE id = ?",
        };
        pool.execute_sql(sql, vec![
            serde_json::Value::String(data_json),
            serde_json::Value::String(session_id.into()),
        ]).await?;
        Ok(())
    }
}

impl Default for SessionStore {
    fn default() -> Self {
        Self::new()
    }
}
