use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    http::{HeaderValue, StatusCode, header::SET_COOKIE},
    response::{IntoResponse, Json, Redirect},
};
use orm::FilterCondition;
use serde::Deserialize;
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
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    // Try real Frappe Document.delete() first so Password fields are cleaned
    // up from __auth and document hooks run. Fall back to the native ORM
    // delete if Python is unavailable.
    let mut params = std::collections::HashMap::new();
    params.insert("doctype".to_string(), serde_json::Value::String(doctype.clone()));
    params.insert("name".to_string(), serde_json::Value::String(name.clone()));

    match call_rust_or_python_method(&state, "frappe.client.delete", params, &headers).await {
        Ok(_) => return (StatusCode::OK, Json(serde_json::json!({ "message": "deleted" }))),
        Err(error::RuntimeError::Python(_)) => {
            // Python failed; fall through to native delete rather than returning
            // the error immediately. Some DocType controllers may not yet load
            // cleanly under the shim, but the row should still be removable.
        }
        Err(e) => return frappe_error_response(e),
    }

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
    let path = find_doctype_json_path(doctype)
        .ok_or_else(|| format!("doctype json not found for {}", doctype))?;
    let content = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| format!("read error: {}", e))?;
    let (js, css) = read_doctype_assets(&path).await?;
    load_doctype_from_content(doctype, &content, cached_timestamp, js, css)
}

fn find_doctype_json_path(doctype: &str) -> Option<PathBuf> {
    let scrubbed = doctype.to_lowercase().replace(" ", "_").replace("-", "_");

    // 1. Search in bundled Frappe app tree by path convention.
    let frappe_base = PathBuf::from("apps/frappe/frappe");
    if let Ok(entries) = std::fs::read_dir(&frappe_base) {
        for entry in entries.flatten() {
            let path = entry
                .path()
                .join("doctype")
                .join(&scrubbed)
                .join(format!("{}.json", scrubbed));
            if path.exists() {
                return Some(path);
            }
        }
    }

    // 2. Search in Rust app doctype fixtures by JSON name.
    if let Some(path) = find_doctype_in_apps_dir("rust_apps", doctype) {
        return Some(path);
    }

    // 3. Search in crates/* rust apps (e.g. kiff_logger).
    if let Some(path) = find_doctype_in_apps_dir("crates", doctype) {
        return Some(path);
    }

    None
}

fn find_doctype_in_apps_dir(apps_dir: &str, doctype: &str) -> Option<PathBuf> {
    let base = PathBuf::from(apps_dir);
    let Ok(app_entries) = std::fs::read_dir(&base) else {
        return None;
    };
    for app_entry in app_entries.flatten() {
        let doctypes_dir = app_entry.path().join("src").join("doctypes");
        if !doctypes_dir.exists() {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&doctypes_dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Ok(files) = std::fs::read_dir(&path) else {
                continue;
            };
            for file in files.flatten() {
                let file_path = file.path();
                if file_path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                if let Ok(content) = std::fs::read_to_string(&file_path) {
                    if let Ok(doc) = serde_json::from_str::<serde_json::Value>(&content) {
                        if doc.get("name")
                            .and_then(|v| v.as_str())
                            .map(|n| n == doctype)
                            .unwrap_or(false)
                        {
                            return Some(file_path);
                        }
                    }
                }
            }
        }
    }
    None
}

fn doctype_asset_paths(
    path: &std::path::Path,
) -> (Option<std::path::PathBuf>, Option<std::path::PathBuf>) {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    let dir = path.parent().unwrap_or_else(|| std::path::Path::new(""));
    let js_path = dir.join(format!("{}.js", stem));
    let css_path = dir.join(format!("{}.css", stem));
    (
        if js_path.exists() { Some(js_path) } else { None },
        if css_path.exists() { Some(css_path) } else { None },
    )
}

async fn read_doctype_assets(
    path: &std::path::Path,
) -> Result<(Option<String>, Option<String>), String> {
    let (js_path, css_path) = doctype_asset_paths(path);
    let js = match js_path {
        Some(p) => Some(
            tokio::fs::read_to_string(&p)
                .await
                .map_err(|e| format!("read doctype js error: {}", e))?,
        ),
        None => None,
    };
    let css = match css_path {
        Some(p) => Some(
            tokio::fs::read_to_string(&p)
                .await
                .map_err(|e| format!("read doctype css error: {}", e))?,
        ),
        None => None,
    };
    Ok((js, css))
}

