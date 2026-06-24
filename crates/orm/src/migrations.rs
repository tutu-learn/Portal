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
