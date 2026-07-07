use permissions::PermissionEngine;

#[tokio::test]
async fn test_role_cache_is_invalidated() {
    let pool = crate::common::setup_test_db().await.unwrap();
    crate::common::create_doctype_table(&pool, "RoleCacheDoc")
        .await
        .unwrap();

    // Create a role and assign it to a user.
    pool.execute_sql(
        r#"INSERT OR IGNORE INTO "role" (name, creation, modified) VALUES ('Reader', datetime('now'), datetime('now'))"#,
        vec![],
    )
    .await
    .unwrap();
    pool.execute_sql(
        r#"INSERT OR IGNORE INTO "user" (name, email, first_name, creation, modified) VALUES ('tester@example.com', 'tester@example.com', 'Tester', datetime('now'), datetime('now'))"#,
        vec![],
    )
    .await
    .unwrap();
    pool.execute_sql(
        r#"INSERT INTO "has_role" (name, parent, parentfield, parenttype, role, idx, creation, modified) VALUES ('tester@example.com-Reader', 'tester@example.com', 'roles', 'User', 'Reader', 0, datetime('now'), datetime('now'))"#,
        vec![],
    )
    .await
    .unwrap();

    // Grant Reader read access.
    crate::common::grant_permission(&pool, "RoleCacheDoc", "Reader", true, false, false, false)
        .await
        .unwrap();

    let engine = PermissionEngine::new();
    assert!(
        engine
            .has_permission(&pool, "tester@example.com", "RoleCacheDoc", "read", None)
            .await
            .unwrap(),
        "user with Reader role should be allowed"
    );

    // Remove the role assignment.
    pool.execute_sql(
        r#"DELETE FROM "has_role" WHERE parent = 'tester@example.com' AND role = 'Reader'"#,
        vec![],
    )
    .await
    .unwrap();

    // Without role-cache invalidation the engine would still allow access.
    engine.clear_roles_cache("tester@example.com");

    assert!(
        !engine
            .has_permission(&pool, "tester@example.com", "RoleCacheDoc", "read", None)
            .await
            .unwrap(),
        "user without Reader role should be denied after cache invalidation"
    );
}
