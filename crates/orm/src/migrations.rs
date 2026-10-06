use crate::domain::DbDomain;
use crate::domain_pools::DomainPools;
use crate::pool::DatabasePool;
use error::Result;
use std::sync::Arc;
use tracing::info;

struct Migration {
    name: &'static str,
    domain: DbDomain,
    sql: &'static str,
}

pub struct Migrator;

impl Migrator {
    pub async fn run(pools: &Arc<DomainPools>) -> Result<()> {
        info!("running domain-aware migrations");

        for domain in DbDomain::all() {
            Self::run_for_domain(pools, *domain).await?;
        }

        info!("migrations complete");
        Ok(())
    }

    async fn run_for_domain(pools: &Arc<DomainPools>, domain: DbDomain) -> Result<()> {
        let pool = pools.pool_for(domain);

        // Every domain tracks its own applied migrations so that future
        // domain-specific schema changes can be rolled out independently.
        let init_sql = r#"
            CREATE TABLE IF NOT EXISTS __kiff_migrations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            )
        "#;
        pool.execute_sql(init_sql, vec![]).await?;

        // Core infrastructure tables must exist before the numbered migrations
        // that reference them (e.g. 003_delete_wildcard_docperms).
        if domain == DbDomain::Core {
            Self::ensure_baseline_tables(&pool).await?;
        }

        for migration in Self::migrations() {
            if migration.domain != domain {
                continue;
            }
            let exists = Self::is_applied(&pool, migration.name).await?;
            if exists {
                continue;
            }
            info!("applying migration {} on {:?}", migration.name, domain);
            pool.execute_sql(migration.sql, vec![]).await?;
            Self::record(&pool, migration.name).await?;
        }

