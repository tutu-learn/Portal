use error::Result;

#[tokio::test]
async fn test_get_doc_loads_child_tables() -> Result<()> {
    let pool = crate::common::setup_test_db().await?;

    // Build a User document with child-table rows.
    let mut doc = orm::Document::new("User", "test-admin@example.com");
    doc.set_field("email", "test-admin@example.com");
    doc.set_field("first_name", "Test");
    doc.set_field("user_type", "System User");
    doc.set_field(
        "roles",
        serde_json::json!([
            { "role": "System Manager" },
            { "role": "Report Manager" },
        ]),
    );

    pool.insert_doc(&doc).await?;

    let fetched = pool.get_doc("User", "test-admin@example.com").await?;
    let roles = fetched
        .get_field("roles")
        .and_then(|v| v.as_array())
        .expect("roles child table should be loaded");
    assert_eq!(roles.len(), 2);

    let role_names: Vec<&str> = roles
        .iter()
        .filter_map(|r| r.get("role").and_then(|v| v.as_str()))
        .collect();
    assert!(role_names.contains(&"System Manager"));
    assert!(role_names.contains(&"Report Manager"));

    // Child rows should carry the child doctype and parent metadata.
    assert!(roles
        .iter()
        .all(|r| r.get("doctype").and_then(|v| v.as_str()) == Some("Has Role")));
    assert!(roles
        .iter()
        .all(|r| r.get("parent").and_then(|v| v.as_str()) == Some("test-admin@example.com")));

    Ok(())
}

#[tokio::test]
async fn test_get_doc_injects_onload_modules() -> Result<()> {
    let pool = crate::common::setup_test_db().await?;

    // The User and Module Profile forms need __onload.all_modules for the
    // module editor.
    let user = pool.get_doc("User", "Administrator").await?;
    let all_modules = user
        .get_field("__onload")
        .and_then(|v| v.get("all_modules"))
        .and_then(|v| v.as_array())
        .expect("__onload.all_modules should be present for User");
    assert!(!all_modules.is_empty());

    let profile = orm::Document::new("Module Profile", "Admin");
    pool.insert_doc(&profile).await?;
    let fetched = pool.get_doc("Module Profile", "Admin").await?;
    let all_modules = fetched
        .get_field("__onload")
        .and_then(|v| v.get("all_modules"))
        .and_then(|v| v.as_array())
        .expect("__onload.all_modules should be present for Module Profile");
    assert!(!all_modules.is_empty());

    Ok(())
}
