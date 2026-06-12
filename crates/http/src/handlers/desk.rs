use crate::AppState;
use axum::{
    extract::{OriginalUri, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
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

    let html = DESK_TEMPLATE
        .replace("{{BOOT_JSON}}", &boot_json)
        .replace("{{JS_INCLUDES}}", &js_includes)
        .replace("{{CSS_INCLUDES}}", &css_includes)
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
        r#"SELECT name, label, title, icon, public, is_hidden, sequence_id, module, parent_page, for_user
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

        let ws_obj = json!({
            "name": name,
            "label": label,
            "title": title,
            "icon": icon,
            "public": public,
            "is_hidden": is_hidden,
            "sequence_id": sequence_id,
            "module": module,
            "parent_page": parent_page,
        });
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

async fn build_boot_info(
    state: &AppState,
    user: Option<&str>,
    bundle_map: &HashMap<String, String>,
) -> serde_json::Value {
    let is_guest = user.is_none();
    let user_name = user.unwrap_or("Guest");

    // Get DB pool for site queries
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
    // Build default_workspace as an object {name, title, public} — the frontend
    // expects frappe.boot.user.default_workspace to be an object, not a string.
    let default_workspace_obj = workspaces.first().map(|ws| {
        json!({
            "name": ws.get("name"),
            "title": ws.get("title"),
            "public": ws.get("public"),
        })
    }).unwrap_or(Value::Null);
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
    boot.insert("docs".to_string(), json!([]));
    boot.insert("allowed_workspaces".to_string(), json!(workspaces));
    boot.insert("module_wise_workspaces".to_string(), Value::Object(module_wise_workspaces));
    boot.insert("dashboards".to_string(), json!([]));
    boot.insert("page_info".to_string(), json!({}));
    boot.insert("allowed_pages".to_string(), json!([]));
    boot.insert("allowed_modules".to_string(), json!([]));
    boot.insert("letter_heads".to_string(), json!({}));
    boot.insert("module_app".to_string(), json!({}));
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

    Value::Object(boot)
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

/// Serve the standalone login page.
pub async fn serve_login() -> impl IntoResponse {
    let path = PathBuf::from("crates/http/assets/login.html");
    match tokio::fs::read_to_string(&path).await {
        Ok(html) => axum::response::Html(html).into_response(),
        Err(_) => error_response("login page not found"),
    }
}

fn error_response(msg: &str) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        [("content-type", "text/plain")],
        msg.to_string(),
    )
        .into_response()
}
