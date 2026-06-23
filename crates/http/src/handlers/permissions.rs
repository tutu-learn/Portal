use crate::AppState;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use serde_json::Value;
use std::collections::HashMap;

/// Native implementation of
/// `frappe.core.page.permission_manager.permission_manager.get_roles_and_doctypes`.
///
/// Returns the list of DocTypes, Roles, and a permission-type map needed by the
/// desk permission manager UI.
pub async fn get_roles_and_doctypes_get(State(state): State<AppState>) -> impl IntoResponse {
    get_roles_and_doctypes_impl(state).await
}

/// POST variant used by Frappe Desk, which sends `frappe.call` requests as POST.
pub async fn get_roles_and_doctypes_post(State(state): State<AppState>) -> impl IntoResponse {
    get_roles_and_doctypes_impl(state).await
}

async fn get_roles_and_doctypes_impl(state: AppState) -> impl IntoResponse {
    let pool = match state.pools.iter().next().map(|e| e.value().clone()) {
        Some(p) => p,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": "no database pool" })),
            );
        }
    };

    let doctypes = match pool
        .execute_sql(
            r#"SELECT name FROM "doctype" WHERE istable = 0 AND name NOT IN ('DocType', 'Patch Log', 'Module Def') ORDER BY name"#,
            vec![],
        )
        .await
    {
        Ok(rows) => rows
            .into_iter()
            .filter_map(|mut row| {
                let name = row.remove("name")?.as_str()?.to_string();
                Some(serde_json::json!({
                    "label": name.clone(),
                    "value": name,
                }))
            })
            .collect::<Vec<_>>(),
        Err(e) => {
            tracing::warn!("failed to load doctypes for permission manager: {}", e);
            vec![]
        }
    };

    let roles = match pool
        .execute_sql(
            r#"SELECT name FROM "role" WHERE name != 'Administrator' ORDER BY name"#,
            vec![],
        )
        .await
    {
        Ok(rows) => rows
            .into_iter()
            .filter_map(|mut row| {
                let name = row.remove("name")?.as_str()?.to_string();
                Some(serde_json::json!({
                    "label": name.clone(),
                    "value": name,
                }))
            })
            .collect::<Vec<_>>(),
        Err(e) => {
            tracing::warn!("failed to load roles for permission manager: {}", e);
            vec![]
        }
    };

    let result = serde_json::json!({
        "doctypes": doctypes,
        "roles": roles,
        "doctype_ptype_map": {},
    });

    (
        StatusCode::OK,
        Json(serde_json::json!({ "message": result })),
    )
}

/// Native implementation of
/// `frappe.core.page.permission_manager.permission_manager.get_permissions`.
pub async fn get_permissions_get(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    get_permissions_impl(state, params).await
}

/// POST variant used by Frappe Desk, which sends `frappe.call` requests as POST.
pub async fn get_permissions_post(
    State(state): State<AppState>,
    crate::extract::AnyBody(body): crate::extract::AnyBody,
) -> impl IntoResponse {
    let params = params_from_body(body);
    get_permissions_impl(state, params).await
}

async fn get_permissions_impl(
    state: AppState,
    params: HashMap<String, String>,
) -> impl IntoResponse {
    let pool = match state.pools.iter().next().map(|e| e.value().clone()) {
        Some(p) => p,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": "no database pool" })),
            );
        }
    };

    let doctype = params.get("doctype").cloned().unwrap_or_default();
    let role = params.get("role").cloned();

    // Load DocType metadata so we can enrich permission rows with flags the UI
    // expects (is_submittable, in_create).
    let mut doctype_meta: HashMap<String, serde_json::Value> = HashMap::new();
    if !doctype.is_empty() {
        if let Ok(rows) = pool
            .execute_sql(
                r#"SELECT name, is_submittable, in_create FROM "doctype" WHERE name = ?"#,
                vec![serde_json::Value::String(doctype.clone())],
            )
            .await
        {
            if let Some(mut row) = rows.into_iter().next() {
                doctype_meta.insert(
                    "is_submittable".into(),
                    row.remove("is_submittable")
                        .unwrap_or(serde_json::json!(0)),
                );
                doctype_meta.insert(
                    "in_create".into(),
                    row.remove("in_create").unwrap_or(serde_json::json!(0)),
                );
            }
        }
    }

    let mut perms = match state.permissions.get_docperms(&pool, &doctype).await {
        Ok(p) => p
            .into_iter()
            .filter(|p| p.parent == doctype)
            .map(|p| {
                serde_json::json!({
                    "parent": p.parent,
                    "role": p.role,
                    "permlevel": p.permlevel,
                    "read": p.read,
                    "write": p.write,
                    "create": p.create,
                    "delete": p.delete,
                    "submit": p.submit,
                    "cancel": p.cancel,
                    "if_owner": p.if_owner,
                    "select": p.select,
                    "report": p.report,
                    "export": p.export,
                    "import": p.import,
                    "share": p.share,
                    "print": p.print,
                    "email": p.email,
                    "mask": p.mask,
                    "amend": p.amend,
                    "is_submittable": doctype_meta.get("is_submittable").cloned().unwrap_or(serde_json::json!(0)),
                    "in_create": doctype_meta.get("in_create").cloned().unwrap_or(serde_json::json!(0)),
                    "linked_doctypes": [],
                })
            })
            .collect::<Vec<_>>(),
        Err(_) => vec![],
    };

    if let Some(role_filter) = role {
        perms.retain(|p| p.get("role").and_then(|r| r.as_str()) == Some(&role_filter));
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({ "message": perms })),
    )
}

