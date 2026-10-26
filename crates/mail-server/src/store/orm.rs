//! ORM-backed implementation of `MailStore`.

use super::{Account, Domain, FetchQuery, FlagMode, Folder, MailStore, MailStoreError, Message, SearchCriteria};
use async_trait::async_trait;
use chrono::Utc;
use orm::DatabasePool;
use serde_json::Value;
use std::collections::HashMap;

/// Mail store backed by the project's ORM/SQLite layer.
#[derive(Clone)]
pub struct OrmMailStore {
    pub(crate) pool: DatabasePool,
}

impl OrmMailStore {
    pub fn new(pool: DatabasePool) -> Self {
        Self { pool }
    }

    /// Normalize `user@domain` or `username` to an e-mail address using the
    /// default domain when no domain part is present.
    async fn normalize_username(&self, username: &str) -> Result<String, MailStoreError> {
        if username.contains('@') {
            return Ok(username.to_lowercase());
        }
        let default_domain = self.default_domain().await?;
        Ok(format!("{}@{}", username.to_lowercase(), default_domain))
    }

    async fn default_domain(&self) -> Result<String, MailStoreError> {
        let rows = self
            .pool
            .execute_sql(
                r#"SELECT name FROM "sebrusmail_domain" WHERE is_default = 1 AND enabled = 1 LIMIT 1"#,
                vec![],
            )
            .await?;
        if let Some(row) = rows.into_iter().next() {
            if let Some(d) = row.get("name").and_then(|v| v.as_str()) {
                return Ok(d.to_string());
            }
        }
        let rows = self
            .pool
            .execute_sql(
                r#"SELECT name FROM "sebrusmail_domain" WHERE enabled = 1 ORDER BY name LIMIT 1"#,
                vec![],
            )
            .await?;
        rows.into_iter()
            .next()
            .and_then(|r| r.get("name").and_then(|v| v.as_str()).map(String::from))
            .ok_or_else(|| MailStoreError::DomainNotFound("no enabled domain".into()))
    }

    fn row_to_account(&self, mut row: HashMap<String, Value>) -> Option<Account> {
        Some(Account {
            email_address: row.remove("email_address")?.as_str()?.to_string(),
            username: row.remove("username").and_then(|v| v.as_str().map(String::from)),
            domain: row.remove("domain")?.as_str()?.to_string(),
            password_hash: row
                .remove("password_hash")
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default(),
            quota_bytes: row.remove("quota_bytes")?.as_i64().unwrap_or(0),
            enabled: row.remove("enabled")?.as_i64().unwrap_or(0) != 0,
            is_domain_admin: row.remove("is_domain_admin")?.as_i64().unwrap_or(0) != 0,
        })
    }

    fn row_to_domain(&self, mut row: HashMap<String, Value>) -> Option<Domain> {
        Some(Domain {
            name: row.remove("name")?.as_str()?.to_string(),
            enabled: row.remove("enabled")?.as_i64().unwrap_or(0) != 0,
            dkim_selector: row.remove("dkim_selector").and_then(|v| v.as_str().map(String::from)),
            dkim_private_key: row.remove("dkim_private_key").and_then(|v| v.as_str().map(String::from)),
            catch_all_account: row.remove("catch_all_account").and_then(|v| v.as_str().map(String::from)),
        })
    }

    fn row_to_folder(&self, mut row: HashMap<String, Value>) -> Option<Folder> {
        Some(Folder {
            name: row.remove("name")?.as_str()?.to_string(),
            uidvalidity: row.remove("uidvalidity")?.as_i64().unwrap_or(1) as u32,
            uidnext: row.remove("uidnext")?.as_i64().unwrap_or(1) as u32,
            exists: row.remove("exists").and_then(|v| v.as_i64()).unwrap_or(0) as u32,
            recent: row.remove("recent").and_then(|v| v.as_i64()).unwrap_or(0) as u32,
            unseen: row.remove("unseen").and_then(|v| v.as_i64()).unwrap_or(0) as u32,
        })
    }

