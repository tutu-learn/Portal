use std::sync::atomic::{AtomicUsize, Ordering};

static DB_COUNTER: AtomicUsize = AtomicUsize::new(0);

pub fn temp_db_path() -> String {
    let n = DB_COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("/tmp/kiff_test_{}.db", n)
}

/// Argon2id hash of the password "admin". Tests log in as Administrator/admin,
/// but `sync_all` generates a random Administrator password for fresh sites, so
/// we overwrite it with this known hash after bootstrapping the test DB.
const ADMIN_PASSWORD_HASH: &str =
    "$argon2id$v=19$m=19456,t=2,p=1$UEWqTMicBrdEJXqPMhP4oA$bR1RecCR37Rw+Spup2ULPNKAZ7H6vZTX4VeqNAfvdkY";

pub async fn setup_test_db() -> error::Result<orm::DatabasePool> {
    let path = temp_db_path();
    let _ = std::fs::remove_file(&path);
    let pool = orm::DatabasePool::connect_sqlite(&path).await?;
    orm::migrations::Migrator::run(&pool).await?;
    orm::doctype_sync::sync_all(&pool, vec![], vec![], vec![], vec![], vec![]).await?;
    set_admin_password(&pool).await?;
    Ok(pool)
}

async fn set_admin_password(pool: &orm::DatabasePool) -> error::Result<()> {
    pool.execute_sql(
        r#"INSERT OR REPLACE INTO "__auth" (name, doctype, fieldname, password, encrypted)
           VALUES ('Administrator', 'User', 'password', ?, 0)"#,
        vec![serde_json::Value::String(ADMIN_PASSWORD_HASH.into())],
    )
    .await?;
    Ok(())
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

    // Register the DocType and its fields in the metadata tables so field-level
    // permlevel enforcement returns the columns the tests rely on.
    pool.execute_sql(
        r#"INSERT OR REPLACE INTO "doctype" (
            name, creation, modified, modified_by, owner, docstatus,
            module, is_submittable, is_tree, istable, track_changes, track_seen, track_views
        ) VALUES (?, datetime('now'), datetime('now'), 'Administrator', 'Administrator', 0,
                  'Core', 0, 0, 0, 0, 0, 0)"#,
        vec![serde_json::Value::String(doctype.into())],
    )
    .await?;

    for (idx, (fieldname, fieldtype, permlevel)) in [
        ("title", "Data", 0),
        ("description", "Text", 0),
        ("status", "Data", 0),
    ]
    .into_iter()
    .enumerate()
    {
        pool.execute_sql(
            r#"INSERT OR REPLACE INTO "docfield" (
                name, creation, modified, modified_by, owner, docstatus,
                parent, fieldname, fieldtype, label, idx, permlevel
            ) VALUES (?, datetime('now'), datetime('now'), 'Administrator', 'Administrator', 0,
                      ?, ?, ?, ?, ?, ?)"#,
            vec![
                serde_json::Value::String(format!("{}-{}", doctype, fieldname)),
                serde_json::Value::String(doctype.into()),
                serde_json::Value::String(fieldname.into()),
                serde_json::Value::String(fieldtype.into()),
                serde_json::Value::String(fieldname.into()),
                serde_json::Value::Number(idx.into()),
                serde_json::Value::Number(permlevel.into()),
            ],
        )
        .await?;
    }
    Ok(())
}

pub async fn grant_permission(
    pool: &orm::DatabasePool,
    doctype: &str,
    role: &str,
    read: bool,
    write: bool,
    create: bool,
    delete: bool,
) -> error::Result<()> {
    pool.execute_sql(
        r#"INSERT OR REPLACE INTO __kiff_docperm (
            parent, role, permlevel, "read", "write", "create", "delete",
            "submit", "cancel", "if_owner", "select", "report", "export", "import",
            "share", "print", "email", "mask", "amend"
        ) VALUES (?, ?, 0, ?, ?, ?, ?, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0)"#,
        vec![
            serde_json::Value::String(doctype.into()),
            serde_json::Value::String(role.into()),
            serde_json::Value::Number((read as i32).into()),
            serde_json::Value::Number((write as i32).into()),
            serde_json::Value::Number((create as i32).into()),
            serde_json::Value::Number((delete as i32).into()),
        ],
    )
    .await?;
    Ok(())
}

pub fn build_app_state(pool: orm::DatabasePool) -> http::AppState {
    build_state_with_apps(pool, vec![Box::new(kiff_logger::KiffLoggerApp)])
}

fn build_state_with_apps(
    pool: orm::DatabasePool,
    apps: Vec<Box<dyn rust_apps_core::RustApp>>,
) -> http::AppState {
    use dashmap::DashMap;
    use std::sync::Arc;

    let pools = Arc::new(DashMap::new());
    pools.insert("test_site".into(), pool);

    let site = config::site::Site::new(
        "test_site".into(),
        std::path::PathBuf::from("/tmp/test_site"),
        config::site::SiteConfig::default(),
    );
    let mut site_manager = config::SiteManager::default();
    site_manager.register_site(site);

    http::AppState {
        config: Arc::new(config::RuntimeConfig::default()),
        site_manager: Arc::new(site_manager),
        pools,
        sessions: Arc::new(session::SessionStore::new()),
        permissions: Arc::new(permissions::PermissionEngine::new()),
        metadata: Arc::new(metadata::Meta::new()),
        pubsub: Arc::new(queue::PubSub::new()),
        translator: Arc::new(sql_translator::SqlTranslator::new(
            sql_translator::TargetDialect::Sqlite,
        )),
        rust_apps: rust_apps_core::RustAppRegistry::new(apps),
        logger: Arc::new(std::sync::OnceLock::new()),
    }
}
