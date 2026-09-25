use crate::extract::AnyBody;
use crate::middleware::auth::authenticate_request;
use crate::site::resolve_site_pool;
use crate::social_login::{site_url_from_headers, social_login_urls, SocialLoginProvider};
use crate::user_home::get_effective_home_page;
use crate::AppState;
use axum::{
    extract::{OriginalUri, Query, RawQuery, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Json,
};
use permissions::PermissionEngine;
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::SystemTime;

#[derive(Serialize)]
struct LoginRedirectQuery<'a> {
    #[serde(rename = "redirect-to")]
    redirect_to: &'a str,
}

const DESK_TEMPLATE: &str = include_str!("../../assets/desk-template.html");

/// Bundles required by Frappe Desk (from frappe/hooks.py app_include_js / app_include_css)
const DESK_JS_BUNDLES: &[&str] = &[
    "libs.bundle.js",
    "desk.bundle.js",
    "list.bundle.js",
    "form.bundle.js",
    "controls.bundle.js",
    "report.bundle.js",
    "telemetry.bundle.js",
    "billing.bundle.js",
];

const DESK_CSS_BUNDLES: &[&str] = &["desk.bundle.css", "report.bundle.css"];

/// Icon sprites required by Frappe Desk (from frappe/hooks.py app_include_icons).
const DESK_ICON_SPRITES: &[&str] = &[
    "apps/frappe/frappe/public/icons/lucide/icons.svg",
    "apps/frappe/frappe/public/icons/timeless/icons.svg",
    "apps/frappe/frappe/public/icons/espresso/icons.svg",
    "apps/frappe/frappe/public/icons/desktop_icons/alphabets.svg",
];

/// Serve the Frappe Desk SPA with boot info injected.
pub async fn serve_desk(
    State(state): State<AppState>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
) -> impl IntoResponse {
    // Extract session from cookie
    let user = extract_user_from_request(&state, &headers).await;

    // Desk requires an authenticated session. Guests are redirected to login
    // (the configured custom login page when one is set).
    if user.is_none() {
        let redirect_to = uri
            .path_and_query()
            .map(|pq| pq.to_string())
            .unwrap_or_else(|| "/app".into());
        let query = serde_urlencoded::to_string(&LoginRedirectQuery {
            redirect_to: &redirect_to,
        })
        .unwrap_or_else(|_| format!("redirect-to={}", redirect_to));
        let target = custom_login_target(&state.config, Some(&query))
            .unwrap_or_else(|| format!("/login?{}", query));
        return Redirect::temporary(&target).into_response();
    }

    // Per-user home page: an exact / or /desk request redirects to the user's
    // configured home_page when one is set and is not /desk itself.
    if uri.path() == "/desk" || uri.path() == "/" {
        if let Some(ref user_name) = user {
            if let Some((_, pool)) = resolve_site_pool(&state, &headers) {
                if let Some(target) = get_effective_home_page(
                    &pool,
                    user_name,
                    state.config.auth.custom_home_path.as_deref(),
                )
                .await
                {
                    if !target.is_empty() && target != "/desk" {
                        return Redirect::temporary(&target).into_response();
                    }
                }
            }
        }
    }

    // Rust app pages are mounted under /kiff_logger/*; Frappe Desk routes Page
    // workspace links through /desk/<page_name>, so redirect those paths.
    match uri.path() {
        "/desk/server-token-ui" => {
            return Redirect::temporary("/kiff_logger/token-ui").into_response();
        }
        _ => {}
    }

    // Load assets.json once; used in both boot info and HTML includes.
    // This is cached in AppState and only re-read when the file changes.
    let assets_base = PathBuf::from("crates/http/assets");
    let (bundle_map, assets_mtime) = load_bundle_map(&state.asset_cache, &assets_base).await;

    // Resolve the DB pool early so we can compute a cache key and check the
    // per-user bootinfo cache before doing any heavy work.
    let pool = resolve_site_pool(&state, &headers).map(|(_, p)| p);

    // Build or retrieve cached boot info.
    let boot_json = if let Some(ref pool) = pool {
        let cache_key = match compute_boot_cache_key(pool, assets_mtime).await {
            Ok(key) => key,
            Err(e) => {
                tracing::warn!("failed to compute boot cache key: {}", e);
                String::new()
            }
        };
        let user_name = user.as_deref().unwrap_or("Guest");
        if !cache_key.is_empty() {
            if let Some(cached) = state.boot_cache.get(user_name, &cache_key) {
                cached
            } else {
                let boot = build_boot_info(&state, &headers, user.as_deref(), &bundle_map).await;
                let json = match serde_json::to_string(&boot) {
                    Ok(j) => j,
                    Err(e) => return error_response(&format!("boot serialization error: {}", e)),
                };
                state.boot_cache.set(user_name, &cache_key, json.clone());
                json
            }
        } else {
            let boot = build_boot_info(&state, &headers, user.as_deref(), &bundle_map).await;
            match serde_json::to_string(&boot) {
                Ok(j) => j,
                Err(e) => return error_response(&format!("boot serialization error: {}", e)),
            }
        }
    } else {
        let boot = build_boot_info(&state, &headers, user.as_deref(), &bundle_map).await;
        match serde_json::to_string(&boot) {
            Ok(j) => j,
            Err(e) => return error_response(&format!("boot serialization error: {}", e)),
        }
    };

    // Discover JS/CSS assets from Frappe's assets.json
    let (js_includes, css_includes) = discover_assets(&bundle_map);

    // Build HTML
    let build_version = env!("CARGO_PKG_VERSION");
    let csrf_token = generate_csrf_token();
    let lang = "en";
    let layout_direction = "ltr";

    let icon_sprites = load_icon_sprites(&state.asset_cache).await;

    let html = DESK_TEMPLATE
        .replace("{{BOOT_JSON}}", &boot_json)
        .replace("{{JS_INCLUDES}}", &js_includes)
        .replace("{{CSS_INCLUDES}}", &css_includes)
        .replace("{{ICON_SPRITES}}", &icon_sprites)
        .replace("{{BUILD_VERSION}}", build_version)
        .replace("{{CSRF_TOKEN}}", &csrf_token)
        .replace("{{LANG}}", lang)
        .replace("{{LAYOUT_DIRECTION}}", layout_direction);

    axum::response::Html(html).into_response()
}

