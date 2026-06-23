use std::sync::atomic::{AtomicUsize, Ordering};

static DB_COUNTER: AtomicUsize = AtomicUsize::new(0);

pub fn temp_db_path() -> String {
    let n = DB_COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("/tmp/kiff_test_{}.db", n)
}

pub async fn setup_test_db() -> error::Result<orm::DatabasePool> {
    let path = temp_db_path();
    let _ = std::fs::remove_file(&path);
    let pool = orm::DatabasePool::connect_sqlite(&path).await?;
    orm::migrations::Migrator::run(&pool).await?;
    orm::doctype_sync::sync_all(&pool, vec![], vec![], vec![]).await?;
    Ok(pool)
}

pub fn teardown_test_db(path: &str) {
    let _ = std::fs::remove_file(path);
}

pub async fn create_doctype_table(pool: &orm::DatabasePool, doctype: &str) -> error::Result<()> {
    let table = doctype.to_lowercase().replace(" ", "_");
    let table = table.strip_prefix("tab").unwrap_or(&table);
    let sql = format!(
        r#"CREATE TABLE IF NOT EXISTS "{}" (
            name TEXT PRIMARY KEY,
            owner TEXT NOT NULL DEFAULT 'Administrator',
            creation TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            modified TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            docstatus INTEGER NOT NULL DEFAULT 0,
            title TEXT,
            description TEXT,
            status TEXT
        )"#,
        table
    );
    pool.execute_sql(&sql, vec![]).await?;
    Ok(())
}

pub fn build_app_state(pool: orm::DatabasePool) -> http::AppState {
    use std::sync::Arc;
    use dashmap::DashMap;

    let pools = Arc::new(DashMap::new());
    pools.insert("test_site".into(), pool);

    http::AppState {
        config: Arc::new(config::RuntimeConfig::default()),
        site_manager: Arc::new(config::SiteManager::default()),
        pools,
        sessions: Arc::new(session::SessionStore::new()),
        permissions: Arc::new(permissions::PermissionEngine::new()),
        metadata: Arc::new(metadata::Meta::new()),
        pubsub: Arc::new(queue::PubSub::new()),
        translator: Arc::new(sql_translator::SqlTranslator::new(sql_translator::TargetDialect::Sqlite)),
        rust_apps: rust_apps_core::RustAppRegistry::default(),
        logger: Arc::new(std::sync::OnceLock::new()),
    }
}
