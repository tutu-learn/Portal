use kiff_sync::outbox::SyncOutboxHook;
use orm::Document;
use std::sync::Arc;

static TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn insert_doc_creates_sync_outbox_row() {
    let _guard = TEST_LOCK.lock().await;
    let path = format!("/tmp/kiff_sync_test_{}.db", std::process::id());
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(format!("{path}-wal"));
    let _ = std::fs::remove_file(format!("{path}-shm"));

    let pool = orm::DatabasePool::connect_sqlite(&path).await.unwrap();
    orm::migrations::Migrator::run(&pool).await.unwrap();

    // insert_doc/save_doc query docfield metadata; create minimal tables.
    pool.execute_sql(
        r#"CREATE TABLE IF NOT EXISTS "doctype" (
            name TEXT PRIMARY KEY,
            creation TEXT,
            modified TEXT,
            modified_by TEXT,
            owner TEXT,
            docstatus INTEGER,
            module TEXT,
            is_submittable INTEGER,
            is_tree INTEGER,
            istable INTEGER,
            track_changes INTEGER,
            track_seen INTEGER,
            track_views INTEGER,
            restrict_to_domain TEXT
        )"#,
        vec![],
    )
    .await
    .unwrap();
    pool.execute_sql(
        r#"CREATE TABLE IF NOT EXISTS "docfield" (
            name TEXT PRIMARY KEY,
            creation TEXT,
            modified TEXT,
            modified_by TEXT,
            owner TEXT,
            docstatus INTEGER,
            parent TEXT,
            fieldname TEXT,
            fieldtype TEXT,
            options TEXT,
            label TEXT,
            idx INTEGER,
            permlevel INTEGER
        )"#,
        vec![],
    )
    .await
    .unwrap();

    // Register the sync outbox hook.
    let hook = SyncOutboxHook::new("test_site", "test_node", pool.clone());
    orm::add_hook_runner(Arc::new(hook)).await;

    // Create a simple table and metadata.
    pool.execute_sql(
        r#"CREATE TABLE "todo" (
            name TEXT PRIMARY KEY,
            owner TEXT NOT NULL DEFAULT 'Administrator',
            creation TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            modified TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            docstatus INTEGER NOT NULL DEFAULT 0,
            title TEXT
        )"#,
        vec![],
    )
    .await
    .unwrap();

    // Insert a document.
    let mut doc = Document::new("Todo", "TODO-001");
    doc.set_field("title", "Buy milk");
    pool.insert_doc(&doc).await.unwrap();

    // Verify the outbox row was captured.
    let rows = pool
        .execute_sql(
            "SELECT doctype, name, action, payload_json FROM __kiff_sync_outbox",
            vec![],
        )
        .await
        .unwrap();

    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.get("doctype").unwrap().as_str().unwrap(), "Todo");
    assert_eq!(row.get("name").unwrap().as_str().unwrap(), "TODO-001");
    assert_eq!(row.get("action").unwrap().as_str().unwrap(), "INSERT");
    let payload = row.get("payload_json").unwrap().as_str().unwrap();
    assert!(payload.contains("Buy milk"));

    // Clean up.
    orm::clear_hook_runners().await;
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(format!("{path}-wal"));
    let _ = std::fs::remove_file(format!("{path}-shm"));
}

#[tokio::test]
async fn update_doc_creates_sync_outbox_row() {
    let _guard = TEST_LOCK.lock().await;
    let path = format!("/tmp/kiff_sync_test_update_{}.db", std::process::id());
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(format!("{path}-wal"));
    let _ = std::fs::remove_file(format!("{path}-shm"));

    let pool = orm::DatabasePool::connect_sqlite(&path).await.unwrap();
    orm::migrations::Migrator::run(&pool).await.unwrap();

    pool.execute_sql(
        r#"CREATE TABLE IF NOT EXISTS "doctype" (
            name TEXT PRIMARY KEY,
            creation TEXT,
            modified TEXT,
            modified_by TEXT,
            owner TEXT,
            docstatus INTEGER,
            module TEXT,
            is_submittable INTEGER,
            is_tree INTEGER,
            istable INTEGER,
            track_changes INTEGER,
            track_seen INTEGER,
            track_views INTEGER,
            restrict_to_domain TEXT
        )"#,
        vec![],
    )
    .await
    .unwrap();
    pool.execute_sql(
        r#"CREATE TABLE IF NOT EXISTS "docfield" (
            name TEXT PRIMARY KEY,
            creation TEXT,
            modified TEXT,
            modified_by TEXT,
            owner TEXT,
            docstatus INTEGER,
            parent TEXT,
            fieldname TEXT,
            fieldtype TEXT,
            options TEXT,
            label TEXT,
            idx INTEGER,
            permlevel INTEGER
        )"#,
        vec![],
    )
    .await
    .unwrap();

    let hook = SyncOutboxHook::new("test_site", "test_node", pool.clone());
    orm::add_hook_runner(Arc::new(hook)).await;

    pool.execute_sql(
        r#"CREATE TABLE "todo" (
            name TEXT PRIMARY KEY,
            owner TEXT NOT NULL DEFAULT 'Administrator',
            creation TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            modified TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            docstatus INTEGER NOT NULL DEFAULT 0,
            title TEXT
        )"#,
        vec![],
    )
    .await
    .unwrap();

    let mut doc = Document::new("Todo", "TODO-002");
    doc.set_field("title", "First title");
    pool.insert_doc(&doc).await.unwrap();

    // Update the document.
    doc.set_field("title", "Updated title");
    pool.save_doc(&doc).await.unwrap();

    let rows = pool
        .execute_sql(
            "SELECT action, payload_json FROM __kiff_sync_outbox ORDER BY created_at",
            vec![],
        )
        .await
        .unwrap();

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].get("action").unwrap().as_str().unwrap(), "INSERT");
    assert_eq!(rows[1].get("action").unwrap().as_str().unwrap(), "UPDATE");
    let payload = rows[1].get("payload_json").unwrap().as_str().unwrap();
    assert!(payload.contains("Updated title"));

    orm::clear_hook_runners().await;
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(format!("{path}-wal"));
    let _ = std::fs::remove_file(format!("{path}-shm"));
}
