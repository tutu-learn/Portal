use error::Result;

#[tokio::test]
async fn test_admin_always_has_permission() -> Result<()> {
    let pool = crate::common::setup_test_db().await?;
    let engine = permissions::PermissionEngine::new();

    let can_read = engine.has_permission(&pool, "Administrator", "User", "read", None).await?;
    assert!(can_read, "Administrator should always have read permission");

    let can_write = engine.has_permission(&pool, "Administrator", "User", "write", None).await?;
    assert!(can_write, "Administrator should always have write permission");

    let can_delete = engine.has_permission(&pool, "Administrator", "User", "delete", None).await?;
    assert!(can_delete, "Administrator should always have delete permission");

    Ok(())
}

#[tokio::test]
async fn test_guest_limited_permission() -> Result<()> {
    let pool = crate::common::setup_test_db().await?;
    let engine = permissions::PermissionEngine::new();

    let can_read = engine.has_permission(&pool, "Guest", "User", "read", None).await?;
    assert!(can_read, "Guest should have read via 'All' role");

    let can_write = engine.has_permission(&pool, "Guest", "User", "write", None).await?;
    assert!(!can_write, "Guest should NOT have write permission");

    Ok(())
}

#[tokio::test]
async fn test_owner_only_permission() -> Result<()> {
    let pool = crate::common::setup_test_db().await?;
    crate::common::create_doctype_table(&pool, "OwnerDoc").await?;

    // Remove the default "All" wildcard permission so it doesn't grant access to everyone
    pool.execute_sql("DELETE FROM __kiff_docperm WHERE parent = '*' AND role = 'All'", vec![]).await?;

    // Insert a custom permission: All users can read OwnerDoc only if owner
    let sql = r#"
        INSERT INTO __kiff_docperm ("parent", "role", "read", "write", "create", "delete", "submit", "cancel", "if_owner")
        VALUES ('OwnerDoc', 'All', 1, 0, 0, 0, 0, 0, 1)
    "#;
    pool.execute_sql(sql, vec![]).await?;

    let engine = permissions::PermissionEngine::new();

    let mut doc = orm::Document::new("OwnerDoc", "OWN-001");
    doc.owner = "alice@example.com".into();
    pool.insert_doc(&doc).await?;

    // User with Sales User role trying to read their own doc
    let can_read_own = engine.has_permission(&pool, "alice@example.com", "OwnerDoc", "read", Some(&doc)).await?;
    assert!(can_read_own, "Owner should be able to read their own doc");

    // Different user trying to read the same doc
    let can_read_other = engine.has_permission(&pool, "bob@example.com", "OwnerDoc", "read", Some(&doc)).await?;
    assert!(!can_read_other, "Non-owner should NOT be able to read owner-only doc");

    Ok(())
}

#[tokio::test]
async fn test_permission_query_conditions() -> Result<()> {
    let pool = crate::common::setup_test_db().await?;
    let engine = permissions::PermissionEngine::new();

    // Admin has no conditions (full access)
    let admin_cond = engine.get_permission_query_conditions(&pool, "Administrator", "User").await?;
    assert!(admin_cond.is_none(), "Admin should have no query conditions");

    // Guest gets owner condition for owner-only perms, or none for full perms
    // Since default 'All' role has full read, Guest should have no conditions
    let guest_cond = engine.get_permission_query_conditions(&pool, "Guest", "User").await?;
    assert!(guest_cond.is_none(), "Guest with 'All' role should have no conditions for User");

    Ok(())
}
