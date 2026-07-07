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

/// Optional client metadata captured at login / on refresh.
/// Stored inside `__kiff_sessions.data` and mirrored into `tabSessions.sessiondata`.
#[derive(Debug, Clone, Default)]
pub struct SessionMetadata {
    pub ip: Option<String>,
    pub user_agent: Option<String>,
}

impl SessionMetadata {
    fn merge_into(&self, data: &mut HashMap<String, serde_json::Value>, now: DateTime<Utc>) {
        if let Some(ip) = &self.ip {
            data.insert("session_ip".into(), serde_json::Value::String(ip.clone()));
        }
        if let Some(ua) = &self.user_agent {
            data.insert("user_agent".into(), serde_json::Value::String(ua.clone()));
        }
        data.insert(
            "last_updated".into(),
            serde_json::Value::String(now.to_rfc3339()),
        );
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
        self.create_with_metadata(pool, user, site, SessionMetadata::default())
            .await
    }

    pub async fn create_with_metadata(
        &self,
        pool: &orm::DatabasePool,
        user: String,
        site: String,
        metadata: SessionMetadata,
    ) -> Result<Session> {
        let now = Utc::now();
        let expires = now + chrono::Duration::hours(24);
        let mut data = HashMap::new();
        data.insert(
            "creation".into(),
            serde_json::Value::String(now.to_rfc3339()),
        );
        metadata.merge_into(&mut data, now);

        let session = Session {
            id: uuid::Uuid::new_v4().to_string(),
            user: user.clone(),
            site: site.clone(),
            created_at: now,
            expires_at: expires,
            data,
        };

        let data_json = serde_json::to_string(&session.data)?;
        let sql = match pool.dialect() {
            "postgres" => {
                r#"
                INSERT INTO __kiff_sessions (id, "user", site, created_at, expires_at, data)
                VALUES ($1, $2, $3, $4, $5, $6)
            "#
            }
            _ => {
                r#"
                INSERT INTO __kiff_sessions (id, user, site, created_at, expires_at, data)
                VALUES (?, ?, ?, ?, ?, ?)
            "#
            }
        };
        pool.execute_sql(
            sql,
            vec![
                serde_json::Value::String(session.id.clone()),
                serde_json::Value::String(user.clone()),
                serde_json::Value::String(site),
                serde_json::Value::String(now.to_rfc3339()),
                serde_json::Value::String(expires.to_rfc3339()),
                serde_json::Value::String(data_json.clone()),
            ],
        )
        .await?;

        // Mirror into tabSessions for full Frappe compatibility.
        self.write_tab_sessions(pool, &session.id, &user, &session.data, metadata)
            .await?;

        Ok(session)
    }