    fn row_to_message(&self, mut row: HashMap<String, Value>) -> Option<Message> {
        let date_str = row.remove("date")?.as_str()?.to_string();
        let date = date_str.parse().unwrap_or_else(|_| Utc::now());
        Some(Message {
            uid: row.remove("uid")?.as_i64().unwrap_or(0) as u32,
            message_id: row.remove("message_id")?.as_str()?.to_string(),
            subject: row.remove("subject").and_then(|v| v.as_str().map(String::from)).unwrap_or_default(),
            from: row.remove("from").and_then(|v| v.as_str().map(String::from)).unwrap_or_default(),
            to: row.remove("to").and_then(|v| v.as_str().map(String::from)).unwrap_or_default(),
            cc: row.remove("cc").and_then(|v| v.as_str().map(String::from)).unwrap_or_default(),
            date,
            size_bytes: row.remove("size_bytes")?.as_i64().unwrap_or(0),
            flags: parse_flags(row.remove("flags")),
            raw_eml: row
                .remove("raw_eml")
                .and_then(|v| v.as_str().map(|s| s.as_bytes().to_vec()))
                .unwrap_or_default(),
            body_text: row.remove("body_text").and_then(|v| v.as_str().map(String::from)).unwrap_or_default(),
            is_read: row.remove("is_read")?.as_i64().unwrap_or(0) != 0,
        })
    }

    async fn ensure_folder(&self, account: &str, folder: &str) -> Result<(), MailStoreError> {
        let exists = self
            .pool
            .execute_sql(
                r#"SELECT 1 FROM "sebrusmail_folder" WHERE account = ? AND name = ? LIMIT 1"#,
                vec![account.into(), folder.into()],
            )
            .await?;
        if !exists.is_empty() {
            return Ok(());
        }
        let uidvalidity = rand::random::<u32>();
        self.pool
            .execute_sql(
                r#"INSERT INTO "sebrusmail_folder" (name, account, uidvalidity, uidnext)
                   VALUES (?, ?, ?, 0)"#,
                vec![folder.into(), account.into(), (uidvalidity as i64).into()],
            )
            .await?;
        Ok(())
    }
}

fn parse_flags(value: Option<Value>) -> Vec<String> {
    match value {
        Some(Value::String(s)) => serde_json::from_str(&s).unwrap_or_default(),
        Some(Value::Array(a)) => a
            .into_iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        _ => Vec::new(),
    }
}

fn flags_to_json(flags: &[String]) -> Value {
    Value::String(serde_json::to_string(flags).unwrap_or_else(|_| "[]".into()))
}

#[async_trait]
impl MailStore for OrmMailStore {
    async fn authenticate(
        &self,
        username: &str,
        password: &str,
    ) -> Result<Option<Account>, MailStoreError> {
        let email = self.normalize_username(username).await?;
        let rows = self
            .pool
            .execute_sql(
                r#"SELECT * FROM "sebrusmail_account" WHERE email_address = ? AND enabled = 1 LIMIT 1"#,
                vec![email.into()],
            )
            .await?;
        let mut row = match rows.into_iter().next() {
            Some(r) => r,
            None => return Ok(None),
        };
        let hash = row
            .remove("password_hash")
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default();
        if hash.is_empty() {
            return Ok(None);
        }
        // Argon2id verification.
        use argon2::{Argon2, PasswordHash, PasswordVerifier};
        let parsed = match PasswordHash::new(&hash) {
            Ok(p) => p,
            Err(_) => return Ok(None),
        };
        if Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_err()
        {
            return Ok(None);
        }
        Ok(self.row_to_account(row))
    }

