use error::Result;
use serde_json::Value;

/// Register a Password field for TestDocType in the metadata tables.
async fn register_password_field(pool: &orm::DatabasePool) -> Result<()> {
    pool.execute_sql(
        r#"INSERT OR REPLACE INTO "docfield" (
            name, creation, modified, modified_by, owner, docstatus,
            parent, fieldname, fieldtype, label, idx, permlevel
        ) VALUES ('TestDocType-client_secret', datetime('now'), datetime('now'),
                  'Administrator', 'Administrator', 0,
                  'TestDocType', 'client_secret', 'Password', 'client_secret', 10, 0)"#,
        vec![],
    )
    .await?;
    Ok(())
}

/// TestDocType table with the Password column Frappe's schema would create.
async fn setup(pool: &orm::DatabasePool) -> Result<()> {
    crate::common::create_doctype_table(pool, "TestDocType").await?;
    register_password_field(pool).await?;
    pool.execute_sql(
        r#"ALTER TABLE "testdoctype" ADD COLUMN client_secret TEXT"#,
        vec![],
    )
    .await?;
    Ok(())
}

async fn auth_secret(pool: &orm::DatabasePool, name: &str) -> Result<Option<String>> {
    let rows = pool
        .execute_sql(
            r#"SELECT password FROM "__auth" WHERE doctype = 'TestDocType' AND name = ? AND fieldname = 'client_secret'"#,
            vec![Value::String(name.into())],
        )
        .await?;
    Ok(rows
        .into_iter()
        .next()
        .and_then(|mut r| r.remove("password"))
        .and_then(|v| v.as_str().map(String::from)))
}

async fn column_value(pool: &orm::DatabasePool, name: &str) -> Result<Option<String>> {
    let rows = pool
        .execute_sql(
            r#"SELECT client_secret FROM "testdoctype" WHERE name = ?"#,
            vec![Value::String(name.into())],
        )
        .await?;
    Ok(rows
        .into_iter()
        .next()
        .and_then(|mut r| r.remove("client_secret"))
        .and_then(|v| v.as_str().map(String::from)))
}

#[tokio::test]
async fn test_save_encrypts_into_auth_and_stores_dummy() -> Result<()> {
    let pool = crate::common::setup_test_db().await?;
    setup(&pool).await?;

    let key = fernet::Fernet::generate_key();
    let mut doc = orm::Document::new("TestDocType", "DOC-P1");
    doc.set_field("title", "keep");
    doc.set_field("client_secret", "s3cr3t");

    orm::password::process_password_fields_for_save(
        &pool,
        "TestDocType",
        "DOC-P1",
        &key,
        &mut doc.fields,
    )
    .await?;

    // The document now carries only the dummy placeholder, like Frappe's
    // _save_passwords leaves it before db_update.
    assert_eq!(
        doc.get_field("client_secret").and_then(|v| v.as_str()),
        Some("******")
    );

    pool.insert_doc(&doc).await?;
    assert_eq!(column_value(&pool, "DOC-P1").await?.as_deref(), Some("******"));

    let stored = auth_secret(&pool, "DOC-P1").await?.expect("auth row");
    assert_ne!(stored, "s3cr3t");
    // Fernet round-trip: the site key decrypts back to the original secret.
    let f = fernet::Fernet::new(&key).unwrap();
    assert_eq!(f.decrypt(&stored).unwrap(), b"s3cr3t");
    Ok(())
}

#[tokio::test]
async fn test_save_dummy_and_clear() -> Result<()> {
    let pool = crate::common::setup_test_db().await?;
    setup(&pool).await?;

    let key = fernet::Fernet::generate_key();

    async fn save(pool: &orm::DatabasePool, key: &str, value: &str) -> Result<()> {
        let mut fields = std::collections::HashMap::new();
        fields.insert("client_secret".to_string(), Value::String(value.into()));
        orm::password::process_password_fields_for_save(pool, "TestDocType", "DOC-P2", key, &mut fields)
            .await
    }

    save(&pool, &key, "original").await?;
    let before = auth_secret(&pool, "DOC-P2").await?.expect("auth row");

    // A dummy placeholder must not overwrite the stored secret.
    save(&pool, &key, "********").await?;
    assert_eq!(auth_secret(&pool, "DOC-P2").await?.as_deref(), Some(before.as_str()));

    // Clearing the field deletes the stored secret.
    save(&pool, &key, "").await?;
    assert_eq!(auth_secret(&pool, "DOC-P2").await?, None);
    Ok(())
}

#[tokio::test]
async fn test_get_doc_returns_dummy_not_secret() -> Result<()> {
    let pool = crate::common::setup_test_db().await?;
    setup(&pool).await?;

    let key = fernet::Fernet::generate_key();
    let mut doc = orm::Document::new("TestDocType", "DOC-P3");
    doc.set_field("client_secret", "s3cr3t");
    orm::password::process_password_fields_for_save(
        &pool,
        "TestDocType",
        "DOC-P3",
        &key,
        &mut doc.fields,
    )
    .await?;
    pool.insert_doc(&doc).await?;

    // Reads come straight from the data table, which only holds the dummy.
    let fetched = pool.get_doc("TestDocType", "DOC-P3").await?;
    assert_eq!(
        fetched.get_field("client_secret").and_then(|v| v.as_str()),
        Some("******")
    );
    Ok(())
}

#[tokio::test]
async fn test_migrate_plaintext_password_values() -> Result<()> {
    let pool = crate::common::setup_test_db().await?;
    setup(&pool).await?;

    // Simulate a legacy database: plaintext secret in the data table column.
    pool.execute_sql(
        r#"INSERT INTO "testdoctype" (name, title, client_secret) VALUES ('DOC-P4', 'legacy', 'plain-secret')"#,
        vec![],
    )
    .await?;

    let key = fernet::Fernet::generate_key();
    orm::password::migrate_plaintext_password_values(&pool, &key).await?;

    // Column is kept (Frappe schemas include password columns) but now holds
    // only the dummy placeholder.
    let cols = pool
        .execute_sql(r#"PRAGMA table_info("testdoctype")"#, vec![])
        .await?;
    assert!(cols.into_iter().any(|mut r| r
        .remove("name")
        .and_then(|v| v.as_str().map(String::from))
        .as_deref()
        == Some("client_secret")));
    assert_eq!(
        column_value(&pool, "DOC-P4").await?.as_deref(),
        Some("************")
    );

    let stored = auth_secret(&pool, "DOC-P4").await?.expect("auth row");
    let f = fernet::Fernet::new(&key).unwrap();
    assert_eq!(f.decrypt(&stored).unwrap(), b"plain-secret");
    Ok(())
}
