use crate::middleware::auth::authenticate_request;
use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    http::{header::SET_COOKIE, HeaderValue, StatusCode},
    response::{IntoResponse, Json, Redirect, Response},
};
use log_engine::LogRecord;
use orm::FilterCondition;
use rust_apps_core::PageFixture;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{info, warn};

fn virtual_doctype_error(doctype: &str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        Json(json!({
            "error": format!("{} is a virtual DocType and is not backed by SQL", doctype),
            "message": "Use the dedicated desk endpoints for this DocType"
        })),
    )
}

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
    if doctype == "Kiff Log Entry" {
        return virtual_doctype_error(&doctype);
    }

    let fields = q
        .fields
        .map(|s| s.split(',').map(|x| x.trim().to_string()).collect());
    let filters: Option<HashMap<String, FilterCondition>> = q.filters.and_then(|s| {
        let raw: Option<HashMap<String, Value>> = serde_json::from_str(&s).ok();
        raw.map(|m| {
            m.into_iter()
                .map(|(k, v)| (k, FilterCondition::Eq(v)))
                .collect()
        })
    });

    // Use first pool for now
    let pool = state.pools.iter().next().map(|e| e.value().clone());
    match pool {
        Some(pool) => match pool
            .get_list(&doctype, filters, fields, q.order_by.as_deref(), q.limit)
            .await
        {
            Ok(docs) => (StatusCode::OK, Json(serde_json::json!({ "data": docs }))),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("{}", e) })),
            ),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "no database pool" })),
        ),
    }
}

pub async fn get_doc(
    State(state): State<AppState>,
    Path((doctype, name)): Path<(String, String)>,
) -> impl IntoResponse {
    let _ = name;
    if doctype == "Kiff Log Entry" {
        return virtual_doctype_error(&doctype);
    }

    let pool = state.pools.iter().next().map(|e| e.value().clone());
    match pool {
        Some(pool) => match pool.get_doc(&doctype, &name).await {
            Ok(doc) => (StatusCode::OK, Json(serde_json::json!({ "data": doc }))),
            Err(e) => (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": format!("{}", e) })),
            ),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "no database pool" })),
        ),
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
    headers: axum::http::HeaderMap,
    Json(body): Json<InsertBody>,
) -> impl IntoResponse {
    if doctype == "Kiff Log Entry" {
        return virtual_doctype_error(&doctype);
    }

    // Prefer the real Frappe Document.insert() path so DocType controllers,
    // validation hooks and child-table handling run. Fall back to the native
    // ORM insert when Python cannot handle the DocType yet.
    let mut py_doc = body.fields.clone();
    py_doc.insert(
        "doctype".to_string(),
        serde_json::Value::String(doctype.clone()),
    );
    let mut params = std::collections::HashMap::new();
    params.insert(
        "doc".to_string(),
        serde_json::Value::Object(py_doc.into_iter().collect()),
    );

    match call_rust_or_python_method(&state, "frappe.client.insert", params, &headers).await {
        Ok(MethodResponse::Json(value)) => {
            let payload = value.get("message").cloned().unwrap_or(value);
            return (
                StatusCode::CREATED,
                Json(serde_json::json!({ "data": payload })),
            );
        }
        Ok(_) => {
            return (
                StatusCode::CREATED,
                Json(serde_json::json!({ "message": "created" })),
            )
        }
        Err(error::RuntimeError::Python(e)) => {
            warn!(doctype = %doctype, error = %e, "Python frappe.client.insert failed, falling back to native ORM insert");
        }
        Err(e) => return frappe_error_response(e),
    }

    let pool = state.pools.iter().next().map(|e| e.value().clone());
    match pool {
        Some(pool) => {
            let mut doc = orm::Document::new(doctype, uuid::Uuid::new_v4().to_string());
            for (k, v) in body.fields {
                doc.set_field(k, v);
            }
            match pool.insert_doc(&doc).await {
                Ok(name) => (
                    StatusCode::CREATED,
                    Json(serde_json::json!({ "data": { "name": name } })),
                ),
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": format!("{}", e) })),
                ),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "no database pool" })),
        ),
    }
}

