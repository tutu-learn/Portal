use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

/// Extract the boot JSON object injected into the Desk HTML template.
fn extract_boot_json(html: &str) -> serde_json::Value {
    let marker = "window.frappe.boot = ";
    let start = html
        .find(marker)
        .expect("boot marker not found in rendered desk HTML");
    let json_start = start + marker.len();
    let rest = &html[json_start..];

    // Track brace depth to find the end of the boot object.
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    let mut end = rest.len();
    for (i, ch) in rest.char_indices() {
        if in_string {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' | '[' => depth += 1,
            '}' | ']' => {
                depth -= 1;
                if depth == 0 {
                    end = i + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    serde_json::from_str(&rest[..end]).expect("boot JSON should parse")
}

#[tokio::test(flavor = "current_thread")]
async fn test_desk_bootinfo_has_module_app_app_data_allowed_modules() {
    let pool = crate::common::setup_test_db().await.unwrap();

    // Seed an extra module/workspace owned by a Rust app so we can verify
    // cross-app aggregation.
    pool.execute_sql(
        r#"INSERT OR REPLACE INTO "module_def" (name, module_name, app_name)
           VALUES ('Kiff Logger Extra', 'Kiff Logger Extra', 'kiff_logger')"#,
        vec![],
    )
    .await
    .unwrap();
    pool.execute_sql(
        r#"INSERT OR REPLACE INTO "workspace" (name, label, module, public)
           VALUES ('kiff-logger-extra', 'Kiff Logger Extra', 'Kiff Logger Extra', 1)"#,
        vec![],
    )
    .await
    .unwrap();

    let state = crate::common::build_app_state(pool);
    let app = http::router::create_router().with_state(state);

    // Guests are redirected to login.
    let guest_response = app
        .clone()
        .oneshot(Request::builder().uri("/desk").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(guest_response.status(), StatusCode::TEMPORARY_REDIRECT);
    let location = guest_response
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .unwrap();
    assert!(location.starts_with("/login"));

    // Log in to obtain a session cookie.
    let login_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/method/login")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"usr": "Administrator", "pwd": "admin"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(login_response.status(), StatusCode::OK);

    let cookie = login_response
        .headers()
        .get_all("set-cookie")
        .iter()
        .find(|c| c.to_str().unwrap().starts_with("sid="))
        .cloned()
        .unwrap();

    // Request the desk page.
    let response = app
        .oneshot(
            Request::builder()
                .uri("/desk")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let html = String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();

    let boot = extract_boot_json(&html);

    // module_app maps scrubbed module names to apps.
    let module_app = boot
        .get("module_app")
        .and_then(|v| v.as_object())
        .expect("module_app should be an object");
    assert_eq!(module_app.get("core"), Some(&serde_json::json!("frappe")));
    assert_eq!(
        module_app.get("kiff_logger_extra"),
        Some(&serde_json::json!("kiff_logger"))
    );

    // app_data contains frappe and any Rust apps.
    let app_data = boot
        .get("app_data")
        .and_then(|v| v.as_array())
        .expect("app_data should be an array");
    let app_names: Vec<&str> = app_data
        .iter()
        .filter_map(|a| a.get("app_name").and_then(|v| v.as_str()))
        .collect();
    assert!(
        app_names.contains(&"frappe"),
        "frappe should be in app_data"
    );

    let frappe_app = app_data
        .iter()
        .find(|a| a.get("app_name") == Some(&serde_json::json!("frappe")))
        .expect("frappe app_data entry");
    assert_eq!(
        frappe_app.get("app_title"),
        Some(&serde_json::json!("Frappe Framework"))
    );
    let frappe_modules = frappe_app
        .get("modules")
        .and_then(|v| v.as_array())
        .expect("frappe modules should be an array");
    assert!(
        frappe_modules.iter().any(|m| m == "Core"),
        "Core module should belong to frappe"
    );

    let kiff_logger_app = app_data
        .iter()
        .find(|a| a.get("app_name") == Some(&serde_json::json!("kiff_logger")))
        .expect("kiff_logger app_data entry");
    let kiff_logger_modules = kiff_logger_app
        .get("modules")
        .and_then(|v| v.as_array())
        .expect("kiff_logger modules should be an array");
    assert!(
        kiff_logger_modules.iter().any(|m| m == "Kiff Logger Extra"),
        "Kiff Logger Extra module should belong to kiff_logger"
    );
    let kiff_logger_workspaces = kiff_logger_app
        .get("workspaces")
        .and_then(|v| v.as_array())
        .expect("kiff_logger workspaces should be an array");
    assert!(
        kiff_logger_workspaces.iter().any(|w| w == "kiff-logger-extra"),
        "kiff-logger-extra workspace should belong to kiff_logger"
    );

    // allowed_modules is populated for the desktop.
    let allowed_modules = boot
        .get("allowed_modules")
        .and_then(|v| v.as_array())
        .expect("allowed_modules should be an array");
    assert!(
        allowed_modules
            .iter()
            .any(|m| { m.get("module_name") == Some(&serde_json::json!("Core")) }),
        "Core should be in allowed_modules"
    );

    // user.allow_modules is populated for system users.
    let allow_modules = boot
        .get("user")
        .and_then(|v| v.get("allow_modules"))
        .and_then(|v| v.as_array())
        .expect("user.allow_modules should be an array");
    assert!(
        allow_modules.iter().any(|m| m == "Core"),
        "Core should be in user.allow_modules"
    );

    // Administrator must see System Manager in boot.user.roles so that
    // permlevel-1 fields (User roles/modules) are visible in the desk.
    let user_roles = boot
        .get("user")
        .and_then(|v| v.get("roles"))
        .and_then(|v| v.as_array())
        .expect("user.roles should be an array");
    assert!(
        user_roles.iter().any(|r| r == "System Manager"),
        "Administrator boot user.roles should include System Manager"
    );
}
