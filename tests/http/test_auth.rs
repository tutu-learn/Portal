use error::Result;

async fn tab_session_row(
    pool: &orm::DatabasePool,
    sid: &str,
) -> Result<Option<std::collections::HashMap<String, serde_json::Value>>> {
    let rows = pool
        .execute_sql(
            r#"SELECT * FROM "tabSessions" WHERE sid = ?"#,
            vec![serde_json::Value::String(sid.into())],
        )
        .await?;
    Ok(rows.into_iter().next())
}

#[tokio::test]
async fn test_session_create_and_get() -> Result<()> {
    let pool = crate::common::setup_test_db().await?;
    let store = session::SessionStore::new();

    let session = store
        .create(&pool, "testuser".into(), "test_site".into())
        .await?;
    assert!(!session.id.is_empty());
    assert_eq!(session.user, "testuser");
    assert_eq!(session.site, "test_site");

    let fetched = store.get(&pool, &session.id).await?;
    assert!(fetched.is_some(), "Session should be retrievable");
    let fetched = fetched.unwrap();
    assert_eq!(fetched.user, "testuser");
    assert_eq!(fetched.id, session.id);

    Ok(())
}

#[tokio::test]
async fn test_session_delete() -> Result<()> {
    let pool = crate::common::setup_test_db().await?;
    let store = session::SessionStore::new();

    let session = store
        .create(&pool, "testuser".into(), "test_site".into())
        .await?;
    let fetched_before = store.get(&pool, &session.id).await?;
    assert!(fetched_before.is_some());

    store.delete(&pool, &session.id).await?;

    let fetched_after = store.get(&pool, &session.id).await?;
    assert!(fetched_after.is_none(), "Session should be deleted");

    Ok(())
}

#[tokio::test]
async fn test_login_creates_session() -> Result<()> {
    let pool = crate::common::setup_test_db().await?;
    let auth = session::AuthService::new(session::SessionStore::new());

    let session = auth
        .login(&pool, "Administrator", "admin", "test_site")
        .await?;
    assert!(!session.id.is_empty());
    assert_eq!(session.user, "Administrator");

    let fetched = auth.logout(&pool, &session.id).await;
    assert!(fetched.is_ok());

    Ok(())
}

#[tokio::test]
async fn test_session_creates_tab_sessions_mirror() -> Result<()> {
    let pool = crate::common::setup_test_db().await?;
    let store = session::SessionStore::new();

    let session = store
        .create_with_metadata(
            &pool,
            "testuser".into(),
            "test_site".into(),
            session::SessionMetadata {
                ip: Some("127.0.0.1".into()),
                user_agent: Some("TestAgent/1.0".into()),
            },
        )
        .await?;

    let row = tab_session_row(&pool, &session.id)
        .await?
        .expect("tabSessions row should exist");
    assert_eq!(row.get("user").and_then(|v| v.as_str()), Some("testuser"));
    assert_eq!(row.get("status").and_then(|v| v.as_str()), Some("Active"));
    assert_eq!(row.get("ip").and_then(|v| v.as_str()), Some("127.0.0.1"));
    assert_eq!(
        row.get("ipaddress").and_then(|v| v.as_str()),
        Some("127.0.0.1")
    );
    assert!(row.get("last_updated").and_then(|v| v.as_str()).is_some());
    assert!(row.get("lastupdate").and_then(|v| v.as_str()).is_some());

    let sessiondata: serde_json::Value = serde_json::from_str(
        row.get("sessiondata")
            .and_then(|v| v.as_str())
            .unwrap_or("{}"),
    )?;
    assert_eq!(sessiondata["session_ip"].as_str(), Some("127.0.0.1"));
    assert_eq!(sessiondata["user_agent"].as_str(), Some("TestAgent/1.0"));
    assert!(sessiondata["creation"].as_str().is_some());
    assert!(sessiondata["last_updated"].as_str().is_some());

    Ok(())
}

#[tokio::test]
async fn test_session_refresh_updates_metadata() -> Result<()> {
    let pool = crate::common::setup_test_db().await?;
    let store = session::SessionStore::new();

    let session = store
        .create_with_metadata(
            &pool,
            "testuser".into(),
            "test_site".into(),
            session::SessionMetadata {
                ip: Some("127.0.0.1".into()),
                user_agent: Some("TestAgent/1.0".into()),
            },
        )
        .await?;

    // Refresh with new metadata.
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    store
        .refresh_metadata(
            &pool,
            &session.id,
            session::SessionMetadata {
                ip: Some("10.0.0.1".into()),
                user_agent: Some("TestAgent/2.0".into()),
            },
        )
        .await?;

    let fetched = store
        .get(&pool, &session.id)
        .await?
        .expect("session should exist");
    assert_eq!(fetched.data["session_ip"].as_str(), Some("10.0.0.1"));
    assert_eq!(fetched.data["user_agent"].as_str(), Some("TestAgent/2.0"));

    let row = tab_session_row(&pool, &session.id)
        .await?
        .expect("tabSessions row should exist");
    assert_eq!(row.get("ip").and_then(|v| v.as_str()), Some("10.0.0.1"));
    assert_eq!(
        row.get("ipaddress").and_then(|v| v.as_str()),
        Some("10.0.0.1")
    );
    let sessiondata: serde_json::Value = serde_json::from_str(
        row.get("sessiondata")
            .and_then(|v| v.as_str())
            .unwrap_or("{}"),
    )?;
    assert_eq!(sessiondata["session_ip"].as_str(), Some("10.0.0.1"));

    Ok(())
}

#[tokio::test]
async fn test_session_delete_removes_mirror() -> Result<()> {
    let pool = crate::common::setup_test_db().await?;
    let store = session::SessionStore::new();

    let session = store
        .create(&pool, "testuser".into(), "test_site".into())
        .await?;
    assert!(tab_session_row(&pool, &session.id).await?.is_some());

    store.delete(&pool, &session.id).await?;

    assert!(tab_session_row(&pool, &session.id).await?.is_none());

    Ok(())
}
