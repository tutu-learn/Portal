use pyo3::prelude::*;

#[pyfunction]
pub fn get_roles(_py: Python<'_>, user: String) -> PyResult<Vec<String>> {
    if user == "Guest" {
        return Ok(vec!["Guest".into(), "All".into()]);
    }

    // If the python-bridge was initialized with a pool, read real roles from
    // the database. Administrator gets every available role, mirroring Frappe's
    // behaviour; other users read their has_role child table rows.
    if let Some(pool) = crate::POOL.get() {
        let runtime = crate::rt();
        return runtime.block_on(async {
            let mut roles = vec![];

            if user == "Administrator" {
                match pool
                    .execute_sql(r#"SELECT name FROM "role" WHERE disabled = 0"#, vec![])
                    .await
                {
                    Ok(rows) => {
                        for mut row in rows {
                            if let Some(role) = row
                                .remove("name")
                                .and_then(|v| v.as_str().map(String::from))
                            {
                                roles.push(role);
                            }
                        }
                    }
                    Err(_) => {
                        roles.extend([
                            "Administrator".into(),
                            "System Manager".into(),
                            "All".into(),
                        ]);
                    }
                }
            } else {
                let sql = format!(
                    r#"SELECT role FROM "has_role" WHERE parenttype = 'User' AND parent = {}"#,
                    pool.placeholder(1)
                );
                if let Ok(rows) = pool
                    .execute_sql(&sql, vec![serde_json::Value::String(user.clone())])
                    .await
                {
                    for mut row in rows {
                        if let Some(role) = row
                            .remove("role")
                            .and_then(|v| v.as_str().map(String::from))
                        {
                            roles.push(role);
                        }
                    }
                }

                for auto in ["All", "Guest"] {
                    if !roles.iter().any(|r| r == auto) {
                        roles.push(auto.into());
                    }
                }

                let user_type_sql = format!(
                    r#"SELECT user_type FROM "user" WHERE name = {}"#,
                    pool.placeholder(1)
                );
                if let Ok(rows) = pool
                    .execute_sql(
                        &user_type_sql,
                        vec![serde_json::Value::String(user.clone())],
                    )
                    .await
                {
                    if let Some(row) = rows.into_iter().next() {
                        if row.get("user_type").and_then(|v| v.as_str()) == Some("System User")
                            && !roles.iter().any(|r| r == "Desk User")
                        {
                            roles.push("Desk User".into());
                        }
                    }
                }
            }

            roles.sort_unstable();
            roles.dedup();
            Ok(roles)
        });
    }

    // Fallback when no pool is available.
    if user == "Administrator" {
        Ok(vec![
            "Administrator".into(),
            "System Manager".into(),
            "All".into(),
        ])
    } else {
        Ok(vec!["All".into()])
    }
}

#[pyfunction]
pub fn has_permission(
    _py: Python<'_>,
    _doctype: String,
    _ptype: String,
    _doc: Option<Bound<'_, PyAny>>,
    _user: Option<String>,
) -> PyResult<bool> {
    // TODO: integrate with permissions crate
    Ok(true)
}
