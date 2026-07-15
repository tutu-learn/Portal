use error::Result;

#[tokio::test]
async fn test_get_list_empty() -> Result<()> {
    let pool = crate::common::setup_test_db().await?;
    crate::common::create_doctype_table(&pool, "EmptyDocType").await?;

    let docs = pool
        .get_list("EmptyDocType", None, None, None, None, None)
        .await?;
    assert!(docs.is_empty());

    Ok(())
}

#[tokio::test]
async fn test_get_list_with_results() -> Result<()> {
    let pool = crate::common::setup_test_db().await?;
    crate::common::create_doctype_table(&pool, "TestDocType").await?;

    for i in 1..=3 {
        let mut doc = orm::Document::new("TestDocType", format!("DOC-LIST-{}", i));
        doc.set_field("title", format!("Title {}", i));
        doc.set_field("status", if i % 2 == 0 { "Active" } else { "Draft" });
        pool.insert_doc(&doc).await?;
    }

    let docs = pool
        .get_list("TestDocType", None, None, None, None, None)
        .await?;
    assert_eq!(docs.len(), 3);

    let filtered = pool
        .get_list(
            "TestDocType",
            Some(serde_json::from_str(r#"{"status": "Active"}"#).unwrap()),
            None,
            None,
            None,
            None,
        )
        .await?;
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].name, "DOC-LIST-2");

    let limited = pool
        .get_list("TestDocType", None, None, None, None, Some(2))
        .await?;
    assert_eq!(limited.len(), 2);

    Ok(())
}
