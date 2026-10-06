use crate::domain::{DbDomain, sharding_enabled};
use crate::pool::DatabasePool;
use error::{Result, RuntimeError};
use sqlx::{Connection, Row};
use std::path::Path;
use tracing::{info, warn};

/// Migrate an existing single-DB site to per-domain SQLite files.
///
/// When `KIFF_SQLITE_SHARDED=1` is set on a site that previously lived in a
/// single `site.db`, the heavy tables are still in `site.db`. This helper
/// attaches each domain DB, copies the schema and data for tables that belong
/// in that domain, drops the originals from `site.db`, and detaches.
///
/// The site should be offline while this runs. A WAL checkpoint is issued
/// before and after so the resulting files are self-contained.
pub async fn migrate_site_to_sharded(site_path: &Path) -> Result<()> {
    if !sharding_enabled() {
        return Err(RuntimeError::Config(
            "sharding is disabled; set KIFF_SQLITE_SHARDED=1 to migrate".into(),
        ));
    }

    info!("migrating site at {:?} to sharded SQLite", site_path);

    let core_pool = DatabasePool::from_raw(
        DatabasePool::connect_sqlite_domain(site_path, DbDomain::Core).await?,
        DbDomain::Core,
    );

    // Ensure everything is in the main DB file before we start moving tables.
    core_pool
        .execute_sql("PRAGMA wal_checkpoint(RESTART)", vec![])
        .await?;

    let tables = core_pool
        .execute_sql(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
            vec![],
        )
        .await?;

    for mut row in tables {
        let table = row
            .remove("name")
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default();
        if table.is_empty() {
            continue;
        }
        let domain = DbDomain::for_table(&table);
        if domain == DbDomain::Core {
            continue;
        }

        migrate_table_to_domain(&core_pool, site_path, &table, domain).await?;
    }

    core_pool
        .execute_sql("PRAGMA wal_checkpoint(RESTART)", vec![])
        .await?;

    info!("sharded migration complete for {:?}", site_path);
    Ok(())
}