pub async fn update_doc(
    State(state): State<AppState>,
    Path((doctype, name)): Path<(String, String)>,
    headers: axum::http::HeaderMap,
    Json(body): Json<HashMap<String, Value>>,
) -> impl IntoResponse {
    if doctype == "Kiff Log Entry" {
        return virtual_doctype_error(&doctype);
    }

    // Keep a copy for the native ORM fallback path.
    let body_for_fallback = body.clone();

    // Prefer the real Frappe Document.save() path so DocType controllers and
    // validation hooks run (e.g. User.validate_allowed_modules).
    // Load the full document (including child tables and mandatory fields)
    // from Python first, then overlay the request body before saving.
    let mut get_params = std::collections::HashMap::new();
    get_params.insert(
        "doctype".to_string(),
        serde_json::Value::String(doctype.clone()),
    );
    get_params.insert("name".to_string(), serde_json::Value::String(name.clone()));

    let mut full_doc =
        match call_rust_or_python_method(&state, "frappe.client.get", get_params, &headers).await {
            Ok(MethodResponse::Json(value)) => value.get("message").cloned().unwrap_or(value),
            Ok(_) => serde_json::Value::Object(Default::default()),
            Err(error::RuntimeError::Python(_)) => serde_json::Value::Object(Default::default()),
            Err(e) => return frappe_error_response(e),
        };

    if let serde_json::Value::Object(ref mut map) = full_doc {
        for (k, v) in body {
            map.insert(k, v);
        }
    }

    let mut save_params = std::collections::HashMap::new();
    save_params.insert("doc".to_string(), full_doc);

    match call_rust_or_python_method(&state, "frappe.client.save", save_params, &headers).await {
        Ok(MethodResponse::Json(value)) => {
            let payload = value.get("message").cloned().unwrap_or(value);
            return (StatusCode::OK, Json(serde_json::json!({ "data": payload })));
        }
        Ok(_) => {
            return (
                StatusCode::OK,
                Json(serde_json::json!({ "message": "updated" })),
            )
        }
        Err(error::RuntimeError::Python(e)) => {
            warn!(doctype = %doctype, name = %name, error = %e, "Python frappe.client.save failed, falling back to native ORM update");
        }
        Err(e) => return frappe_error_response(e),
    }

    // Fall back to the native ORM update when Python cannot handle the DocType yet.
    let pool = state.pools.iter().next().map(|e| e.value().clone());
    match pool {
        Some(pool) => match pool.get_doc(&doctype, &name).await {
            Ok(mut doc) => {
                for (k, v) in body_for_fallback {
                    doc.set_field(k, v);
                }
                match pool.save_doc(&doc).await {
                    Ok(_) => (StatusCode::OK, Json(serde_json::json!({ "data": doc }))),
                    Err(e) => (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({ "error": format!("{}", e) })),
                    ),
                }
            }
            Err(e) => (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": format!("{}", e) })),
            ),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "no database pool" })),
        ),
    }
}

async fn delete_doc_inner(
    state: &AppState,
    doctype: &str,
    name: &str,
    headers: &axum::http::HeaderMap,
) -> (StatusCode, Json<Value>) {
    // Try real Frappe Document.delete() first so Password fields are cleaned
    // up from __auth and document hooks run. Fall back to the native ORM
    // delete if Python is unavailable.
    let mut params = std::collections::HashMap::new();
    params.insert(
        "doctype".to_string(),
        serde_json::Value::String(doctype.to_string()),
    );
    params.insert(
        "name".to_string(),
        serde_json::Value::String(name.to_string()),
    );

    match call_rust_or_python_method(state, "frappe.client.delete", params, headers).await {
        Ok(_) => {
            return (
                StatusCode::OK,
                Json(serde_json::json!({ "message": "deleted" })),
            )
        }
        Err(error::RuntimeError::Python(_)) => {
            // Python failed; fall through to native delete rather than returning
            // the error immediately. Some DocType controllers may not yet load
            // cleanly under the shim, but the row should still be removable.
        }
        Err(e) => return frappe_error_response(e),
    }

    // Python path failed. Before falling back to the native ORM delete,
    // enforce Frappe-style delete permission so the fallback cannot be used
    // to bypass permission checks.
    let user = match authenticate_request(state, headers).await {
        Some(u) => u.user,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "unauthorized" })),
            )
        }
    };

    let pool = state.pools.iter().next().map(|e| e.value().clone());
    match pool {
        Some(pool) => {
            let doc = pool.get_doc(doctype, name).await.ok();
            match state
                .permissions
                .has_permission(&pool, &user, doctype, "delete", doc.as_ref())
                .await
            {
                Ok(true) => match pool.delete_doc(doctype, name).await {
                    Ok(_) => (
                        StatusCode::OK,
                        Json(serde_json::json!({ "message": "deleted" })),
                    ),
                    Err(e) => (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({ "error": format!("{}", e) })),
                    ),
                },
                Ok(false) => (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({ "error": "permission denied" })),
                ),
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": format!("{}", e) })),
                ),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "no database pool" })),
        ),
    }
}

pub async fn delete_doc(
    State(state): State<AppState>,
    Path((doctype, name)): Path<(String, String)>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    if doctype == "Kiff Log Entry" {
        return virtual_doctype_error(&doctype);
    }

    delete_doc_inner(&state, &doctype, &name, &headers).await
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

    match load_page(state, name, &user).await {
        Ok(doc) => {
            let mut resp = serde_json::Map::new();
            resp.insert("docs".to_string(), serde_json::Value::Array(vec![doc]));
            (StatusCode::OK, Json(serde_json::Value::Object(resp)))
        }
        Err(ref e) if e == "not_permitted" => (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "exc": "Not permitted" })),
        ),
        Err(ref e) if e.starts_with("page json not found") => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("{}", e) })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("{}", e) })),
        ),
    }
}

async fn load_page(state: &AppState, name: &str, user: &str) -> Result<serde_json::Value, String> {
    // Try Rust app fixtures first. These are loaded into memory at startup so
    // pages work even when the app's source tree is not deployed.
    for fixture in state.rust_apps.all_pages() {
        if fixture.name == name {
            return load_page_from_fixture(state, user, &fixture).await;
        }
    }

    // Fall back to on-disk JSON for Frappe core pages.
    load_page_from_json(state, name, user).await
}

