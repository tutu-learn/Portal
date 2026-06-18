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

    (StatusCode::OK, Json(serde_json::json!({ "message": result })))
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

async fn get_permissions_impl(state: AppState, params: HashMap<String, String>) -> impl IntoResponse {
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

    let mut perms = match state.permissions.get_docperms(&pool, &doctype).await {
        Ok(p) => p
            .into_iter()
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
                    "is_submittable": 0,
                    "in_create": 0,
                    "linked_doctypes": [],
                })
            })
            .collect::<Vec<_>>(),
        Err(_) => vec![],
    };

    if let Some(role_filter) = role {
        perms.retain(|p| p.get("role").and_then(|r| r.as_str()) == Some(&role_filter));
    }

    (StatusCode::OK, Json(serde_json::json!({ "message": perms })))
}

/// Extract parameters from a Frappe `frappe.call` POST body.
///
/// The desk client typically sends either form-encoded `args=<json>` or direct
/// form fields. This helper normalises both into a plain key/value map.
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