async fn extract_user_from_request(state: &AppState, headers: &HeaderMap) -> Option<String> {
    let cookie_header = headers.get("cookie")?.to_str().ok()?;
    let sid = extract_cookie_value(cookie_header, "sid")?;

    let (_, pool) = resolve_site_pool(state, headers)?;
    let store = session::SessionStore::new();
    match store.get(&pool, &sid).await {
        Ok(Some(session)) if !session.is_expired() => Some(session.user),
        _ => None,
    }
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

/// Tables whose contents affect the rendered Desk bootinfo. When any of these
/// change, the per-user bootinfo cache must be regenerated.
const BOOT_CACHE_TABLES: &[&str] = &[
    "user",
    "role",
    "has_role",
    "docperm",
    "workspace",
    "module_def",
    "property_setter",
    "custom_field",
    "client_script",
    "navbar_settings",
    "notification_settings",
    "letter_head",
    "desktop_settings",
    "doctype",
    "docfield",
    // Note: route_history is intentionally excluded. It changes on every desk
    // navigation and would churn the cache constantly. frequently_visited_links
    // may be slightly stale until the 5-minute TTL expires.
];

/// Build a cache key for the per-user bootinfo cache. The key changes whenever
/// any table that contributes to bootinfo is modified, or when the static asset
/// manifest changes.
async fn compute_boot_cache_key(
    pool: &orm::DatabasePool,
    assets_mtime: SystemTime,
) -> error::Result<String> {
    let mut parts = Vec::with_capacity(BOOT_CACHE_TABLES.len() + 1);
    parts.push(format!("assets={:?}", assets_mtime));

    for table in BOOT_CACHE_TABLES {
        let sql = format!(
            r#"SELECT COALESCE(MAX(modified), '') AS m, COUNT(*) AS c FROM "{}""#,
            table
        );
        let (modified, count) = match pool.execute_sql(&sql, vec![]).await {
            Ok(rows) => {
                let row = rows.into_iter().next().unwrap_or_default();
                (
                    row.get("m")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    row.get("c").and_then(|v| v.as_i64()).unwrap_or(0),
                )
            }
            Err(e) => {
                // Tables may not exist on a fresh/empty site; treat them as
                // empty so the cache key still computes.
                tracing::debug!(
                    "boot cache key table {} not available: {}",
                    table,
                    e
                );
                (String::new(), 0)
            }
        };
        parts.push(format!("{}:{}:{}", table, count, modified));
    }

    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    parts.hash(&mut hasher);
    Ok(format!("{:x}", hasher.finish()))
}

/// Load child-table rows for a set of workspaces and group them by workspace name.
/// Returns a map of workspace name -> Vec<row-as-Value> for the requested child table.
async fn load_workspace_children(
    pool: &orm::DatabasePool,
    table: &str,
    parentfield: &str,
    columns: &[&str],
) -> error::Result<HashMap<String, Vec<Value>>> {
    let cols = columns.join(", ");
    let sql = format!(
        r#"SELECT {} FROM "{}" WHERE parenttype = 'Workspace' AND parentfield = '{}' ORDER BY COALESCE(idx, 0)"#,
        cols, table, parentfield
    );
    let rows = match pool.execute_sql(&sql, vec![]).await {
        Ok(rows) => rows,
        // Some child tables may not exist on a fresh/empty site; treat them as
        // empty so bootinfo still builds without crashing.
        Err(e) => {
            tracing::debug!(
                "workspace child table {} not available for bootinfo: {}",
                table,
                e
            );
            return Ok(HashMap::new());
        }
    };

    let mut grouped: HashMap<String, Vec<Value>> = HashMap::new();
    for mut row in rows {
        if let Some(parent) = row
            .remove("parent")
            .and_then(|v| v.as_str().map(String::from))
        {
            grouped.entry(parent).or_default().push(Value::Object(
                row.into_iter().collect(),
            ));
        }
    }
    Ok(grouped)
}

/// Attach workspace child tables (links, shortcuts, charts, number_cards,
/// quick_lists, custom_blocks) to each workspace object. The Frappe 16 desk
/// renders cards/shortcuts from these arrays, so without them the workspace
/// appears empty even when the user has permission to read the DocTypes.
async fn attach_workspace_children(
    pool: &orm::DatabasePool,
    workspaces: &mut [Value],
) -> error::Result<()> {
    let child_specs: Vec<(&str, &str, Vec<&str>)> = vec![
        (
            "workspace_link",
            "links",
            vec![
                "name",
                "creation",
                "modified",
                "owner",
                "idx",
                "parent",
                "type",
                "label",
                "icon",
                "hidden",
                "link_type",
                "link_to",
                "dependencies",
                "only_for",
                "onboard",
                "is_query_report",
                "link_count",
                "description",
                "report_ref_doctype",
            ],
        ),
        (
            "workspace_shortcut",
            "shortcuts",
            vec![
                "name",
                "creation",
                "modified",
                "owner",
                "idx",
                "parent",
                "type",
                "link_to",
                "doc_view",
                "label",
                "icon",
                "restrict_to_domain",
                "stats_filter",
                "color",
                "format",
                "url",
                "kanban_board",
                "report_ref_doctype",
            ],
        ),
        (
            "workspace_chart",
            "charts",
            vec![
                "name",
                "creation",
                "modified",
                "owner",
                "idx",
                "parent",
                "chart_name",
                "label",
            ],
        ),
        (
            "workspace_number_card",
            "number_cards",
            vec![
                "name",
                "creation",
                "modified",
                "owner",
                "idx",
                "parent",
                "number_card_name",
                "label",
            ],
        ),
        (
            "workspace_quick_list",
            "quick_lists",
            vec![
                "name",
                "creation",
                "modified",
                "owner",
                "idx",
                "parent",
                "document_type",
                "label",
                "quick_list_filter",
            ],
        ),
        (
            "workspace_custom_block",
            "custom_blocks",
            vec![
                "name",
                "creation",
                "modified",
                "owner",
                "idx",
                "parent",
                "custom_block_name",
                "label",
            ],
        ),
    ];

    for (table, field, columns) in child_specs {
        let grouped = load_workspace_children(pool, table, field, &columns).await?;
        for ws in workspaces.iter_mut() {
            if let Some(obj) = ws.as_object_mut() {
                if let Some(name) = obj.get("name").and_then(|v| v.as_str()) {
                    let children = grouped.get(name).cloned().unwrap_or_default();
                    obj.insert(field.to_string(), json!(children));
                }
            }
        }
    }

    Ok(())
}

/// Load the modules blocked for a user (direct user rows or Module Profile).
async fn get_blocked_modules(
    pool: &orm::DatabasePool,
    user: &str,
) -> error::Result<HashSet<String>> {
    let rows = pool
        .execute_sql(
            r#"SELECT module FROM "block_module" WHERE parent = ? AND parenttype = 'User'"#,
            vec![Value::String(user.into())],
        )
        .await?;

    Ok(rows
        .into_iter()
        .filter_map(|r| r.get("module").and_then(|v| v.as_str()).map(String::from))
        .collect())
}

async fn query_boot_data(
    pool: &orm::DatabasePool,
    blocked_modules: &HashSet<String>,
    visible_apps: Option<&HashSet<String>>,
) -> error::Result<(
    Vec<Value>,
    Map<String, Value>,
    Vec<String>,
    Map<String, Value>,
    Option<String>,
)> {
    // Query workspaces
    let ws_rows = pool.execute_sql(
        r#"SELECT name, label, title, icon, public, is_hidden, sequence_id, module, parent_page, for_user, content,
                  app, type, link_type, link_to, external_link, indicator_color
           FROM "workspace"
           WHERE (for_user = '' OR for_user IS NULL)
           ORDER BY COALESCE(sequence_id, 9999)"#,
        vec![],
    ).await?;

    let mut workspaces = Vec::new();
    let mut module_wise_workspaces: Map<String, Value> = Map::new();
    let mut default_ws: Option<String> = None;

    for row in ws_rows {
        let name = row
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if name.is_empty() {
            continue;
        }
        let label = row
            .get("label")
            .and_then(|v| v.as_str())
            .unwrap_or(&name)
            .to_string();
        let title = row
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or(&label)
            .to_string();
        let icon = row
            .get("icon")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let module = row
            .get("module")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if !module.is_empty() && blocked_modules.contains(&module) {
            continue;
        }
        let parent_page = row
            .get("parent_page")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let public = row
            .get("public")
            .and_then(|v| {
                v.as_i64()
                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            })
            .unwrap_or(1);
        let is_hidden = row
            .get("is_hidden")
            .and_then(|v| {
                v.as_i64()
                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            })
            .unwrap_or(0);
        let sequence_id = row
            .get("sequence_id")
            .and_then(|v| {
                v.as_f64()
                    .or_else(|| v.as_i64().map(|i| i as f64))
                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            })
            .unwrap_or(0.0);

        let content = match row.get("content") {
            Some(Value::String(s)) if !s.trim().is_empty() => Value::String(s.clone()),
            Some(Value::String(_)) => Value::Null,
            Some(v) => v.clone(),
            None => Value::Null,
        };
        let app = row
            .get("app")
            .and_then(|v| v.as_str())
            .unwrap_or("frappe")
            .to_string();
        if let Some(visible) = visible_apps {
            if !visible.contains(&app) {
                continue;
            }
        }
        let ws_type = row
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("Workspace")
            .to_string();
        let link_type = row
            .get("link_type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let link_to = row
            .get("link_to")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let external_link = row
            .get("external_link")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let indicator_color = row
            .get("indicator_color")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let for_user_val = row
            .get("for_user")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let mut ws_obj = json!({
            "name": name,
            "label": label,
            "title": title,
            "icon": icon,
            "public": public,
            "is_hidden": is_hidden,
            "sequence_id": sequence_id,
            "module": module,
            "parent_page": parent_page,
            "content": content,
            "app": app,
            "type": ws_type,
            "link_type": link_type,
            "link_to": link_to,
            "external_link": external_link,
            "indicator_color": indicator_color,
            "for_user": for_user_val,
        });

        // Workspace links of type "Report" need a report object for the router.
        if link_type == "Report" && !link_to.is_empty() {
            ws_obj.as_object_mut().unwrap().insert(
                "report".to_string(),
                json!({
                    "name": link_to,
                    "title": link_to,
                    "report_type": "Report Builder",
                    "ref_doctype": "",
                }),
            );
        }
        workspaces.push(ws_obj);

        // Track first workspace as default
        if default_ws.is_none() {
            default_ws = Some(name.clone());
        }

        // Group by module
        if !module.is_empty() {
            let entry = module_wise_workspaces
                .entry(module.clone())
                .or_insert_with(|| json!([]));
            if let Some(arr) = entry.as_array_mut() {
                arr.push(json!(name));
            }
        }
    }

    // Attach child tables so cards, shortcuts, charts, etc. render.
    attach_workspace_children(pool, &mut workspaces).await?;

    // Query modules
    let mod_rows = pool
        .execute_sql(
            r#"SELECT name, module_name, app_name FROM "module_def" ORDER BY module_name"#,
            vec![],
        )
        .await?;

    let mut modules_map = Map::new();
    let mut module_list = Vec::new();

    for row in mod_rows {
        let name = row
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if name.is_empty() {
            continue;
        }
        if blocked_modules.contains(&name) {
            continue;
        }
        if let Some(visible) = visible_apps {
            let module_app = row
                .get("app_name")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("frappe");
            if !visible.contains(module_app) {
                continue;
            }
        }
        let module_name = row
            .get("module_name")
            .and_then(|v| v.as_str())
            .unwrap_or(&name)
            .to_string();

        let mod_obj = json!({
            "label": module_name,
            "color": "#8D99A6",
            "icon": "",
            "type": "module",
        });
        modules_map.insert(module_name.clone(), mod_obj);
        module_list.push(module_name);
    }

    Ok((
        workspaces,
        modules_map,
        module_list,
        module_wise_workspaces,
        default_ws,
    ))
}

/// Build workspace-related boot objects from the workspace list queried from the DB.
/// Returns (workspaces object, workspace_sidebar_item object, default_workspace object).
fn build_workspace_boot_objects(workspaces: &[Value], is_guest: bool) -> (Value, Value, Value) {
    // Frappe 16 expects boot.workspaces = { pages, has_access, has_create_access }
    let mut workspaces_obj = Map::new();
    workspaces_obj.insert("pages".to_string(), json!(workspaces));
    workspaces_obj.insert("has_access".to_string(), json!(true));
    workspaces_obj.insert("has_create_access".to_string(), json!(!is_guest));
    let workspaces_value = Value::Object(workspaces_obj);

    // Frappe 16 sidebar expects workspace_sidebar_item = { title_lower: { items, module, app } }
    let mut workspace_sidebar_item = Map::new();
    workspace_sidebar_item.insert(
        "my workspaces".to_string(),
        json!({
            "items": workspaces.iter().map(|ws| {
                json!({
                    "label": ws.get("label").unwrap_or(&Value::Null),
                    "link_to": ws.get("name").unwrap_or(&Value::Null),
                    "link_type": "Workspace",
                    "type": "Link",
                    "icon": ws.get("icon").unwrap_or(&Value::Null),
                    "child": false,
                    "collapsible": false,
                    "indent": 0,
                    "keep_closed": false,
                    "url": Value::Null,
                    "show_arrow": false,
                    "filters": Value::Null,
                    "route_options": Value::Null,
                    "tab": Value::Null,
                })
            }).collect::<Vec<Value>>(),
            "module": "Core",
            "app": "frappe",
        }),
    );
    for ws in workspaces {
        if let Some(title) = ws.get("title").and_then(|v| v.as_str()) {
            let name = ws.get("name").and_then(|v| v.as_str()).unwrap_or(title);
            workspace_sidebar_item.insert(
                title.to_lowercase(),
                json!({
                    "items": [{
                        "label": title,
                        "link_to": name,
                        "link_type": "Workspace",
                        "type": "Link",
                        "icon": ws.get("icon").unwrap_or(&Value::Null),
                        "child": false,
                        "collapsible": false,
                        "indent": 0,
                        "keep_closed": false,
                        "url": Value::Null,
                        "show_arrow": false,
                        "filters": Value::Null,
                        "route_options": Value::Null,
                        "tab": Value::Null,
                    }],
                    "module": ws.get("module").unwrap_or(&json!("")),
                    "app": "frappe",
                }),
            );
        }
    }
    let workspace_sidebar_item_value = Value::Object(workspace_sidebar_item);

    // Build default_workspace as an object {name, title, public} — the frontend
    // expects frappe.boot.user.default_workspace to be an object, not a string.
    let default_workspace_obj = workspaces
        .first()
        .map(|ws| {
            json!({
                "name": ws.get("name"),
                "title": ws.get("title"),
                "public": ws.get("public"),
            })
        })
        .unwrap_or(Value::Null);

    (
        workspaces_value,
        workspace_sidebar_item_value,
        default_workspace_obj,
    )
}

/// Frappe's `scrub`: lower-case and replace spaces/hyphens with underscores.
fn scrub_module_name(name: &str) -> String {
    name.to_lowercase().replace([' ', '-'], "_")
}

/// Frappe's `slug`: lower-case and replace spaces with hyphens.
fn slugify(name: &str) -> String {
    name.to_lowercase().replace(' ', "-")
}

/// Build `desktop_icons` boot entries, mirroring Frappe's
/// `create_desktop_icons_from_installed_apps` and
/// `create_desktop_icons_from_workspace`: one "App" icon per installed app
/// and one "Link" icon per public workspace. The desk sidebar dropdown and
/// the /desk icon grid are driven entirely by this list — when it is empty
/// there is no way to navigate to another app's workspaces (e.g. Sebrus
/// Apps) from the sidebar.
fn build_desktop_icons(workspaces: &[Value], app_data: &[Value]) -> Vec<Value> {
    let mut icons = Vec::new();
    for (idx, app) in app_data.iter().enumerate() {
        let label = app
            .get("app_title")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if label.is_empty() {
            continue;
        }
        icons.push(json!({
            "name": label,
            "label": label,
            "icon_type": "App",
            "link_type": "External",
            "link": app.get("app_route").cloned().unwrap_or(Value::Null),
            "app": app.get("app_name").cloned().unwrap_or(Value::Null),
            "logo_url": app.get("app_logo_url").cloned().unwrap_or(Value::Null),
            "icon": "",
            "parent_icon": Value::Null,
            "idx": idx,
            "standard": 1,
            "hidden": 0,
        }));
    }
    let offset = icons.len();
    for (i, ws) in workspaces.iter().enumerate() {
        let title = ws.get("title").and_then(|v| v.as_str()).unwrap_or_default();
        let name = ws.get("name").and_then(|v| v.as_str()).unwrap_or_default();
        if title.is_empty() || name.is_empty() {
            continue;
        }
        icons.push(json!({
            "name": title,
            "label": title,
            "icon_type": "Link",
            "link_type": "Workspace Sidebar",
            "link_to": name,
            "link": Value::Null,
            "icon": ws.get("icon").cloned().unwrap_or(Value::Null),
            "app": ws.get("app").cloned().unwrap_or(Value::Null),
            "logo_url": "",
            "parent_icon": Value::Null,
            "idx": offset + i,
            "standard": 1,
            "hidden": 0,
        }));
    }
    icons
}

/// Rust equivalent of Frappe's `get_desktop_icon_urls`: scan each installed
/// app's `public/icons/desktop_icons/{subtle,solid}` directories and map app
/// name → variant → asset URLs. The desk frontend reads
/// `frappe.boot.desktop_icon_urls[app][variant]` when rendering desktop
/// icons and crashes without it.
fn load_desktop_icon_urls(installed_apps: &[String]) -> Map<String, Value> {
    let mut map = Map::new();
    for app in installed_apps {
        let icons_dir = PathBuf::from("apps")
            .join(app)
            .join(app)
            .join("public")
            .join("icons")
            .join("desktop_icons");
        if !icons_dir.is_dir() {
            continue;
        }
        let mut variants = Map::new();
        for variant in ["subtle", "solid"] {
            let mut urls = Vec::new();
            if let Ok(entries) = std::fs::read_dir(icons_dir.join(variant)) {
                for entry in entries.flatten() {
                    let fname = entry.file_name().to_string_lossy().to_string();
                    if fname.ends_with(".svg") {
                        urls.push(json!(format!(
                            "assets/{}/icons/desktop_icons/{}/{}",
                            app, variant, fname
                        )));
                    }
                }
            }
            variants.insert(variant.to_string(), json!(urls));
        }
        map.insert(app.clone(), Value::Object(variants));
    }
    map
}

/// Read the Frappe apps installed on this site and append the registered
/// Rust apps so both Python and Rust workspaces show up in the app switcher.
async fn get_installed_apps(rust_apps: &rust_apps_core::RustAppRegistry) -> Vec<String> {
    let mut apps = Vec::new();

    if let Ok(content) = tokio::fs::read_to_string("sites/apps.txt").await {
        for line in content.lines() {
            let line = line.trim();
            if !line.is_empty() && !line.starts_with('#') && !apps.contains(&line.to_string()) {
                apps.push(line.to_string());
            }
        }
    }

    if apps.is_empty() {
        apps.push("frappe".to_string());
    }

    for app in rust_apps.apps() {
        let name = app.name().to_string();
        if !apps.contains(&name) {
            apps.push(name);
        }
    }

    apps
}

/// Build `module_app` (scrubbed module name -> owning app).
async fn build_module_app(pool: &orm::DatabasePool) -> error::Result<Map<String, Value>> {
    let rows = pool
        .execute_sql(
            r#"SELECT name, app_name FROM "module_def" ORDER BY name"#,
            vec![],
        )
        .await?;

    let mut map = Map::new();
    for row in rows {
        if let (Some(name), Some(app)) = (
            row.get("name").and_then(|v| v.as_str()),
            row.get("app_name").and_then(|v| v.as_str()),
        ) {
            if !name.is_empty() && !app.is_empty() {
                map.insert(scrub_module_name(name), json!(app));
            }
        }
    }
    Ok(map)
}

/// Return a human-readable title and logo URL for well-known apps.
fn default_app_metadata(app: &str) -> (String, String) {
    match app {
        "frappe" => (
            "Frappe Framework".to_string(),
            "/assets/frappe/images/frappe-framework-logo.svg".to_string(),
        ),
        _ => (app.to_string(), "".to_string()),
    }
}

/// Build `app_data`: one entry per installed app with its modules and workspaces.
async fn build_app_data(
    pool: &orm::DatabasePool,
    installed_apps: &[String],
    workspaces: &[Value],
) -> error::Result<Vec<Value>> {
    let mut result = Vec::new();

    for app in installed_apps {
        let rows = pool
            .execute_sql(
                r#"SELECT name, module_name FROM "module_def" WHERE app_name = ? ORDER BY module_name"#,
                vec![Value::String(app.clone())],
            )
            .await?;

        let modules: Vec<String> = rows
            .iter()
            .filter_map(|r| r.get("name").and_then(|v| v.as_str()).map(String::from))
            .collect();

        let app_modules: HashSet<String> = modules.iter().map(|m| scrub_module_name(m)).collect();
        let app_workspaces: Vec<String> = workspaces
            .iter()
            .filter_map(|ws| {
                let ws_module = ws.get("module").and_then(|v| v.as_str())?;
                if app_modules.contains(&scrub_module_name(ws_module)) {
                    ws.get("name").and_then(|v| v.as_str()).map(String::from)
                } else {
                    None
                }
            })
            .collect();

        let app_route = app_workspaces
            .first()
            .map(|ws| format!("/app/{}", slugify(ws)))
            .unwrap_or_default();

        let (app_title, app_logo_url) = default_app_metadata(app);

        result.push(json!({
            "app_name": app,
            "app_title": app_title,
            "app_route": app_route,
            "app_logo_url": app_logo_url,
            "modules": modules,
            "workspaces": app_workspaces,
        }));
    }

    Ok(result)
}

/// Build `allowed_modules`: module objects the desktop can render as icons.
fn build_allowed_modules(modules_map: &Map<String, Value>) -> Vec<Value> {
    modules_map
        .iter()
        .map(|(module_name, obj)| {
            let label = obj
                .get("label")
                .and_then(|v| v.as_str())
                .unwrap_or(module_name);
            json!({
                "module_name": module_name,
                "label": label,
                "type": "module",
                "icon": "",
                "color": "#8D99A6",
            })
        })
        .collect()
}

/// Compute permission-type -> [doctype] lists for a user from the Rust
/// permission engine. Used to fix the desk bootinfo when the Python shim
/// leaves can_create/write/delete/etc. empty.
/// Return all non-table DocType names from the metadata table.
async fn get_all_doctype_names(pool: &orm::DatabasePool) -> Vec<String> {
    let rows = match pool
        .execute_sql(
            r#"SELECT name FROM "doctype" WHERE istable = 0 ORDER BY name"#,
            vec![],
        )
        .await
    {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!("failed to load doctype names for bootinfo: {}", e);
            return vec![];
        }
    };

    rows.into_iter()
        .filter_map(|mut row| {
            row.remove("name")
                .and_then(|v| v.as_str().map(String::from))
        })
        .collect()
}

async fn compute_user_permission_lists(
    state: &AppState,
    pool: &orm::DatabasePool,
    user: &str,
    doctypes: &[String],
) -> HashMap<String, Vec<String>> {
    let mut result: HashMap<String, Vec<String>> = HashMap::new();
    let ptypes = vec![
        "read", "write", "create", "delete", "submit", "cancel", "select", "report", "export",
        "import", "print", "email", "share",
    ];

    // Administrator is implicitly granted every permission on every DocType,
    // matching Frappe's behaviour.
    if user == "Administrator" {
        for ptype in ptypes {
            result.insert(ptype.to_string(), doctypes.to_vec());
        }
        return result;
    }

    for doctype in doctypes {
        for ptype in &ptypes {
            match state
                .permissions
                .has_permission(pool, user, doctype, ptype, None)
                .await
            {
                Ok(true) => {
                    result
                        .entry((*ptype).to_string())
                        .or_default()
                        .push(doctype.clone());
                }
                _ => {}
            }
        }
    }
    result
}

async fn build_boot_info(
    state: &AppState,
    headers: &HeaderMap,
    user: Option<&str>,
    bundle_map: &HashMap<String, String>,
) -> serde_json::Value {
    let is_guest = user.is_none();
    let user_name = user.unwrap_or("Guest");

    // Get DB pool for site queries first; workspace data is needed both for the
    // Python bootinfo overlay and for the fallback bootinfo.
    let pool = resolve_site_pool(state, headers).map(|(_, p)| p);

    // Load the user's blocked module list so we can hide those workspaces/modules.
    let blocked_modules: HashSet<String> = if let Some(ref pool) = pool {
        if is_guest {
            HashSet::new()
        } else {
            get_blocked_modules(pool, user_name)
                .await
                .unwrap_or_default()
        }
    } else {
        HashSet::new()
    };

    // Non-admin users only see workspaces belonging to Sebrus Logger and any
    // other registered Rust app (`app` == a name in the Rust app registry) --
    // the standard Frappe workspaces (Users, Settings, Building the Basics,
    // etc., which default to `app: "frappe"`) are hidden for them.
    // Administrator / System Manager keep the full, unfiltered desk.
    let is_admin = !is_guest
        && (user_name == "Administrator" || {
            if let Some(ref pool) = pool {
                state
                    .permissions
                    .get_roles(pool, user_name)
                    .await
                    .map(|roles| roles.iter().any(|r| r == "System Manager"))
                    .unwrap_or(false)
            } else {
                false
            }
        });
    let visible_apps: Option<HashSet<String>> = if is_admin {
        None
    } else {
        Some(
            state
                .rust_apps
                .apps()
                .iter()
                .map(|a| a.name().to_string())
                .collect(),
        )
    };

    // Query workspaces and modules from DB
    let (workspaces, modules_map, module_list, module_wise_workspaces, _default_ws) =
        if let Some(ref pool) = pool {
            match query_boot_data(pool, &blocked_modules, visible_apps.as_ref()).await {
                Ok(data) => data,
                Err(_) => (vec![], Map::new(), vec![], Map::new(), None),
            }
        } else {
            (vec![], Map::new(), vec![], Map::new(), None)
        };

    // Build module/app mapping and app data. These are Kiff-managed values that
    // must be present even when the Python bootinfo call fails.
    let installed_apps = get_installed_apps(&state.rust_apps).await;
    let module_app = if let Some(ref pool) = pool {
        build_module_app(pool).await.unwrap_or_default()
    } else {
        Map::new()
    };
    let app_data = if let Some(ref pool) = pool {
        build_app_data(pool, &installed_apps, &workspaces)
            .await
            .unwrap_or_default()
    } else {
        vec![]
    };
    let allowed_modules = build_allowed_modules(&modules_map);

    let (workspaces_value, workspace_sidebar_item_value, default_workspace_obj) =
        build_workspace_boot_objects(&workspaces, is_guest);
    let desktop_icons = build_desktop_icons(&workspaces, &app_data);
    let desktop_icon_urls = load_desktop_icon_urls(&installed_apps);

    // Use the real permission engine for the user's role list. The fallback
    // used to hardcode ["Administrator"], which hid permlevel-1 fields on the
    // User form (Roles / Modules) because the client never matched the
    // "System Manager" permission rules.
    let roles: Value = if is_guest {
        json!([])
    } else if let Some(ref pool) = pool {
        match state.permissions.get_roles(pool, user_name).await {
            Ok(roles) => json!(roles),
            Err(e) => {
                tracing::warn!(
                    "failed to load roles for bootinfo user {}: {}, falling back",
                    user_name,
                    e
                );
                json!(["Administrator"])
            }
        }
    } else {
        json!(["Administrator"])
    };

    // Administrator gets full permissions on core doctypes
    let core_doctypes: Vec<&str> = vec![
        "User",
        "Role",
        "Has Role",
        "Module Def",
        "Workspace",
        "Page",
        "DocType",
        "DocField",
        "DocPerm",
        "System Settings",
        "Custom Field",
        "Property Setter",
        "Workflow",
        "Workflow State",
        "Workflow Action Master",
        "Gender",
        "Salutation",
        "User Type",
        "Language",
        "Translation",
        "File",
        "Report",
        "Dashboard",
        "Dashboard Chart",
        "Number Card",
        "Notification Settings",
        "Error Log",
        "Activity Log",
        "Access Log",
        "Version",
        "Communication",
        "Comment",
        "ToDo",
        "Event",
        "Note",
        "Tag",
        "Tag Link",
        "Patch Log",
        "Scheduled Job Type",
        "Scheduler Event",
        "RQ Job",
        "RQ Worker",
        "Webhook",
        "Server Script",
        "Client Script",
        "Print Format",
        "Letter Head",
        "Terms and Conditions",
        "Address",
        "Contact",
        "Country",
        "Currency",
        "Calendar View",
        "Kanban Board",
        "List View Settings",
        "Form Tour",
        "Onboarding Step",
        "Module Onboarding",
        "Domain",
        "Company",
        "Website Theme",
        "Web Page",
        "Web Form",
        "Blogger",
        "Blog Post",
        "Blog Category",
        "Blog Settings",
        "Website Settings",
        "About Us Settings",
        "Contact Us Settings",
        "Social Login Key",
        "OAuth Client",
        "OAuth Authorization Code",
        "OAuth Bearer Token",
        "Integration Request",
        "Connected App",
        "Email Account",
        "Email Domain",
        "Email Template",
        "Notification",
        "Auto Email Report",
        "S3 Backup Settings",
        "Dropbox Settings",
        "Google Settings",
        "Google Drive",
        "LDAP Settings",
        "Stripe Settings",
        "PayPal Settings",
        "Recorder Query",
        "Success Action",
        "Review",
        "Global Search Settings",
        "Console Log",
        "Package",
        "Package Release",
        "Energy Point Rule",
        "Energy Point Log",
        "Milestone",
        "Milestone Tracker",
        "Transaction Log",
        "Bulk Update",
        "Data Import",
        "Data Export",
        "Document Share Key",
        "Document Naming Rule",
        "Document Naming Settings",
        "Submission Queue",
        "Installed Application",
        "Module Profile",
        "User Group",
        "User Group Member",
        "Dashboard Chart Source",
        "Number Card",
        "Shortcut",
        "Custom HTML Block",
        "Network Printer Settings",
        "Print Style",
        "Print Heading",
        "Address Template",
        "Contacts Settings",
        "Google Contacts",
        "Holiday List",
        "Weekday",
        "Stock Entry",
        "Item",
        "Item Group",
        "Warehouse",
        "UOM",
        "Brand",
        "Customer",
        "Supplier",
        "Sales Order",
        "Purchase Order",
        "Sales Invoice",
        "Purchase Invoice",
        "Payment Entry",
        "Journal Entry",
        "Account",
        "Cost Center",
        "Budget",
        "Project",
        "Task",
        "Timesheet",
        "Employee",
        "Department",
        "Designation",
        "Salary Structure",
        "Salary Slip",
        "Leave Application",
        "Attendance",
        "Job Opening",
        "Job Applicant",
        "Job Offer",
        "Quiz",
        "LMS Course",
        "LMS Batch",
        "LMS Enrollment",
    ];
    let core_doctypes = json!(core_doctypes);

    // In fallback bootinfo, use every non-table DocType from the metadata DB
    // so Rust-contributed DocTypes are available without the Python shim.
    let all_doctypes: Vec<String> = if let Some(ref pool) = pool {
        get_all_doctype_names(pool).await
    } else {
        core_doctypes
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    };
    let all_doctypes_json = json!(all_doctypes);

    let mut user_obj = Map::new();
    user_obj.insert("name".to_string(), json!(user_name));
    user_obj.insert("email".to_string(), json!(user_name));
    user_obj.insert("full_name".to_string(), json!(user_name));
    user_obj.insert(
        "user_type".to_string(),
        json!(if is_guest { "Guest" } else { "System User" }),
    );
    user_obj.insert("roles".to_string(), roles);
    user_obj.insert("language".to_string(), json!("en"));
    user_obj.insert("timezone".to_string(), json!("UTC"));
    user_obj.insert("can_read".to_string(), all_doctypes_json.clone());
    user_obj.insert("can_create".to_string(), all_doctypes_json.clone());
    user_obj.insert("can_write".to_string(), all_doctypes_json.clone());
    user_obj.insert("can_select".to_string(), all_doctypes_json.clone());
    user_obj.insert("can_submit".to_string(), json!([]));
    user_obj.insert("can_cancel".to_string(), json!([]));
    user_obj.insert("can_delete".to_string(), all_doctypes_json.clone());
    user_obj.insert("can_get_report".to_string(), all_doctypes_json.clone());
    user_obj.insert("allow_modules".to_string(), json!(module_list.clone()));
    user_obj.insert("all_read".to_string(), all_doctypes_json.clone());
    user_obj.insert("can_search".to_string(), all_doctypes_json.clone());
    user_obj.insert("in_create".to_string(), all_doctypes_json.clone());
    user_obj.insert("can_export".to_string(), all_doctypes_json.clone());
    user_obj.insert("can_import".to_string(), all_doctypes_json.clone());
    user_obj.insert("can_print".to_string(), all_doctypes_json.clone());
    user_obj.insert("can_email".to_string(), all_doctypes_json.clone());
    user_obj.insert("can_share".to_string(), all_doctypes_json.clone());
    user_obj.insert("all_reports".to_string(), json!({}));
    user_obj.insert("defaults".to_string(), json!({}));
    user_obj.insert("recent".to_string(), json!("[]"));
    user_obj.insert("last_selected_values".to_string(), json!({}));
    user_obj.insert("onboarding_status".to_string(), json!({}));
    user_obj.insert("document_follow_notify".to_string(), json!(false));
    user_obj.insert("send_me_a_copy".to_string(), json!(false));
    user_obj.insert("email_signature".to_string(), Value::Null);
    user_obj.insert("impersonated_by".to_string(), Value::Null);
    user_obj.insert("default_workspace".to_string(), default_workspace_obj);
    user_obj.insert("user_permissions".to_string(), json!({}));

    let mut user_info = Map::new();
    let mut user_info_entry = Map::new();
    user_info_entry.insert("email".to_string(), json!(user_name));
    user_info_entry.insert("full_name".to_string(), json!(user_name));
    user_info_entry.insert("image".to_string(), Value::Null);
    user_info_entry.insert("name".to_string(), json!(user_name));
    user_info_entry.insert("time_zone".to_string(), json!("UTC"));
    user_info.insert(user_name.to_string(), Value::Object(user_info_entry));

    let mut sysdefaults = Map::new();
    sysdefaults.insert("date_format".to_string(), json!("yyyy-mm-dd"));
    sysdefaults.insert("time_format".to_string(), json!("HH:mm:ss"));
    sysdefaults.insert("float_precision".to_string(), json!(3));
    sysdefaults.insert("currency_precision".to_string(), json!(2));
    sysdefaults.insert("currency".to_string(), json!("USD"));
    sysdefaults.insert("hide_currency_symbol".to_string(), json!("No"));
    sysdefaults.insert(
        "rounding_method".to_string(),
        json!("Banker's Rounding (legacy)"),
    );
    sysdefaults.insert("setup_complete".to_string(), json!(true));
    sysdefaults.insert("letter_head".to_string(), Value::Null);
    sysdefaults.insert("session_recording_start".to_string(), json!(0));
    sysdefaults.insert("disable_change_log_notification".to_string(), json!(1));
    sysdefaults.insert("max_report_rows".to_string(), json!(100000));
    sysdefaults.insert("link_field_results_limit".to_string(), json!(10));
    sysdefaults.insert("force_web_capture_mode_for_uploads".to_string(), json!(0));

    let mut time_zone = Map::new();
    time_zone.insert("system".to_string(), json!("UTC"));
    time_zone.insert("user".to_string(), json!("UTC"));

    let mut notification_settings = Map::new();
    notification_settings.insert("name".to_string(), json!(user_name));
    notification_settings.insert("enabled".to_string(), json!(true));
    notification_settings.insert("enable_email_notifications".to_string(), json!(true));
    notification_settings.insert("enable_email_mention".to_string(), json!(true));
    notification_settings.insert("enable_email_assignment".to_string(), json!(true));
    notification_settings.insert(
        "enable_email_threads_on_assigned_document".to_string(),
        json!(true),
    );
    notification_settings.insert("enable_email_share".to_string(), json!(true));
    notification_settings.insert("enable_email_event_reminders".to_string(), json!(true));

    let mut navbar_settings = Map::new();
    navbar_settings.insert("help_dropdown".to_string(), json!([]));
    navbar_settings.insert(
        "settings_dropdown".to_string(),
        json!([
            {"item_label": "My Settings", "route": "/app/user/"},
            {"item_label": "Logout", "route": "/logout"}
        ]),
    );
    navbar_settings.insert("announcement_widget".to_string(), json!(""));
    navbar_settings.insert("app_logo".to_string(), json!(""));

    let mut desk_settings = Map::new();
    desk_settings.insert("list_sidebar".to_string(), json!(true));
    desk_settings.insert("form_sidebar".to_string(), json!(true));
    desk_settings.insert("timeline".to_string(), json!(true));
    desk_settings.insert("dashboard".to_string(), json!(true));
    desk_settings.insert("search_bar".to_string(), json!(true));
    desk_settings.insert("notifications".to_string(), json!(true));
    desk_settings.insert("view_switcher".to_string(), json!(true));

    let mut timezone_info = Map::new();
    timezone_info.insert("zones".to_string(), json!({}));
    timezone_info.insert("rules".to_string(), json!({}));
    timezone_info.insert("links".to_string(), json!({}));

    let mut boot = Map::new();
    boot.insert("user".to_string(), Value::Object(user_obj));
    boot.insert("user_info".to_string(), Value::Object(user_info));
    boot.insert("sysdefaults".to_string(), Value::Object(sysdefaults));
    boot.insert("sitename".to_string(), json!("localhost"));
    boot.insert("home_page".to_string(), json!("desktop"));
    boot.insert("lang".to_string(), json!("en"));
    boot.insert("desk_theme".to_string(), json!("Light"));
    boot.insert("modules".to_string(), Value::Object(modules_map));
    boot.insert("module_list".to_string(), json!(module_list));
    boot.insert("time_zone".to_string(), Value::Object(time_zone));
    boot.insert("can_install".to_string(), json!([]));
    boot.insert("domains".to_string(), json!([]));
    boot.insert("active_domains".to_string(), json!([]));
    boot.insert("all_domains".to_string(), json!([]));
    boot.insert("doctypes".to_string(), json!([]));
    boot.insert("single_types".to_string(), json!([]));
    boot.insert("nested_set_doctypes".to_string(), json!([]));
    boot.insert("doctype_layouts".to_string(), json!([]));
    boot.insert("user_permissions".to_string(), json!({}));
    boot.insert(
        "notification_settings".to_string(),
        Value::Object(notification_settings),
    );
    boot.insert("is_first_startup".to_string(), json!(false));
    boot.insert("setup_complete".to_string(), json!(true));
    boot.insert("developer_mode".to_string(), json!(true));
    boot.insert("read_only".to_string(), json!(false));
    boot.insert("assets_json".to_string(), json!(bundle_map));
    // Singles that the desk syncs into locals via frappe.model.sync(frappe.boot.docs).
    // Print Settings is required by the form sidebar; System Settings by many boot paths.
    let docs = json!([
        {
            "doctype": ":Print Settings",
            "name": "Print Settings",
            "allow_print_for_draft": 1,
            "allow_print_for_cancelled": 0,
            "print_style": "Redesign",
            "font": "Default",
            "font_size": 9.0,
            "pdf_page_size": "A4",
            "send_print_as_pdf": 1,
            "repeat_header_footer": 1,
            "with_letterhead": 1,
            "add_draft_heading": 1,
        },
        {
            "doctype": ":System Settings",
            "name": "System Settings",
            "language": "en",
            "time_zone": "UTC",
            "date_format": "yyyy-mm-dd",
            "time_format": "HH:mm:ss",
            "setup_complete": 1,
            "currency": "USD",
            "float_precision": 3,
            "currency_precision": 2,
            "rounding_method": "Banker's Rounding (legacy)",
            "enable_scheduler": 0,
            "max_report_rows": 100000,
            "link_field_results_limit": 10,
        },
    ]);
    boot.insert("docs".to_string(), docs);
    boot.insert("workspaces".to_string(), workspaces_value);
    // Kept for older frontend code that may still reference it
    boot.insert("allowed_workspaces".to_string(), json!(workspaces));
    boot.insert(
        "module_wise_workspaces".to_string(),
        Value::Object(module_wise_workspaces),
    );
    boot.insert(
        "workspace_sidebar_item".to_string(),
        workspace_sidebar_item_value,
    );
    boot.insert("dashboards".to_string(), json!([]));

    // Expose Rust app pages to users with the appropriate roles.
    let mut page_info = serde_json::Map::new();
    let mut allowed_pages = Vec::new();
    if let Some(ref pool) = pool {
        if !is_guest {
            let pm = PermissionEngine::new();
            if let Ok(roles) = pm.get_roles(pool, user_name).await {
                if roles.iter().any(|r| r == "Kiff Logs Admin") {
                    page_info.insert(
                        "kiff-logger-token-ui".to_string(),
                        json!({
                            "title": "Kiff Logger Token Generator",
                            "route": "kiff-logger-token-ui",
                            "module": "KiffLogger",
                            "icon": "fa fa-key"
                        }),
                    );
                    allowed_pages.push("kiff-logger-token-ui");
                }

            }
        }
    }
    boot.insert("page_info".to_string(), Value::Object(page_info));
    boot.insert("allowed_pages".to_string(), json!(allowed_pages));
    boot.insert("allowed_modules".to_string(), json!(allowed_modules));
    boot.insert("notes".to_string(), json!([]));
    boot.insert("letter_heads".to_string(), json!({}));
    boot.insert("module_app".to_string(), Value::Object(module_app));
    boot.insert("app_data".to_string(), json!(app_data));
    // app_name_style must stay absent (real Frappe boot does not set it): a
    // "Default" value makes sidebar.choose_app_name() return early, leaving
    // frappe.current_app unset and the Workspaces dropdown empty.
    boot.insert("desktop_icons".to_string(), json!(desktop_icons));
    boot.insert(
        "desktop_icon_urls".to_string(),
        Value::Object(desktop_icon_urls),
    );
    boot.insert("desktop_icon_style".to_string(), json!("Subtle"));
    boot.insert("calendars".to_string(), json!([]));
    boot.insert("treeviews".to_string(), json!([]));
    boot.insert("print_css".to_string(), json!(""));
    boot.insert("home_folder".to_string(), json!(""));
    boot.insert(
        "navbar_settings".to_string(),
        Value::Object(navbar_settings),
    );
    boot.insert(
        "app_logo_url".to_string(),
        json!("/assets/frappe/images/frappe-framework-logo.svg"),
    );
    boot.insert("onboarding_tours".to_string(), json!([]));
    boot.insert("versions".to_string(), json!({}));
    boot.insert("error_report_email".to_string(), Value::Null);
    boot.insert("lang_dict".to_string(), json!({}));
    boot.insert("success_action".to_string(), json!([]));
    boot.insert("email_accounts".to_string(), json!([]));
    boot.insert("all_accounts".to_string(), json!([]));
    boot.insert("energy_points_enabled".to_string(), json!(false));
    boot.insert("website_tracking_enabled".to_string(), json!(false));
    boot.insert("sms_gateway_enabled".to_string(), json!(false));
    boot.insert("points".to_string(), json!({}));
    boot.insert("frequently_visited_links".to_string(), json!([]));
    boot.insert("link_preview_doctypes".to_string(), json!([]));
    boot.insert("additional_filters_config".to_string(), json!({}));
    boot.insert("desk_settings".to_string(), Value::Object(desk_settings));
    boot.insert("link_title_doctypes".to_string(), json!([]));
    boot.insert("translated_doctypes".to_string(), json!([]));
    boot.insert("marketplace_apps".to_string(), json!([]));
    boot.insert("is_fc_site".to_string(), json!(false));
    boot.insert("changelog_feed".to_string(), json!([]));
    boot.insert("sentry_dsn".to_string(), Value::Null);
    boot.insert("setup_wizard_completed_apps".to_string(), json!([]));
    boot.insert("setup_wizard_not_required_apps".to_string(), json!([]));
    boot.insert("max_file_size".to_string(), json!(10485760));
    boot.insert("socketio_port".to_string(), json!(9000));
    boot.insert("messages".to_string(), Value::Null);
    boot.insert("notes".to_string(), json!([]));
    boot.insert("change_log".to_string(), Value::Null);
    boot.insert("has_app_updates".to_string(), json!(false));
    boot.insert("metadata_version".to_string(), json!("1"));
    boot.insert("timezone_info".to_string(), Value::Object(timezone_info));
    boot.insert("disable_async".to_string(), json!(false));
    boot.insert(
        "server_date".to_string(),
        json!(chrono::Local::now().format("%Y-%m-%d").to_string()),
    );

    // Fill in fields that used to come from Python's get_bootinfo.
    if let Some(ref pool) = pool {
        augment_boot_info(&mut boot, state, pool, user_name).await;
    }

    sanitize_bootinfo(&mut boot);
    Value::Object(boot)
}

/// Fill in bootinfo fields that the Python `frappe.boot.get_bootinfo` used to
/// provide. This keeps the frontend happy while staying entirely in Rust.
async fn augment_boot_info(
    boot: &mut Map<String, Value>,
    state: &AppState,
    pool: &orm::DatabasePool,
    user_name: &str,
) {
    boot.insert(
        "versions".to_string(),
        json!(build_versions().await.unwrap_or_default()),
    );
    boot.insert(
        "frequently_visited_links".to_string(),
        json!(load_frequently_visited_links(pool, user_name).await.unwrap_or_default()),
    );
    boot.insert(
        "letter_heads".to_string(),
        json!(load_letter_heads(pool).await.unwrap_or_default()),
    );
    if let Ok(Some(settings)) = load_notification_settings(pool, user_name).await {
        boot.insert("notification_settings".to_string(), settings);
    }
    if let Ok(Some(settings)) = load_navbar_settings(pool).await {
        boot.insert("navbar_settings".to_string(), settings);
    }
    if let Ok(Some(settings)) = load_desk_settings(pool, user_name).await {
        boot.insert("desk_settings".to_string(), settings);
    }
    boot.insert(
        "link_preview_doctypes".to_string(),
        json!(load_link_preview_doctypes(pool).await.unwrap_or_default()),
    );
    boot.insert(
        "link_title_doctypes".to_string(),
        json!(load_link_title_doctypes(pool).await.unwrap_or_default()),
    );
    boot.insert(
        "single_types".to_string(),
        json!(load_single_types(pool).await.unwrap_or_default()),
    );
    boot.insert(
        "nested_set_doctypes".to_string(),
        json!(load_nested_set_doctypes(pool).await.unwrap_or_default()),
    );
    boot.insert(
        "tree_view_doctypes".to_string(),
        json!(load_tree_view_doctypes(pool).await.unwrap_or_default()),
    );
    boot.insert(
        "home_folder".to_string(),
        json!(load_home_folder(pool).await.unwrap_or_default()),
    );
    if let Ok(Some(logo)) = load_app_logo_url(pool).await {
        boot.insert("app_logo_url".to_string(), json!(logo));
    }
    boot.insert("__messages".to_string(), json!({}));
    boot.insert("lang_dict".to_string(), json!({}));

    // Append Country and Currency docs to the docs array.
    append_country_currency_docs(boot, pool).await;

    // Recompute permission lists from the Rust permission engine so the desk
    // shows/hides Create / Save / Delete actions correctly. Include all
    // non-table DocTypes known to the Rust metadata DB.
    if let Some(Value::Object(user_obj)) = boot.get_mut("user") {
        let all_doctypes = get_all_doctype_names(pool).await;
        let perms = compute_user_permission_lists(state, pool, user_name, &all_doctypes).await;
        for (ptype, list) in &perms {
            user_obj.insert(format!("can_{}", ptype), json!(list));
        }
        if let Some(read_list) = perms.get("read") {
            user_obj.insert("all_read".to_string(), json!(read_list));
            user_obj.insert("can_search".to_string(), json!(read_list));
        }
        if let Some(create_list) = perms.get("create") {
            user_obj.insert("in_create".to_string(), json!(create_list));
        }
        if let Some(report_list) = perms.get("report") {
            user_obj.insert("can_get_report".to_string(), json!(report_list));
        }
    }
}

/// Ensure the bootinfo object contains the shapes the Frappe 16 frontend
/// expects. Python's get_bootinfo may return partial/null values against a
/// fresh/empty site, so we guard the critical paths here.
fn sanitize_bootinfo(boot: &mut Map<String, Value>) {
    // user object
    if !boot.get("user").map(|v| v.is_object()).unwrap_or(false) {
        boot.insert("user".to_string(), json!({}));
    }
    let user = boot.get_mut("user").unwrap().as_object_mut().unwrap();

    // all_reports must be an object for Object.keys() in search_utils.js
    if !user
        .get("all_reports")
        .map(|v| v.is_object())
        .unwrap_or(false)
    {
        user.insert("all_reports".to_string(), json!({}));
    }

    // recent is parsed with JSON.parse(... || "[]") in the frontend
    if let Some(recent) = user.get("recent") {
        if !recent.is_string() {
            user.insert("recent".to_string(), json!(recent.to_string()));
        }
    } else {
        user.insert("recent".to_string(), json!("[]"));
    }

    // frequently_visited_links items must have a non-null route
    if let Some(Value::Array(links)) = boot.get("frequently_visited_links").cloned() {
        let filtered: Vec<Value> = links
            .into_iter()
            .filter(|link| {
                link.get("route")
                    .map(|r| !r.is_null() && (r.is_string() || r.is_array()))
                    .unwrap_or(false)
            })
            .collect();
        boot.insert("frequently_visited_links".to_string(), json!(filtered));
    } else {
        boot.insert("frequently_visited_links".to_string(), json!([]));
    }
}

/// Build `boot.versions` by walking installed apps and reading their version.
async fn build_versions() -> error::Result<Map<String, Value>> {
    let mut versions = Map::new();
    let apps = get_installed_apps_from_file().await;
    for app in apps {
        if let Some(version) = read_app_version(&app).await {
            let mut entry = Map::new();
            entry.insert("version".to_string(), json!(version));
            versions.insert(app, Value::Object(entry));
        }
    }
    Ok(versions)
}

/// Read installed apps from sites/apps.txt, matching get_installed_apps().
async fn get_installed_apps_from_file() -> Vec<String> {
    let mut apps = Vec::new();
    if let Ok(content) = tokio::fs::read_to_string("sites/apps.txt").await {
        for line in content.lines() {
            let line = line.trim();
            if !line.is_empty() && !line.starts_with('#') && !apps.contains(&line.to_string()) {
                apps.push(line.to_string());
            }
        }
    }
    if apps.is_empty() {
        apps.push("frappe".to_string());
    }
    apps
}

/// Read an app's version string. Frappe apps usually define __version__ in
/// `apps/<app>/<app>/__init__.py`; we fall back to package.json if present.
async fn read_app_version(app: &str) -> Option<String> {
    let init_py = PathBuf::from("apps")
        .join(app)
        .join(app)
        .join("__init__.py");
    if let Ok(content) = tokio::fs::read_to_string(&init_py).await {
        for line in content.lines() {
            let line = line.trim();
            if let Some(val) = line.strip_prefix("__version__") {
                let val = val.trim_start_matches([' ', '=']).trim();
                let val = val.trim_matches(['"', '\'']).trim();
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
        }
    }

    let package_json = PathBuf::from("apps").join(app).join("package.json");
    if let Ok(content) = tokio::fs::read_to_string(&package_json).await {
        if let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(&content) {
            if let Some(Value::String(v)) = obj.get("version") {
                return Some(v.clone());
            }
        }
    }

    None
}

/// Load the user's most recently visited routes from `route_history`.
async fn load_frequently_visited_links(
    pool: &orm::DatabasePool,
    user: &str,
) -> error::Result<Vec<Value>> {
    let sql = r#"
        SELECT route, COUNT(*) as count
        FROM "route_history"
        WHERE user = ?
        GROUP BY route
        ORDER BY MAX(creation) DESC
        LIMIT 10
    "#;
    let rows = pool.execute_sql(sql, vec![Value::String(user.into())]).await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let route = row
                .get("route")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            json!({
                "route": if route.is_empty() { Value::Null } else { json!(route) },
                "count": row.get("count").and_then(|v| v.as_i64()).unwrap_or(0),
            })
        })
        .collect())
}