async fn load_page_from_fixture(
    state: &AppState,
    user: &str,
    fixture: &PageFixture,
) -> Result<serde_json::Value, String> {
    let mut doc: serde_json::Value =
        serde_json::from_str(&fixture.json).map_err(|e| format!("parse error: {}", e))?;

    check_page_roles(state, &doc, user).await?;

    let script = build_page_script(&fixture.templates, &fixture.script);

    if let serde_json::Value::Object(ref mut map) = doc {
        map.insert("script".to_string(), serde_json::Value::String(script));
        map.insert(
            "style".to_string(),
            serde_json::Value::String(fixture.style.clone()),
        );
    }

    Ok(doc)
}

fn extract_page_roles(doc: &serde_json::Value) -> Vec<String> {
    doc.get("roles")
        .and_then(|r| r.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.get("role").and_then(|r| r.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

async fn check_page_roles(
    state: &AppState,
    doc: &serde_json::Value,
    user: &str,
) -> Result<(), String> {
    let allowed_roles = extract_page_roles(doc);
    if !allowed_roles.is_empty() && user != "Administrator" {
        let user_roles = get_user_roles(state, user).await;
        let has_role = user_roles.iter().any(|r| allowed_roles.contains(r));
        if !has_role {
            return Err("not_permitted".into());
        }
    }
    Ok(())
}

fn build_page_script(templates: &HashMap<String, String>, script: &str) -> String {
    let mut template_script = String::new();
    for (filename, content) in templates {
        template_script.push_str(&html_to_js_template(filename, content));
    }
    format!("{}{}", template_script, script)
}

async fn load_page_from_json(
    state: &AppState,
    name: &str,
    user: &str,
) -> Result<serde_json::Value, String> {
    let scrubbed = name.to_lowercase().replace(" ", "_").replace("-", "_");

    let mut page_path = None;

    // 1. Standard Frappe app pages.
    let frappe_base = PathBuf::from("apps/frappe/frappe");
    if let Ok(entries) = std::fs::read_dir(&frappe_base) {
        for entry in entries.flatten() {
            let path = entry
                .path()
                .join("page")
                .join(&scrubbed)
                .join(format!("{}.json", scrubbed));
            if path.exists() {
                page_path = Some(path);
                break;
            }
        }
    }

    // 2. Project-specific Rust app pages.
    if page_path.is_none() {
        let custom_bases: Vec<PathBuf> = vec![
            PathBuf::from("crates/kiff_logger/src/pages"),
            PathBuf::from("rust_apps/sebrus_logger/src/pages"),
        ];
        for base in custom_bases {
            let path = base.join(&scrubbed).join(format!("{}.json", scrubbed));
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

    check_page_roles(state, &doc, user).await?;

    // Load assets from the page directory.
    let dir = path.parent().unwrap().to_path_buf();
    let js_path = dir.join(format!("{}.js", scrubbed));
    let css_path = dir.join(format!("{}.css", scrubbed));

    // Convert any .html templates in the page directory into frappe.templates
    // entries, matching Frappe's Page.load_assets behaviour. This lets page
    // scripts call frappe.render_template("<name>", {}) without the template
    // needing to be bundled into a desk asset bundle.
    let mut templates: HashMap<String, String> = HashMap::new();
    let mut entries = tokio::fs::read_dir(&dir)
        .await
        .map_err(|e| format!("read dir error: {}", e))?;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("html") {
            let content = tokio::fs::read_to_string(&path)
                .await
                .map_err(|e| format!("read html error: {}", e))?;
            let filename = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            templates.insert(filename.to_string(), content);
        }
    }

    let script = tokio::fs::read_to_string(&js_path)
        .await
        .unwrap_or_default();
    let style = tokio::fs::read_to_string(&css_path)
        .await
        .unwrap_or_default();

    let script = build_page_script(&templates, &script);

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

/// Native Rust implementation of frappe.desk.form.load.getdoc.
/// Loads a single document (with child tables and __onload data) from the
/// native ORM so forms that rely on controller onload data work even when the
/// Python bridge cannot run the DocType controller cleanly.
pub async fn getdoc_native(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let doctype = params.get("doctype").cloned().unwrap_or_default();
    let name = params.get("name").cloned().unwrap_or_default();

    if doctype == "Kiff Log Entry" {
        return get_kiff_log_doc(state, &name).await;
    }

    let pool = state.pools.iter().next().map(|e| e.value().clone());
    match pool {
        Some(pool) => match pool.get_doc(&doctype, &name).await {
            Ok(doc) => {
                let permissions = if let Some(user) = authenticate_request(&state, &headers).await {
                    let ptypes = vec![
                        "read", "write", "create", "delete", "submit", "cancel", "select",
                        "report", "export", "import", "print", "email", "share",
                    ];
                    let mut perms = serde_json::Map::new();
                    for ptype in ptypes {
                        let allowed = state
                            .permissions
                            .has_permission(&pool, &user.user, &doctype, ptype, Some(&doc))
                            .await
                            .unwrap_or(false);
                        perms.insert(ptype.to_string(), json!(if allowed { 1 } else { 0 }));
                    }
                    Value::Object(perms)
                } else {
                    Value::Object(serde_json::Map::new())
                };

                let docinfo = json!({
                    "doctype": doctype,
                    "name": name,
                    "attachments": [],
                    "communications": [],
                    "automated_messages": [],
                    "versions": [],
                    "assignments": [],
                    "permissions": permissions,
                    "shared": [],
                    "views": [],
                    "additional_timeline_content": [],
                    "milestones": [],
                    "is_document_followed": false,
                    "tags": [],
                    "document_email": Value::Null,
                    "custom_perm_types": [],
                    "comments": [],
                    "assignment_logs": [],
                    "attachment_logs": [],
                    "user_info": {},
                });
                let mut resp = serde_json::Map::new();
                resp.insert("docs".to_string(), json!([doc]));
                resp.insert("docinfo".to_string(), docinfo);
                (StatusCode::OK, Json(Value::Object(resp)))
            }
            Err(error::RuntimeError::NotFound(_)) => (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": format!("{} {} not found", doctype, name) })),
            ),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("{}", e) })),
            ),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "no database pool" })),
        ),
    }
}