/// Extract parameters from a Frappe `frappe.call` POST body.
///
/// The desk client typically sends either form-encoded `args=<json>` or direct
/// form fields. This helper normalises both into a plain key/value map.
/// Parse a boolean-ish form value into 0 or 1.
///
/// Accepts "1"/"0", "true"/"false", and "yes"/"no" (case-insensitive).
fn parse_boolish(value: Option<&String>) -> i64 {
    match value.map(|s| s.as_str()) {
        Some("1") | Some("true") | Some("True") | Some("TRUE") | Some("yes") | Some("Yes")
        | Some("YES") => 1,
        _ => 0,
    }
}

fn params_from_body(body: Value) -> HashMap<String, String> {
    let mut params = HashMap::new();
    let Some(map) = body.as_object() else {
        return params;
    };

    if let Some(Value::String(args)) = map.get("args") {
        if let Ok(parsed) = serde_json::from_str::<HashMap<String, Value>>(args) {
            for (k, v) in parsed {
                if let Some(s) = v.as_str() {
                    params.insert(k, s.to_string());
                }
            }
        }
    }

    for (k, v) in map {
        if k == "args" {
            continue;
        }
        if let Some(s) = v.as_str() {
            params.insert(k.clone(), s.to_string());
        }
    }

    params
}