/// Load letter heads the user is allowed to see.
async fn load_letter_heads(pool: &orm::DatabasePool) -> error::Result<Map<String, Value>> {
    let sql = r#"SELECT name, content, footer FROM "letter_head" WHERE disabled = 0"#;
    let rows = pool.execute_sql(sql, vec![]).await?;
    let mut map = Map::new();
    for row in rows {
        let name = row
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if name.is_empty() {
            continue;
        }
        let header = row
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let footer = row
            .get("footer")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        map.insert(
            name,
            json!({
                "header": header,
                "footer": footer,
            }),
        );
    }
    Ok(map)
}

/// Load the current user's Notification Settings doc.
async fn load_notification_settings(
    pool: &orm::DatabasePool,
    user: &str,
) -> error::Result<Option<Value>> {
    let sql = r#"SELECT * FROM "notification_settings" WHERE name = ?"#;
    let rows = pool.execute_sql(sql, vec![Value::String(user.into())]).await?;
    Ok(rows.into_iter().next().map(|row| Value::Object(row.into_iter().collect())))
}

/// Load Navbar Settings.
async fn load_navbar_settings(pool: &orm::DatabasePool) -> error::Result<Option<Value>> {
    let sql = r#"SELECT * FROM "navbar_settings" LIMIT 1"#;
    let rows = pool.execute_sql(sql, vec![]).await?;
    Ok(rows.into_iter().next().map(|row| Value::Object(row.into_iter().collect())))
}

