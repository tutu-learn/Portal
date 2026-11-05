use error::Result;

/// Restores ANRI_ADMIN_PASSWORD on drop so a panicking assertion can't leak
/// the override into other tests running in this process.
struct EnvRestore {
    previous: Option<String>,
}

impl EnvRestore {
    fn new() -> Self {
        Self {
            previous: std::env::var("ANRI_ADMIN_PASSWORD").ok(),
        }
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        match &self.previous {
            Some(v) => std::env::set_var("ANRI_ADMIN_PASSWORD", v),
            None => std::env::remove_var("ANRI_ADMIN_PASSWORD"),
        }
    }
}

async fn stored_admin_hash(pool: &orm::DatabasePool) -> Result<Option<String>> {
    let rows = pool
        .execute_sql(
            r#"SELECT password FROM "__auth" WHERE name = 'Administrator' AND doctype = 'User' AND fieldname = 'password'"#,
            vec![],
        )
        .await?;
    Ok(rows
        .into_iter()
        .next()
        .and_then(|mut r| r.remove("password"))
        .and_then(|v| v.as_str().map(String::from)))
}

/// ANRI_ADMIN_PASSWORD must apply on every startup, not just first bootstrap:
/// it overrides an existing (random or user-changed) Administrator password,
/// rotations take effect on the next boot, and unsetting it stops the
/// override. All in one test so the process-global env mutations can't
/// interleave with each other.
#[tokio::test]
async fn admin_password_env_var_applies_on_every_boot() -> Result<()> {
    let _restore = EnvRestore::new();
    let path = crate::common::temp_db_path();
    let pool = orm::DatabasePool::connect_sqlite(&path).await?;
    orm::migrations::Migrator::run(&pool.domain_pools()).await?;
    crate::common::ensure_identity_tables(&pool).await?;
    let auth = session::AuthService::new(session::SessionStore::new());

    // First boot without the env var: a random password is seeded and left
    // untouched by later env-less boots.
    std::env::remove_var("ANRI_ADMIN_PASSWORD");
    orm::doctype_sync::ensure_core_users_and_roles(&pool).await?;
    let first = stored_admin_hash(&pool).await?.expect("seeded admin password");
    orm::doctype_sync::ensure_core_users_and_roles(&pool).await?;
    assert_eq!(
        stored_admin_hash(&pool).await?.as_deref(),
        Some(first.as_str())
    );

    // With the env var set it wins on every startup, even though a password
    // already exists.
    std::env::set_var("ANRI_ADMIN_PASSWORD", "env-password-one");
    orm::doctype_sync::ensure_core_users_and_roles(&pool).await?;
    let hash = stored_admin_hash(&pool).await?.expect("admin password");
    assert!(auth.verify_password("env-password-one", &hash).await?);
    assert!(!auth.verify_password("wrong", &hash).await?);

    // A changed value rotates the password on the next boot.
    std::env::set_var("ANRI_ADMIN_PASSWORD", "env-password-two");
    orm::doctype_sync::ensure_core_users_and_roles(&pool).await?;
    let hash = stored_admin_hash(&pool).await?.expect("admin password");
    assert!(auth.verify_password("env-password-two", &hash).await?);
    assert!(!auth.verify_password("env-password-one", &hash).await?);

    // Unsetting the env var stops the override: the current password
    // survives restarts.
    std::env::remove_var("ANRI_ADMIN_PASSWORD");
    orm::doctype_sync::ensure_core_users_and_roles(&pool).await?;
    let hash = stored_admin_hash(&pool).await?.expect("admin password");
    assert!(auth.verify_password("env-password-two", &hash).await?);

    pool.close().await;
    crate::common::teardown_test_db(&path);
    Ok(())
}
