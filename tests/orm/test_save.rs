use error::Result;

#[tokio::test]
async fn test_insert_doc() -> Result<()> {
    let pool = crate::common::setup_test_db().await?;
    crate::common::create_doctype_table(&pool, "TestDocType").await?;

    let mut doc = orm::Document::new("TestDocType", "DOC-002");
    doc.set_field("title", "Inserted");
    let name = pool.insert_doc(&doc).await?;
    assert_eq!(name, "DOC-002");

    let exists = pool.exists("TestDocType", "DOC-002").await?;
    assert!(exists);

    let fetched = pool.get_doc("TestDocType", "DOC-002").await?;
    assert_eq!(fetched.get_field("title").and_then(|v| v.as_str()), Some("Inserted"));

    Ok(())
}

#[tokio::test]
async fn test_save_doc_update() -> Result<()> {
    let pool = crate::common::setup_test_db().await?;
    crate::common::create_doctype_table(&pool, "TestDocType").await?;

    let mut doc = orm::Document::new("TestDocType", "DOC-003");
    doc.set_field("title", "Original");
    pool.insert_doc(&doc).await?;

    let mut doc = pool.get_doc("TestDocType", "DOC-003").await?;
    doc.set_field("title", "Updated");
    pool.save_doc(&doc).await?;

    let fetched = pool.get_doc("TestDocType", "DOC-003").await?;
    assert_eq!(fetched.get_field("title").and_then(|v| v.as_str()), Some("Updated"));

    Ok(())
}

#[tokio::test]
async fn test_delete_doc() -> Result<()> {
    let pool = crate::common::setup_test_db().await?;
    crate::common::create_doctype_table(&pool, "TestDocType").await?;

    let mut doc = orm::Document::new("TestDocType", "DOC-004");
    doc.set_field("title", "To Delete");
    pool.insert_doc(&doc).await?;

    let exists_before = pool.exists("TestDocType", "DOC-004").await?;
    assert!(exists_before);

    pool.delete_doc("TestDocType", "DOC-004").await?;

    let exists_after = pool.exists("TestDocType", "DOC-004").await?;
    assert!(!exists_after);

    Ok(())
}