/// Native Rust implementation of frappe.desk.form.load.getdoctype.
/// Loads doctype metadata from Rust app fixtures (in-memory) or JSON files in
/// apps/frappe/frappe/*/doctype/, instead of relying on the Python bridge and
/// missing DB tables.
pub async fn getdoctype_native(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let doctype = params.get("doctype").cloned().unwrap_or_default();
    let _with_parent = params.get("with_parent").map(|s| s == "1").unwrap_or(false);
    let cached_timestamp = params.get("cached_timestamp").cloned().unwrap_or_default();

    match load_doctype_metadata(&state, &doctype, &cached_timestamp).await {
        Ok(docs) => {
            let mut resp = serde_json::Map::new();
            resp.insert("docs".to_string(), serde_json::Value::Array(docs));
            resp.insert(
                "user_settings".to_string(),
                serde_json::Value::String("{}".into()),
            );
            (StatusCode::OK, Json(serde_json::Value::Object(resp)))
        }
        Err(ref e) if e == "use_cache" => {
            let mut resp = serde_json::Map::new();
            resp.insert(
                "message".to_string(),
                serde_json::Value::String("use_cache".into()),
            );
            resp.insert("docs".to_string(), serde_json::json!([]));
            resp.insert(
                "user_settings".to_string(),
                serde_json::Value::String("{}".into()),
            );
            (StatusCode::OK, Json(serde_json::Value::Object(resp)))
        }
        Err(ref e) if e.starts_with("doctype json not found") => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("{}", e) })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("{}", e) })),
        ),
    }
}

/// Native Rust implementation of `frappe.desk.search.search_link` (GET).
pub async fn search_link(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    search_link_impl(state, params).await
}

/// Native Rust implementation of `frappe.desk.search.search_link` (POST).
///
/// Link fields send POST requests once the user starts typing.
pub async fn search_link_post(
    State(state): State<AppState>,
    crate::extract::AnyBody(body): crate::extract::AnyBody,
) -> impl IntoResponse {
    let mut params = HashMap::new();
    if let Some(map) = body.as_object() {
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
    }
    search_link_impl(state, params).await
}

/// Native Rust implementation of `frappe.client.validate_link_and_fetch`.
///
/// Link fields call this after a value is selected to confirm the document
/// exists and to fetch any dependent fields. The Python implementation relies
/// on Frappe's search_widget and DB layer, so this native version performs a
/// simple existence check against the underlying table.
pub async fn validate_link_and_fetch(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    validate_link_and_fetch_impl(state, params).await
}

pub async fn validate_link_and_fetch_post(
    State(state): State<AppState>,
    crate::extract::AnyBody(body): crate::extract::AnyBody,
) -> impl IntoResponse {
    let mut params = HashMap::new();
    if let Some(map) = body.as_object() {
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
    }
    validate_link_and_fetch_impl(state, params).await
}

async fn validate_link_and_fetch_impl(
    state: AppState,
    params: HashMap<String, String>,
) -> impl IntoResponse {
    let pool = match state.pools.iter().next().map(|e| e.value().clone()) {
        Some(p) => p,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "message": {} })),
            );
        }
    };

    let doctype = params.get("doctype").cloned().unwrap_or_default();
    let docname = params.get("docname").cloned().unwrap_or_default();

    if doctype.is_empty() || docname.is_empty() {
        return (StatusCode::OK, Json(json!({ "message": {} })));
    }

    let table = doctype.to_lowercase().replace(" ", "_").replace("-", "_");
    let table = table.strip_prefix("tab").unwrap_or(&table);

    let sql = format!(r#"SELECT "name" FROM "{}" WHERE "name" = ? LIMIT 1"#, table);
    let exists = match pool
        .execute_sql(&sql, vec![Value::String(docname.clone())])
        .await
    {
        Ok(rows) => !rows.is_empty(),
        Err(e) => {
            warn!(
                "validate_link_and_fetch failed for {} {}: {}",
                doctype, docname, e
            );
            false
        }
    };

    if exists {
        (
            StatusCode::OK,
            Json(json!({ "message": { "name": docname } })),
        )
    } else {
        (StatusCode::OK, Json(json!({ "message": {} })))
    }
}

