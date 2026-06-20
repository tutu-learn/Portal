use crate::AppState;
use axum::{
    extract::{OriginalUri, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::path::PathBuf;

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

const DESK_CSS_BUNDLES: &[&str] = &[
    "desk.bundle.css",
    "report.bundle.css",
];

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

    // Desk requires an authenticated session. Guests are redirected to login.
    if user.is_none() {
        let redirect_to = uri.path_and_query().map(|pq| pq.to_string()).unwrap_or_else(|| "/app".into());
        let query = serde_urlencoded::to_string(&LoginRedirectQuery { redirect_to: &redirect_to })
            .unwrap_or_else(|_| format!("redirect-to={}", redirect_to));
        return Redirect::temporary(&format!("/login?{}", query)).into_response();
    }

    // Load assets.json once; used in both boot info and HTML includes
    let assets_base = PathBuf::from("crates/http/assets");
    let bundle_map = load_bundle_map(&assets_base).await;

    // Build boot info
    let boot = build_boot_info(&state, user.as_deref(), &bundle_map).await;
    let boot_json = match serde_json::to_string(&boot) {
        Ok(j) => j,
        Err(e) => return error_response(&format!("boot serialization error: {}", e)),
    };

    // Discover JS/CSS assets from Frappe's assets.json
    let (js_includes, css_includes) = discover_assets(&bundle_map);

    // Build HTML
    let build_version = env!("CARGO_PKG_VERSION");
    let csrf_token = generate_csrf_token();
    let lang = "en";
    let layout_direction = "ltr";

    let icon_sprites = load_icon_sprites().await;

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

    let pool = state.pools.iter().next()?.value().clone();
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

async fn query_boot_data(
    pool: &orm::DatabasePool,
) -> error::Result<(Vec<Value>, Map<String, Value>, Vec<String>, Map<String, Value>, Option<String>)> {
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
        let name = row.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if name.is_empty() {
            continue;
        }
        let label = row.get("label").and_then(|v| v.as_str()).unwrap_or(&name).to_string();
        let title = row.get("title").and_then(|v| v.as_str()).unwrap_or(&label).to_string();
        let icon = row.get("icon").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let module = row.get("module").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let parent_page = row.get("parent_page").and_then(|v| v.as_str()).unwrap_or("").to_string();

        let public = row.get("public")
            .and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
            .unwrap_or(1);
        let is_hidden = row.get("is_hidden")
            .and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
            .unwrap_or(0);
        let sequence_id = row.get("sequence_id")
            .and_then(|v| v.as_f64().or_else(|| v.as_i64().map(|i| i as f64)).or_else(|| v.as_str().and_then(|s| s.parse().ok())))
            .unwrap_or(0.0);

        let content = match row.get("content") {
            Some(Value::String(s)) if !s.trim().is_empty() => Value::String(s.clone()),
            Some(Value::String(_)) => Value::Null,
            Some(v) => v.clone(),
            None => Value::Null,
        };
        let app = row.get("app").and_then(|v| v.as_str()).unwrap_or("frappe").to_string();
        let ws_type = row.get("type").and_then(|v| v.as_str()).unwrap_or("Workspace").to_string();
        let link_type = row.get("link_type").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let link_to = row.get("link_to").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let external_link = row.get("external_link").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let indicator_color = row.get("indicator_color").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let for_user_val = row.get("for_user").and_then(|v| v.as_str()).unwrap_or("").to_string();

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

    // Query modules
    let mod_rows = pool.execute_sql(
        r#"SELECT name, module_name FROM "module_def" ORDER BY module_name"#,
        vec![],
    ).await?;

    let mut modules_map = Map::new();
    let mut module_list = Vec::new();

    for row in mod_rows {
        let name = row.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if name.is_empty() {
            continue;
        }
        let module_name = row.get("module_name").and_then(|v| v.as_str()).unwrap_or(&name).to_string();

        let mod_obj = json!({
            "label": module_name,
            "color": "#8D99A6",
            "icon": "",
            "type": "module",
        });
        modules_map.insert(module_name.clone(), mod_obj);
        module_list.push(module_name);
    }

    Ok((workspaces, modules_map, module_list, module_wise_workspaces, default_ws))
}

/// Build workspace-related boot objects from the workspace list queried from the DB.
/// Returns (workspaces object, workspace_sidebar_item object, default_workspace object).
fn build_workspace_boot_objects(
    workspaces: &[Value],
    is_guest: bool,
) -> (Value, Value, Value) {
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
    let default_workspace_obj = workspaces.first().map(|ws| {
        json!({
            "name": ws.get("name"),
            "title": ws.get("title"),
            "public": ws.get("public"),
        })
    }).unwrap_or(Value::Null);

    (workspaces_value, workspace_sidebar_item_value, default_workspace_obj)
}

async fn build_boot_info(
    state: &AppState,
    user: Option<&str>,
    bundle_map: &HashMap<String, String>,
) -> serde_json::Value {
    let is_guest = user.is_none();
    let user_name = user.unwrap_or("Guest");

    // Get DB pool for site queries first; workspace data is needed both for the
    // Python bootinfo overlay and for the fallback bootinfo.
    let pool = state.pools.iter().next().map(|e| e.value().clone());

    // Query workspaces and modules from DB
    let (workspaces, modules_map, module_list, module_wise_workspaces, _default_ws) =
        if let Some(ref pool) = pool {
            match query_boot_data(pool).await {
                Ok(data) => data,
                Err(_) => (vec![], Map::new(), vec![], Map::new(), None),
            }
        } else {
            (vec![], Map::new(), vec![], Map::new(), None)
        };

    let (workspaces_value, workspace_sidebar_item_value, default_workspace_obj) =
        build_workspace_boot_objects(&workspaces, is_guest);

    // Try to build bootinfo via the real Frappe boot module through the Python bridge.
    // Frappe 16's frontend expects many fields that are tedious to hardcode; delegating
    // to the real framework is the most compatible path. We then overlay our own values
    // (assets_json, user info, workspace sidebar, etc.) so the Kiff runtime stays in control.
    if !is_guest {
        let u = user_name.to_string();
        let py_boot = tokio::task::spawn_blocking(move || {
            kiff_core::call_method_with_user(
                "frappe.boot.get_bootinfo",
                &serde_json::json!({}),
                Some(&u),
            )
        })
        .await;

        match &py_boot {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => tracing::warn!("frappe.boot.get_bootinfo failed: {}", e),
            Err(e) => tracing::warn!("frappe.boot.get_bootinfo task panicked: {}", e),
        }

        if let Some(Value::Object(mut wrapper)) = py_boot.ok().and_then(|r| r.ok()) {
            // call_method_with_user returns {"message": <bootinfo>} for standard API methods.
            let mut boot = if let Some(Value::Object(boot)) = wrapper.remove("message") {
                boot
            } else {
                wrapper
            };
            // Overlay Kiff-specific / runtime-controlled fields.
            boot.insert("assets_json".to_string(), json!(bundle_map));
            boot.insert("sitename".to_string(), json!("localhost"));
            boot.insert("home_page".to_string(), json!("Workspaces"));
            boot.insert("lang".to_string(), json!("en"));
            boot.insert("desk_theme".to_string(), json!("Light"));
            boot.insert("developer_mode".to_string(), json!(true));
            boot.insert("socketio_port".to_string(), json!(9000));
            boot.insert("disable_async".to_string(), json!(false));
            boot.insert("server_date".to_string(), json!(chrono::Local::now().format("%Y-%m-%d").to_string()));
            boot.insert("metadata_version".to_string(), json!("1"));
            // Replace Python-generated workspace data with the Rust-built version so
            // Rust-app workspaces (Audit Ready, Policies, Procedures, ISO 27001) show
            // up in the desk sidebar and a valid default workspace is set.
            boot.insert("workspaces".to_string(), workspaces_value);
            boot.insert("allowed_workspaces".to_string(), json!(workspaces));
            boot.insert("module_wise_workspaces".to_string(), Value::Object(module_wise_workspaces));
            boot.insert("workspace_sidebar_item".to_string(), workspace_sidebar_item_value);
            if let Some(Value::Object(user_obj)) = boot.get_mut("user") {
                user_obj.insert("default_workspace".to_string(), default_workspace_obj);
            }
            sanitize_bootinfo(&mut boot);
            return Value::Object(boot);
        }
    }

    let roles: Value = if is_guest {
        json!([])
    } else {
        json!([{"role": "Administrator"}])
    };

    // Administrator gets full permissions on core doctypes
    let core_doctypes: Vec<&str> = vec![
        "User", "Role", "Has Role", "Module Def", "Workspace", "Page",
        "DocType", "DocField", "DocPerm", "System Settings",
        "Custom Field", "Property Setter", "Workflow", "Workflow State",
        "Workflow Action Master", "Gender", "Salutation", "User Type",
        "Language", "Translation", "File", "Report", "Dashboard",
        "Dashboard Chart", "Number Card", "Notification Settings",
        "Error Log", "Activity Log", "Access Log", "Version",
        "Communication", "Comment", "ToDo", "Event", "Note",
        "Tag", "Tag Link", "Patch Log", "Scheduled Job Type",
        "Scheduler Event", "RQ Job", "RQ Worker", "Webhook",
        "Server Script", "Client Script", "Print Format",
        "Letter Head", "Terms and Conditions", "Address",
        "Contact", "Country", "Currency", "Calendar View",
        "Kanban Board", "List View Settings", "Form Tour",
        "Onboarding Step", "Module Onboarding", "Domain",
        "Company", "Website Theme", "Web Page", "Web Form",
        "Blogger", "Blog Post", "Blog Category", "Blog Settings",
        "Website Settings", "About Us Settings", "Contact Us Settings",
        "Social Login Key", "OAuth Client", "OAuth Authorization Code",
        "OAuth Bearer Token", "Integration Request", "Connected App",
        "Email Account", "Email Domain", "Email Template",
        "Notification", "Auto Email Report", "S3 Backup Settings",
        "Dropbox Settings", "Google Settings", "Google Drive",
        "LDAP Settings", "Stripe Settings", "PayPal Settings",
        "Recorder Query", "Success Action", "Review",
        "Global Search Settings", "Console Log", "Package",
        "Package Release", "Energy Point Rule", "Energy Point Log",
        "Milestone", "Milestone Tracker", "Transaction Log",
        "Bulk Update", "Data Import", "Data Export",
        "Document Share Key", "Document Naming Rule",
        "Document Naming Settings", "Submission Queue",
        "Installed Application", "Module Profile", "User Group",
        "User Group Member", "Dashboard Chart Source",
        "Number Card", "Shortcut", "Custom HTML Block",
        "Network Printer Settings", "Print Style", "Print Heading",
        "Address Template", "Contacts Settings", "Google Contacts",
        "Holiday List", "Weekday", "Stock Entry", "Item",
        "Item Group", "Warehouse", "UOM", "Brand",
        "Customer", "Supplier", "Sales Order", "Purchase Order",
        "Sales Invoice", "Purchase Invoice", "Payment Entry",
        "Journal Entry", "Account", "Cost Center",
        "Budget", "Project", "Task", "Timesheet",
        "Employee", "Department", "Designation", "Salary Structure",
        "Salary Slip", "Leave Application", "Attendance",
        "Job Opening", "Job Applicant", "Job Offer",
        "Quiz", "LMS Course", "LMS Batch", "LMS Enrollment",
    ];
    let core_doctypes = json!(core_doctypes);

    let mut user_obj = Map::new();
    user_obj.insert("name".to_string(), json!(user_name));
    user_obj.insert("email".to_string(), json!(user_name));
    user_obj.insert("full_name".to_string(), json!(user_name));
    user_obj.insert("user_type".to_string(), json!(if is_guest { "Guest" } else { "System User" }));
    user_obj.insert("roles".to_string(), roles);
    user_obj.insert("language".to_string(), json!("en"));
    user_obj.insert("timezone".to_string(), json!("UTC"));
    user_obj.insert("can_read".to_string(), core_doctypes.clone());
    user_obj.insert("can_create".to_string(), core_doctypes.clone());
    user_obj.insert("can_write".to_string(), core_doctypes.clone());
    user_obj.insert("can_select".to_string(), core_doctypes.clone());
    user_obj.insert("can_submit".to_string(), json!([]));
    user_obj.insert("can_cancel".to_string(), json!([]));
    user_obj.insert("can_delete".to_string(), core_doctypes.clone());
    user_obj.insert("can_get_report".to_string(), core_doctypes.clone());
    user_obj.insert("allow_modules".to_string(), json!(module_list.clone()));
    user_obj.insert("all_read".to_string(), core_doctypes.clone());
    user_obj.insert("can_search".to_string(), core_doctypes.clone());
    user_obj.insert("in_create".to_string(), json!([]));
    user_obj.insert("can_export".to_string(), core_doctypes.clone());
    user_obj.insert("can_import".to_string(), core_doctypes.clone());
    user_obj.insert("can_print".to_string(), core_doctypes.clone());
    user_obj.insert("can_email".to_string(), core_doctypes.clone());
    user_obj.insert("can_share".to_string(), core_doctypes.clone());
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
    sysdefaults.insert("rounding_method".to_string(), json!("Banker's Rounding (legacy)"));
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
    notification_settings.insert("enable_email_threads_on_assigned_document".to_string(), json!(true));
    notification_settings.insert("enable_email_share".to_string(), json!(true));
    notification_settings.insert("enable_email_event_reminders".to_string(), json!(true));

    let mut navbar_settings = Map::new();
    navbar_settings.insert("help_dropdown".to_string(), json!([]));
    navbar_settings.insert("settings_dropdown".to_string(), json!([
        {"item_label": "My Settings", "route": "/app/user/"},
        {"item_label": "Logout", "route": "/logout"}
    ]));
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
    boot.insert("home_page".to_string(), json!("Workspaces"));
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
    boot.insert("notification_settings".to_string(), Value::Object(notification_settings));
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
    boot.insert("module_wise_workspaces".to_string(), Value::Object(module_wise_workspaces));
    boot.insert("workspace_sidebar_item".to_string(), workspace_sidebar_item_value);
    boot.insert("dashboards".to_string(), json!([]));
    boot.insert("page_info".to_string(), json!({}));
    boot.insert("allowed_pages".to_string(), json!([]));
    boot.insert("allowed_modules".to_string(), json!([]));
    boot.insert("notes".to_string(), json!([]));
    boot.insert("letter_heads".to_string(), json!({}));
    boot.insert("module_app".to_string(), json!({}));
    boot.insert("app_data".to_string(), json!([]));
    boot.insert("app_name_style".to_string(), json!("Default"));
    boot.insert("desktop_icons".to_string(), json!([]));
    boot.insert("calendars".to_string(), json!([]));
    boot.insert("treeviews".to_string(), json!([]));
    boot.insert("print_css".to_string(), json!(""));
    boot.insert("home_folder".to_string(), json!(""));
    boot.insert("navbar_settings".to_string(), Value::Object(navbar_settings));
    boot.insert("app_logo_url".to_string(), json!("/assets/frappe/images/frappe-framework-logo.svg"));
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
    boot.insert("server_date".to_string(), json!(chrono::Local::now().format("%Y-%m-%d").to_string()));

    sanitize_bootinfo(&mut boot);
    Value::Object(boot)
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
    if !user.get("all_reports").map(|v| v.is_object()).unwrap_or(false) {
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

async fn load_icon_sprites() -> String {
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
    sprites
}

async fn load_bundle_map(assets_base: &PathBuf) -> HashMap<String, String> {
    let path = assets_base.join("assets.json");
    if path.exists() {
        match tokio::fs::read_to_string(&path).await {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => HashMap::new(),
        }
    } else {
        HashMap::new()
    }
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
            css_tags.push_str(&format!(
                r#"<link rel="stylesheet" href="{}">"#,
                path
            ));
            css_tags.push('\n');
        }
    }

    (js_tags, css_tags)
}

fn generate_csrf_token() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Enabled social login provider as stored in the Social Login Key doctype.
#[derive(Debug)]
struct SocialLoginProvider {
    name: String,
    provider_name: String,
    client_id: String,
    authorize_url: String,
    redirect_url: String,
    auth_url_data: Option<Value>,
    custom_base_url: bool,
    base_url: Option<String>,
    icon: Option<String>,
}

/// Build the absolute site URL from the request Host header.
fn site_url_from_headers(headers: &HeaderMap) -> String {
    let host = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("localhost:8000");
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("http");
    format!("{}://{}", scheme, host)
}

/// Load enabled Social Login Keys from the database.
async fn get_social_login_providers(pool: &orm::DatabasePool) -> Vec<SocialLoginProvider> {
    let sql = r#"SELECT name, client_id, base_url, provider_name, icon,
                        authorize_url, redirect_url, auth_url_data, custom_base_url
                 FROM "social_login_key"
                 WHERE enable_social_login = 1
                 ORDER BY name"#;

    let rows = match pool.execute_sql(sql, vec![]).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("failed to load social login keys: {}", e);
            return vec![];
        }
    };

    rows.into_iter()
        .filter_map(|mut row| {
            let client_id = row.remove("client_id")?.as_str()?.to_string();
            if client_id.is_empty() {
                return None;
            }
            let name = row.remove("name")?.as_str()?.to_string();
            let provider_name = row
                .remove("provider_name")
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_else(|| name.clone());
            let authorize_url = row.remove("authorize_url")?.as_str()?.to_string();
            let redirect_url = row.remove("redirect_url")?.as_str()?.to_string();
            let auth_url_data = row.remove("auth_url_data").filter(|v| !v.is_null());
            let custom_base_url = row
                .remove("custom_base_url")
                .and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
                .unwrap_or(0)
                == 1;
            let base_url = row.remove("base_url").and_then(|v| v.as_str().map(String::from));
            let icon = row.remove("icon").and_then(|v| v.as_str().map(String::from));

            Some(SocialLoginProvider {
                name,
                provider_name,
                client_id,
                authorize_url,
                redirect_url,
                auth_url_data,
                custom_base_url,
                base_url,
                icon,
            })
        })
        .collect()
}

