use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

#[tokio::test]
async fn test_getpage_permission_manager() {
    let pool = crate::common::setup_test_db().await.unwrap();
    let state = crate::common::build_app_state(pool);
    let app = http::router::create_router().with_state(state);

    // Log in as Administrator to satisfy page role checks.
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

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/method/frappe.desk.desk_page.getpage?name=permission-manager")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let docs = json["docs"].as_array().unwrap();
    assert_eq!(docs[0]["name"], "permission-manager");
    assert!(docs[0]["script"].as_str().unwrap().contains("permission_manager"));
    assert!(docs[0]["script"]
        .as_str()
        .unwrap()
        .contains(r#"frappe.templates["permission_manager_help"]"#));
}

#[tokio::test]
async fn test_getpage_post_permission_manager() {
    let pool = crate::common::setup_test_db().await.unwrap();
    let state = crate::common::build_app_state(pool);
    let app = http::router::create_router().with_state(state);

    // Log in as Administrator to satisfy page role checks.
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

    // Frappe Desk sends getpage as a POST with form-encoded args.
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/method/frappe.desk.desk_page.getpage")
                .header("content-type", "application/x-www-form-urlencoded")
                .header("cookie", cookie)
                .body(Body::from("args=%7B%22name%22%3A%22permission-manager%22%7D"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let docs = json["docs"].as_array().unwrap();
    assert_eq!(docs[0]["name"], "permission-manager");
}

#[tokio::test]
async fn test_api_get_doc_not_found() {
    let pool = crate::common::setup_test_db().await.unwrap();
    let state = crate::common::build_app_state(pool);
    let app = http::router::create_router().with_state(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/resource/TestDocType/NONEXISTENT")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_api_get_list_empty() {
    let pool = crate::common::setup_test_db().await.unwrap();
    crate::common::create_doctype_table(&pool, "EmptyDoc").await.unwrap();

    let state = crate::common::build_app_state(pool);
    let app = http::router::create_router().with_state(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/resource/EmptyDoc")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = json.get("data").and_then(|v| v.as_array()).unwrap();
    assert!(data.is_empty());
}

#[tokio::test]
async fn test_api_insert_and_get_doc() {
    let pool = crate::common::setup_test_db().await.unwrap();
    crate::common::create_doctype_table(&pool, "ApiDoc").await.unwrap();

    let state = crate::common::build_app_state(pool);
    let app = http::router::create_router().with_state(state);

    // Insert a document
    let insert_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/resource/ApiDoc")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"title": "API Test"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(insert_response.status(), StatusCode::CREATED);

    let body = insert_response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let name = json["data"]["name"].as_str().unwrap();
    assert!(!name.is_empty());

    // Fetch the document
    let get_response = app
        .oneshot(
            Request::builder()
                .uri(&format!("/api/resource/ApiDoc/{}", name))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(get_response.status(), StatusCode::OK);

    let body = get_response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let title = json["data"]["title"].as_str().unwrap();
    assert_eq!(title, "API Test");
}

#[tokio::test]
async fn test_permission_manager_get_roles_and_doctypes() {
    let pool = crate::common::setup_test_db().await.unwrap();
    let state = crate::common::build_app_state(pool);
    let app = http::router::create_router().with_state(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/method/frappe.core.page.permission_manager.permission_manager.get_roles_and_doctypes")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json["message"]["doctypes"].as_array().unwrap().len() > 0);
    assert!(json["message"]["roles"].as_array().unwrap().len() > 0);
}

#[tokio::test]
async fn test_permission_manager_get_roles_and_doctypes_post() {
    let pool = crate::common::setup_test_db().await.unwrap();
    let state = crate::common::build_app_state(pool);
    let app = http::router::create_router().with_state(state);

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/method/frappe.core.page.permission_manager.permission_manager.get_roles_and_doctypes")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json["message"]["doctypes"].as_array().unwrap().len() > 0);
    assert!(json["message"]["roles"].as_array().unwrap().len() > 0);
}

#[tokio::test]
async fn test_permission_manager_get_permissions_post() {
    let pool = crate::common::setup_test_db().await.unwrap();
    let state = crate::common::build_app_state(pool);
    let app = http::router::create_router().with_state(state);

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/method/frappe.core.page.permission_manager.permission_manager.get_permissions")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from("args=%7B%22doctype%22%3A%22User%22%7D"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json["message"].is_array());
}

#[tokio::test]
async fn test_login_endpoint() {
    let pool = crate::common::setup_test_db().await.unwrap();
    let state = crate::common::build_app_state(pool);
    let app = http::router::create_router().with_state(state);

    let response = app
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

    assert_eq!(response.status(), StatusCode::OK);

    let cookies: Vec<_> = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .cloned()
        .collect();

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["message"].as_str(), Some("Logged In"));
    let mut has_sid = false;
    for cookie in cookies {
        if cookie.to_str().unwrap().contains("sid=") {
            has_sid = true;
        }
    }
    assert!(has_sid, "Response should set sid cookie");
}
