use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

#[tokio::test]
async fn test_api_get_doc_not_found() {
    let pool = crate::common::setup_test_db().await.unwrap();
    let state = crate::common::build_app_state(pool);
    let app = http::router::create_router(state);

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
    let app = http::router::create_router(state);

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
    let app = http::router::create_router(state);

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
async fn test_login_endpoint() {
    let pool = crate::common::setup_test_db().await.unwrap();
    let state = crate::common::build_app_state(pool);
    let app = http::router::create_router(state);

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