async fn migrate_table_to_domain(
    core_pool: &DatabasePool,
    site_path: &Path,
    table: &str,
    domain: DbDomain,
) -> Result<()> {
    let domain_path = domain.file_path(site_path);
    let db_url = domain_path.to_string_lossy().to_string();

    // Use a single dedicated SQLite connection for the whole ATTACH/copy/drop
    // dance. Connection pools keep idle readers around and can make ATTACH of
    // another WAL-mode file fail with "database is locked" inside a
    // transaction, so we bypass the pool for the migration step itself.
    let mut conn = connect_direct(&domain_path).await?;

    // Detach first in case a previous interrupted run left it attached.
    let _ = sqlx::query("DETACH DATABASE core_db")
        .execute(&mut conn)
        .await;

    // ATTACH the core DB (site.db) to the domain connection. This lets us
    // read the schema/data from site.db and write directly into the domain DB
    // without moving data through the Rust process.
    let core_path = DbDomain::Core.file_path(site_path);
    sqlx::query(&format!(
        "ATTACH DATABASE '{}' AS core_db",
        core_path.display()
    ))
    .execute(&mut conn)
    .await
    .map_err(|e| RuntimeError::Config(format!("attach site.db for {}: {}", table, e)))?;

    // If the table already exists in the domain DB, it was likely created by a
    // sharded-runtime boot while site.db still held the data. If it is empty,
    // backfill from site.db; if it already has rows, assume the migration is
    // already done.
    let existing: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?",
    )
    .bind(table)
    .fetch_one(&mut conn)
    .await
    .map_err(|e| RuntimeError::Config(format!("check existing {}: {}", table, e)))?;

    let domain_has_data: i64 = if existing > 0 {
        sqlx::query_scalar(&format!("SELECT COUNT(*) FROM \"{}\"", table))
            .fetch_one(&mut conn)
            .await
            .map_err(|e| RuntimeError::Config(format!("count rows in domain {}: {}", table, e)))?
    } else {
        0
    };

    if existing > 0 && domain_has_data > 0 {
        warn!(
            table = %table,
            domain = ?domain,
            rows = domain_has_data,
            "table already populated in domain DB, skipping"
        );
        let _ = sqlx::query("DETACH DATABASE core_db").execute(&mut conn).await;
        return Ok(());
    }

    info!("moving table {} to {:?}", table, domain);

    if existing > 0 && domain_has_data == 0 {
        // The runtime may have created an empty table with a newer schema.
        // Drop it so we can recreate from site.db's schema and copy the data.
        sqlx::query(&format!("DROP TABLE \"{}\"", table))
            .execute(&mut conn)
            .await
            .map_err(|e| RuntimeError::Config(format!("drop stale empty {}: {}", table, e)))?;
    }

    // Recreate the table schema in the domain DB from site.db's definition.
    let create_rows = sqlx::query(
        "SELECT sql FROM core_db.sqlite_master WHERE type = 'table' AND name = ?",
    )
    .bind(table)
    .fetch_all(&mut conn)
    .await
    .map_err(|e| RuntimeError::Config(format!("read create sql for {}: {}", table, e)))?;

    let create_sql = create_rows
        .into_iter()
        .next()
        .and_then(|r| r.try_get::<String, _>("sql").ok())
        .ok_or_else(|| RuntimeError::Config(format!("missing CREATE TABLE for {}", table)))?;

    // The direct connection is already on the domain DB, so the CREATE can
    // be used as-is.
    sqlx::query(&create_sql)
        .execute(&mut conn)
        .await
        .map_err(|e| RuntimeError::Config(format!("create {} in domain db: {}", table, e)))?;

    // Copy data from the attached core DB into the domain DB table.
    sqlx::query(&format!(
        "INSERT INTO \"{}\" SELECT * FROM core_db.\"{}\"",
        table, table
    ))
    .execute(&mut conn)
    .await
    .map_err(|e| RuntimeError::Config(format!("copy data for {}: {}", table, e)))?;

    // Recreate indexes in the domain DB.
    let index_rows = sqlx::query(
        "SELECT name, sql FROM core_db.sqlite_master WHERE type = 'index' AND tbl_name = ?",
    )
    .bind(table)
    .fetch_all(&mut conn)
    .await
    .map_err(|e| RuntimeError::Config(format!("read indexes for {}: {}", table, e)))?;

    for r in index_rows {
        let index_name = r.try_get::<String, _>("name").unwrap_or_default();
        let index_sql = r.try_get::<String, _>("sql").ok();
        if index_name.is_empty() {
            continue;
        }
        // SQLite creates implicit indexes for UNIQUE constraints; their sql is
        // NULL and they are recreated by the CREATE TABLE above.
        if let Some(sql) = index_sql {
            sqlx::query(&sql)
                .execute(&mut conn)
                .await
                .map_err(|e| RuntimeError::Config(format!("create index {}: {}", index_name, e)))?;
        }
    }

    // Drop the original table from core.
    sqlx::query(&format!("DROP TABLE core_db.\"{}\"", table))
        .execute(&mut conn)
        .await
        .map_err(|e| RuntimeError::Config(format!("drop {} from site.db: {}", table, e)))?;

    let _ = sqlx::query("DETACH DATABASE core_db").execute(&mut conn).await;

    // Checkpoint the domain DB so the WAL is merged into the main file.
    sqlx::query("PRAGMA wal_checkpoint(RESTART)")
        .execute(&mut conn)
        .await
        .map_err(|e| RuntimeError::Config(format!("checkpoint domain {}: {}", table, e)))?;

    drop(conn);

    // Also checkpoint the core DB so site.db is compact after the DROP.
    core_pool
        .execute_sql("PRAGMA wal_checkpoint(RESTART)", vec![])
        .await?;

    info!("moved table {} to {:?} ({})", table, domain, db_url);
    Ok(())
}

async fn connect_direct(path: &Path) -> Result<sqlx::SqliteConnection> {
    use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous};
    use std::str::FromStr;

    let opts = SqliteConnectOptions::from_str(&format!(
        "sqlite:{}?mode=rwc",
        path.to_string_lossy()
    ))
    .map_err(|e| RuntimeError::Config(format!("invalid sqlite url: {}", e)))?
    .journal_mode(SqliteJournalMode::Wal)
    .synchronous(SqliteSynchronous::Normal)
    .busy_timeout(std::time::Duration::from_secs(30));

    sqlx::SqliteConnection::connect_with(&opts)
        .await
        .map_err(|e| RuntimeError::Config(format!("connect direct to {:?}: {}", path, e)))
}