fn read_doctype_assets_sync(
    path: &std::path::Path,
) -> Result<(Option<String>, Option<String>), String> {
    let (js_path, css_path) = doctype_asset_paths(path);
    let js = match js_path {
        Some(p) => Some(
            std::fs::read_to_string(&p)
                .map_err(|e| format!("read doctype js error: {}", e))?,
        ),
        None => None,
    };
    let css = match css_path {
        Some(p) => Some(
            std::fs::read_to_string(&p)
                .map_err(|e| format!("read doctype css error: {}", e))?,
        ),
        None => None,
    };
    Ok((js, css))
}

fn load_doctype_from_content(
    doctype: &str,
    content: &str,
    cached_timestamp: &str,
    js: Option<String>,
    css: Option<String>,
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

    // Inject client-side controller script and stylesheet so form-level
    // behaviour (e.g. the User RoleEditor) runs in the desk.
    if let serde_json::Value::Object(ref mut map) = doc {
        if let Some(js_code) = js {
            map.insert("__js".to_string(), serde_json::Value::String(js_code));
        }
        if let Some(css_code) = css {
            map.insert("__css".to_string(), serde_json::Value::String(css_code));
        }
    }

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
    let path = find_doctype_json_path(doctype)
        .ok_or_else(|| format!("doctype json not found for {}", doctype))?;
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("read error: {}", e))?;
    let (js, css) = read_doctype_assets_sync(&path)?;
    load_doctype_from_content(doctype, &content, cached_timestamp, js, css)
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

    method_response(&state, &method, params, &headers).await
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

    method_response(&state, &method, params, &headers).await
}

/// Output of a method call: either a normal JSON payload or an HTTP redirect.
enum MethodResponse {
    Json(serde_json::Value),
    Redirect { location: String, cookie: Option<String> },
}

impl IntoResponse for MethodResponse {
    fn into_response(self) -> axum::response::Response {
        match self {
            MethodResponse::Json(value) => (StatusCode::OK, Json(value)).into_response(),
            MethodResponse::Redirect { location, cookie } => {
                let mut res = Redirect::temporary(&location).into_response();
                if let Some(cookie) = cookie {
                    if let Ok(value) = HeaderValue::from_str(&cookie) {
                        res.headers_mut().insert(SET_COOKIE, value);
                    }
                }
                res
            }
        }
    }
}

async fn method_response(
    state: &AppState,
    method: &str,
    params: HashMap<String, Value>,
    headers: &axum::http::HeaderMap,
) -> axum::response::Response {
    match call_rust_or_python_method(state, method, params, headers).await {
        Ok(result) => result.into_response(),
        Err(e) => frappe_error_response(e).into_response(),
    }
}

async fn call_rust_or_python_method(
    state: &AppState,
    method: &str,
    params: HashMap<String, Value>,
    headers: &axum::http::HeaderMap,
) -> error::Result<MethodResponse> {
    // Try Rust apps first.
    if let Some(result) = state.rust_apps.call_method(method, state.clone(), params.clone()).await? {
        // Frappe clients expect { "message": <value> } for /api/method/* calls.
        return Ok(MethodResponse::Json(serde_json::json!({ "message": result })));
    }

    // Fall back to Python method dispatcher.
    let user = session_user_from_request(state, headers).await;
    let body = serde_json::to_value(params).unwrap_or(Value::Object(Default::default()));
    let result = kiff_core::call_method_with_user(method, &body, user.as_deref())?;

    // Frappe login/OAuth flows signal a redirect by setting
    // frappe.local.response["type"] == "redirect" with a "location" URL.
    // Detect that and return a real HTTP redirect (with a session cookie when
    // the Python flow just authenticated a user).
    if let Some(map) = result.as_object() {
        if map.get("type").and_then(|v| v.as_str()) == Some("redirect") {
            if let Some(location) = map.get("location").and_then(|v| v.as_str()) {
                let cookie = session_cookie_after_py_login(state, user.as_deref()).await;
                return Ok(MethodResponse::Redirect {
                    location: location.to_string(),
                    cookie,
                });
            }
        }
    }

    Ok(MethodResponse::Json(result))
}