    async fn resolve_domain(&self, domain: &str) -> Result<Option<Domain>, MailStoreError> {
        let rows = self
            .pool
            .execute_sql(
                r#"SELECT * FROM "sebrusmail_domain" WHERE name = ? AND enabled = 1 LIMIT 1"#,
                vec![domain.to_lowercase().into()],
            )
            .await?;
        Ok(rows.into_iter().next().and_then(|r| self.row_to_domain(r)))
    }

    async fn resolve_recipient(&self, address: &str) -> Result<Vec<String>, MailStoreError> {
        let address = address.to_lowercase();
        let domain = match address.split_once('@') {
            Some((_, d)) => d.to_string(),
            None => return Err(MailStoreError::InvalidAddress(address)),
        };

        // Direct account match.
        let rows = self
            .pool
            .execute_sql(
                r#"SELECT email_address FROM "sebrusmail_account" WHERE email_address = ? AND enabled = 1 LIMIT 1"#,
                vec![address.clone().into()],
            )
            .await?;
        if !rows.is_empty() {
            return Ok(vec![address]);
        }

        // Alias match.
        let alias_rows = self
            .pool
            .execute_sql(
                r#"SELECT destination_accounts FROM "sebrusmail_alias" WHERE address = ? AND domain = ? AND enabled = 1 LIMIT 1"#,
                vec![address.into(), domain.clone().into()],
            )
            .await?;
        if let Some(row) = alias_rows.into_iter().next() {
            let dests = parse_flags(row.get("destination_accounts").cloned());
            let mut out = Vec::new();
            for d in dests {
                let d = d.to_lowercase();
                if d.contains('@') {
                    out.push(d);
                } else {
                    out.push(format!("{}@{}", d, domain));
                }
            }
            if !out.is_empty() {
                return Ok(out);
            }
        }

        // Catch-all.
        let catch_rows = self
            .pool
            .execute_sql(
                r#"SELECT catch_all_account FROM "sebrusmail_domain" WHERE name = ? AND enabled = 1 AND catch_all_account IS NOT NULL AND catch_all_account != '' LIMIT 1"#,
                vec![domain.clone().into()],
            )
            .await?;
        if let Some(row) = catch_rows.into_iter().next() {
            if let Some(target) = row.get("catch_all_account").and_then(|v| v.as_str()) {
                let target = target.to_lowercase();
                return Ok(vec![if target.contains('@') {
                    target
                } else {
                    format!("{}@{}", target, domain)
                }]);
            }
        }

        Ok(Vec::new())
    }

    async fn append_message(
        &self,
        account_email: &str,
        folder: &str,
        raw: &[u8],
    ) -> Result<u32, MailStoreError> {
        self.ensure_folder(account_email, folder).await?;

        // Increment uidnext atomically.
        let rows = self
            .pool
            .execute_sql(
                r#"UPDATE "sebrusmail_folder" SET uidnext = uidnext + 1
                   WHERE account = ? AND name = ?
                   RETURNING uidnext"#,
                vec![account_email.into(), folder.into()],
            )
            .await?;
        let uid = rows
            .into_iter()
            .next()
            .and_then(|mut r| r.remove("uidnext").and_then(|v| v.as_i64()).map(|i| i as u32))
            .unwrap_or(1);

        // Parse the message for indexed fields.
        let parsed = email::parse_raw_email(raw);
        let now = Utc::now();
        let message_id = parsed
            .as_ref()
            .map(|p| p.message_id.clone())
            .unwrap_or_else(|| format!("<sebrus-{}@generated>", uuid::Uuid::new_v4()));
        let subject = parsed.as_ref().map(|p| p.subject.clone()).unwrap_or_default();
        let from = parsed.as_ref().map(|p| p.sender.clone()).unwrap_or_default();
        let to = parsed
            .as_ref()
            .map(|p| p.recipients.join(", "))
            .unwrap_or_default();
        let date = parsed
            .as_ref()
            .and_then(|p| p.received_at.parse().ok())
            .unwrap_or(now);
        let body_text = parsed.as_ref().map(|p| p.body_text.clone()).unwrap_or_default();
        let raw_str = String::from_utf8_lossy(raw).to_string();
        let name = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();

        self.pool
            .execute_sql(
                r#"INSERT INTO "sebrusmail_message" (
                    name, creation, modified, modified_by, owner, docstatus,
                    account, folder, uid, message_id, subject, "from", "to", cc, date,
                    size_bytes, flags, raw_eml, body_text, is_read
                ) VALUES (?, ?, ?, 'Mail Server', 'Mail Server', 0, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0)"#,
                vec![
                    name.into(),
                    now.clone().into(),
                    now.into(),
                    account_email.into(),
                    folder.into(),
                    (uid as i64).into(),
                    message_id.into(),
                    subject.into(),
                    from.into(),
                    to.into(),
                    "".into(),
                    date.to_rfc3339().into(),
                    (raw.len() as i64).into(),
                    flags_to_json(&[]),
                    raw_str.into(),
                    body_text.into(),
                ],
            )
            .await?;