/// Native Rust implementation of `frappe.desk.reportview.get`.
///
/// The desk list view uses this endpoint. For normal DocTypes we fall back to
/// the ORM; for the virtual `Kiff Log Entry` DocType we read from the log
/// engine instead.
pub async fn reportview_get(
    State(state): State<AppState>,
    crate::extract::AnyBody(body): crate::extract::AnyBody,
) -> Response {
    let params = reportview_params_from_body(body);
    let doctype = params.get("doctype").cloned().unwrap_or_default();

    if doctype == "Kiff Log Entry" {
        return reportview_kiff_log_get(state, params).await;
    }

    info!(
        "reportview.get doctype={} fields={:?} filters={:?}",
        doctype,
        params.get("fields"),
        params.get("filters")
    );

    // For non-virtual DocTypes, use the ORM's get_list and compress the result
    // into the reportview shape.
    let pool = match state.pools.iter().next().map(|e| e.value().clone()) {
        Some(p) => p,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "message": "no database pool" })),
            )
                .into_response();
        }
    };

    let fields = params
        .get("fields")
        .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
        .or_else(|| {
            params.get("fields").map(|s| {
                s.split(',')
                    .map(|x| x.trim().to_string())
                    .collect::<Vec<_>>()
            })
        });
    // The desk sends link-field title expressions like `sop.name as sop_name`.
    // The native ORM cannot resolve joins, so keep only the base link field.
    let fields = fields.map(|f| sanitize_reportview_fields(&f));

    // Filter requested fields to columns that actually exist in the data table.
    // This avoids 500s when the desk asks for fields like `disabled` that are
    // not present on every DocType.
    let fields = match fields {
        Some(fields) => {
            let table = reportview_table_name(&doctype);
            let cols: std::collections::HashSet<String> = pool
                .execute_sql(&format!("PRAGMA table_info(\"{}\")", table), vec![])
                .await
                .map(|rows| {
                    rows.into_iter()
                        .filter_map(|r| r.get("name").and_then(|v| v.as_str()).map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            if cols.is_empty() {
                Some(fields)
            } else {
                Some(
                    fields
                        .into_iter()
                        .filter(|f| cols.contains(f))
                        .collect::<Vec<_>>(),
                )
            }
        }
        None => None,
    };

    let filters = params.get("filters").and_then(|s| {
        let raw: Option<HashMap<String, Value>> = serde_json::from_str(s).ok();
        raw.map(|m| {
            m.into_iter()
                .map(|(k, v)| (k, FilterCondition::Eq(v)))
                .collect::<HashMap<_, _>>()
        })
    });

    let limit_start = params
        .get("limit_start")
        .or_else(|| params.get("start"))
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);
    let page_length = params
        .get("limit_page_length")
        .or_else(|| params.get("page_length"))
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(20);

    match pool
        .get_list(
            &doctype,
            filters,
            fields,
            None,
            Some(limit_start + page_length),
        )
        .await
    {
        Ok(docs) => {
            let docs: Vec<Value> = docs
                .into_iter()
                .skip(limit_start)
                .take(page_length)
                .map(|d| serde_json::to_value(d).unwrap_or_default())
                .collect();
            (
                StatusCode::OK,
                Json(json!({ "message": compress_reportview(&docs) })),
            )
                .into_response()
        }
        Err(e) => {
            warn!(
                "reportview.get failed for doctype={} fields={:?} filters={:?}: {}",
                doctype,
                params.get("fields"),
                params.get("filters"),
                e
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "message": format!("{}", e) })),
            )
                .into_response()
        }
    }
}

/// Native Rust implementation of `frappe.desk.reportview.get_count`.
pub async fn reportview_get_count(
    State(state): State<AppState>,
    crate::extract::AnyBody(body): crate::extract::AnyBody,
) -> Response {
    let params = reportview_params_from_body(body);
    let doctype = params.get("doctype").cloned().unwrap_or_default();

    if doctype == "Kiff Log Entry" {
        return kiff_log_count(state, params).await;
    }

    let pool = match state.pools.iter().next().map(|e| e.value().clone()) {
        Some(p) => p,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "message": "no database pool" })),
            )
                .into_response();
        }
    };

    let filters = params.get("filters").and_then(|s| {
        let raw: Option<HashMap<String, Value>> = serde_json::from_str(s).ok();
        raw.map(|m| {
            m.into_iter()
                .map(|(k, v)| (k, FilterCondition::Eq(v)))
                .collect::<HashMap<_, _>>()
        })
    });

    match pool.get_list(&doctype, filters, None, None, None).await {
        Ok(docs) => (StatusCode::OK, Json(json!({ "message": docs.len() }))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "message": format!("{}", e) })),
        )
            .into_response(),
    }
}

