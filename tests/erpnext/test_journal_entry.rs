use error::Result;

#[tokio::test]
async fn test_create_journal_entry() -> Result<()> {
    let pool = crate::common::setup_test_db().await?;
    // Journal Entry table without the tab prefix
    let sql = r#"
        CREATE TABLE IF NOT EXISTS "journal_entry" (
            name TEXT PRIMARY KEY,
            owner TEXT NOT NULL DEFAULT 'Administrator',
            creation TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            modified TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            docstatus INTEGER NOT NULL DEFAULT 0,
            title TEXT,
            posting_date TEXT,
            total_debit REAL DEFAULT 0.0,
            total_credit REAL DEFAULT 0.0
        )
    "#;
    pool.execute_sql(sql, vec![]).await?;

    let mut doc = orm::Document::new("Journal Entry", "JE-2024-001");
    doc.set_field("title", "Test Journal Entry");
    doc.set_field("posting_date", "2024-01-15");
    doc.set_field("total_debit", 1000.0);
    doc.set_field("total_credit", 1000.0);

    let name = pool.insert_doc(&doc).await?;
    assert_eq!(name, "JE-2024-001");

    let fetched = pool.get_doc("Journal Entry", "JE-2024-001").await?;
    assert_eq!(fetched.name, "JE-2024-001");
    assert_eq!(
        fetched.get_field("title").and_then(|v| v.as_str()),
        Some("Test Journal Entry")
    );
    assert_eq!(
        fetched.get_field("posting_date").and_then(|v| v.as_str()),
        Some("2024-01-15")
    );

    let list = pool
        .get_list("Journal Entry", None, None, None, None, None)
        .await?;
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "JE-2024-001");

    pool.delete_doc("Journal Entry", "JE-2024-001").await?;
    let exists = pool.exists("Journal Entry", "JE-2024-001").await?;
    assert!(!exists);

    Ok(())
}
