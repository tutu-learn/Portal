#![allow(dead_code)]

use std::sync::atomic::{AtomicUsize, Ordering};

static DB_COUNTER: AtomicUsize = AtomicUsize::new(0);
static TEMPLATE_DB: tokio::sync::OnceCell<String> = tokio::sync::OnceCell::const_new();

pub fn temp_db_path() -> String {
    let n = DB_COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("/tmp/kiff_test_{}_{}.db", std::process::id(), n)
}

fn remove_db_files(path: &str) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{path}-wal"));
    let _ = std::fs::remove_file(format!("{path}-shm"));
}

/// Argon2id hash of the password "admin". Tests log in as Administrator/admin,
/// but `sync_all` generates a random Administrator password for fresh sites, so
/// we overwrite it with this known hash after bootstrapping the test DB.
const ADMIN_PASSWORD_HASH: &str =
    "$argon2id$v=19$m=19456,t=2,p=1$UEWqTMicBrdEJXqPMhP4oA$bR1RecCR37Rw+Spup2ULPNKAZ7H6vZTX4VeqNAfvdkY";

pub async fn setup_test_db() -> error::Result<orm::DatabasePool> {
    let template = template_db().await?;
    let path = temp_db_path();
    remove_db_files(&path);
    std::fs::copy(&template, &path)?;
    orm::DatabasePool::connect_sqlite(&path).await
}

// sync_all replays ~5k statements over the full DocType set and costs seconds,
// so it runs once per test process into a template DB; each test gets a file copy.
async fn template_db() -> error::Result<String> {
    Ok(TEMPLATE_DB
        .get_or_try_init(|| async {
            let path = format!("/tmp/kiff_test_template_{}.db", std::process::id());
            remove_db_files(&path);
            let pool = orm::DatabasePool::connect_sqlite(&path).await?;
            orm::migrations::Migrator::run(&pool.domain_pools()).await?;
            orm::doctype_sync::sync_all(&pool, vec![], vec![], vec![], vec![], vec![]).await?;
            if !frappe_fixtures_available() {
                // Fixture-less checkout (clean CI clone): sync_all had no
                // DocTypes to sync and skipped seeding, so create the identity
                // tables and seed the users/roles the tests log in with.
                ensure_identity_tables(&pool).await?;
                orm::doctype_sync::ensure_core_users_and_roles(&pool).await?;
            }
            set_admin_password(&pool).await?;
            pool.execute_sql("PRAGMA wal_checkpoint(TRUNCATE)", vec![])
                .await?;
            pool.close().await;
            Ok::<String, error::RuntimeError>(path)
        })
        .await?
        .clone())
}

/// True when the gitignored Frappe source tree is present (local dev
/// checkouts). Clean CI clones don't have it; tests that assert fixture
/// content must skip instead of fail.
pub fn frappe_fixtures_available() -> bool {
    std::path::Path::new("apps/frappe/frappe").is_dir()
}

/// Minimal identity tables for fixture-less databases. On real sites DocType
/// sync creates these from the frappe fixtures with the full column set; only
/// the columns the seed upserts and the permission engine reads are here.
async fn ensure_identity_tables(pool: &orm::DatabasePool) -> error::Result<()> {
    for sql in [
        r#"CREATE TABLE IF NOT EXISTS "user" (
            name TEXT PRIMARY KEY,
            owner TEXT NOT NULL DEFAULT 'Administrator',
            creation TEXT, modified TEXT, modified_by TEXT,
            docstatus INTEGER NOT NULL DEFAULT 0,
            first_name TEXT, email TEXT,
            enabled INTEGER NOT NULL DEFAULT 1,
            user_type TEXT
        )"#,
        r#"CREATE TABLE IF NOT EXISTS "role" (
            name TEXT PRIMARY KEY,
            owner TEXT NOT NULL DEFAULT 'Administrator',
            creation TEXT, modified TEXT, modified_by TEXT,
            docstatus INTEGER NOT NULL DEFAULT 0,
            role_name TEXT,
            desk_access INTEGER NOT NULL DEFAULT 0,
            disabled INTEGER NOT NULL DEFAULT 0
        )"#,
        r#"CREATE TABLE IF NOT EXISTS "has_role" (
            name TEXT PRIMARY KEY,
            owner TEXT NOT NULL DEFAULT 'Administrator',
            creation TEXT, modified TEXT, modified_by TEXT,
            docstatus INTEGER NOT NULL DEFAULT 0,
            idx INTEGER NOT NULL DEFAULT 0,
            parent TEXT, parentfield TEXT, parenttype TEXT, role TEXT
        )"#,
        r#"CREATE TABLE IF NOT EXISTS "user_permission" (
            name TEXT PRIMARY KEY,
            owner TEXT NOT NULL DEFAULT 'Administrator',
            creation TEXT, modified TEXT, modified_by TEXT,
            docstatus INTEGER NOT NULL DEFAULT 0,
            "user" TEXT, allow TEXT, for_value TEXT, applicable_for TEXT,
            is_default INTEGER NOT NULL DEFAULT 0
        )"#,
    ] {
        pool.execute_sql(sql, vec![]).await?;
    }
    Ok(())
}

async fn set_admin_password(pool: &orm::DatabasePool) -> error::Result<()> {
    // __auth is normally created by ensure_core_users_and_roles during seeding,
    // which is skipped on fixture-less databases — create it here if needed.
    pool.execute_sql(
        r#"CREATE TABLE IF NOT EXISTS "__auth" (
            name TEXT,
            doctype TEXT,
            fieldname TEXT,
            password TEXT,
            encrypted INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (name, doctype, fieldname)
        )"#,
        vec![],
    )
    .await?;
    pool.execute_sql(
        r#"INSERT OR REPLACE INTO "__auth" (name, doctype, fieldname, password, encrypted)
           VALUES ('Administrator', 'User', 'password', ?, 0)"#,
        vec![serde_json::Value::String(ADMIN_PASSWORD_HASH.into())],
    )
    .await?;
    Ok(())
}

pub fn teardown_test_db(path: &str) {
    remove_db_files(path);
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
    pools.insert("test_site".into(), Arc::new(orm::DomainPools::from_core_pool(pool)));

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
        boot_cache: Arc::new(rust_apps_core::BootCache::new()),
        asset_cache: Arc::new(rust_apps_core::AssetCache::default()),
        private_file_gate: None,
        login_limiter: Arc::new(rust_apps_core::LoginRateLimiter::new(5, std::time::Duration::from_secs(300), std::time::Duration::from_secs(300))),
    }
}