fn reportview_params_from_body(body: Value) -> HashMap<String, String> {
    let mut params = HashMap::new();
    if let Some(map) = body.as_object() {
        if let Some(Value::String(args)) = map.get("args") {
            if let Ok(parsed) = serde_json::from_str::<HashMap<String, Value>>(args) {
                for (k, v) in parsed {
                    insert_value(&mut params, k, v);
                }
            }
        }
        for (k, v) in map {
            if k == "args" {
                continue;
            }
            insert_value(&mut params, k.clone(), v.clone());
        }
    }
    params
}

fn insert_value(params: &mut HashMap<String, String>, key: String, value: Value) {
    match value {
        Value::String(s) => {
            params.insert(key, s);
        }
        Value::Array(arr) => {
            params.insert(key, serde_json::to_string(&arr).unwrap_or_default());
        }
        Value::Object(obj) => {
            params.insert(key, serde_json::to_string(&obj).unwrap_or_default());
        }
        Value::Number(n) => {
            params.insert(key, n.to_string());
        }
        Value::Bool(b) => {
            params.insert(key, b.to_string());
        }
        Value::Null => {}
    }
}

async fn get_kiff_log_doc(state: AppState, name: &str) -> (StatusCode, Json<Value>) {
    let service = match state.logger.get() {
        Some(s) => s.clone(),
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": "log engine not initialized" })),
            );
        }
    };

    let _ = service.commit().await;

    // Name format: KLE-<timestamp_ms>-<index>. Query broadly and match by name.
    let records = match service.query("*", 100_000).await {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("log query failed: {}", e) })),
            );
        }
    };

    for (idx, rec) in records.into_iter().enumerate() {
        let candidate = format!("KLE-{}-{}", rec.timestamp, idx);
        if candidate == name {
            let doc = log_record_to_doc(rec, idx);
            let docinfo = json!({
                "doctype": "Kiff Log Entry",
                "name": name,
                "attachments": [],
                "communications": [],
                "automated_messages": [],
                "versions": [],
                "assignments": [],
                "permissions": {},
                "shared": [],
                "views": [],
                "additional_timeline_content": [],
                "milestones": [],
                "is_document_followed": false,
                "tags": [],
                "document_email": Value::Null,
                "custom_perm_types": [],
                "comments": [],
                "assignment_logs": [],
                "attachment_logs": [],
                "user_info": {},
            });
            let mut resp = serde_json::Map::new();
            resp.insert("docs".to_string(), json!([doc]));
            resp.insert("docinfo".to_string(), docinfo);
            return (StatusCode::OK, Json(Value::Object(resp)));
        }
    }

    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": format!("Kiff Log Entry {} not found", name) })),
    )
}

fn compress_reportview(data: &[Value]) -> Value {
    if data.is_empty() {
        return json!({ "keys": [], "values": [], "user_info": {} });
    }
    let keys: Vec<String> = data[0]
        .as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    let values: Vec<Vec<Value>> = data
        .iter()
        .map(|row| {
            keys.iter()
                .map(|k| row.get(k).cloned().unwrap_or(Value::Null))
                .collect()
        })
        .collect();
    json!({ "keys": keys, "values": values, "user_info": {} })
}

/// Convert a DocType name into the physical SQL table name used by the ORM.
fn reportview_table_name(doctype: &str) -> String {
    let name = doctype.to_lowercase().replace(" ", "_");
    name.strip_prefix("tab").unwrap_or(&name).to_string()
}

/// Strip SQL aliases / table prefixes from the desk list-view field list so
/// the native ORM can fetch them. For example `sop.name as sop_name` becomes
/// `sop`.
fn sanitize_reportview_fields(fields: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    fields
        .iter()
        .filter_map(|f| {
            if f.is_empty() {
                return None;
            }
            let base = if let Some(pos) = f.to_lowercase().find(" as ") {
                f[..pos].trim()
            } else {
                f.trim()
            };
            let base = base.trim_start_matches('`').trim_end_matches('`');
            let base = base.split('.').next().unwrap_or(base).to_string();
            if base.is_empty() || !seen.insert(base.clone()) {
                None
            } else {
                Some(base)
            }
        })
        .collect()
}

async fn reportview_kiff_log_get(state: AppState, params: HashMap<String, String>) -> Response {
    let limit_start = params
        .get("limit_start")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);
    let page_length = params
        .get("limit_page_length")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(20);
    let query = params.get("filters").cloned().unwrap_or_default();
    let query = kiff_log_query_from_filters(&query);

    let service = match state.logger.get() {
        Some(s) => s.clone(),
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "message": "log engine not initialized" })),
            )
                .into_response();
        }
    };

    // Commit staged logs so recently-ingested records are searchable.
    let _ = service.commit().await;

    let records = match service.query(&query, limit_start + page_length).await {
        Ok(r) => r,
        Err(e) => {
            warn!("kiff log query failed: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "message": format!("log query failed: {}", e) })),
            )
                .into_response();
        }
    };

    let docs: Vec<Value> = records
        .into_iter()
        .skip(limit_start)
        .take(page_length)
        .enumerate()
        .map(|(idx, rec)| log_record_to_doc(rec, idx))
        .collect();

    (
        StatusCode::OK,
        Json(json!({ "message": compress_reportview(&docs) })),
    )
        .into_response()
}

