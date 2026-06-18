use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Json, IntoResponse},
};
use orm::FilterCondition;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    pub fields: Option<String>,
    #[serde(default)]
    pub filters: Option<String>,
    #[serde(default)]
    pub order_by: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

pub async fn get_list(
    State(state): State<AppState>,
    Path(doctype): Path<String>,
    Query(q): Query<ListQuery>,
) -> impl IntoResponse {
    let fields = q.fields.map(|s| s.split(',').map(|x| x.trim().to_string()).collect());
    let filters: Option<HashMap<String, FilterCondition>> = q.filters.and_then(|s| {
        let raw: Option<HashMap<String, Value>> = serde_json::from_str(&s).ok();
        raw.map(|m| m.into_iter().map(|(k, v)| (k, FilterCondition::Eq(v))).collect())
    });

    // Use first pool for now
    let pool = state.pools.iter().next().map(|e| e.value().clone());
    match pool {
        Some(pool) => match pool.get_list(&doctype, filters, fields, q.order_by.as_deref(), q.limit).await {
            Ok(docs) => (StatusCode::OK, Json(serde_json::json!({ "data": docs }))),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("{}", e) }))),
        },
        None => (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({ "error": "no database pool" }))),
    }
}

pub async fn get_doc(
    State(state): State<AppState>,
    Path((doctype, name)): Path<(String, String)>,
) -> impl IntoResponse {
    let pool = state.pools.iter().next().map(|e| e.value().clone());
    match pool {
        Some(pool) => match pool.get_doc(&doctype, &name).await {
            Ok(doc) => (StatusCode::OK, Json(serde_json::json!({ "data": doc }))),
            Err(e) => (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": format!("{}", e) }))),
        },
        None => (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({ "error": "no database pool" }))),
    }
}

#[derive(Debug, Deserialize)]
pub struct InsertBody {
    #[serde(flatten)]
    pub fields: HashMap<String, Value>,
}

pub async fn insert_doc(
    State(state): State<AppState>,
    Path(doctype): Path<String>,
    Json(body): Json<InsertBody>,
) -> impl IntoResponse {
    let pool = state.pools.iter().next().map(|e| e.value().clone());
    match pool {
        Some(pool) => {
            let mut doc = orm::Document::new(doctype.clone(), uuid::Uuid::new_v4().to_string());
            for (k, v) in body.fields {
                doc.set_field(k, v);
            }
            match pool.insert_doc(&doc).await {
                Ok(name) => (StatusCode::CREATED, Json(serde_json::json!({ "data": { "name": name } }))),
                Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("{}", e) }))),
            }
        }
        None => (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({ "error": "no database pool" }))),
    }
}

pub async fn update_doc(
    State(state): State<AppState>,
    Path((doctype, name)): Path<(String, String)>,
    Json(body): Json<HashMap<String, Value>>,
) -> impl IntoResponse {
    let pool = state.pools.iter().next().map(|e| e.value().clone());
    match pool {
        Some(pool) => match pool.get_doc(&doctype, &name).await {
            Ok(mut doc) => {
                for (k, v) in body {
                    doc.set_field(k, v);
                }
                match pool.save_doc(&doc).await {
                    Ok(_) => (StatusCode::OK, Json(serde_json::json!({ "data": doc }))),
                    Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("{}", e) }))),
                }
            }
            Err(e) => (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": format!("{}", e) }))),
        },
        None => (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({ "error": "no database pool" }))),
    }
}

pub async fn delete_doc(
    State(state): State<AppState>,
    Path((doctype, name)): Path<(String, String)>,
) -> impl IntoResponse {
    let pool = state.pools.iter().next().map(|e| e.value().clone());
    match pool {
        Some(pool) => match pool.delete_doc(&doctype, &name).await {
            Ok(_) => (StatusCode::OK, Json(serde_json::json!({ "message": "deleted" }))),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("{}", e) }))),
        },
        None => (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({ "error": "no database pool" }))),
    }
}