/// Native implementation of
/// `frappe.core.page.permission_manager.permission_manager.add`.
pub async fn add_permission_post(
    State(state): State<AppState>,
    crate::extract::AnyBody(body): crate::extract::AnyBody,
) -> impl IntoResponse {
    let params = params_from_body(body);
    let pool = match state.pools.iter().next().map(|e| e.value().clone()) {
        Some(p) => p,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": "no database pool" })),
            );
        }
    };

    let doctype = params.get("parent").cloned().unwrap_or_default();
    let role = params.get("role").cloned().unwrap_or_default();
    let permlevel = params
        .get("permlevel")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);

    if doctype.is_empty() || role.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "parent and role are required" })),
        );
    }

    let exists = pool
        .execute_sql(
            r#"SELECT 1 FROM __kiff_docperm WHERE parent = ? AND role = ? AND permlevel = ? AND if_owner = 0 LIMIT 1"#,
            vec![
                serde_json::Value::String(doctype.clone()),
                serde_json::Value::String(role.clone()),
                serde_json::Value::Number(permlevel.into()),
            ],
        )
        .await;
    if let Ok(rows) = exists {
        if !rows.is_empty() {
            return (
                StatusCode::OK,
                Json(serde_json::json!({ "message": "OK" })),
            );
        }
    }

    let sql = r#"
        INSERT INTO __kiff_docperm (
            parent, role, permlevel, "read", "write", "create", "delete", "submit", "cancel",
            if_owner, "select", "report", "export", "import", "share", "print", "email", "mask", "amend"
        ) VALUES (?, ?, ?, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0)
    "#;

    match pool
        .execute_sql(
            sql,
            vec![
                serde_json::Value::String(doctype.clone()),
                serde_json::Value::String(role),
                serde_json::Value::Number(permlevel.into()),
            ],
        )
        .await
    {
        Ok(_) => {
            state.permissions.clear_perm_cache(&doctype);
            (StatusCode::OK, Json(serde_json::json!({ "message": "OK" })))
        }
        Err(e) => {
            tracing::warn!("failed to add permission: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        }
    }
}

/// Native implementation of
/// `frappe.core.page.permission_manager.permission_manager.update`.
pub async fn update_permission_post(
    State(state): State<AppState>,
    crate::extract::AnyBody(body): crate::extract::AnyBody,
) -> impl IntoResponse {
    let params = params_from_body(body);
    let pool = match state.pools.iter().next().map(|e| e.value().clone()) {
        Some(p) => p,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": "no database pool" })),
            );
        }
    };

    let doctype = params.get("doctype").cloned().unwrap_or_default();
    let role = params.get("role").cloned().unwrap_or_default();
    let permlevel = params
        .get("permlevel")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);
    let ptype = params.get("ptype").cloned().unwrap_or_default();
    let value = parse_boolish(params.get("value"));
    let if_owner = parse_boolish(params.get("if_owner"));

    if doctype.is_empty() || role.is_empty() || ptype.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "doctype, role and ptype are required" })),
        );
    }

    // Only allow known permission columns to be updated.
    let allowed = [
        "read", "write", "create", "delete", "submit", "cancel", "if_owner", "select", "report",
        "export", "import", "share", "print", "email", "mask", "amend",
    ];
    if !allowed.contains(&ptype.as_str()) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "invalid ptype" })),
        );
    }

    // If no matching row exists, create one with sensible defaults so the UI
    // can toggle permissions without requiring a separate add step.
    let exists = pool
        .execute_sql(
            r#"SELECT 1 FROM __kiff_docperm WHERE parent = ? AND role = ? AND permlevel = ? AND if_owner = ? LIMIT 1"#,
            vec![
                serde_json::Value::String(doctype.clone()),
                serde_json::Value::String(role.clone()),
                serde_json::Value::Number(permlevel.into()),
                serde_json::Value::Number(if_owner.into()),
            ],
        )
        .await;
    if let Ok(rows) = exists {
        if rows.is_empty() {
            let ensure_sql = r#"
                INSERT INTO __kiff_docperm (
                    parent, role, permlevel, "read", "write", "create", "delete", "submit", "cancel",
                    if_owner, "select", "report", "export", "import", "share", "print", "email", "mask", "amend"
                ) VALUES (?, ?, ?, 1, 0, 0, 0, 0, 0, ?, 0, 0, 0, 0, 0, 0, 0, 0, 0)
            "#;
            let _ = pool
                .execute_sql(
                    ensure_sql,
                    vec![
                        serde_json::Value::String(doctype.clone()),
                        serde_json::Value::String(role.clone()),
                        serde_json::Value::Number(permlevel.into()),
                        serde_json::Value::Number(if_owner.into()),
                    ],
                )
                .await;
        }
    }

    let update_sql = format!(
        r#"UPDATE __kiff_docperm SET "{}" = ? WHERE parent = ? AND role = ? AND permlevel = ? AND if_owner = ?"#,
        ptype
    );

    match pool
        .execute_sql(
            &update_sql,
            vec![
                serde_json::Value::Number(value.into()),
                serde_json::Value::String(doctype.clone()),
                serde_json::Value::String(role),
                serde_json::Value::Number(permlevel.into()),
                serde_json::Value::Number(if_owner.into()),
            ],
        )
        .await
    {
        Ok(_) => {
            state.permissions.clear_perm_cache(&doctype);
            (
                StatusCode::OK,
                Json(serde_json::json!({ "message": "refresh" })),
            )
        }
        Err(e) => {
            tracing::warn!("failed to update permission: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        }
    }
}

/// Native implementation of
/// `frappe.core.page.permission_manager.permission_manager.remove`.
pub async fn remove_permission_post(
    State(state): State<AppState>,
    crate::extract::AnyBody(body): crate::extract::AnyBody,
) -> impl IntoResponse {
    let params = params_from_body(body);
    let pool = match state.pools.iter().next().map(|e| e.value().clone()) {
        Some(p) => p,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": "no database pool" })),
            );
        }
    };

    let doctype = params.get("doctype").cloned().unwrap_or_default();
    let role = params.get("role").cloned().unwrap_or_default();
    let permlevel = params
        .get("permlevel")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);
    let if_owner = parse_boolish(params.get("if_owner"));

    if doctype.is_empty() || role.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "doctype and role are required" })),
        );
    }

    let sql = r#"DELETE FROM __kiff_docperm WHERE parent = ? AND role = ? AND permlevel = ? AND if_owner = ?"#;
    match pool
        .execute_sql(
            sql,
            vec![
                serde_json::Value::String(doctype.clone()),
                serde_json::Value::String(role),
                serde_json::Value::Number(permlevel.into()),
                serde_json::Value::Number(if_owner.into()),
            ],
        )
        .await
    {
        Ok(_) => {
            state.permissions.clear_perm_cache(&doctype);
            (StatusCode::OK, Json(serde_json::json!({ "message": "OK" })))
        }
        Err(e) => {
            tracing::warn!("failed to remove permission: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        }
    }
}

/// Native implementation of
/// `frappe.core.page.permission_manager.permission_manager.get_users_with_role`.
pub async fn get_users_with_role_post(
    State(state): State<AppState>,
    crate::extract::AnyBody(body): crate::extract::AnyBody,
) -> impl IntoResponse {
    let params = params_from_body(body);
    let pool = match state.pools.iter().next().map(|e| e.value().clone()) {
        Some(p) => p,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": "no database pool" })),
            );
        }
    };

    let role = params.get("role").cloned().unwrap_or_default();
    let users = match pool
        .execute_sql(
            r#"SELECT DISTINCT parent FROM "has_role" WHERE role = ? AND parenttype = 'User' ORDER BY parent"#,
            vec![serde_json::Value::String(role)],
        )
        .await
    {
        Ok(rows) => rows
            .into_iter()
            .filter_map(|mut row| row.remove("parent").and_then(|v| v.as_str().map(String::from)))
            .collect::<Vec<_>>(),
        Err(e) => {
            tracing::warn!("failed to load users with role: {}", e);
            vec![]
        }
    };

    (StatusCode::OK, Json(serde_json::json!({ "message": users })))
}