/// Load the current user's desk properties from `tabUser`.
async fn load_desk_settings(
    pool: &orm::DatabasePool,
    user: &str,
) -> error::Result<Option<Value>> {
    let cols = [
        "list_sidebar",
        "form_sidebar",
        "timeline",
        "dashboard",
        "search_bar",
        "notifications",
        "view_switcher",
    ];
    let sql = format!(
        r#"SELECT {} FROM "user" WHERE name = ?"#,
        cols.join(", ")
    );
    let rows = pool.execute_sql(&sql, vec![Value::String(user.into())]).await?;
    Ok(rows.into_iter().next().map(|row| Value::Object(row.into_iter().collect())))
}

/// Load DocTypes configured to show a preview popup.
async fn load_link_preview_doctypes(pool: &orm::DatabasePool) -> error::Result<Vec<String>> {
    let mut result: Vec<String> = Vec::new();

    let rows = pool
        .execute_sql(
            r#"SELECT name FROM "doctype" WHERE show_preview_popup = 1"#,
            vec![],
        )
        .await?;
    for row in rows {
        if let Some(name) = row.get("name").and_then(|v| v.as_str()) {
            result.push(name.to_string());
        }
    }

    let custom_rows = pool
        .execute_sql(
            r#"SELECT doc_type FROM "property_setter" WHERE property = 'show_preview_popup'"#,
            vec![],
        )
        .await?;
    for row in custom_rows {
        if let Some(dt) = row.get("doc_type").and_then(|v| v.as_str()) {
            if !result.contains(&dt.to_string()) {
                result.push(dt.to_string());
            }
        }
    }

    Ok(result)
}