/// Native Rust implementation of frappe.desk.desk_page.getpage.
/// Loads page metadata and assets from JSON/files in apps/frappe/frappe/*/page/
/// so the desk can render pages such as the permission manager.
pub async fn getpage(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let name = params.get("name").cloned().unwrap_or_default();
    getpage_response(&state, &name, &headers).await
}

pub async fn getpage_post(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    crate::extract::AnyBody(body): crate::extract::AnyBody,
) -> impl IntoResponse {
    let name = extract_name_from_body(body);
    getpage_response(&state, &name, &headers).await
}

fn extract_name_from_body(body: Value) -> String {
    match body {
        Value::Object(mut map) => {
            if let Some(Value::String(name)) = map.remove("name") {
                return name;
            }
            if let Some(args) = map.remove("args") {
                return match args {
                    Value::String(s) => serde_json::from_str::<HashMap<String, Value>>(&s)
                        .ok()
                        .and_then(|mut m| m.remove("name"))
                        .and_then(|v| match v {
                            Value::String(s) => Some(s),
                            _ => None,
                        })
                        .unwrap_or_default(),
                    Value::Object(mut m) => m
                        .remove("name")
                        .and_then(|v| match v {
                            Value::String(s) => Some(s),
                            _ => None,
                        })
                        .unwrap_or_default(),
                    _ => String::new(),
                };
            }
            String::new()
        }
        _ => String::new(),
    }
}

async fn getpage_response(
    state: &AppState,
    name: &str,
    headers: &axum::http::HeaderMap,
) -> (StatusCode, Json<Value>) {
    let user = session_user_from_request(state, headers).await;

    // Basic permission check: require an authenticated user. Pages with role
    // restrictions are checked below.
    let user = match user {
        Some(u) => u,
        None => {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({ "exc": "Not permitted" })),
            );
        }
    };

    match load_page_from_json(state, name, &user).await {
        Ok(doc) => {
            let mut resp = serde_json::Map::new();
            resp.insert("docs".to_string(), serde_json::Value::Array(vec![doc]));
            (StatusCode::OK, Json(serde_json::Value::Object(resp)))
        }
        Err(ref e) if e == "not_permitted" => (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "exc": "Not permitted" })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("{}", e) })),
        ),
    }
}

async fn load_page_from_json(state: &AppState, name: &str, user: &str) -> Result<serde_json::Value, String> {
    let scrubbed = name.to_lowercase().replace(" ", "_").replace("-", "_");
    let base = PathBuf::from("apps/frappe/frappe");

    let mut page_path = None;
    if let Ok(entries) = std::fs::read_dir(&base) {
        for entry in entries.flatten() {
            let path = entry.path().join("page").join(&scrubbed).join(format!("{}.json", scrubbed));
            if path.exists() {
                page_path = Some(path);
                break;
            }
        }
    }

    let path = page_path.ok_or_else(|| format!("page json not found for {}", name))?;
    let content = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| format!("read error: {}", e))?;
    let mut doc: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("parse error: {}", e))?;

    // Enforce page roles if defined.
    let allowed_roles: Vec<String> = doc
        .get("roles")
        .and_then(|r| r.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.get("role").and_then(|r| r.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default();

    if !allowed_roles.is_empty() {
        let user_roles = get_user_roles(&state, user).await;
        let has_role = user_roles.iter().any(|r| allowed_roles.contains(r));
        if !has_role && user != "Administrator" {
            return Err("not_permitted".into());
        }
    }

    // Load assets from the page directory.
    let dir = path.parent().unwrap().to_path_buf();
    let js_path = dir.join(format!("{}.js", scrubbed));
    let css_path = dir.join(format!("{}.css", scrubbed));

    // Convert any .html templates in the page directory into frappe.templates
    // entries, matching Frappe's Page.load_assets behaviour. This lets page
    // scripts call frappe.render_template("<name>", {}) without the template
    // needing to be bundled into a desk asset bundle.
    let mut template_script = String::new();
    let mut entries = tokio::fs::read_dir(&dir)
        .await
        .map_err(|e| format!("read dir error: {}", e))?;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("html") {
            let content = tokio::fs::read_to_string(&path)
                .await
                .map_err(|e| format!("read html error: {}", e))?;
            let filename = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
            template_script.push_str(&html_to_js_template(filename, &content));
        }
    }

    let script = if js_path.exists() {
        let js = tokio::fs::read_to_string(&js_path)
            .await
            .map_err(|e| format!("read script error: {}", e))?;
        format!("{}{}", template_script, js)
    } else {
        template_script
    };

    let style = if css_path.exists() {
        tokio::fs::read_to_string(&css_path)
            .await
            .map_err(|e| format!("read style error: {}", e))?
    } else {
        String::new()
    };

    if let serde_json::Value::Object(ref mut map) = doc {
        map.insert("script".to_string(), serde_json::Value::String(script));
        map.insert("style".to_string(), serde_json::Value::String(style));
    }

    Ok(doc)
}