/// Build the OAuth2 authorization URL for a provider.
fn build_authorize_url(
    provider: &SocialLoginProvider,
    site_url: &str,
    redirect_to: Option<&str>,
) -> Option<String> {
    let authorize_url = if provider.custom_base_url {
        match &provider.base_url {
            Some(base) => build_oauth_url(base, &provider.authorize_url),
            None => return None,
        }
    } else {
        provider.authorize_url.clone()
    };

    let redirect_uri = if provider.redirect_url.starts_with("http://")
        || provider.redirect_url.starts_with("https://")
    {
        provider.redirect_url.clone()
    } else {
        format!("{}{}", site_url.trim_end_matches('/'), provider.redirect_url)
    };

    let token = uuid::Uuid::new_v4().simple().to_string();
    let state = json!({
        "site": site_url,
        "token": token,
        "redirect_to": redirect_to.unwrap_or(""),
    });
    let state_b64 = BASE64.encode(state.to_string().as_bytes());

    let mut params: HashMap<String, String> = HashMap::new();
    params.insert("client_id".to_string(), provider.client_id.clone());
    params.insert("redirect_uri".to_string(), redirect_uri);
    params.insert("state".to_string(), state_b64);

    if let Some(Value::Object(map)) = &provider.auth_url_data {
        for (k, v) in map {
            if let Some(s) = v.as_str() {
                params.insert(k.clone(), s.to_string());
            } else if !v.is_null() {
                params.insert(k.clone(), v.to_string());
            }
        }
    }

    // Default OAuth2 parameters if the provider config did not supply them.
    params.entry("response_type".to_string()).or_insert_with(|| "code".to_string());

    // Microsoft Entra ID (v2.0) requires a scope parameter on the authorize request.
    // If the provider config left it out, default to the standard OIDC scopes so login works.
    if authorize_url.contains("login.microsoftonline.com") && authorize_url.contains("/oauth2/v2.0/authorize") {
        params.entry("scope".to_string()).or_insert_with(|| "openid email profile".to_string());
    }

    let query = match serde_urlencoded::to_string(&params) {
        Ok(q) => q,
        Err(_) => return None,
    };

    Some(format!("{}?{}", authorize_url, query))
}