        Ok(())
    }

    fn migrations() -> &'static [Migration] {
        &[
            Migration {
                name: "001_baseline_schema",
                domain: DbDomain::Core,
                sql: "SELECT 1",
            },
            Migration {
                name: "002_docperm_extra_columns",
                domain: DbDomain::Core,
                sql: r#"
                ALTER TABLE __kiff_docperm ADD COLUMN "select" INTEGER NOT NULL DEFAULT 0;
                ALTER TABLE __kiff_docperm ADD COLUMN "report" INTEGER NOT NULL DEFAULT 0;
                ALTER TABLE __kiff_docperm ADD COLUMN "export" INTEGER NOT NULL DEFAULT 0;
                ALTER TABLE __kiff_docperm ADD COLUMN "import" INTEGER NOT NULL DEFAULT 0;
                ALTER TABLE __kiff_docperm ADD COLUMN "share" INTEGER NOT NULL DEFAULT 0;
                ALTER TABLE __kiff_docperm ADD COLUMN "print" INTEGER NOT NULL DEFAULT 0;
                ALTER TABLE __kiff_docperm ADD COLUMN "email" INTEGER NOT NULL DEFAULT 0;
                "#,
            },
            Migration {
                name: "003_remove_wildcard_docperms",
                domain: DbDomain::Core,
                sql: r#"DELETE FROM __kiff_docperm WHERE parent = '*'"#,
            },
            Migration {
                name: "004_docperm_mask_amend_columns",
                domain: DbDomain::Core,
                sql: r#"
                ALTER TABLE __kiff_docperm ADD COLUMN "mask" INTEGER NOT NULL DEFAULT 0;
                ALTER TABLE __kiff_docperm ADD COLUMN "amend" INTEGER NOT NULL DEFAULT 0;
                "#,
            },
            Migration {
                name: "005_fieldperm_table",
                domain: DbDomain::Core,
                sql: r#"
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
            },
            Migration {
                name: "006_sod_table",
                domain: DbDomain::Core,
                sql: r#"
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
            },
            // These columns are now part of the Strongroom DocType JSON fixtures and
            // are created by doctype_sync before migrations run. Keep the migration
            // records so existing databases do not re-apply them, but use no-op SQL
            // so fresh databases do not fail with "duplicate column" errors.
            Migration {
                name: "007_journal_entry_line_tb_transfer_id",
                domain: DbDomain::Core,
                sql: "SELECT 1",
            },
            Migration {
                name: "008_trust_transaction_tb_transfer_id",
                domain: DbDomain::Core,
                sql: "SELECT 1",
            },
            Migration {
                name: "009_invoice_settlement_columns",
                domain: DbDomain::Core,
                sql: "SELECT 1",
            },
            // restrict_to_domain is part of the metadata table layout created by
            // doctype_sync::create_metadata_tables, which also adds it to older
            // databases via add_column_if_missing. The doctype table does not
            // exist yet when migrations run on a fresh site, so keep the
            // migration record but use no-op SQL — same as 007-009 above.
            Migration {
                name: "010_doctype_restrict_to_domain",
                domain: DbDomain::Core,
                sql: "SELECT 1",
            },
            Migration {
                name: "011_docshare_table",
                domain: DbDomain::Core,
                sql: r#"
                CREATE TABLE IF NOT EXISTS __kiff_docshare (
                    name TEXT PRIMARY KEY,
                    user TEXT NOT NULL,
                    share_doctype TEXT NOT NULL,
                    share_name TEXT NOT NULL,
                    "read" INTEGER NOT NULL DEFAULT 0,
                    "write" INTEGER NOT NULL DEFAULT 0,
                    "share" INTEGER NOT NULL DEFAULT 0,
                    everyone INTEGER NOT NULL DEFAULT 0,
                    creation TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    modified TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );
                CREATE INDEX IF NOT EXISTS idx_docshare_lookup ON __kiff_docshare(user, share_doctype, share_name);
                CREATE INDEX IF NOT EXISTS idx_docshare_everyone ON __kiff_docshare(everyone, share_doctype, share_name);
                "#,
            },
            Migration {
                name: "012_sync_outbox_tables",
                domain: DbDomain::Core,
                sql: r#"
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

                CREATE TABLE IF NOT EXISTS __kiff_sync_state (
                    site TEXT PRIMARY KEY,
                    last_applied_lsn INTEGER NOT NULL DEFAULT 0,
                    last_sent_lsn INTEGER NOT NULL DEFAULT 0,
                    node_id TEXT NOT NULL
                );
                "#,
            },
        ]
    }

    /// Create core infrastructure tables that numbered migrations assume exist.
    ///
    /// These used to be created unconditionally before numbered migrations. They
    /// are still created up front (and only in the Core domain) so that older
    /// migrations such as `003_remove_wildcard_docperms` can reference them.
    async fn ensure_baseline_tables(pool: &DatabasePool) -> Result<()> {
        pool.execute_sql(
            r#"
            CREATE TABLE IF NOT EXISTS __kiff_sessions (
                id TEXT PRIMARY KEY,
                user TEXT NOT NULL,
                site TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                expires_at TEXT NOT NULL,
                data TEXT NOT NULL DEFAULT '{}'
            )
            "#,
            vec![],
        )
        .await?;

        pool.execute_sql(
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
            "#,
            vec![],
        )
        .await?;

        pool.execute_sql(
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
            "#,
            vec![],
        )
        .await?;

        pool.execute_sql(
            "CREATE INDEX IF NOT EXISTS idx_queue_status ON __kiff_queue(queue, status, created_at)",
            vec![],
        )
        .await?;

        pool.execute_sql(
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
            "#,
            vec![],
        )
        .await?;

        pool.execute_sql(
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
            "#,
            vec![],
        )
        .await?;

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
        let sql = "INSERT INTO __kiff_migrations (name, applied_at) VALUES (?, datetime('now'))";
        pool.execute_sql(sql, vec![serde_json::Value::String(name.into())])
            .await?;
        Ok(())
    }
}
