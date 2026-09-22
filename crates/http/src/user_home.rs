use orm::DatabasePool;

/// Normalize a stored home-page path (the user's own override, or the
/// admin-set default): coerce to a leading slash, rewrite the legacy `/app`
/// prefix to `/desk`, and reject protocol-relative paths (`//host/...`),
/// which would otherwise act as an open redirect. Returns an empty string
/// for blank or unsafe input.
pub fn normalize_stored_home_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let with_slash = if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    };
    if with_slash.starts_with("//") {
        return String::new();
    }
    if let Some(rest) = with_slash.strip_prefix("/app") {
        format!("/desk{rest}")
    } else {
        with_slash
    }
}

/// Validate `[auth] custom_home_path` / `CUSTOM_HOME_PATH` (the global,
/// app-provided post-login landing page). Unlike [`normalize_stored_home_path`]
/// this does not forgive a missing leading slash — a malformed config value
/// should be rejected outright rather than silently coerced.
pub fn valid_configured_home_path(path: Option<&str>) -> Option<String> {
    let path = path?.trim();
    if path.is_empty() || !path.starts_with('/') || path.starts_with("//") {
        return None;
    }
    Some(path.to_string())
}

/// Resolve the effective landing page for a user, in priority order:
/// 1. the user's own override (`User.user_home_page`)
/// 2. the admin-set default (`User.home_page`)
/// 3. the global `custom_home_path` setting
///
/// Returns `None` if none of the three yields a usable path.
pub async fn get_effective_home_page(
    pool: &DatabasePool,
    user_name: &str,
    global_default: Option<&str>,
) -> Option<String> {
    let rows = pool
        .execute_sql(
            &format!(
                r#"SELECT user_home_page, home_page FROM "user" WHERE name = {} LIMIT 1"#,
                pool.placeholder(1)
            ),
            vec![serde_json::Value::String(user_name.into())],
        )
        .await
        .ok()?;

    let mut row = rows.into_iter().next()?;

    let user_set = row
        .remove("user_home_page")
        .and_then(|v| v.as_str().map(normalize_stored_home_path))
        .filter(|s| !s.is_empty());
    if user_set.is_some() {
        return user_set;
    }

    let admin_set = row
        .remove("home_page")
        .and_then(|v| v.as_str().map(normalize_stored_home_path))
        .filter(|s| !s.is_empty());
    if admin_set.is_some() {
        return admin_set;
    }

    valid_configured_home_path(global_default)
}

/// Persist the current user's own home-page override.
pub async fn set_user_home_page(
    pool: &DatabasePool,
    user_name: &str,
    home_page: &str,
) -> error::Result<()> {
    pool.execute_sql(
        &format!(
            r#"UPDATE "user" SET user_home_page = {} WHERE name = {}"#,
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

/// Persist the admin-set default home page for a given user.
pub async fn set_admin_home_page(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_stored_home_path_coerces_leading_slash_and_rewrites_app() {
        assert_eq!(normalize_stored_home_path("app/crm"), "/desk/crm");
        assert_eq!(normalize_stored_home_path("/app"), "/desk");
        assert_eq!(normalize_stored_home_path("/desk/build"), "/desk/build");
    }

    #[test]
    fn normalize_stored_home_path_rejects_blank_and_open_redirect() {
        assert_eq!(normalize_stored_home_path(""), "");
        assert_eq!(normalize_stored_home_path("   "), "");
        assert_eq!(normalize_stored_home_path("//evil.example.com"), "");
    }

    #[test]
    fn valid_configured_home_path_accepts_app_provided_portal() {
        assert_eq!(
            valid_configured_home_path(Some("/sebrus_logger/dashboard")),
            Some("/sebrus_logger/dashboard".to_string())
        );
    }

    #[test]
    fn valid_configured_home_path_rejects_unset_and_malformed() {
        assert_eq!(valid_configured_home_path(None), None);
        assert_eq!(valid_configured_home_path(Some("")), None);
        assert_eq!(valid_configured_home_path(Some("not-a-path")), None);
        assert_eq!(
            valid_configured_home_path(Some("//evil.example.com")),
            None
        );
    }
}