/// Convert HTML template content into a `frappe.templates` JS assignment,
/// mirroring Frappe's `frappe.build.html_to_js_template`.
fn html_to_js_template(name: &str, content: &str) -> String {
    let scrubbed = scrub_html_template(content);
    format!("frappe.templates[\"{}\"] = '{}';\n", name, scrubbed)
}

/// Scrub HTML template content so it can safely live inside a single-quoted
/// JavaScript string. Whitespace is collapsed, HTML comments are removed, and
/// characters that would break the JS string are escaped.
fn scrub_html_template(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let mut in_comment = false;
    let mut chars = content.chars().peekable();

    while let Some(c) = chars.next() {
        if in_comment {
            if c == '-' && chars.peek() == Some(&'-') {
                chars.next();
                if chars.peek() == Some(&'>') {
                    chars.next();
                    in_comment = false;
                }
            }
            continue;
        }

        if c == '<' && chars.peek() == Some(&'!') {
            chars.next();
            if chars.peek() == Some(&'-') {
                chars.next();
                if chars.peek() == Some(&'-') {
                    chars.next();
                    in_comment = true;
                    continue;
                }
                result.push('<');
                result.push('!');
                result.push('-');
                continue;
            }
            result.push('<');
            result.push('!');
            continue;
        }

        if c.is_whitespace() {
            result.push(' ');
            while chars.peek().map(|c| c.is_whitespace()).unwrap_or(false) {
                chars.next();
            }
        } else if c == '\\' {
            result.push_str("\\\\");
        } else if c == '\'' {
            result.push_str("\\'");
        } else {
            result.push(c);
        }
    }

    result
}

async fn get_user_roles(state: &AppState, user: &str) -> Vec<String> {
    let pool = state.pools.iter().next().map(|e| e.value().clone());
    match pool {
        Some(pool) => state
            .permissions
            .get_roles(&pool, user)
            .await
            .unwrap_or_default(),
        None => {
            // Fallback for tests / no-pool scenarios.
            if user == "Administrator" {
                vec![
                    "Administrator".into(),
                    "System Manager".into(),
                    "All".into(),
                ]
            } else {
                vec!["All".into()]
            }
        }
    }
}

/// Native Rust implementation of frappe.desk.form.load.getdoctype.
/// Loads doctype metadata from JSON files in apps/frappe/frappe/*/doctype/
/// instead of relying on the Python bridge and missing DB tables.
pub async fn getdoctype_native(
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let doctype = params.get("doctype").cloned().unwrap_or_default();
    let _with_parent = params.get("with_parent").map(|s| s == "1").unwrap_or(false);
    let cached_timestamp = params.get("cached_timestamp").cloned().unwrap_or_default();

    match load_doctype_from_json(&doctype, &cached_timestamp).await {
        Ok(docs) => {
            let mut resp = serde_json::Map::new();
            resp.insert("docs".to_string(), serde_json::Value::Array(docs));
            resp.insert("user_settings".to_string(), serde_json::Value::String("{}".into()));
            (StatusCode::OK, Json(serde_json::Value::Object(resp)))
        }
        Err(ref e) if e == "use_cache" => {
            let mut resp = serde_json::Map::new();
            resp.insert("message".to_string(), serde_json::Value::String("use_cache".into()));
            resp.insert("docs".to_string(), serde_json::json!([]));
            resp.insert("user_settings".to_string(), serde_json::Value::String("{}".into()));
            (StatusCode::OK, Json(serde_json::Value::Object(resp)))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("{}", e) })),
        ),
    }
}

