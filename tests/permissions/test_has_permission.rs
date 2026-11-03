use error::Result;

#[tokio::test]
async fn test_admin_always_has_permission() -> Result<()> {
    let pool = crate::common::setup_test_db().await?;
    let engine = permissions::PermissionEngine::new();

    let can_read = engine
        .has_permission(&pool, "Administrator", "User", "read", None)
        .await?;
    assert!(can_read, "Administrator should always have read permission");

    let can_write = engine
        .has_permission(&pool, "Administrator", "User", "write", None)
        .await?;
    assert!(
        can_write,
        "Administrator should always have write permission"
    );

    let can_delete = engine
        .has_permission(&pool, "Administrator", "User", "delete", None)
        .await?;
    assert!(
        can_delete,
        "Administrator should always have delete permission"
    );

    Ok(())
}

#[tokio::test]
async fn test_guest_limited_permission() -> Result<()> {
    if !crate::common::frappe_fixtures_available() {
        eprintln!("skipping: relies on the Gender doctype's seeded fixture grants");
        return Ok(());
    }
    let pool = crate::common::setup_test_db().await?;
    let engine = permissions::PermissionEngine::new();

    // "Gender" is seeded with a plain read permission for the "All" role.
    let can_read = engine
        .has_permission(&pool, "Guest", "Gender", "read", None)
        .await?;
    assert!(can_read, "Guest should have read via 'All' role");

    let can_write = engine
        .has_permission(&pool, "Guest", "Gender", "write", None)
        .await?;
    assert!(!can_write, "Guest should NOT have write permission");

    Ok(())
}

#[tokio::test]
async fn test_owner_only_permission() -> Result<()> {
    let pool = crate::common::setup_test_db().await?;
    crate::common::create_doctype_table(&pool, "OwnerDoc").await?;

    // Remove the default "All" wildcard permission so it doesn't grant access to everyone
    pool.execute_sql(
        "DELETE FROM __kiff_docperm WHERE parent = '*' AND role = 'All'",
        vec![],
    )
    .await?;

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
    let can_read_own = engine
        .has_permission(&pool, "alice@example.com", "OwnerDoc", "read", Some(&doc))
        .await?;
    assert!(can_read_own, "Owner should be able to read their own doc");

    // Different user trying to read the same doc
    let can_read_other = engine
        .has_permission(&pool, "bob@example.com", "OwnerDoc", "read", Some(&doc))
        .await?;
    assert!(
        !can_read_other,
        "Non-owner should NOT be able to read owner-only doc"
    );

    Ok(())
}

#[tokio::test]
async fn test_permission_query_conditions() -> Result<()> {
    if !crate::common::frappe_fixtures_available() {
        eprintln!("skipping: relies on the Gender doctype's seeded fixture grants");
        return Ok(());
    }
    let pool = crate::common::setup_test_db().await?;
    let engine = permissions::PermissionEngine::new();

    // Admin has no conditions (full access)
    let admin_cond = engine
        .get_permission_query_conditions(&pool, "Administrator", "User")
        .await?;
    assert!(
        admin_cond.is_none(),
        "Admin should have no query conditions"
    );

    // Guest gets owner condition for owner-only perms, or none for full perms.
    // "Gender" is seeded with a plain read permission for the "All" role.
    let guest_cond = engine
        .get_permission_query_conditions(&pool, "Guest", "Gender")
        .await?;
    assert!(
        guest_cond.is_none(),
        "Guest with 'All' role should have no conditions for Gender"
    );

    Ok(())
}

#[tokio::test]
async fn test_audit_roles_have_full_read() -> Result<()> {
    let pool = crate::common::setup_test_db().await?;
    orm::doctype_sync::ensure_core_users_and_roles(&pool).await?;
    crate::common::create_doctype_table(&pool, "AuditTarget").await?;

    // Make sure no blanket wildcard grant skews the baseline denial below.
    pool.execute_sql(
        "DELETE FROM __kiff_docperm WHERE parent = '*' AND role = 'All'",
        vec![],
    )
    .await?;

    // Assign the new roles to two users.
    for (user, role) in [
        ("auditor@example.com", "Auditor"),
        ("auditdev@example.com", "Audit Developer"),
    ] {
        pool.execute_sql(
            r#"INSERT INTO "has_role" (name, parent, parentfield, parenttype, role)
               VALUES (?, ?, 'roles', 'User', ?)"#,
            vec![
                serde_json::Value::String(format!("hr-{}-{}", user, role)),
                serde_json::Value::String(user.into()),
                serde_json::Value::String(role.into()),
            ],
        )
        .await?;
    }

    let engine = permissions::PermissionEngine::new();

    for user in ["auditor@example.com", "auditdev@example.com"] {
        // Full read covers a doctype that has no explicit grants at all.
        let can_read = engine
            .has_permission(&pool, user, "AuditTarget", "read", None)
            .await?;
        assert!(can_read, "{} should read any doctype", user);

        // ... including the schema meta doctypes behind finding "DocField 403".
        let can_read_docfield = engine
            .has_permission(&pool, user, "DocField", "read", None)
            .await?;
        assert!(can_read_docfield, "{} should read DocField", user);
        let can_read_doctype = engine
            .has_permission(&pool, user, "DocType", "read", None)
            .await?;
        assert!(can_read_doctype, "{} should read DocType", user);

        // Ptypes that follow read are available too.
        let can_export = engine
            .has_permission(&pool, user, "AuditTarget", "export", None)
            .await?;
        assert!(can_export, "{} should export via read", user);

        // The roles stay read-only.
        let can_write = engine
            .has_permission(&pool, user, "AuditTarget", "write", None)
            .await?;
        assert!(!can_write, "{} must not write", user);
        let can_create = engine
            .has_permission(&pool, user, "AuditTarget", "create", None)
            .await?;
        assert!(!can_create, "{} must not create", user);
    }

    // A user without either role is still denied on the ungranted doctype.
    let denied = engine
        .has_permission(&pool, "nobody@example.com", "AuditTarget", "read", None)
        .await?;
    assert!(!denied, "user without audit roles must stay denied");

    // List queries see no row restriction for the audit roles...
    let cond = engine
        .get_permission_query_conditions(&pool, "auditor@example.com", "AuditTarget")
        .await?;
    assert!(cond.is_none(), "Auditor should have unrestricted list reads");

    // ... while the unprivileged user gets the deny-all condition ("1 = 0").
    let denied_cond = engine
        .get_permission_query_conditions(&pool, "nobody@example.com", "AuditTarget")
        .await?;
    assert_eq!(denied_cond.as_deref(), Some("1 = 0"));

    Ok(())
}