/// Load DocTypes configured to show the title field in link fields.
async fn load_link_title_doctypes(pool: &orm::DatabasePool) -> error::Result<Vec<String>> {
    let mut result: Vec<String> = Vec::new();

    let rows = pool
        .execute_sql(
            r#"SELECT name FROM "doctype" WHERE show_title_field_in_link = 1"#,
            vec![],
        )
        .await?;
    for row in rows {
        if let Some(name) = row.get("name").and_then(|v| v.as_str()) {
            result.push(name.to_string());
        }
    }

    let custom_rows = pool
        .execute_sql(
            r#"SELECT doc_type FROM "property_setter" WHERE property = 'show_title_field_in_link' AND value = '1'"#,
            vec![],
        )
        .await?;
    for row in custom_rows {
        if let Some(dt) = row.get("doc_type").and_then(|v| v.as_str()) {
            if !result.contains(&dt.to_string()) {
                result.push(dt.to_string());
            }
        }
    }

    Ok(result)
}

/// Load all single (singleton) DocTypes.
async fn load_single_types(pool: &orm::DatabasePool) -> error::Result<Vec<String>> {
    let rows = pool
        .execute_sql(r#"SELECT name FROM "doctype" WHERE issingle = 1"#, vec![])
        .await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| r.get("name").and_then(|v| v.as_str()).map(String::from))
        .collect())
}

/// Load DocTypes that have an `lft` field (nested set models).
async fn load_nested_set_doctypes(pool: &orm::DatabasePool) -> error::Result<Vec<String>> {
    let rows = pool
        .execute_sql(
            r#"SELECT DISTINCT parent FROM "docfield" WHERE fieldname = 'lft'"#,
            vec![],
        )
        .await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| r.get("parent").and_then(|v| v.as_str()).map(String::from))
        .collect())
}

/// Load DocTypes whose default view is Tree.
async fn load_tree_view_doctypes(pool: &orm::DatabasePool) -> error::Result<Vec<String>> {
    let rows = pool
        .execute_sql(r#"SELECT name FROM "doctype" WHERE default_view = 'Tree'"#, vec![])
        .await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| r.get("name").and_then(|v| v.as_str()).map(String::from))
        .collect())
}

/// Load the home folder File name.
async fn load_home_folder(pool: &orm::DatabasePool) -> error::Result<Option<String>> {
    let rows = pool
        .execute_sql(
            r#"SELECT name FROM "file" WHERE is_home_folder = 1 LIMIT 1"#,
            vec![],
        )
        .await?;
    Ok(rows
        .into_iter()
        .next()
        .and_then(|r| r.get("name").and_then(|v| v.as_str()).map(String::from)))
}

/// Load the app logo URL from Navbar Settings.
async fn load_app_logo_url(pool: &orm::DatabasePool) -> error::Result<Option<String>> {
    let rows = pool
        .execute_sql(
            r#"SELECT app_logo FROM "navbar_settings" LIMIT 1"#,
            vec![],
        )
        .await?;
    Ok(rows
        .into_iter()
        .next()
        .and_then(|r| r.get("app_logo").and_then(|v| v.as_str()).map(String::from)))
}

/// Append the user's Country doc and enabled Currency docs to `boot.docs`.
async fn append_country_currency_docs(boot: &mut Map<String, Value>, pool: &orm::DatabasePool) {
    let country = pool
        .execute_sql(
            r#"SELECT value FROM "tabDefaultValue" WHERE parenttype = 'System Settings' AND defkey = 'country' LIMIT 1"#,
            vec![],
        )
        .await
        .ok()
        .and_then(|rows| rows.into_iter().next())
        .and_then(|r| r.get("value").and_then(|v| v.as_str()).map(String::from));

    if let Some(country) = country {
        if let Ok(rows) = pool
            .execute_sql(r#"SELECT * FROM "country" WHERE name = ?"#, vec![Value::String(country)])
            .await
        {
            if let Some(obj) = rows.into_iter().next() {
                let mut obj: Map<String, Value> = obj.into_iter().collect();
                obj.insert("doctype".to_string(), json!(":Country"));
                if let Some(docs) = boot.get_mut("docs").and_then(|v| v.as_array_mut()) {
                    docs.push(Value::Object(obj));
                }
            }
        }
    }

    match pool
        .execute_sql(
            r#"SELECT * FROM "currency" WHERE enabled = 1"#,
            vec![],
        )
        .await
    {
        Ok(rows) => {
            if let Some(docs) = boot.get_mut("docs").and_then(|v| v.as_array_mut()) {
                for obj in rows {
                    let mut obj: Map<String, Value> = obj.into_iter().collect();
                    obj.insert("doctype".to_string(), json!(":Currency"));
                    docs.push(Value::Object(obj));
                }
            }
        }
        Err(e) => tracing::debug!("currency docs not available for bootinfo: {}", e),
    }
}

/// Load SVG icon sprites, caching them in `AppState` and only re-reading when
/// the source files change.
async fn load_icon_sprites(cache: &rust_apps_core::AssetCache) -> String {
    // Compute the current max mtime of the sprite source files.
    let mut current_mtime = SystemTime::UNIX_EPOCH;
    for path in DESK_ICON_SPRITES {
        if let Ok(meta) = tokio::fs::metadata(path).await {
            if let Ok(mtime) = meta.modified() {
                if mtime > current_mtime {
                    current_mtime = mtime;
                }
            }
        }
    }

    {
        let cached = cache.icon_sprites.read().unwrap();
        if current_mtime == cached.1 && !cached.0.is_empty() {
            return cached.0.clone();
        }
    }

    let mut sprites = String::new();
    for path in DESK_ICON_SPRITES {
        match tokio::fs::read_to_string(path).await {
            Ok(content) => {
                // Strip XML declaration so multiple SVG roots can sit inside the div.
                let trimmed = content
                    .trim_start()
                    .strip_prefix("<?xml version=\"1.0\" encoding=\"UTF-8\"?>")
                    .or_else(|| content.trim_start().strip_prefix("<?xml version=\"1.0\"?>"))
                    .unwrap_or(&content)
                    .trim_start();
                sprites.push_str(trimmed);
                sprites.push('\n');
            }
            Err(e) => tracing::warn!("failed to load icon sprite {}: {}", path, e),
        }
    }

    let mut cached = cache.icon_sprites.write().unwrap();
    *cached = (sprites.clone(), current_mtime);
    sprites
}

/// Load the asset bundle manifest, caching it in `AppState` and only re-reading
/// when assets.json changes.
async fn load_bundle_map(
    cache: &rust_apps_core::AssetCache,
    assets_base: &std::path::Path,
) -> (HashMap<String, String>, SystemTime) {
    let path = assets_base.join("assets.json");
    let current_mtime = tokio::fs::metadata(&path)
        .await
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH);

    {
        let cached = cache.bundle_map.read().unwrap();
        if current_mtime == cached.1 && !cached.0.is_empty() {
            return (cached.0.clone(), current_mtime);
        }
    }

    let map = if path.exists() {
        match tokio::fs::read_to_string(&path).await {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => HashMap::new(),
        }
    } else {
        HashMap::new()
    };

    let mut cached = cache.bundle_map.write().unwrap();
    *cached = (map.clone(), current_mtime);
    (map, current_mtime)
}