async fn load_doctype_from_json(
    doctype: &str,
    cached_timestamp: &str,
) -> Result<Vec<serde_json::Value>, String> {
    let scrubbed = doctype.to_lowercase().replace(" ", "_").replace("-", "_");

    // Search in apps/frappe/frappe/*/doctype/<scrubbed>/<scrubbed>.json
    let base = PathBuf::from("apps/frappe/frappe");
    let mut found = None;

    if let Ok(entries) = std::fs::read_dir(&base) {
        for entry in entries.flatten() {
            let path = entry.path().join("doctype").join(&scrubbed).join(format!("{}.json", scrubbed));
            if path.exists() {
                found = Some(path);
                break;
            }
        }
    }

    let path = found.ok_or_else(|| format!("doctype json not found for {}", doctype))?;
    let content = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| format!("read error: {}", e))?;
    load_doctype_from_content(doctype, &content, cached_timestamp)
}

fn load_doctype_from_content(
    doctype: &str,
    content: &str,
    cached_timestamp: &str,
) -> Result<Vec<serde_json::Value>, String> {
    let mut doc: serde_json::Value =
        serde_json::from_str(content).map_err(|e| format!("parse error: {}", e))?;

    // Check cache timestamp
    if !cached_timestamp.is_empty() {
        if let Some(modified) = doc.get("modified").and_then(|m| m.as_str()) {
            if modified == cached_timestamp {
                return Err("use_cache".into());
            }
        }
    }

    let doctype_name = doc.get("name").and_then(|v| v.as_str()).unwrap_or(doctype).to_string();

    // Ensure child fields have doctype/parent/parentfield/idx set
    if let Some(fields) = doc.get_mut("fields").and_then(|f| f.as_array_mut()) {
        for (idx, field) in fields.iter_mut().enumerate() {
            if let serde_json::Value::Object(map) = field {
                map.entry("doctype".to_string())
                    .or_insert(serde_json::Value::String("DocField".into()));
                map.entry("parent".to_string())
                    .or_insert(serde_json::Value::String(doctype_name.clone()));
                map.entry("parenttype".to_string())
                    .or_insert(serde_json::Value::String("DocType".into()));
                map.entry("parentfield".to_string())
                    .or_insert(serde_json::Value::String("fields".into()));
                map.entry("idx".to_string())
                    .or_insert(serde_json::Value::Number((idx + 1).into()));
            }
        }
    }

    // Ensure permissions have doctype/parent/parentfield/idx set
    if let Some(perms) = doc.get_mut("permissions").and_then(|p| p.as_array_mut()) {
        for (idx, perm) in perms.iter_mut().enumerate() {
            if let serde_json::Value::Object(map) = perm {
                map.entry("doctype".to_string())
                    .or_insert(serde_json::Value::String("DocPerm".into()));
                map.entry("parent".to_string())
                    .or_insert(serde_json::Value::String(doctype_name.clone()));
                map.entry("parenttype".to_string())
                    .or_insert(serde_json::Value::String("DocType".into()));
                map.entry("parentfield".to_string())
                    .or_insert(serde_json::Value::String("permissions".into()));
                map.entry("idx".to_string())
                    .or_insert(serde_json::Value::Number((idx + 1).into()));
            }
        }
    }

    let mut docs = vec![doc.clone()];

    // Bundle child-table metas so forms with Table / Table MultiSelect render.
    let table_fieldtypes = ["Table", "Table MultiSelect"];
    let child_doctypes: Vec<String> = doc.get("fields")
        .and_then(|f| f.as_array())
        .map(|arr| arr.iter()
            .filter(|f| f.get("fieldtype").and_then(|v| v.as_str()).map(|t| table_fieldtypes.contains(&t)).unwrap_or(false))
            .filter_map(|f| f.get("options").and_then(|v| v.as_str()).map(|s| s.to_string()))
            .collect())
        .unwrap_or_default();

    for child_dt in child_doctypes {
        if child_dt == doctype {
            continue;
        }
        if let Ok(child_meta) = load_child_doctype_from_json(&child_dt, cached_timestamp) {
            docs.extend(child_meta);
        }
    }

    Ok(docs)
}

