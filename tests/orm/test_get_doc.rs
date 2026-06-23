use error::Result;

#[tokio::test]
async fn test_get_doc_not_found() -> Result<()> {
    let pool = crate::common::setup_test_db().await?;
    crate::common::create_doctype_table(&pool, "TestDocType").await?;
    let result = pool.get_doc("TestDocType", "NONEXISTENT").await;
    assert!(
        matches!(result, Err(error::RuntimeError::NotFound(_))),
        "Expected NotFound error, got {:?}",
        result
    );
    Ok(())
}

#[tokio::test]
async fn test_get_doc_found() -> Result<()> {
    let pool = crate::common::setup_test_db().await?;
    crate::common::create_doctype_table(&pool, "TestDocType").await?;

    let mut doc = orm::Document::new("TestDocType", "DOC-001");
    doc.set_field("title", "Test Title");
    doc.set_field("description", "A description");
    pool.insert_doc(&doc).await?;

    let fetched = pool.get_doc("TestDocType", "DOC-001").await?;
    assert_eq!(fetched.name, "DOC-001");
    assert_eq!(fetched.doctype, "TestDocType");
    assert_eq!(
        fetched.get_field("title").and_then(|v| v.as_str()),
        Some("Test Title")
    );
    assert_eq!(
        fetched.get_field("description").and_then(|v| v.as_str()),
        Some("A description")
    );

    Ok(())
}