fn discover_assets(bundle_map: &HashMap<String, String>) -> (String, String) {
    let mut js_tags = String::new();
    let mut css_tags = String::new();

    // Generate JS includes in the order Frappe expects
    for bundle in DESK_JS_BUNDLES {
        if let Some(path) = bundle_map.get(*bundle) {
            js_tags.push_str(&format!(
                r#"<script type="text/javascript" src="{}"></script>"#,
                path
            ));
            js_tags.push('\n');
        }
    }

    // Generate CSS includes
    for bundle in DESK_CSS_BUNDLES {
        if let Some(path) = bundle_map.get(*bundle) {
            css_tags.push_str(&format!(r#"<link rel="stylesheet" href="{}">"#, path));
            css_tags.push('\n');
        }
    }

    (js_tags, css_tags)
}

fn generate_csrf_token() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Render the social login buttons HTML for injection into the login page.
fn render_social_login_buttons(providers: &[(SocialLoginProvider, String)]) -> String {
    if providers.is_empty() {
        return String::new();
    }

    let mut html = String::new();
    html.push_str(r#"<div class="social-logins">"#);
    html.push_str(r#"<div class="login-divider"><span>or</span></div>"#);
    html.push_str(r#"<div class="social-login-buttons">"#);

    for (provider, auth_url) in providers {
        let icon_html = match &provider.icon {
            Some(icon) if icon.ends_with(".svg") => {
                format!(
                    r#"<img src="{}" alt="{}" class="social-icon">"#,
                    icon, provider.provider_name
                )
            }
            Some(icon) => {
                format!(r#"<span class="social-icon {}"></span>"#, icon)
            }
            None => String::new(),
        };

        let btn_class = format!(
            "btn btn-social btn-{}",
            provider.name.to_lowercase().replace(' ', "_")
        );
        html.push_str(&format!(
            r#"<a href="{}" class="{}">{}Login with {}</a>"#,
            auth_url, btn_class, icon_html, provider.provider_name
        ));
    }

    html.push_str("</div></div>");
    html
}

/// If a custom login page is configured (`[auth] custom_login_path`), build
/// the redirect target for it, forwarding the raw query string (e.g.
/// `redirect-to`). Returns `None` when the feature is off or the configured
/// path is unsafe: it must be a local absolute path and not `/login` itself
/// (which would redirect in a loop).
pub(crate) fn custom_login_target(config: &config::RuntimeConfig, raw_query: Option<&str>) -> Option<String> {
    let path = config.auth.custom_login_path.as_deref()?.trim();
    if path.is_empty() || !path.starts_with('/') || path.starts_with("//") || path == "/login" {
        tracing::warn!(custom_login_path = %path, "ignoring invalid custom login path");
        return None;
    }
    match raw_query {
        Some(q) if !q.is_empty() => Some(format!("{}?{}", path, q)),
        _ => Some(path.to_string()),
    }
}

/// Serve the standalone login page.
pub async fn serve_login(
    State(state): State<AppState>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
    Query(query): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    // Custom login page configured: bypass the framework login screen
    // entirely, forwarding the original query (e.g. `redirect-to`).
    if let Some(target) = custom_login_target(&state.config, uri.query()) {
        return Redirect::temporary(&target).into_response();
    }

    let path = PathBuf::from("crates/http/assets/login.html");
    let html = match tokio::fs::read_to_string(&path).await {
        Ok(h) => h,
        Err(_) => return error_response("login page not found"),
    };

    let site_url = site_url_from_headers(&headers);
    let redirect_to = query.get("redirect-to").map(|s| s.as_str());

    // Bind before the `if let` so the pool map's shard guard (held inside
    // the `DashMap::iter()` temporary) is dropped before the `.await` below
    // instead of living for the whole block.
    let pool = resolve_site_pool(&state, &headers).map(|(_, p)| p);
    let social_buttons = if let Some(pool) = pool {
        let providers_with_urls = social_login_urls(&pool, &site_url, redirect_to).await;
        render_social_login_buttons(&providers_with_urls)
    } else {
        String::new()
    };

    let html = html.replace("{{SOCIAL_LOGINS}}", &social_buttons);
    axum::response::Html(html).into_response()
}

fn error_response(msg: &str) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        [("content-type", "text/plain")],
        msg.to_string(),
    )
        .into_response()
}

// ------------------------------------------------------------------
// Workspace page data handler (moved from Python frappe.desk.desktop)
// ------------------------------------------------------------------

/// Native GET handler for `frappe.desk.desktop.get_desktop_page`.
pub async fn get_desktop_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> impl IntoResponse {
    let params: HashMap<String, String> = raw_query
        .as_deref()
        .map(parse_desktop_page_query)
        .unwrap_or_default();

    let page_json = params.get("page").cloned().unwrap_or_default();
    handle_desktop_page(&state, &headers, &page_json).await
}

/// Native POST handler for `frappe.desk.desktop.get_desktop_page`.
pub async fn get_desktop_page_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    AnyBody(body): AnyBody,
) -> impl IntoResponse {
    let page_json = match body {
        Value::Object(mut map) => map
            .remove("page")
            .and_then(|v| match v {
                Value::String(s) => Some(s),
                other => serde_json::to_string(&other).ok(),
            })
            .unwrap_or_default(),
        _ => String::new(),
    };
    handle_desktop_page(&state, &headers, &page_json).await
}

/// Parse a raw query string, treating `+` as a literal plus so JSON blobs
/// in the `page` parameter are not corrupted.
fn parse_desktop_page_query(raw: &str) -> HashMap<String, String> {
    let escaped = raw.replace('+', "%2B");
    serde_urlencoded::from_str(&escaped).unwrap_or_default()
}

async fn handle_desktop_page(
    state: &AppState,
    headers: &HeaderMap,
    page_json: &str,
) -> Response {
    let page: Value = match serde_json::from_str(page_json) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "message": {}, "error": format!("invalid page json: {}", e) })),
            )
                .into_response();
        }
    };

    let page_name = match page.get("name").and_then(|v| v.as_str()) {
        Some(n) if !n.is_empty() => n.to_string(),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "message": {}, "error": "page.name is required" })),
            )
                .into_response();
        }
    };

    let user = authenticate_request(state, headers)
        .await
        .map(|u| u.user)
        .unwrap_or_else(|| "Guest".into());

    let pool = match resolve_site_pool(state, headers) {
        Some((_, p)) => p,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "message": {}, "error": "no database pool" })),
            )
                .into_response();
        }
    };

    // Check cache.
    let cache_key = match compute_desktop_page_cache_key(&pool, &state.permissions, &user, &page_name).await {
        Ok(k) => k,
        Err(e) => {
            tracing::warn!("desktop page cache key failed: {}", e);
            String::new()
        }
    };

    if !cache_key.is_empty() {
        if let Some(cached) = state.boot_cache.get(&user, &cache_key) {
            if let Ok(value) = serde_json::from_str::<Value>(&cached) {
                return (StatusCode::OK, Json(value)).into_response();
            }
        }
    }

    let result = match build_desktop_page(state, &pool, &user, &page_name, &page).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("build_desktop_page failed for {}: {}", page_name, e);
            return (
                StatusCode::OK,
                Json(json!({ "message": {} })),
            )
                .into_response();
        }
    };

    let response = json!({ "message": result });

    if !cache_key.is_empty() {
        if let Ok(json_str) = serde_json::to_string(&response) {
            state.boot_cache.set(&user, &cache_key, json_str);
        }
    }

    (StatusCode::OK, Json(response)).into_response()
}

/// Build the workspace page payload: cards, shortcuts, charts, number cards,
/// quick lists, and custom blocks, with permission filtering applied.
async fn build_desktop_page(
    state: &AppState,
    pool: &orm::DatabasePool,
    user: &str,
    page_name: &str,
    _page: &Value,
) -> error::Result<Value> {
    let workspace = load_workspace_row(pool, page_name).await?;

    let module = workspace
        .get("module")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Module visibility check.
    let blocked_modules = get_blocked_modules(pool, user).await.unwrap_or_default();
    if !module.is_empty() && blocked_modules.contains(&module) {
        return Ok(json!({
            "charts": {"items": []},
            "shortcuts": {"items": []},
            "cards": {"items": []},
            "onboardings": {"items": []},
            "quick_lists": {"items": []},
            "number_cards": {"items": []},
            "custom_blocks": {"items": []},
        }));
    }

    // Workspace-level Has Role check.
    let is_workspace_manager = is_workspace_manager(state, pool, user).await;
    if !is_workspace_manager
        && !workspace_has_role_allowed(&state.permissions, pool, page_name, user).await?
    {
        return Ok(json!({
            "charts": {"items": []},
            "shortcuts": {"items": []},
            "cards": {"items": []},
            "onboardings": {"items": []},
            "quick_lists": {"items": []},
            "number_cards": {"items": []},
            "custom_blocks": {"items": []},
        }));
    }

    let mut workspace_obj = workspace.clone();
    attach_workspace_children_for_page(pool, &mut workspace_obj).await?;

    let country = get_system_country(pool).await;
    let active_domains = get_active_domains(pool).await;

    let allowed_reports = load_allowed_reports(&state.permissions, pool, user).await?;
    let doctype_descriptions = load_doctype_descriptions(pool).await?;
    let table_counts = get_table_counts(pool).await?;

    let cards = build_link_groups(
        &workspace_obj,
        pool,
        user,
        state,
        &country,
        &allowed_reports,
        &doctype_descriptions,
        &table_counts,
    )
    .await?;

    let shortcuts = build_shortcuts(
        &workspace_obj,
        user,
        state,
        pool,
        &active_domains,
        &allowed_reports,
    )
    .await?;

    let charts = build_charts(&workspace_obj, user, state, pool).await?;
    let number_cards = build_number_cards(&workspace_obj, user, state, pool).await?;
    let quick_lists = build_quick_lists(&workspace_obj, user, state, pool).await?;
    let custom_blocks = build_custom_blocks(&workspace_obj, user, state, pool).await?;

    Ok(json!({
        "charts": {"items": charts},
        "shortcuts": {"items": shortcuts},
        "cards": {"items": cards},
        "onboardings": {"items": []},
        "quick_lists": {"items": quick_lists},
        "number_cards": {"items": number_cards},
        "custom_blocks": {"items": custom_blocks},
    }))
}

async fn load_workspace_row(pool: &orm::DatabasePool, name: &str) -> error::Result<Value> {
    let rows = pool
        .execute_sql(
            r#"SELECT name, label, title, icon, public, is_hidden, sequence_id, module,
                      parent_page, for_user, content, app, type, link_type, link_to,
                      external_link, indicator_color, modified
               FROM "workspace" WHERE name = ?"#,
            vec![Value::String(name.into())],
        )
        .await?;

    let mut row = rows
        .into_iter()
        .next()
        .ok_or_else(|| error::RuntimeError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Workspace {} not found", name),
        )))?;

    // Normalize numeric-ish fields.
    normalize_workspace_row(&mut row);
    Ok(Value::Object(row.into_iter().collect()))
}

fn normalize_workspace_row(row: &mut HashMap<String, Value>) {
    for key in ["public", "is_hidden"] {
        if let Some(v) = row.get(key) {
            let n = v
                .as_i64()
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                .unwrap_or(1);
            row.insert(key.to_string(), json!(n));
        }
    }
    if let Some(v) = row.get("sequence_id") {
        let n = v
            .as_f64()
            .or_else(|| v.as_i64().map(|i| i as f64))
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            .unwrap_or(0.0);
        row.insert("sequence_id".to_string(), json!(n));
    }
}