    pub async fn get(&self, pool: &orm::DatabasePool, session_id: &str) -> Result<Option<Session>> {
        let sql = match pool.dialect() {
            "postgres" => "SELECT * FROM __kiff_sessions WHERE id = $1 LIMIT 1",
            _ => "SELECT * FROM __kiff_sessions WHERE id = ? LIMIT 1",
        };
        let rows = pool
            .execute_sql(sql, vec![serde_json::Value::String(session_id.into())])
            .await?;
        let mut row = match rows.into_iter().next() {
            Some(r) => r,
            None => return Ok(None),
        };

        let expires_str = row
            .remove("expires_at")
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default();
        let expires: DateTime<Utc> = expires_str
            .parse()
            .map_err(|e| RuntimeError::Validation(format!("invalid expires_at: {}", e)))?;

        if Utc::now() > expires {
            // Clean up expired session
            let _ = self.delete(pool, session_id).await;
            return Ok(None);
        }

        let data_json = row
            .remove("data")
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_else(|| "{}".into());
        let data: HashMap<String, serde_json::Value> =
            serde_json::from_str(&data_json).unwrap_or_default();

        Ok(Some(Session {
            id: row
                .remove("id")
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default(),
            user: row
                .remove("user")
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default(),
            site: row
                .remove("site")
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default(),
            created_at: row
                .remove("created_at")
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
        pool.execute_sql(sql, vec![serde_json::Value::String(session_id.into())])
            .await?;

        // Keep the Frappe mirror table in sync.
        let mirror_sql = "DELETE FROM \"tabSessions\" WHERE sid = ?";
        pool.execute_sql(
            mirror_sql,
            vec![serde_json::Value::String(session_id.into())],
        )
        .await?;
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
        pool.execute_sql(
            sql,
            vec![
                serde_json::Value::String(data_json.clone()),
                serde_json::Value::String(session_id.into()),
            ],
        )
        .await?;

        // Refresh the mirror row's sessiondata blob. ip/status/last_updated are
        // not changed by this helper; use `refresh_metadata` for that.
        let mirror_sql = match pool.dialect() {
            "postgres" => "UPDATE \"tabSessions\" SET sessiondata = $1 WHERE sid = $2",
            _ => "UPDATE \"tabSessions\" SET sessiondata = ? WHERE sid = ?",
        };
        pool.execute_sql(
            mirror_sql,
            vec![
                serde_json::Value::String(data_json),
                serde_json::Value::String(session_id.into()),
            ],
        )
        .await?;
        Ok(())
    }

    /// Refresh client metadata on an existing session. Called on every
    /// authenticated request so that `last_updated` stays current.
    pub async fn refresh_metadata(
        &self,
        pool: &orm::DatabasePool,
        session_id: &str,
        metadata: SessionMetadata,
    ) -> Result<()> {
        let now = Utc::now();

        // Read current data so we don't clobber other fields.
        let session = match self.get(pool, session_id).await? {
            Some(s) => s,
            None => return Ok(()),
        };
        let mut data = session.data;
        metadata.merge_into(&mut data, now);

        let data_json = serde_json::to_string(&data)?;
        let kiff_sql = match pool.dialect() {
            "postgres" => "UPDATE __kiff_sessions SET data = $1 WHERE id = $2",
            _ => "UPDATE __kiff_sessions SET data = ? WHERE id = ?",
        };
        pool.execute_sql(
            kiff_sql,
            vec![
                serde_json::Value::String(data_json.clone()),
                serde_json::Value::String(session_id.into()),
            ],
        )
        .await?;

        let ip = data
            .get("session_ip")
            .and_then(|v| v.as_str().map(String::from));
        self.write_tab_sessions(
            pool,
            session_id,
            &session.user,
            &data,
            SessionMetadata { ip, ..metadata },
        )
        .await?;
        Ok(())
    }

    /// Upsert a row in `tabSessions` from the canonical Kiff session data.
    async fn write_tab_sessions(
        &self,
        pool: &orm::DatabasePool,
        sid: &str,
        user: &str,
        data: &HashMap<String, serde_json::Value>,
        metadata: SessionMetadata,
    ) -> Result<()> {
        let now = Utc::now();
        let ip = metadata
            .ip
            .or_else(|| {
                data.get("session_ip")
                    .and_then(|v| v.as_str().map(String::from))
            })
            .unwrap_or_default();

        // `tabSessions.sessiondata` mirrors `__kiff_sessions.data`. Frappe's
        // `User.active_sessions` parses this JSON and reads `session_ip`,
        // `user_agent`, `last_updated`, and `creation` directly.
        let sessiondata_json = serde_json::to_string(data)?;

        // Upsert the mirror row using dialect-specific syntax. We keep both
        // `ip`/`last_updated` (requested by the Kiff workstream) and
        // `ipaddress`/`lastupdate` (the actual Frappe column names) so existing
        // Frappe Python code such as `frappe.sessions` continues to work.
        let (sql, params): (&str, Vec<serde_json::Value>) = match pool.dialect() {
            "postgres" => (
                r#"
                INSERT INTO "tabSessions" (sid, "user", sessiondata, ip, last_updated, ipaddress, lastupdate, status, creation)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                ON CONFLICT (sid) DO UPDATE
                SET "user" = EXCLUDED."user",
                    sessiondata = EXCLUDED.sessiondata,
                    ip = EXCLUDED.ip,
                    last_updated = EXCLUDED.last_updated,
                    ipaddress = EXCLUDED.ipaddress,
                    lastupdate = EXCLUDED.lastupdate,
                    status = EXCLUDED.status
            "#,
                vec![
                    serde_json::Value::String(sid.into()),
                    serde_json::Value::String(user.into()),
                    serde_json::Value::String(sessiondata_json),
                    serde_json::Value::String(ip.clone()),
                    serde_json::Value::String(now.to_rfc3339()),
                    serde_json::Value::String(ip.clone()),
                    serde_json::Value::String(now.to_rfc3339()),
                    serde_json::Value::String("Active".into()),
                    serde_json::Value::String(
                        data.get("creation")
                            .and_then(|v| v.as_str().map(String::from))
                            .unwrap_or_else(|| now.to_rfc3339()),
                    ),
                ],
            ),
            _ => (
                r#"
                INSERT OR REPLACE INTO "tabSessions" (sid, user, sessiondata, ip, last_updated, ipaddress, lastupdate, status, creation)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
                vec![
                    serde_json::Value::String(sid.into()),
                    serde_json::Value::String(user.into()),
                    serde_json::Value::String(sessiondata_json),
                    serde_json::Value::String(ip.clone()),
                    serde_json::Value::String(now.to_rfc3339()),
                    serde_json::Value::String(ip),
                    serde_json::Value::String(now.to_rfc3339()),
                    serde_json::Value::String("Active".into()),
                    serde_json::Value::String(
                        data.get("creation")
                            .and_then(|v| v.as_str().map(String::from))
                            .unwrap_or_else(|| now.to_rfc3339()),
                    ),
                ],
            ),
        };
        pool.execute_sql(sql, params).await?;

        Ok(())
    }
}

impl Default for SessionStore {
    fn default() -> Self {
        Self::new()
    }
}
