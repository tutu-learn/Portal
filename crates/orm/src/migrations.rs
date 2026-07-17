use crate::pool::DatabasePool;
use error::Result;
use tracing::info;

pub struct Migrator;

impl Migrator {
    pub async fn run(pool: &DatabasePool) -> Result<()> {
        info!("running migrations");

        // Dialect-specific CREATE TABLE for migrations tracking
        let init_sql = match pool.dialect() {
            "postgres" => {
                r#"
                CREATE TABLE IF NOT EXISTS __kiff_migrations (
                    id SERIAL PRIMARY KEY,
                    name TEXT NOT NULL UNIQUE,
                    applied_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
                )
            "#
            }
            _ => {
                r#"
                CREATE TABLE IF NOT EXISTS __kiff_migrations (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    name TEXT NOT NULL UNIQUE,
                    applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                )
            "#
            }
        };
        pool.execute_sql(init_sql, vec![]).await?;

        // Session table
        let session_sql = match pool.dialect() {
            "postgres" => {
                r#"
                CREATE TABLE IF NOT EXISTS __kiff_sessions (
                    id TEXT PRIMARY KEY,
                    user TEXT NOT NULL,
                    site TEXT NOT NULL,
                    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    expires_at TIMESTAMPTZ NOT NULL,
                    data JSONB NOT NULL DEFAULT '{}'
                )
            "#
            }
            _ => {
                r#"
                CREATE TABLE IF NOT EXISTS __kiff_sessions (
                    id TEXT PRIMARY KEY,
                    user TEXT NOT NULL,
                    site TEXT NOT NULL,
                    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    expires_at TEXT NOT NULL,
                    data TEXT NOT NULL DEFAULT '{}'
                )
            "#
            }
        };
        pool.execute_sql(session_sql, vec![]).await?;

        // Frappe-compatible Sessions mirror table.
        // `__kiff_sessions` remains the canonical session store; `tabSessions`
        // is kept in sync so that Frappe's `User.active_sessions` and any
        // existing queries against `tabSessions` continue to work unchanged.
        let frappe_sessions_sql = match pool.dialect() {
            "postgres" => {
                r#"
                CREATE TABLE IF NOT EXISTS "tabSessions" (
                    sid TEXT PRIMARY KEY,
                    "user" TEXT NOT NULL,
                    sessiondata JSONB NOT NULL DEFAULT '{}',
                    ip TEXT,
                    last_updated TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    ipaddress TEXT,
                    lastupdate TIMESTAMPTZ,
                    status TEXT NOT NULL DEFAULT 'Active',
                    creation TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
                )
            "#
            }
            _ => {
                r#"
                CREATE TABLE IF NOT EXISTS "tabSessions" (
                    sid TEXT PRIMARY KEY,
                    user TEXT NOT NULL,
                    sessiondata TEXT NOT NULL DEFAULT '{}',
                    ip TEXT,
                    last_updated TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    ipaddress TEXT,
                    lastupdate TEXT,
                    status TEXT NOT NULL DEFAULT 'Active',
                    creation TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                )
            "#
            }
        };
        pool.execute_sql(frappe_sessions_sql, vec![]).await?;

        // Queue table
        let queue_sql = match pool.dialect() {
            "postgres" => {
                r#"
                CREATE TABLE IF NOT EXISTS __kiff_queue (
                    id TEXT PRIMARY KEY,
                    method TEXT NOT NULL,
                    queue TEXT NOT NULL,
                    kwargs JSONB NOT NULL DEFAULT '{}',
                    status TEXT NOT NULL DEFAULT 'queued',
                    site TEXT NOT NULL,
                    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    error TEXT
                )
            "#
            }
            _ => {
                r#"
                CREATE TABLE IF NOT EXISTS __kiff_queue (
                    id TEXT PRIMARY KEY,
                    method TEXT NOT NULL,
                    queue TEXT NOT NULL,
                    kwargs TEXT NOT NULL DEFAULT '{}',
                    status TEXT NOT NULL DEFAULT 'queued',
                    site TEXT NOT NULL,
                    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    error TEXT
                )
            "#
            }
        };
        pool.execute_sql(queue_sql, vec![]).await?;

        // Queue index
        pool.execute_sql(
            "CREATE INDEX IF NOT EXISTS idx_queue_status ON __kiff_queue(queue, status, created_at)",
            vec![],
        ).await?;

        // Permission table (simplified DocPerm)
        let perm_sql = match pool.dialect() {
            "postgres" => {
                r#"
                CREATE TABLE IF NOT EXISTS __kiff_docperm (
                    id SERIAL PRIMARY KEY,
                    parent TEXT NOT NULL,
                    role TEXT NOT NULL,
                    permlevel INTEGER NOT NULL DEFAULT 0,
                    "read" INTEGER NOT NULL DEFAULT 0,
                    "write" INTEGER NOT NULL DEFAULT 0,
                    "create" INTEGER NOT NULL DEFAULT 0,
                    "delete" INTEGER NOT NULL DEFAULT 0,
                    "submit" INTEGER NOT NULL DEFAULT 0,
                    "cancel" INTEGER NOT NULL DEFAULT 0,
                    if_owner INTEGER NOT NULL DEFAULT 0
                )
            "#
            }
            _ => {
                r#"
                CREATE TABLE IF NOT EXISTS __kiff_docperm (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    parent TEXT NOT NULL,
                    role TEXT NOT NULL,
                    permlevel INTEGER NOT NULL DEFAULT 0,
                    "read" INTEGER NOT NULL DEFAULT 0,
                    "write" INTEGER NOT NULL DEFAULT 0,
                    "create" INTEGER NOT NULL DEFAULT 0,
                    "delete" INTEGER NOT NULL DEFAULT 0,
                    "submit" INTEGER NOT NULL DEFAULT 0,
                    "cancel" INTEGER NOT NULL DEFAULT 0,
                    if_owner INTEGER NOT NULL DEFAULT 0
                )
            "#
            }
        };
        pool.execute_sql(perm_sql, vec![]).await?;

        // Bearer-token table for Kiff Logger external ingest.
        let logger_tokens_sql = match pool.dialect() {
            "postgres" => {
                r#"
                CREATE TABLE IF NOT EXISTS __kiff_logger_tokens (
                    name TEXT PRIMARY KEY,
                    token_name TEXT NOT NULL,
                    token_hash TEXT NOT NULL,
                    "user" TEXT NOT NULL,
                    role TEXT NOT NULL DEFAULT 'Kiff Logs',
                    enabled INTEGER NOT NULL DEFAULT 1,
                    description TEXT,
                    last_used_at TIMESTAMPTZ,
                    creation TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    modified TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    modified_by TEXT NOT NULL DEFAULT 'Administrator',
                    owner TEXT NOT NULL DEFAULT 'Administrator',
                    docstatus INTEGER NOT NULL DEFAULT 0
                )
            "#
            }
            _ => {
                r#"
                CREATE TABLE IF NOT EXISTS __kiff_logger_tokens (
                    name TEXT PRIMARY KEY,
                    token_name TEXT NOT NULL,
                    token_hash TEXT NOT NULL,
                    "user" TEXT NOT NULL,
                    role TEXT NOT NULL DEFAULT 'Kiff Logs',
                    enabled INTEGER NOT NULL DEFAULT 1,
                    description TEXT,
                    last_used_at TEXT,
                    creation TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    modified TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    modified_by TEXT NOT NULL DEFAULT 'Administrator',
                    owner TEXT NOT NULL DEFAULT 'Administrator',
                    docstatus INTEGER NOT NULL DEFAULT 0
                )
            "#
            }
        };
        pool.execute_sql(logger_tokens_sql, vec![]).await?;

        let migrations = vec![
            ("001_baseline_schema", "SELECT 1"),
            (
                "002_docperm_extra_columns",
                r#"
                ALTER TABLE __kiff_docperm ADD COLUMN "select" INTEGER NOT NULL DEFAULT 0;
                ALTER TABLE __kiff_docperm ADD COLUMN "report" INTEGER NOT NULL DEFAULT 0;
                ALTER TABLE __kiff_docperm ADD COLUMN "export" INTEGER NOT NULL DEFAULT 0;
                ALTER TABLE __kiff_docperm ADD COLUMN "import" INTEGER NOT NULL DEFAULT 0;
                ALTER TABLE __kiff_docperm ADD COLUMN "share" INTEGER NOT NULL DEFAULT 0;
                ALTER TABLE __kiff_docperm ADD COLUMN "print" INTEGER NOT NULL DEFAULT 0;
                ALTER TABLE __kiff_docperm ADD COLUMN "email" INTEGER NOT NULL DEFAULT 0;
                "#,
            ),
            (
                "003_remove_wildcard_docperms",
                r#"DELETE FROM __kiff_docperm WHERE parent = '*'"#,
            ),
            (
                "004_docperm_mask_amend_columns",
                r#"
                ALTER TABLE __kiff_docperm ADD COLUMN "mask" INTEGER NOT NULL DEFAULT 0;
                ALTER TABLE __kiff_docperm ADD COLUMN "amend" INTEGER NOT NULL DEFAULT 0;
                "#,
            ),
            (
                "005_fieldperm_table",
                r#"
                CREATE TABLE IF NOT EXISTS __kiff_fieldperm (
                    name TEXT PRIMARY KEY,
                    parent TEXT NOT NULL,
                    fieldname TEXT NOT NULL,
                    permlevel INTEGER NOT NULL DEFAULT 0,
                    role TEXT NOT NULL,
                    "read" INTEGER NOT NULL DEFAULT 0,
                    "write" INTEGER NOT NULL DEFAULT 0,
                    creation TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    modified TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );
                CREATE INDEX IF NOT EXISTS idx_fieldperm_parent_field ON __kiff_fieldperm(parent, fieldname);
                CREATE INDEX IF NOT EXISTS idx_fieldperm_role ON __kiff_fieldperm(role);
                "#,
            ),
            (
                "006_sod_table",
                r#"
                CREATE TABLE IF NOT EXISTS __kiff_sod (
                    name TEXT PRIMARY KEY,
                    doctype TEXT NOT NULL,
                    role_a TEXT NOT NULL,
                    role_b TEXT NOT NULL,
                    creation TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    modified TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );
                CREATE INDEX IF NOT EXISTS idx_sod_doctype ON __kiff_sod(doctype);
                "#,
            ),
            // These columns are now part of the Strongroom DocType JSON fixtures and
            // are created by doctype_sync before migrations run. Keep the migration
            // records so existing databases do not re-apply them, but use no-op SQL
            // so fresh databases do not fail with "duplicate column" errors.
            ("007_journal_entry_line_tb_transfer_id", "SELECT 1"),
            ("008_trust_transaction_tb_transfer_id", "SELECT 1"),
            ("009_invoice_settlement_columns", "SELECT 1"),
            // restrict_to_domain is part of the metadata table layout created by
            // doctype_sync::create_metadata_tables, which also adds it to older
            // databases via add_column_if_missing. The doctype table does not
            // exist yet when migrations run on a fresh site, so keep the
            // migration record but use no-op SQL — same as 007-009 above.
            ("010_doctype_restrict_to_domain", "SELECT 1"),
        ];

        for (name, sql) in migrations {
            let exists = Self::is_applied(pool, name).await?;
            if exists {
                continue;
            }
            info!("applying migration: {}", name);
            pool.execute_sql(sql, vec![]).await?;
            Self::record(pool, name).await?;
        }

        info!("migrations complete");
        Ok(())
    }

    async fn is_applied(pool: &DatabasePool, name: &str) -> Result<bool> {
        let sql = "SELECT 1 FROM __kiff_migrations WHERE name = ? LIMIT 1";
        let rows = pool
            .execute_sql(sql, vec![serde_json::Value::String(name.into())])
            .await?;
        Ok(!rows.is_empty())
    }

    async fn record(pool: &DatabasePool, name: &str) -> Result<()> {
        let sql = match pool.dialect() {
            "postgres" => {
                "INSERT INTO __kiff_migrations (name, applied_at) VALUES ($1, CURRENT_TIMESTAMP)"
            }
            _ => "INSERT INTO __kiff_migrations (name, applied_at) VALUES (?, datetime('now'))",
        };
        pool.execute_sql(sql, vec![serde_json::Value::String(name.into())])
            .await?;
        Ok(())
    }
}
