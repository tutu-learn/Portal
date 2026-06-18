use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use rust_apps_core::{AppContext, AppState, RustApp};
use tower::util::ServiceExt;
use std::collections::HashMap;

fn build_state() -> AppState {
    use std::sync::Arc;
    use dashmap::DashMap;

    AppState {
        config: Arc::new(config::RuntimeConfig::default()),
        site_manager: Arc::new(config::SiteManager::default()),
        pools: Arc::new(DashMap::new()),
        sessions: Arc::new(session::SessionStore::new()),
        permissions: Arc::new(permissions::PermissionEngine::new()),
        metadata: Arc::new(metadata::Meta::new()),
        pubsub: Arc::new(queue::PubSub::new()),
        translator: Arc::new(sql_translator::SqlTranslator::new(sql_translator::TargetDialect::Sqlite)),
        rust_apps: rust_apps_core::RustAppRegistry::default(),
    }
}

#[tokio::test]
async fn sample_app_route_is_mounted() {
    let state = build_state();
    let app = sample::SampleApp;
    let ctx = AppContext::new(app.name(), state.clone());
    let router = app.routes(&ctx, http::router::create_router()).with_state(state);

    let response = router
        .oneshot(
            Request::builder()
                .uri("/sample/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["app"], "sample");
}

#[tokio::test]
async fn sample_app_api_method() {
    let state = build_state();
    let registry = rust_apps_core::RustAppRegistry::new(vec![Box::new(sample::SampleApp)]);
    let params = HashMap::from([("name".to_string(), serde_json::json!("Tester"))]);

    let result = registry
        .call_method("sample.hello", state, params)
        .await
        .unwrap()
        .expect("method should be found");

    assert_eq!(result["message"], "Hello from sample: Tester");
}

#[tokio::test]
async fn sample_app_doctype_fixture() {
    let app = sample::SampleApp;
    let fixtures = app.doctypes();
    assert_eq!(fixtures.len(), 1);
    assert_eq!(fixtures[0].name, "Rust ToDo");
}