async fn kiff_log_count(state: AppState, params: HashMap<String, String>) -> Response {
    let query = params.get("filters").cloned().unwrap_or_default();
    let query = kiff_log_query_from_filters(&query);

    let service = match state.logger.get() {
        Some(s) => s.clone(),
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "message": "log engine not initialized" })),
            )
                .into_response();
        }
    };

    let _ = service.commit().await;

    match service.query(&query, 1_000_000).await {
        Ok(records) => (StatusCode::OK, Json(json!({ "message": records.len() }))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "message": format!("log query failed: {}", e) })),
        )
            .into_response(),
    }
}

fn kiff_log_query_from_filters(filters_json: &str) -> String {
    if filters_json.is_empty() {
        return "*".to_string();
    }
    // The desk may send filters as a JSON list like [["Kiff Log Entry", "level", "=", "ERROR"]].
    // Convert the simple equality filters into a Tantivy query string.
    if let Ok(filters) = serde_json::from_str::<Vec<Vec<Value>>>(filters_json) {
        let parts: Vec<String> = filters
            .into_iter()
            .filter_map(|f| {
                if f.len() >= 4 {
                    let field = f.get(1)?.as_str()?;
                    let op = f.get(2)?.as_str()?;
                    let value = f.get(3)?.as_str()?;
                    if op == "=" {
                        Some(format!("{}:{}", field, value))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();
        if parts.is_empty() {
            "*".to_string()
        } else {
            parts.join(" AND ")
        }
    } else {
        "*".to_string()
    }
}

fn log_record_to_doc(rec: LogRecord, idx: usize) -> Value {
    let ts_secs = rec.timestamp / 1000;
    let dt = chrono::DateTime::from_timestamp(ts_secs, 0)
        .map(|d| d.to_rfc3339())
        .unwrap_or_else(|| rec.timestamp.to_string());
    let name = format!("KLE-{}-{}", rec.timestamp, idx);

    let mut doc = serde_json::Map::new();
    doc.insert("name".to_string(), json!(name));
    doc.insert("doctype".to_string(), json!("Kiff Log Entry"));
    doc.insert("timestamp".to_string(), json!(dt));
    doc.insert("level".to_string(), json!(rec.level));
    doc.insert("service".to_string(), json!(rec.service));
    doc.insert("message".to_string(), json!(rec.message));

    if let Some(doctype) = rec.fields.get("doctype").and_then(|v| v.as_str()) {
        doc.insert("doctype_field".to_string(), json!(doctype));
    }
    if let Some(docname) = rec.fields.get("docname").and_then(|v| v.as_str()) {
        doc.insert("docname".to_string(), json!(docname));
    }
    if let Some(event) = rec.fields.get("event").and_then(|v| v.as_str()) {
        doc.insert("event".to_string(), json!(event));
    }
    if let Some(status) = rec.fields.get("status").and_then(|v| v.as_str()) {
        doc.insert("status".to_string(), json!(status));
    }
    if let Some(severity) = rec.fields.get("severity").and_then(|v| v.as_str()) {
        doc.insert("severity".to_string(), json!(severity));
    }
    if !rec.fields.is_empty() {
        let raw = serde_json::to_string(&rec.fields).unwrap_or_else(|_| "{}".to_string());
        doc.insert("raw_fields".to_string(), json!(raw));
    }

    Value::Object(doc)
}

async fn search_link_impl(state: AppState, params: HashMap<String, String>) -> impl IntoResponse {
    let pool = match state.pools.iter().next().map(|e| e.value().clone()) {
        Some(p) => p,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "message": [] })),
            );
        }
    };

    let doctype = params.get("doctype").cloned().unwrap_or_default();
    let txt = params.get("txt").cloned().unwrap_or_default();
    let page_length = params
        .get("page_length")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(10);
    let filters_json = params.get("filters").cloned().unwrap_or_default();

    if doctype.is_empty() {
        return (StatusCode::OK, Json(json!({ "message": [] })));
    }

    let table = doctype.to_lowercase().replace(" ", "_").replace("-", "_");
    let table = table.strip_prefix("tab").unwrap_or(&table);

    let mut conditions = Vec::new();
    let mut query_params: Vec<Value> = Vec::new();

    // Equality filters (dict of field -> value). Link queries commonly send
    // {"istable": 0} for DocType, {"enabled": 1}, etc.
    if !filters_json.is_empty() {
        if let Ok(filters) = serde_json::from_str::<HashMap<String, Value>>(&filters_json) {
            for (field, value) in filters {
                if field == "include_disabled" {
                    continue;
                }
                conditions.push(format!("\"{}\" = ?", field));
                query_params.push(value);
            }
        }
    }

    // Text search on the name column (and title, if one is configured).
    let search_term = format!("%{}%", txt.replace('%', "\\%").replace('_', "\\_"));
    let title_field: Option<String> = if doctype == "DocType" {
        None
    } else {
        pool.execute_sql(
            r#"SELECT title_field FROM "doctype" WHERE name = ?"#,
            vec![Value::String(doctype.clone())],
        )
        .await
        .ok()
        .and_then(|mut rows| rows.pop())
        .and_then(|mut row| row.remove("title_field"))
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .filter(|s| !s.is_empty() && s != "name")
    };

    let mut or_conditions = vec![format!("\"name\" LIKE ?")];
    query_params.push(Value::String(search_term.clone()));
    if let Some(t) = title_field {
        if t != "name" {
            or_conditions.push(format!("\"{}\" LIKE ?", t));
            query_params.push(Value::String(search_term));
        }
    }
    conditions.push(format!("({})", or_conditions.join(" OR ")));

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };

    let sql = format!(
        r#"SELECT "name" FROM "{}"{} ORDER BY "name" LIMIT {}"#,
        table, where_clause, page_length
    );

    let results = match pool.execute_sql(&sql, query_params).await {
        Ok(rows) => rows
            .into_iter()
            .filter_map(|mut row| {
                let name = row.remove("name")?.as_str()?.to_string();
                Some(json!({ "value": name.clone(), "description": name }))
            })
            .collect::<Vec<_>>(),
        Err(e) => {
            warn!("search_link failed for {}: {}", doctype, e);
            vec![]
        }
    };

    (StatusCode::OK, Json(json!({ "message": results })))
}

