//! Framework-level change capture into a local sync outbox.

use crate::protocol::{Action, FileRef};
use error::Result;
use orm::{DocHookRunner, Document};
use std::sync::Arc;
use tracing::debug;
use uuid::Uuid;

/// Tables whose mutations should never be captured for sync.
const SYSTEM_TABLES: &[&str] = &[
    "__kiff_migrations",
    "__kiff_sessions",
    "__kiff_queue",
    "__kiff_docperm",
    "__kiff_fieldperm",
    "__kiff_sod",
    "__kiff_docshare",
    "__kiff_logger_tokens",
    "__kiff_sync_outbox",
    "__kiff_sync_state",
    "tabSessions",
];

/// Captures every document mutation as a row in `__kiff_sync_outbox`.
#[derive(Debug, Clone)]
pub struct SyncOutboxHook {
    site_id: String,
    node_id: String,
    pool: orm::DatabasePool,
}

impl SyncOutboxHook {
    pub fn new(site_id: impl Into<String>, node_id: impl Into<String>, pool: orm::DatabasePool) -> Self {
        Self {
            site_id: site_id.into(),
            node_id: node_id.into(),
            pool,
        }
    }
}

#[async_trait::async_trait]
impl DocHookRunner for SyncOutboxHook {
    async fn run_hook(&self, event: &str, doctype: &str, doc: &Document) -> Result<()> {
        if SYSTEM_TABLES.contains(&doctype) {
            return Ok(());
        }

        let action = match event {
            "after_insert" => Action::Insert,
            "after_save" => Action::Update,
            "after_trash" => Action::Delete,
            _ => return Ok(()),
        };

        debug!(
            site = %self.site_id,
            node = %self.node_id,
            doctype = %doctype,
            name = %doc.name,
            action = %action,
            "capturing sync outbox row"
        );

        let payload_json = serde_json::to_string(&doc)?;

        let files = extract_file_refs(doc);
        let file_refs_json = serde_json::to_string(&files).unwrap_or_else(|_| "[]".into());

        let op_id = Uuid::new_v4().to_string();
        let sql = r#"
            INSERT INTO __kiff_sync_outbox
                (id, site, op_id, lsn, doctype, name, action, payload_json, file_refs_json, created_at)
            VALUES
                (?, ?, ?, NULL, ?, ?, ?, ?, ?, datetime('now'))
        "#;

        self.pool
            .execute_sql(
                sql,
                vec![
                    serde_json::Value::String(Uuid::new_v4().to_string()),
                    serde_json::Value::String(self.site_id.clone()),
                    serde_json::Value::String(op_id),
                    serde_json::Value::String(doctype.into()),
                    serde_json::Value::String(doc.name.clone()),
                    serde_json::Value::String(action.to_string()),
                    serde_json::Value::String(payload_json),
                    serde_json::Value::String(file_refs_json),
                ],
            )
            .await?;

        Ok(())
    }
}

fn extract_file_refs(doc: &Document) -> Vec<FileRef> {
    let mut refs = Vec::new();

    // TODO: once we have DocType metadata available without async, look up
    // field types and only treat Attach/Attach Image fields as file refs.
    // For now, scan string-valued fields that look like site-relative file
    // paths.
    for (key, value) in &doc.fields {
        let Some(path) = value.as_str() else {
            continue;
        };
        if key.starts_with("__") {
            continue;
        }
        if looks_like_file_path(path) {
            refs.push(FileRef {
                hash: String::new(), // populated during upload staging
                size: 0,
                path: path.into(),
                encrypted_key: Vec::new(),
            });
        }
    }
    refs
}

fn looks_like_file_path(value: &str) -> bool {
    value.starts_with("/files/")
        || value.starts_with("/private/files/")
        || value.starts_with("private/files/")
        || value.starts_with("public/files/")
}

/// Install the sync outbox tables for a site.
pub async fn ensure_outbox_tables(pool: &orm::DatabasePool) -> Result<()> {
    let outbox_sql = r#"
        CREATE TABLE IF NOT EXISTS __kiff_sync_outbox (
            id TEXT PRIMARY KEY,
            site TEXT NOT NULL,
            op_id TEXT NOT NULL UNIQUE,
            lsn INTEGER,
            doctype TEXT NOT NULL,
            name TEXT NOT NULL,
            action TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            file_refs_json TEXT NOT NULL DEFAULT '[]',
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            sent_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_sync_outbox_sent
            ON __kiff_sync_outbox(sent_at, created_at);
    "#;

    let state_sql = r#"
        CREATE TABLE IF NOT EXISTS __kiff_sync_state (
            site TEXT PRIMARY KEY,
            last_applied_lsn INTEGER NOT NULL DEFAULT 0,
            last_sent_lsn INTEGER NOT NULL DEFAULT 0,
            node_id TEXT NOT NULL
        );
    "#;

    pool.execute_sql(outbox_sql, vec![]).await?;
    pool.execute_sql(state_sql, vec![]).await?;
    Ok(())
}

/// Register a sync outbox hook for the given site.
pub async fn register(site_id: &str, node_id: &str, pool: &orm::DatabasePool) {
    let hook = SyncOutboxHook::new(site_id, node_id, pool.clone());
    orm::add_hook_runner(Arc::new(hook)).await;
}
