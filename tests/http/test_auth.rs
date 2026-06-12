use error::Result;

#[tokio::test]
async fn test_session_create_and_get() -> Result<()> {
    let pool = crate::common::setup_test_db().await?;
    let store = session::SessionStore::new();

    let session = store.create(&pool, "testuser".into(), "test_site".into()).await?;
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

    let session = store.create(&pool, "testuser".into(), "test_site".into()).await?;
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

    let session = auth.login(&pool, "Administrator", "admin", "test_site").await?;
    assert!(!session.id.is_empty());
    assert_eq!(session.user, "Administrator");

    let fetched = auth.logout(&pool, &session.id).await;
    assert!(fetched.is_ok());

    Ok(())
}