/// Join a base URL with a relative or absolute OAuth URL.
fn build_oauth_url(base_url: &str, url: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") {
        return url.to_string();
    }
    format!("{}{}", base_url.trim_end_matches('/'), url)
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
                format!(r#"<img src="{}" alt="{}" class="social-icon">"#, icon, provider.provider_name)
            }
            Some(icon) => {
                format!(r#"<span class="social-icon {}"></span>"#, icon)
            }
            None => String::new(),
        };

        let btn_class = format!("btn btn-social btn-{}", provider.name.to_lowercase().replace(' ', "_"));
        html.push_str(&format!(
            r#"<a href="{}" class="{}">{}Login with {}</a>"#,
            auth_url, btn_class, icon_html, provider.provider_name
        ));
    }

    html.push_str("</div></div>");
    html
}

/// Serve the standalone login page.
pub async fn serve_login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let path = PathBuf::from("crates/http/assets/login.html");
    let html = match tokio::fs::read_to_string(&path).await {
        Ok(h) => h,
        Err(_) => return error_response("login page not found"),
    };

    let site_url = site_url_from_headers(&headers);
    let redirect_to = query.get("redirect-to").map(|s| s.as_str());

    let social_buttons = if let Some(pool) = state.pools.iter().next().map(|e| e.value().clone()) {
        let providers = get_social_login_providers(&pool).await;
        let providers_with_urls: Vec<_> = providers
            .into_iter()
            .filter_map(|p| {
                let url = build_authorize_url(&p, &site_url, redirect_to)?;
                Some((p, url))
            })
            .collect();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_oauth_url_with_absolute_url() {
        assert_eq!(
            build_oauth_url("https://example.com", "https://login.microsoftonline.com/common/oauth2/authorize"),
            "https://login.microsoftonline.com/common/oauth2/authorize"
        );
    }

    #[test]
    fn test_build_oauth_url_with_relative_url() {
        assert_eq!(
            build_oauth_url("https://example.com", "/oauth2/authorize"),
            "https://example.com/oauth2/authorize"
        );
    }

    #[test]
    fn test_build_oauth_url_with_base_trailing_slash() {
        assert_eq!(
            build_oauth_url("https://example.com/", "/oauth2/authorize"),
            "https://example.com/oauth2/authorize"
        );
    }

    #[test]
    fn test_build_authorize_url_for_office365() {
        let provider = SocialLoginProvider {
            name: "office_365".to_string(),
            provider_name: "Office 365".to_string(),
            client_id: "test-client-id".to_string(),
            authorize_url: "https://login.microsoftonline.com/common/oauth2/authorize".to_string(),
            redirect_url: "/api/method/frappe.integrations.oauth2_logins.login_via_office365".to_string(),
            auth_url_data: Some(json!({"response_type": "code", "scope": "openid"})),
            custom_base_url: false,
            base_url: None,
            icon: Some("/assets/frappe/icons/social/office_365.svg".to_string()),
        };

        let url = build_authorize_url(&provider, "http://localhost:8000", Some("/app")).unwrap();
        assert!(url.starts_with("https://login.microsoftonline.com/common/oauth2/authorize?"));
        assert!(url.contains("client_id=test-client-id"));
        assert!(url.contains("redirect_uri=http%3A%2F%2Flocalhost%3A8000%2Fapi%2Fmethod%2Ffrappe.integrations.oauth2_logins.login_via_office365"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("scope=openid"));
        assert!(url.contains("state="));
    }

    #[test]
    fn test_build_authorize_url_for_microsoft_entra_v2_defaults_scope() {
        let provider = SocialLoginProvider {
            name: "microsoft".to_string(),
            provider_name: "Microsoft".to_string(),
            client_id: "test-client-id".to_string(),
            authorize_url: "https://login.microsoftonline.com/1d6f2f1f-694e-4308-a2ba-bb00bb00fa46/oauth2/v2.0/authorize".to_string(),
            redirect_url: "/api/method/frappe.integrations.oauth2_logins.login_via_microsoft".to_string(),
            auth_url_data: Some(json!({"response_type": "code"})),
            custom_base_url: false,
            base_url: None,
            icon: Some("/assets/frappe/icons/social/office_365.svg".to_string()),
        };

        let url = build_authorize_url(&provider, "https://compliance-system.sebrus.dev", Some("/desk")).unwrap();
        assert!(url.starts_with("https://login.microsoftonline.com/1d6f2f1f-694e-4308-a2ba-bb00bb00fa46/oauth2/v2.0/authorize?"));
        assert!(url.contains("client_id=test-client-id"));
        assert!(url.contains("redirect_uri=https%3A%2F%2Fcompliance-system.sebrus.dev%2Fapi%2Fmethod%2Ffrappe.integrations.oauth2_logins.login_via_microsoft"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("scope=openid+email+profile"));
        assert!(url.contains("state="));
    }

    #[test]
    fn test_render_social_login_buttons_includes_provider() {
        let provider = SocialLoginProvider {
            name: "office_365".to_string(),
            provider_name: "Office 365".to_string(),
            client_id: "test-client-id".to_string(),
            authorize_url: "https://login.microsoftonline.com/common/oauth2/authorize".to_string(),
            redirect_url: "/api/method/frappe.integrations.oauth2_logins.login_via_office365".to_string(),
            auth_url_data: None,
            custom_base_url: false,
            base_url: None,
            icon: Some("/assets/frappe/icons/social/office_365.svg".to_string()),
        };

        let auth_url = "https://login.microsoftonline.com/common/oauth2/authorize?test=1".to_string();
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
}