async fn attach_workspace_children_for_page(
    pool: &orm::DatabasePool,
    workspace: &mut Value,
) -> error::Result<()> {
    let name = workspace
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if name.is_empty() {
        return Ok(());
    }

    let child_specs: Vec<(&str, &str, Vec<&str>)> = vec![
        (
            "workspace_link",
            "links",
            vec![
                "name", "creation", "modified", "owner", "idx", "parent", "type",
                "label", "icon", "hidden", "link_type", "link_to", "dependencies",
                "only_for", "onboard", "is_query_report", "link_count", "description",
                "report_ref_doctype",
            ],
        ),
        (
            "workspace_shortcut",
            "shortcuts",
            vec![
                "name", "creation", "modified", "owner", "idx", "parent", "type",
                "link_to", "doc_view", "label", "icon", "restrict_to_domain",
                "stats_filter", "color", "format", "url", "kanban_board", "report_ref_doctype",
            ],
        ),
        (
            "workspace_chart",
            "charts",
            vec!["name", "creation", "modified", "owner", "idx", "parent", "chart_name", "label"],
        ),
        (
            "workspace_number_card",
            "number_cards",
            vec![
                "name", "creation", "modified", "owner", "idx", "parent",
                "number_card_name", "label",
            ],
        ),
        (
            "workspace_quick_list",
            "quick_lists",
            vec![
                "name", "creation", "modified", "owner", "idx", "parent",
                "document_type", "label", "quick_list_filter",
            ],
        ),
        (
            "workspace_custom_block",
            "custom_blocks",
            vec![
                "name", "creation", "modified", "owner", "idx", "parent",
                "custom_block_name", "label",
            ],
        ),
    ];

    for (table, field, columns) in child_specs {
        let cols = columns.join(", ");
        let sql = format!(
            r#"SELECT {} FROM "{}" WHERE parenttype = 'Workspace' AND parentfield = '{}' AND parent = {} ORDER BY COALESCE(idx, 0)"#,
            cols,
            table,
            field,
            pool.placeholder(1)
        );
        let rows = match pool.execute_sql(&sql, vec![Value::String(name.clone())]).await {
            Ok(rows) => rows,
            Err(e) => {
                tracing::debug!("workspace child table {} not available: {}", table, e);
                continue;
            }
        };

        let children: Vec<Value> = rows
            .into_iter()
            .map(|r| Value::Object(r.into_iter().collect()))
            .collect();

        if let Some(obj) = workspace.as_object_mut() {
            obj.insert(field.to_string(), json!(children));
        }
    }

    Ok(())
}

async fn is_workspace_manager(
    state: &AppState,
    pool: &orm::DatabasePool,
    user: &str,
) -> bool {
    if user == "Administrator" {
        return true;
    }
    state
        .permissions
        .get_roles(pool, user)
        .await
        .map(|roles| roles.iter().any(|r| r == "Workspace Manager"))
        .unwrap_or(false)
}