        // TODO: extract attachments and insert into mail_attachment.

        Ok(uid)
    }

    async fn list_folders(&self, account_email: &str) -> Result<Vec<Folder>, MailStoreError> {
        let rows = self
            .pool
            .execute_sql(
                r#"SELECT name, uidvalidity, uidnext FROM "sebrusmail_folder" WHERE account = ? ORDER BY name"#,
                vec![account_email.into()],
            )
            .await?;
        let mut folders = Vec::new();
        for row in rows {
            if let Some(mut f) = self.row_to_folder(row) {
                // Compute counts.
                let rows = self
                    .pool
                    .execute_sql(
                        r#"SELECT is_read FROM "sebrusmail_message" WHERE account = ? AND folder = ?"#,
                        vec![account_email.into(), f.name.clone().into()],
                    )
                    .await?;
                f.exists = rows.len() as u32;
                f.unseen = rows
                    .iter()
                    .filter(|r| {
                        r.get("is_read")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0)
                            == 0
                    })
                    .count() as u32;
                folders.push(f);
            }
        }
        Ok(folders)
    }

    async fn select_folder(
        &self,
        account_email: &str,
        folder: &str,
    ) -> Result<Folder, MailStoreError> {
        self.ensure_folder(account_email, folder).await?;
        let rows = self
            .pool
            .execute_sql(
                r#"SELECT name, uidvalidity, uidnext FROM "sebrusmail_folder" WHERE account = ? AND name = ? LIMIT 1"#,
                vec![account_email.into(), folder.into()],
            )
            .await?;
        let mut folder = rows
            .into_iter()
            .next()
            .and_then(|r| self.row_to_folder(r))
            .ok_or_else(|| MailStoreError::Other("folder missing".into()))?;
        let rows = self
            .pool
            .execute_sql(
                r#"SELECT is_read FROM "sebrusmail_message" WHERE account = ? AND folder = ?"#,
                vec![account_email.into(), folder.name.clone().into()],
            )
            .await?;
        folder.exists = rows.len() as u32;
        folder.unseen = rows
            .iter()
            .filter(|r| {
                r.get("is_read")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0)
                    == 0
            })
            .count() as u32;
        Ok(folder)
    }

    async fn fetch_messages(
        &self,
        account_email: &str,
        folder: &str,
        uids: &[u32],
        _query: &FetchQuery,
    ) -> Result<Vec<Message>, MailStoreError> {
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders: Vec<String> = uids.iter().map(|_| "?".to_string()).collect();
        let sql = format!(
            r#"SELECT * FROM "sebrusmail_message" WHERE account = ? AND folder = ? AND uid IN ({}) ORDER BY uid"#,
            placeholders.join(", ")
        );
        let mut params: Vec<Value> = vec![account_email.into(), folder.into()];
        for uid in uids {
            params.push((*uid as i64).into());
        }
        let rows = self.pool.execute_sql(&sql, params).await?;
        Ok(rows.into_iter().filter_map(|r| self.row_to_message(r)).collect())
    }

    async fn search_messages(
        &self,
        account_email: &str,
        folder: &str,
        criteria: &SearchCriteria,
    ) -> Result<Vec<u32>, MailStoreError> {
        let (where_clause, params) = match criteria {
            SearchCriteria::All => (
                "account = ? AND folder = ?".to_string(),
                vec![account_email.into(), folder.into()],
            ),
            SearchCriteria::Unseen => (
                "account = ? AND folder = ? AND is_read = 0".to_string(),
                vec![account_email.into(), folder.into()],
            ),
            SearchCriteria::Recent => (
                "account = ? AND folder = ?".to_string(),
                vec![account_email.into(), folder.into()],
            ),
            SearchCriteria::Uid(uids) => {
                let placeholders: Vec<String> = uids.iter().map(|_| "?".to_string()).collect();
                (
                    format!(
                        "account = ? AND folder = ? AND uid IN ({})",
                        placeholders.join(", ")
                    ),
                    {
                        let mut p = vec![account_email.into(), folder.into()];
                        for uid in uids {
                            p.push((*uid as i64).into());
                        }
                        p
                    },
                )
            }
            SearchCriteria::From(s) => (
                "account = ? AND folder = ? AND \"from\" LIKE ?".to_string(),
                vec![
                    account_email.into(),
                    folder.into(),
                    format!("%{s}%").into(),
                ],
            ),
            SearchCriteria::Subject(s) => (
                "account = ? AND folder = ? AND subject LIKE ?".to_string(),
                vec![
                    account_email.into(),
                    folder.into(),
                    format!("%{s}%").into(),
                ],
            ),
        };
        let sql = format!(r#"SELECT uid FROM "sebrusmail_message" WHERE {where_clause} ORDER BY uid"#);
        let rows = self.pool.execute_sql(&sql, params).await?;
        Ok(rows
            .into_iter()
            .filter_map(|mut r| r.remove("uid").and_then(|v| v.as_i64()).map(|i| i as u32))
            .collect())
    }

    async fn set_flags(
        &self,
        account_email: &str,
        folder: &str,
        uids: &[u32],
        flags: &[String],
        mode: FlagMode,
    ) -> Result<(), MailStoreError> {
        if uids.is_empty() {
            return Ok(());
        }

        // Read existing flags, apply change, write back.
        let placeholders: Vec<String> = uids.iter().map(|_| "?".to_string()).collect();
        let sql = format!(
            r#"SELECT uid, flags, is_read FROM "sebrusmail_message" WHERE account = ? AND folder = ? AND uid IN ({})"#,
            placeholders.join(", ")
        );
        let mut params: Vec<Value> = vec![account_email.into(), folder.into()];
        for uid in uids {
            params.push((*uid as i64).into());
        }
        let rows = self.pool.execute_sql(&sql, params).await?;

        for mut row in rows {
            let uid = row
                .remove("uid")
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as u32;
            let mut current: Vec<String> = parse_flags(row.get("flags").cloned());
            match mode {
                FlagMode::Replace => current = flags.to_vec(),
                FlagMode::Add => {
                    for f in flags {
                        if !current.contains(f) {
                            current.push(f.clone());
                        }
                    }
                }
                FlagMode::Remove => {
                    current.retain(|f| !flags.contains(f));
                }
            }
            let is_read = current.iter().any(|f| f.eq_ignore_ascii_case("\\seen"));
            self.pool
                .execute_sql(
                    r#"UPDATE "sebrusmail_message" SET flags = ?, is_read = ? WHERE account = ? AND folder = ? AND uid = ?"#,
                    vec![
                        flags_to_json(&current),
                        (is_read as i64).into(),
                        account_email.into(),
                        folder.into(),
                        (uid as i64).into(),
                    ],
                )
                .await?;
        }
        Ok(())
    }
}