fn load_child_doctype_from_json(
    doctype: &str,
    cached_timestamp: &str,
) -> Result<Vec<serde_json::Value>, String> {
    let scrubbed = doctype.to_lowercase().replace(" ", "_").replace("-", "_");
    let base = PathBuf::from("apps/frappe/frappe");

    if let Ok(entries) = std::fs::read_dir(&base) {
        for entry in entries.flatten() {
            let path = entry.path().join("doctype").join(&scrubbed).join(format!("{}.json", scrubbed));
            if path.exists() {
                let content = std::fs::read_to_string(&path)
                    .map_err(|e| format!("read error: {}", e))?;
                return load_doctype_from_content(doctype, &content, cached_timestamp);
            }
        }
    }

    Err(format!("doctype json not found for {}", doctype))
}

fn extract_cookie_value(header: &str, name: &str) -> Option<String> {
    for pair in header.split(';') {
        let pair = pair.trim();
        if let Some((key, value)) = pair.split_once('=') {
            if key.trim() == name {
                return Some(value.trim().to_string());
            }
        }
    }
    None
}

async fn session_user_from_request(state: &AppState, headers: &axum::http::HeaderMap) -> Option<String> {
    let cookie_header = headers.get("cookie")?.to_str().ok()?;
    let sid = extract_cookie_value(cookie_header, "sid")?;
    let pool = state.pools.iter().next()?.value().clone();
    let store = session::SessionStore::new();
    match store.get(&pool, &sid).await {
        Ok(Some(session)) if !session.is_expired() => Some(session.user),
        _ => None,
    }
}

pub async fn call_method_get(
    State(state): State<AppState>,
    Path(method): Path<String>,
    headers: axum::http::HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let params: HashMap<String, Value> = params
        .into_iter()
        .map(|(k, v)| (k, Value::String(v)))
        .collect();

    match call_rust_or_python_method(&state, &method, params, &headers).await {
        Ok(result) => (StatusCode::OK, Json(result)),
        Err(e) => frappe_error_response(e),
    }
}

pub async fn call_method(
    State(state): State<AppState>,
    Path(method): Path<String>,
    headers: axum::http::HeaderMap,
    crate::extract::AnyBody(body): crate::extract::AnyBody,
) -> impl IntoResponse {
    let params = match body {
        Value::Object(map) => map.into_iter().collect::<HashMap<String, Value>>(),
        _ => HashMap::new(),
    };

    match call_rust_or_python_method(&state, &method, params, &headers).await {
        Ok(result) => (StatusCode::OK, Json(result)),
        Err(e) => frappe_error_response(e),
    }
}

async fn call_rust_or_python_method(
    state: &AppState,
    method: &str,
    params: HashMap<String, Value>,
    headers: &axum::http::HeaderMap,
) -> error::Result<Value> {
    // Try Rust apps first.
    if let Some(result) = state.rust_apps.call_method(method, state.clone(), params.clone()).await? {
        return Ok(result);
    }

    // Fall back to Python method dispatcher.
    let user = session_user_from_request(state, headers).await;
    let body = serde_json::to_value(params).unwrap_or(Value::Object(Default::default()));
    kiff_core::call_method_with_user(method, &body, user.as_deref())
}

fn frappe_error_response(e: error::RuntimeError) -> (StatusCode, Json<serde_json::Value>) {
    match e {
        error::RuntimeError::Python(msg) => {
            // Frappe JS (request.js) calls JSON.parse(r.exc), so exc must be a
            // JSON-encoded array string e.g. '["Traceback..."]', not a raw string.
            let exc_json = serde_json::to_string(&serde_json::json!([msg]))
                .unwrap_or_else(|_| "[]".to_string());
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "exc": exc_json,
                    "exc_type": "RuntimeError",
                    "_server_messages": "[]"
                })),
            )
        }
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("{}", e) })),
        ),
    }
}
