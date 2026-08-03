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

#[tokio::test]
async fn test_apply_password_fields_encrypts_into_auth() -> Result<()> {
    let pool = crate::common::setup_test_db().await?;
    crate::common::create_doctype_table(&pool, "TestDocType").await?;
    register_password_field(&pool).await?;

    let key = fernet::Fernet::generate_key();
    let mut fields = std::collections::HashMap::new();
    fields.insert("client_secret".to_string(), Value::String("s3cr3t".into()));
    fields.insert("title".to_string(), Value::String("keep".into()));

    let extracted = orm::password::extract_password_fields(&pool, "TestDocType", &mut fields).await?;
    // The password field is stripped from the document map; other fields stay.
    assert!(!fields.contains_key("client_secret"));
    assert_eq!(
        fields.get("title").and_then(|v| v.as_str()),
        Some("keep")
    );
    assert_eq!(extracted.len(), 1);

    orm::password::apply_password_fields(&pool, "TestDocType", "DOC-P1", &key, &extracted).await?;

    let stored = auth_secret(&pool, "DOC-P1").await?.expect("auth row");
    assert_ne!(stored, "s3cr3t");
    // Fernet round-trip: the site key decrypts back to the original secret.
    let f = fernet::Fernet::new(&key).unwrap();
    assert_eq!(f.decrypt(&stored).unwrap(), b"s3cr3t");
    Ok(())
}

#[tokio::test]
async fn test_apply_password_fields_dummy_and_clear() -> Result<()> {
    let pool = crate::common::setup_test_db().await?;
    crate::common::create_doctype_table(&pool, "TestDocType").await?;
    register_password_field(&pool).await?;

    let key = fernet::Fernet::generate_key();

    async fn store(pool: &orm::DatabasePool, key: &str, value: &str) -> Result<()> {
        let mut fields = std::collections::HashMap::new();
        fields.insert("client_secret".to_string(), Value::String(value.into()));
        let extracted = orm::password::extract_password_fields(pool, "TestDocType", &mut fields).await?;
        orm::password::apply_password_fields(pool, "TestDocType", "DOC-P2", key, &extracted).await
    }

    store(&pool, &key, "original").await?;
    let before = auth_secret(&pool, "DOC-P2").await?.expect("auth row");

    // A dummy placeholder must not overwrite the stored secret.
    store(&pool, &key, orm::password::DUMMY_PASSWORD).await?;
    assert_eq!(auth_secret(&pool, "DOC-P2").await?.as_deref(), Some(before.as_str()));

    // Clearing the field deletes the stored secret.
    store(&pool, &key, "").await?;
    assert_eq!(auth_secret(&pool, "DOC-P2").await?, None);
    Ok(())
}

#[tokio::test]
async fn test_get_doc_masks_password_fields() -> Result<()> {
    let pool = crate::common::setup_test_db().await?;
    crate::common::create_doctype_table(&pool, "TestDocType").await?;
    register_password_field(&pool).await?;

    let key = fernet::Fernet::generate_key();
    let mut doc = orm::Document::new("TestDocType", "DOC-P3");
    doc.set_field("title", "masked");
    pool.insert_doc(&doc).await?;

    // No stored secret: the field loads as null (Frappe initialises every
    // non-table field to None; controllers rely on the attribute existing).
    let fetched = pool.get_doc("TestDocType", "DOC-P3").await?;
    assert!(
        fetched
            .get_field("client_secret")
            .is_some_and(|v| v.is_null())
    );

    // Stored secret: the loaded document shows the dummy placeholder.
    let mut fields = std::collections::HashMap::new();
    fields.insert("client_secret".to_string(), Value::String("s3cr3t".into()));
    let extracted = orm::password::extract_password_fields(&pool, "TestDocType", &mut fields).await?;
    orm::password::apply_password_fields(&pool, "TestDocType", "DOC-P3", &key, &extracted).await?;

    let fetched = pool.get_doc("TestDocType", "DOC-P3").await?;
    assert_eq!(
        fetched.get_field("client_secret").and_then(|v| v.as_str()),
        Some(orm::password::DUMMY_PASSWORD)
    );
    Ok(())
}

#[tokio::test]
async fn test_migrate_plaintext_password_columns() -> Result<()> {
    let pool = crate::common::setup_test_db().await?;
    crate::common::create_doctype_table(&pool, "TestDocType").await?;
    register_password_field(&pool).await?;

    // Simulate a legacy database: a plaintext column on the data table.
    pool.execute_sql(r#"ALTER TABLE "testdoctype" ADD COLUMN client_secret TEXT"#, vec![])
        .await?;
    pool.execute_sql(
        r#"INSERT INTO "testdoctype" (name, title, client_secret) VALUES ('DOC-P4', 'legacy', 'plain-secret')"#,
        vec![],
    )
    .await?;

    let key = fernet::Fernet::generate_key();
    orm::password::migrate_plaintext_password_columns(&pool, &key).await?;

    // Column dropped, secret moved into __auth Fernet-encrypted.
    let cols = pool
        .execute_sql(r#"PRAGMA table_info("testdoctype")"#, vec![])
        .await?;
    assert!(!cols.into_iter().any(|mut r| r
        .remove("name")
        .and_then(|v| v.as_str().map(String::from))
        .as_deref()
        == Some("client_secret")));

    let stored = auth_secret(&pool, "DOC-P4").await?.expect("auth row");
    let f = fernet::Fernet::new(&key).unwrap();
    assert_eq!(f.decrypt(&stored).unwrap(), b"plain-secret");
    Ok(())
}
