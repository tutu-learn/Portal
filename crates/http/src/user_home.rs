use orm::DatabasePool;

/// Read the configured home page for a user.
pub async fn get_user_home_page(pool: &DatabasePool, user_name: &str) -> Option<String> {
    let rows = pool
        .execute_sql(
            &format!(
                r#"SELECT home_page FROM "user" WHERE name = {} LIMIT 1"#,
                pool.placeholder(1)
            ),
            vec![serde_json::Value::String(user_name.into())],
        )
        .await
        .ok()?;

    rows.into_iter()
        .next()
        .and_then(|mut row| row.remove("home_page"))
        .and_then(|v| v.as_str().map(String::from))
        .filter(|s| !s.is_empty())
}

/// Persist a user's home page.
pub async fn set_user_home_page(
    pool: &DatabasePool,
    user_name: &str,
    home_page: &str,
) -> error::Result<()> {
    pool.execute_sql(
        &format!(
            r#"UPDATE "user" SET home_page = {} WHERE name = {}"#,
            pool.placeholder(1),
            pool.placeholder(2)
        ),
        vec![
            serde_json::Value::String(home_page.into()),
            serde_json::Value::String(user_name.into()),
        ],
    )
    .await?;
    Ok(())
}