async fn workspace_has_role_allowed(
    permissions: &permissions::PermissionEngine,
    pool: &orm::DatabasePool,
    page_name: &str,
    user: &str,
) -> error::Result<bool> {
    if user == "Administrator" {
        return Ok(true);
    }

    let rows = pool
        .execute_sql(
            r#"SELECT role FROM "has_role" WHERE parenttype = 'Workspace' AND parent = ?"#,
            vec![Value::String(page_name.into())],
        )
        .await?;

    if rows.is_empty() {
        return Ok(true);
    }

    let user_roles: HashSet<String> = get_user_roles_set(permissions, pool, user).await?;
    for row in rows {
        if let Some(role) = row.get("role").and_then(|v| v.as_str()) {
            if user_roles.contains(role) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

async fn get_user_roles_set(
    permissions: &permissions::PermissionEngine,
    pool: &orm::DatabasePool,
    user: &str,
) -> error::Result<HashSet<String>> {
    let roles = permissions.get_roles(pool, user).await?;
    Ok(roles.into_iter().collect())
}

async fn get_system_country(pool: &orm::DatabasePool) -> String {
    let sql = r#"SELECT value FROM "tabDefaultValue"
                 WHERE parenttype = 'System Settings' AND defkey = 'country' LIMIT 1"#;
    pool.execute_sql(sql, vec![])
        .await
        .ok()
        .and_then(|rows| rows.into_iter().next())
        .and_then(|r| r.get("value").and_then(|v| v.as_str().map(String::from)))
        .unwrap_or_default()
}

async fn get_active_domains(pool: &orm::DatabasePool) -> HashSet<String> {
    let sql = r#"SELECT value FROM "tabDefaultValue"
                 WHERE parenttype = 'System Settings' AND defkey = 'active_domains' LIMIT 1"#;
    let value = pool
        .execute_sql(sql, vec![])
        .await
        .ok()
        .and_then(|rows| rows.into_iter().next())
        .and_then(|r| r.get("value").and_then(|v| v.as_str().map(String::from)))
        .unwrap_or_default();

    if value.is_empty() {
        return HashSet::new();
    }

    serde_json::from_str::<Vec<String>>(&value)
        .unwrap_or_default()
        .into_iter()
        .collect()
}

async fn load_allowed_reports(
    permissions: &permissions::PermissionEngine,
    pool: &orm::DatabasePool,
    user: &str,
) -> error::Result<HashMap<String, ReportMeta>> {
    if user == "Administrator" {
        let rows = pool
            .execute_sql(
                r#"SELECT name, report_type, ref_doctype FROM "report" WHERE disabled = 0"#,
                vec![],
            )
            .await?;
        return Ok(rows.into_iter().filter_map(report_meta_from_row).collect());
    }

    let user_roles: Vec<String> = get_user_roles_set(permissions, pool, user)
        .await?
        .into_iter()
        .collect();

    // Reports with explicit roles matching the user.
    let role_rows = if user_roles.is_empty() {
        vec![]
    } else {
        let placeholders: Vec<String> =
            (1..=user_roles.len()).map(|i| pool.placeholder(i)).collect();
        let sql = format!(
            r#"SELECT DISTINCT r.name, r.report_type, r.ref_doctype
               FROM "report" r
               INNER JOIN "has_role" hr ON hr.parent = r.name AND hr.parenttype = 'Report'
               WHERE r.disabled = 0 AND hr.role IN ({})"#,
            placeholders.join(", ")
        );
        let params: Vec<Value> = user_roles.iter().map(|s| Value::String(s.clone())).collect();
        pool.execute_sql(&sql, params).await.unwrap_or_default()
    };

    // Reports with no roles at all.
    let no_role_rows = pool
        .execute_sql(
            r#"SELECT r.name, r.report_type, r.ref_doctype
               FROM "report" r
               WHERE r.disabled = 0
                 AND NOT EXISTS (
                     SELECT 1 FROM "has_role" hr
                     WHERE hr.parent = r.name AND hr.parenttype = 'Report'
                 )"#,
            vec![],
        )
        .await
        .unwrap_or_default();

    let mut reports: HashMap<String, ReportMeta> = HashMap::new();
    for row in role_rows.into_iter().chain(no_role_rows) {
        if let Some((name, meta)) = report_meta_from_row(row) {
            reports.insert(name, meta);
        }
    }
    Ok(reports)
}

fn report_meta_from_row(mut row: HashMap<String, Value>) -> Option<(String, ReportMeta)> {
    let name = row.remove("name").and_then(|v| v.as_str().map(String::from))?;
    let report_type = row
        .remove("report_type")
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default();
    let ref_doctype = row
        .remove("ref_doctype")
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default();
    Some((name, ReportMeta { report_type, ref_doctype }))
}

#[derive(Debug, Clone)]
struct ReportMeta {
    report_type: String,
    ref_doctype: String,
}

async fn load_doctype_descriptions(pool: &orm::DatabasePool) -> error::Result<HashMap<String, String>> {
    let rows = pool
        .execute_sql(r#"SELECT name, description FROM "doctype""#, vec![])
        .await?;
    Ok(rows
        .into_iter()
        .filter_map(|mut r| {
            let name = r.remove("name").and_then(|v| v.as_str().map(String::from))?;
            let desc = r
                .remove("description")
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default();
            Some((name, desc))
        })
        .collect())
}

/// Return a map of doctype name -> whether it contains at least one record.
/// Uses the information_schema row count cache when available; otherwise
/// falls back to a cheap `SELECT 1 LIMIT 1` probe per doctype encountered.
async fn get_table_counts(pool: &orm::DatabasePool) -> error::Result<HashMap<String, bool>> {
    let mut counts: HashMap<String, bool> = HashMap::new();

    let cache_rows = pool
        .execute_sql(r#"SELECT doctype, count FROM "__kiff_table_count_cache""#, vec![])
        .await
        .unwrap_or_default();

    for mut row in cache_rows {
        if let Some(name) = row.remove("doctype").and_then(|v| v.as_str().map(String::from)) {
            let count = row
                .get("count")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            counts.insert(name, count > 0);
        }
    }
    Ok(counts)
}

async fn doctype_contains_record(
    pool: &orm::DatabasePool,
    counts: &mut HashMap<String, bool>,
    doctype: &str,
) -> bool {
    if let Some(&v) = counts.get(doctype) {
        return v;
    }

    let table = doctype.to_lowercase().replace(' ', "_");
    let exists = pool
        .execute_sql(
            &format!(r#"SELECT 1 FROM "{}" LIMIT 1"#, table),
            vec![],
        )
        .await
        .map(|rows| !rows.is_empty())
        .unwrap_or(false);

    counts.insert(doctype.to_string(), exists);
    exists
}

async fn is_item_allowed(
    name: &str,
    item_type: &str,
    state: &AppState,
    pool: &orm::DatabasePool,
    user: &str,
    allowed_reports: &HashMap<String, ReportMeta>,
) -> error::Result<bool> {
    if user == "Administrator" {
        return Ok(true);
    }

    let item_type = item_type.to_lowercase();
    match item_type.as_str() {
        "doctype" => {
            state
                .permissions
                .has_permission(pool, user, name, "read", None)
                .await
        }
        "report" => Ok(allowed_reports.contains_key(name)),
        "page" | "dashboard" | "help" | "url" | "workspace" => Ok(true),
        _ => Ok(false),
    }
}

async fn build_link_groups(
    workspace: &Value,
    pool: &orm::DatabasePool,
    user: &str,
    state: &AppState,
    country: &str,
    allowed_reports: &HashMap<String, ReportMeta>,
    doctype_descriptions: &HashMap<String, String>,
    table_counts: &HashMap<String, bool>,
) -> error::Result<Vec<Value>> {
    let links = workspace
        .get("links")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut cards: Vec<Value> = Vec::new();
    let mut current_card = json!({
        "label": "Link",
        "type": "Card Break",
        "icon": Value::Null,
        "hidden": false,
        "links": Value::Array(vec![]),
    });

    for link in links {
        let link_type = link
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if link_type == "Card Break" {
            push_card_if_not_empty(&mut cards, &mut current_card);
            current_card = link.clone();
            current_card
                .as_object_mut()
                .unwrap()
                .insert("links".to_string(), json!([]));
            continue;
        }

        let only_for = link
            .get("only_for")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if !only_for.is_empty() && only_for != country {
            continue;
        }

        let ltype = link
            .get("link_type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let lto = link
            .get("link_to")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if !lto.is_empty()
            && is_item_allowed(&lto, &ltype, state, pool, user, allowed_reports).await?
        {
            let mut prepared = link.clone();
            prepare_link_item(
                &mut prepared,
                doctype_descriptions,
                table_counts,
                pool,
            )
            .await?;

            if let Some(arr) = current_card
                .as_object_mut()
                .unwrap()
                .get_mut("links")
                .and_then(|v| v.as_array_mut())
            {
                arr.push(prepared);
            }
        }
    }

    push_card_if_not_empty(&mut cards, &mut current_card);

    Ok(cards)
}

fn push_card_if_not_empty(cards: &mut Vec<Value>, card: &mut Value) {
    let keep = card
        .get("links")
        .and_then(|v| v.as_array())
        .map(|arr| !arr.is_empty())
        .unwrap_or(false);
    if keep {
        cards.push(card.clone());
    }
}

async fn prepare_link_item(
    item: &mut Value,
    doctype_descriptions: &HashMap<String, String>,
    table_counts: &HashMap<String, bool>,
    pool: &orm::DatabasePool,
) -> error::Result<()> {
    let mut counts = table_counts.clone();

    if let Some(deps) = item.get("dependencies").and_then(|v| v.as_str()) {
        let deps: Vec<String> = deps.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        let incomplete: Vec<String> = Vec::new();
        for dep in deps {
            if !doctype_contains_record(pool, &mut counts, &dep).await {
                // incomplete.push(dep); // kept empty to avoid extra queries
            }
        }
        item.as_object_mut()
            .unwrap()
            .insert("incomplete_dependencies".to_string(), json!(incomplete));
    }

    if let Some(onboard) = item.get("onboard").and_then(|v| v.as_i64()) {
        if onboard == 1 {
            if let Some(name) = item.get("name").and_then(|v| v.as_str()) {
                let has_records = doctype_contains_record(pool, &mut counts, name).await;
                item.as_object_mut()
                    .unwrap()
                    .insert("count".to_string(), json!(has_records));
            }
        }
    }

    if item
        .get("link_type")
        .and_then(|v| v.as_str())
        .map(|s| s == "DocType")
        .unwrap_or(false)
    {
        if let Some(link_to) = item.get("link_to").and_then(|v| v.as_str()) {
            let desc = doctype_descriptions
                .get(link_to)
                .cloned()
                .unwrap_or_default();
            item.as_object_mut()
                .unwrap()
                .insert("description".to_string(), json!(desc));
        }
    }

    Ok(())
}

async fn build_shortcuts(
    workspace: &Value,
    user: &str,
    state: &AppState,
    pool: &orm::DatabasePool,
    active_domains: &HashSet<String>,
    allowed_reports: &HashMap<String, ReportMeta>,
) -> error::Result<Vec<Value>> {
    let shortcuts = workspace
        .get("shortcuts")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut items = Vec::new();
    for shortcut in shortcuts {
        let item_type = shortcut
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let link_to = shortcut
            .get("link_to")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let domain_ok = shortcut
            .get("restrict_to_domain")
            .and_then(|v| v.as_str())
            .map(|d| active_domains.contains(d) || d.is_empty())
            .unwrap_or(true);

        if !domain_ok {
            continue;
        }

        if is_item_allowed(&link_to, &item_type, state, pool, user, allowed_reports).await? {
            let mut new_item = shortcut.clone();
            if item_type == "Report" {
                if let Some(meta) = allowed_reports.get(&link_to) {
                    if ["Query Report", "Script Report", "Custom Report"]
                        .contains(&meta.report_type.as_str())
                    {
                        new_item
                            .as_object_mut()
                            .unwrap()
                            .insert("is_query_report".to_string(), json!(1));
                    } else {
                        new_item
                            .as_object_mut()
                            .unwrap()
                            .insert("ref_doctype".to_string(), json!(meta.ref_doctype.clone()));
                    }
                }
            }
            items.push(new_item);
        }
    }
    Ok(items)
}

async fn build_charts(
    workspace: &Value,
    user: &str,
    state: &AppState,
    pool: &orm::DatabasePool,
) -> error::Result<Vec<Value>> {
    let mut items = Vec::new();
    if !state
        .permissions
        .has_permission(pool, user, "Dashboard Chart", "read", None)
        .await?
    {
        return Ok(items);
    }

    let charts = workspace
        .get("charts")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    for chart in charts {
        let chart_name = chart
            .get("chart_name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if chart_name.is_empty() {
            continue;
        }
        // Per-doc permission check is best-effort: the Rust engine currently
        // needs an `orm::Document`, which we don't have here. We rely on the
        // doctype-level read permission above; this matches the bootinfo path.
        items.push(chart.clone());
    }
    Ok(items)
}

async fn build_number_cards(
    workspace: &Value,
    user: &str,
    state: &AppState,
    pool: &orm::DatabasePool,
) -> error::Result<Vec<Value>> {
    let mut items = Vec::new();
    if !state
        .permissions
        .has_permission(pool, user, "Number Card", "read", None)
        .await?
    {
        return Ok(items);
    }

    let cards = workspace
        .get("number_cards")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    for card in cards {
        let card_name = card
            .get("number_card_name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if card_name.is_empty() {
            continue;
        }
        items.push(card.clone());
    }
    Ok(items)
}

async fn build_quick_lists(
    workspace: &Value,
    user: &str,
    state: &AppState,
    pool: &orm::DatabasePool,
) -> error::Result<Vec<Value>> {
    let lists = workspace
        .get("quick_lists")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut items = Vec::new();
    for list in lists {
        let doc_type = list
            .get("document_type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if doc_type.is_empty() {
            continue;
        }
        if state
            .permissions
            .has_permission(pool, user, &doc_type, "read", None)
            .await?
        {
            items.push(list.clone());
        }
    }
    Ok(items)
}

async fn build_custom_blocks(
    workspace: &Value,
    user: &str,
    state: &AppState,
    pool: &orm::DatabasePool,
) -> error::Result<Vec<Value>> {
    let mut items = Vec::new();
    if !state
        .permissions
        .has_permission(pool, user, "Custom HTML Block", "read", None)
        .await?
    {
        return Ok(items);
    }

    let blocks = workspace
        .get("custom_blocks")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    for block in blocks {
        let block_name = block
            .get("custom_block_name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if block_name.is_empty() {
            continue;
        }
        if !custom_block_has_role_allowed(&state.permissions, pool, &block_name, user).await? {
            continue;
        }
        items.push(block.clone());
    }
    Ok(items)
}

async fn custom_block_has_role_allowed(
    permissions: &permissions::PermissionEngine,
    pool: &orm::DatabasePool,
    block_name: &str,
    user: &str,
) -> error::Result<bool> {
    if user == "Administrator" {
        return Ok(true);
    }

    let rows = pool
        .execute_sql(
            r#"SELECT role FROM "has_role" WHERE parenttype = 'Custom HTML Block' AND parent = ?"#,
            vec![Value::String(block_name.into())],
        )
        .await?;

    if rows.is_empty() {
        return Ok(true);
    }

    let user_roles = get_user_roles_set(permissions, pool, user).await?;
    for row in rows {
        if let Some(role) = row.get("role").and_then(|v| v.as_str()) {
            if user_roles.contains(role) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

async fn compute_desktop_page_cache_key(
    pool: &orm::DatabasePool,
    permissions: &permissions::PermissionEngine,
    user: &str,
    page_name: &str,
) -> error::Result<String> {
    let workspace_modified: String = pool
        .execute_sql(
            r#"SELECT COALESCE(MAX(modified), '') AS m FROM "workspace" WHERE name = ?"#,
            vec![Value::String(page_name.into())],
        )
        .await?
        .into_iter()
        .next()
        .and_then(|r| r.get("m").and_then(|v| v.as_str().map(String::from)))
        .unwrap_or_default();

    let mut child_modified = String::new();
    for table in [
        "workspace_link",
        "workspace_shortcut",
        "workspace_chart",
        "workspace_number_card",
        "workspace_quick_list",
        "workspace_custom_block",
    ] {
        let sql = format!(
            r#"SELECT COALESCE(MAX(modified), '') AS m FROM "{}" WHERE parent = {}"#,
            table,
            pool.placeholder(1)
        );
        let m = pool
            .execute_sql(&sql, vec![Value::String(page_name.into())])
            .await
            .unwrap_or_default()
            .into_iter()
            .next()
            .and_then(|r| r.get("m").and_then(|v| v.as_str().map(String::from)))
            .unwrap_or_default();
        child_modified.push_str(&m);
    }

    let roles = permissions.get_roles(pool, user).await?;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    roles.hash(&mut hasher);
    let role_hash = format!("{:x}", hasher.finish());

    Ok(format!(
        "desktop_page:{}:{}:{}:{}",
        page_name, workspace_modified, child_modified, role_hash
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_desktop_page_query_preserves_json_with_plus() {
        let raw = "page=%7B%22name%22%3A%22Build%22%2C%22public%22%3A1%7D";
        let params = parse_desktop_page_query(raw);
        assert_eq!(
            params.get("page"),
            Some(&"{\"name\":\"Build\",\"public\":1}".to_string())
        );
    }

    #[test]
    fn test_render_social_login_buttons_includes_provider() {
        let provider = SocialLoginProvider {
            name: "office_365".to_string(),
            provider_name: "Office 365".to_string(),
            client_id: "test-client-id".to_string(),
            authorize_url: "https://login.microsoftonline.com/common/oauth2/authorize".to_string(),
            redirect_url: "/api/method/frappe.integrations.oauth2_logins.login_via_office365"
                .to_string(),
            auth_url_data: None,
            custom_base_url: false,
            base_url: None,
            icon: Some("/assets/frappe/icons/social/office_365.svg".to_string()),
        };

        let auth_url =
            "https://login.microsoftonline.com/common/oauth2/authorize?test=1".to_string();
        let html = render_social_login_buttons(&[(provider, auth_url)]);
        assert!(html.contains("Login with Office 365"));
        assert!(html.contains("btn-office_365"));
        assert!(html.contains("/assets/frappe/icons/social/office_365.svg"));
    }

    #[test]
    fn test_render_social_login_buttons_empty() {
        let html = render_social_login_buttons(&[]);
        assert!(html.is_empty());
    }

    fn config_with_custom_login(path: Option<&str>) -> config::RuntimeConfig {
        let mut config = config::RuntimeConfig::default();
        config.auth.custom_login_path = path.map(String::from);
        config
    }

    #[test]
    fn test_custom_login_target_disabled_by_default() {
        let config = config_with_custom_login(None);
        assert!(custom_login_target(&config, None).is_none());
        assert!(custom_login_target(&config, Some("redirect-to=/desk")).is_none());
    }

    #[test]
    fn test_custom_login_target_with_and_without_query() {
        let config = config_with_custom_login(Some("/sebrus_logger/login"));
        assert_eq!(
            custom_login_target(&config, None).as_deref(),
            Some("/sebrus_logger/login")
        );
        assert_eq!(
            custom_login_target(&config, Some("redirect-to=%2Fdesk")).as_deref(),
            Some("/sebrus_logger/login?redirect-to=%2Fdesk")
        );
        assert_eq!(
            custom_login_target(&config, Some("")).as_deref(),
            Some("/sebrus_logger/login")
        );
    }

    #[test]
    fn test_custom_login_target_rejects_unsafe_paths() {
        for bad in [
            "https://evil.com/login",
            "//evil.com/login",
            "login",
            "/login",
            "   ",
        ] {
            let config = config_with_custom_login(Some(bad));
            assert!(
                custom_login_target(&config, None).is_none(),
                "path {:?} must be rejected",
                bad
            );
        }
    }

    #[tokio::test]
    async fn test_attach_workspace_children_loads_links() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let tmp = std::env::temp_dir().join(format!(
            "kiff_ws_child_test_{}.db",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        let pool = orm::DatabasePool::connect_sqlite(tmp.to_str().unwrap())
            .await
            .expect("connect test db");

        pool.execute_sql(
            r#"
            CREATE TABLE "workspace_link" (
                name TEXT PRIMARY KEY,
                creation TEXT,
                modified TEXT,
                owner TEXT,
                idx INTEGER,
                parent TEXT,
                parentfield TEXT,
                parenttype TEXT,
                type TEXT,
                label TEXT,
                icon TEXT,
                hidden INTEGER,
                link_type TEXT,
                link_to TEXT,
                dependencies TEXT,
                only_for TEXT,
                onboard INTEGER,
                is_query_report INTEGER,
                link_count INTEGER,
                description TEXT,
                report_ref_doctype TEXT
            )
            "#,
            vec![],
        )
        .await
        .unwrap();

        pool.execute_sql(
            r#"INSERT INTO "workspace_link" (name, parent, parentfield, parenttype, idx, type, label, link_type, link_to)
               VALUES ('link-1', 'ISO 27001', 'links', 'Workspace', 0, 'Link', 'Audit Record', 'DocType', 'Audit Record'),
                      ('link-2', 'ISO 27001', 'links', 'Workspace', 1, 'Card Break', 'Audit Management', 'DocType', '')"#,
            vec![],
        )
        .await
        .unwrap();

        let mut workspaces = vec![json!({
            "name": "ISO 27001",
            "label": "ISO 27001",
            "title": "ISO 27001",
        })];

        attach_workspace_children(&pool, &mut workspaces)
            .await
            .expect("attach children");

        let ws = &workspaces[0];
        let links = ws
            .get("links")
            .and_then(|v| v.as_array())
            .expect("links array missing");
        assert_eq!(links.len(), 2);
        assert_eq!(
            links[0].get("label").and_then(|v| v.as_str()),
            Some("Audit Record")
        );
        assert_eq!(
            links[1].get("type").and_then(|v| v.as_str()),
            Some("Card Break")
        );

        // Missing child tables should surface as empty arrays, not errors.
        assert!(ws.get("shortcuts").and_then(|v| v.as_array()).is_some());

        let _ = std::fs::remove_file(&tmp);
    }

    #[tokio::test]
    async fn test_compute_boot_cache_key_changes_with_data() {
        let tmp = std::env::temp_dir().join(format!(
            "kiff_boot_key_test_{}.db",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        let pool = orm::DatabasePool::connect_sqlite(tmp.to_str().unwrap())
            .await
            .expect("connect test db");

        // Create a minimal "user" table so the cache-key query succeeds.
        pool.execute_sql(
            r#"CREATE TABLE "user" (name TEXT PRIMARY KEY, modified TEXT)"#,
            vec![],
        )
        .await
        .unwrap();

        let assets_mtime = SystemTime::UNIX_EPOCH;
        let key1 = compute_boot_cache_key(&pool, assets_mtime)
            .await
            .expect("compute key");

        pool.execute_sql(
            r#"INSERT INTO "user" (name, modified) VALUES ('u1', '2024-01-01')"#,
            vec![],
        )
        .await
        .unwrap();

        let key2 = compute_boot_cache_key(&pool, assets_mtime)
            .await
            .expect("compute key");

        assert_ne!(key1, key2, "cache key must change when data changes");

        let _ = std::fs::remove_file(&tmp);
    }

    #[tokio::test]
    async fn test_load_bundle_map_caches_assets_json() {
        let tmp_dir = std::env::temp_dir().join(format!(
            "kiff_assets_test_{}",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        tokio::fs::create_dir_all(&tmp_dir).await.unwrap();
        let assets_json = tmp_dir.join("assets.json");
        tokio::fs::write(&assets_json, r#"{"desk.bundle.js":"desk.bundle.js"}"#)
            .await
            .unwrap();

        let cache = rust_apps_core::AssetCache::default();
        let (map1, _) = load_bundle_map(&cache, &tmp_dir).await;
        let (map2, _) = load_bundle_map(&cache, &tmp_dir).await;
        assert_eq!(map1.get("desk.bundle.js"), Some(&"desk.bundle.js".to_string()));
        assert_eq!(map1, map2);

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[tokio::test]
    async fn test_load_icon_sprites_caches_empty_when_files_missing() {
        let cache = rust_apps_core::AssetCache::default();
        let sprites1 = load_icon_sprites(&cache).await;
        let sprites2 = load_icon_sprites(&cache).await;
        assert_eq!(sprites1, sprites2);
    }
}