/// Load DocType metadata. Rust app fixtures are checked first because they are
/// kept in memory after startup, so they work even when the source tree (e.g.
/// `crates/kiff_logger`) is not present in the deployed environment.
async fn load_doctype_metadata(
    state: &AppState,
    doctype: &str,
    cached_timestamp: &str,
) -> Result<Vec<serde_json::Value>, String> {
    for fixture in state.rust_apps.all_doctypes() {
        if fixture.name == doctype {
            return load_doctype_from_content(doctype, &fixture.json, cached_timestamp, None, None);
        }
    }

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
                        if doc
                            .get("name")
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
        if js_path.exists() {
            Some(js_path)
        } else {
            None
        },
        if css_path.exists() {
            Some(css_path)
        } else {
            None
        },
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
        Some(p) => {
            Some(std::fs::read_to_string(&p).map_err(|e| format!("read doctype js error: {}", e))?)
        }
        None => None,
    };
    let css = match css_path {
        Some(p) => Some(
            std::fs::read_to_string(&p).map_err(|e| format!("read doctype css error: {}", e))?,
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

    // Ensure common meta arrays that the desk client expects are present.
    // If we had to inject them, bump `modified` so browsers with a stale
    // cached version request fresh metadata.
    let mut injected_meta = false;
    if let serde_json::Value::Object(ref mut map) = doc {
        for key in ["states", "actions", "links"] {
            if !map.contains_key(key) {
                map.insert(key.to_string(), serde_json::Value::Array(vec![]));
                injected_meta = true;
            }
        }
        if injected_meta {
            map.insert(
                "modified".to_string(),
                serde_json::Value::String(chrono::Utc::now().to_rfc3339()),
            );
        }
    }

    // Check cache timestamp
    if !cached_timestamp.is_empty() {
        if let Some(modified) = doc.get("modified").and_then(|m| m.as_str()) {
            if modified == cached_timestamp && !injected_meta {
                return Err("use_cache".into());
            }
        }
    }

    let doctype_name = doc
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(doctype)
        .to_string();

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
    let child_doctypes: Vec<String> = doc
        .get("fields")
        .and_then(|f| f.as_array())
        .map(|arr| {
            arr.iter()
                .filter(|f| {
                    f.get("fieldtype")
                        .and_then(|v| v.as_str())
                        .map(|t| table_fieldtypes.contains(&t))
                        .unwrap_or(false)
                })
                .filter_map(|f| {
                    f.get("options")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                })
                .collect()
        })
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
    let content = std::fs::read_to_string(&path).map_err(|e| format!("read error: {}", e))?;
    let (js, css) = read_doctype_assets_sync(&path)?;
    load_doctype_from_content(doctype, &content, cached_timestamp, js, css)
}

async fn session_user_from_request(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Option<String> {
    authenticate_request(state, headers).await.map(|u| u.user)
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
    Redirect {
        location: String,
        cookie: Option<String>,
    },
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
    // The Frappe desk UI invokes deletes through frappe.client.delete rather
    // than the REST DELETE endpoint. Route that through the same Python-then-
    // native fallback so deletions don't fail when the Python shim cannot
    // complete the full Frappe delete flow (e.g. creating Deleted Document).
    if method == "frappe.client.delete" {
        if let (Some(Value::String(doctype)), Some(Value::String(name))) =
            (params.get("doctype"), params.get("name"))
        {
            if doctype == "Kiff Log Entry" {
                return virtual_doctype_error(doctype).into_response();
            }
            return delete_doc_inner(state, doctype, name, headers)
                .await
                .into_response();
        }
    }

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
    let user = session_user_from_request(state, headers).await;
    if let Some(result) = state
        .rust_apps
        .call_method(method, state.clone(), params.clone(), user.clone())
        .await?
    {
        // Frappe clients expect { "message": <value> } for /api/method/* calls.
        return Ok(MethodResponse::Json(
            serde_json::json!({ "message": result }),
        ));
    }

    // Fall back to Python method dispatcher.
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
    let session = store
        .create(&pool, py_user, "localhost".into())
        .await
        .ok()?;
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
            response
                .headers()
                .get("location")
                .unwrap()
                .to_str()
                .unwrap(),
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