/// If the Python method just authenticated a user (request had no session but
/// frappe.session is now a real user), create a persisted Rust session and
/// return a formatted Set-Cookie header.
async fn session_cookie_after_py_login(
    state: &AppState,
    existing_user: Option<&str>,
) -> Option<String> {
    // Don't create a new session if the request already carried a valid one.
    if existing_user.is_some() {
        return None;
    }
    let pool = state.pools.iter().next()?.value().clone();
    let py_user = kiff_core::current_py_session_user()?;
    if py_user == "Guest" {
        return None;
    }
    let store = session::SessionStore::new();
    let session = store.create(&pool, py_user, "localhost".into()).await.ok()?;
    Some(format!(
        "sid={}; Path=/; HttpOnly; SameSite=Lax; Max-Age=86400",
        session.id
    ))
}

fn parse_python_exception(msg: &str) -> (String, String) {
    // Traceback ends with lines like:
    //   _frappe_exceptions_real.InvalidEmailAddressError: ddd is not a valid Email Address
    //   _frappe_exceptions_real.PermissionError
    // Extract the short exception type and the human-readable message.
    let last_line = msg.lines().last().unwrap_or(msg).trim();
    let (type_part, message_part) = match last_line.split_once(": ") {
        Some((ty, message)) => (ty, Some(message)),
        None => (last_line, None),
    };

    let exc_type = type_part
        .split('.')
        .last()
        .filter(|s| s.ends_with("Error") || s.ends_with("Exception") || s.ends_with("Warning"))
        .unwrap_or("RuntimeError")
        .to_string();

    let exc_msg = message_part
        .map(|m| m.to_string())
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| exc_type.clone());

    (exc_type, exc_msg)
}

fn frappe_error_response(e: error::RuntimeError) -> (StatusCode, Json<serde_json::Value>) {
    match e {
        error::RuntimeError::Python(msg) => {
            let (exc_type, exc_msg) = parse_python_exception(&msg);

            // Frappe JS (request.js) calls JSON.parse(r.exc), so exc must be a
            // JSON-encoded array string e.g. '["Traceback..."]', not a raw string.
            let exc_json = serde_json::to_string(&serde_json::json!([msg]))
                .unwrap_or_else(|_| "[]".to_string());

            // Make validation/error messages visible in the desk UI.
            let server_message = serde_json::json!({
                "message": exc_msg,
                "title": exc_type.clone(),
                "indicator": "red",
            });
            let server_messages_json = serde_json::to_string(&serde_json::json!([
                serde_json::to_string(&server_message).unwrap_or_default()
            ]))
            .unwrap_or_else(|_| "[]".to_string());

            (
                // Frappe's JS client routes HTTP 417 to the error callback and
                // shows _server_messages; returning 200 caused it to treat the
                // exception as a successful save, leading to TypeErrors.
                StatusCode::EXPECTATION_FAILED,
                Json(serde_json::json!({
                    "exc": exc_json,
                    "exc_type": exc_type,
                    "_server_messages": server_messages_json,
                })),
            )
        }
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("{}", e) })),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    #[tokio::test]
    async fn method_response_redirect_sets_location_and_cookie() {
        let response = MethodResponse::Redirect {
            location: "/desk".to_string(),
            cookie: Some("sid=abc123; Path=/; HttpOnly; SameSite=Lax; Max-Age=86400".to_string()),
        }
        .into_response();

        assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT);
        assert_eq!(
            response.headers().get("location").unwrap().to_str().unwrap(),
            "/desk"
        );
        assert!(
            response
                .headers()
                .get_all("set-cookie")
                .iter()
                .any(|c| c.to_str().unwrap().starts_with("sid=abc123")),
            "redirect should set sid cookie"
        );
    }

    #[tokio::test]
    async fn method_response_json_returns_ok() {
        let response = MethodResponse::Json(serde_json::json!({ "message": "ok" })).into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["message"], "ok");
    }
}
