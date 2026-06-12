use crate::pool::DatabasePool;
use error::Result;
use tracing::info;

pub struct Migrator;

impl Migrator {
    pub async fn run(pool: &DatabasePool) -> Result<()> {
        info!("running migrations");

        // Dialect-specific CREATE TABLE for migrations tracking
        let init_sql = match pool.dialect() {
            "postgres" => r#"
                CREATE TABLE IF NOT EXISTS __kiff_migrations (
                    id SERIAL PRIMARY KEY,
                    name TEXT NOT NULL UNIQUE,
                    applied_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
                )
            "#,
            _ => r#"
                CREATE TABLE IF NOT EXISTS __kiff_migrations (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    name TEXT NOT NULL UNIQUE,
                    applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                )
            "#,
        };
        pool.execute_sql(init_sql, vec![]).await?;

        // Session table
        let session_sql = match pool.dialect() {
            "postgres" => r#"
                CREATE TABLE IF NOT EXISTS __kiff_sessions (
                    id TEXT PRIMARY KEY,
                    user TEXT NOT NULL,
                    site TEXT NOT NULL,
                    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    expires_at TIMESTAMPTZ NOT NULL,
                    data JSONB NOT NULL DEFAULT '{}'
                )
            "#,
            _ => r#"
                CREATE TABLE IF NOT EXISTS __kiff_sessions (
                    id TEXT PRIMARY KEY,
                    user TEXT NOT NULL,
                    site TEXT NOT NULL,
                    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    expires_at TEXT NOT NULL,
                    data TEXT NOT NULL DEFAULT '{}'
                )
            "#,
        };
        pool.execute_sql(session_sql, vec![]).await?;

        // Queue table
        let queue_sql = match pool.dialect() {
            "postgres" => r#"
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
            "#,
            _ => r#"
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
            "#,
        };
        pool.execute_sql(queue_sql, vec![]).await?;

        // Queue index
        pool.execute_sql(
            "CREATE INDEX IF NOT EXISTS idx_queue_status ON __kiff_queue(queue, status, created_at)",
            vec![],
        ).await?;

        // Permission table (simplified DocPerm)
        let perm_sql = match pool.dialect() {
            "postgres" => r#"
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
            "#,
            _ => r#"
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
            "#,
        };
        pool.execute_sql(perm_sql, vec![]).await?;

        // Default permissions for Administrator
        let admin_perms = vec![
            ("*", "Administrator", 1, 1, 1, 1, 1, 1, 0),
            ("*", "System Manager", 1, 1, 1, 1, 1, 1, 0),
            ("*", "All", 1, 0, 0, 0, 0, 0, 0),
        ];
        for (parent, role, r, w, c, d, s, cn, owner) in admin_perms {
            let sql = r#"
                INSERT OR IGNORE INTO __kiff_docperm ("parent", "role", "read", "write", "create", "delete", "submit", "cancel", "if_owner")
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#;
            let _ = pool.execute_sql(sql, vec![
                serde_json::Value::String(parent.into()),
                serde_json::Value::String(role.into()),
                serde_json::Value::Number(r.into()),
                serde_json::Value::Number(w.into()),
                serde_json::Value::Number(c.into()),
                serde_json::Value::Number(d.into()),
                serde_json::Value::Number(s.into()),
                serde_json::Value::Number(cn.into()),
                serde_json::Value::Number(owner.into()),
            ]).await;
        }

        let migrations = vec![
            ("001_baseline_schema", "SELECT 1"),
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
        let rows = pool.execute_sql(sql, vec![serde_json::Value::String(name.into())]).await?;
        Ok(!rows.is_empty())
    }

    async fn record(pool: &DatabasePool, name: &str) -> Result<()> {
        let sql = match pool.dialect() {
            "postgres" => "INSERT INTO __kiff_migrations (name, applied_at) VALUES ($1, CURRENT_TIMESTAMP)",
            _ => "INSERT INTO __kiff_migrations (name, applied_at) VALUES (?, datetime('now'))",
        };
        pool.execute_sql(sql, vec![serde_json::Value::String(name.into())]).await?;
        Ok(())
    }
}
