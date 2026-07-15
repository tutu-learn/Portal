use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use rust_apps_core::{AppContext, AppState, RustApp};
use std::collections::HashMap;
use tower::util::ServiceExt;

fn build_state() -> AppState {
    use dashmap::DashMap;
    use std::sync::Arc;

    AppState {
        config: Arc::new(config::RuntimeConfig::default()),
        site_manager: Arc::new(config::SiteManager::default()),
        pools: Arc::new(DashMap::new()),
        sessions: Arc::new(session::SessionStore::new()),
        permissions: Arc::new(permissions::PermissionEngine::new()),
        metadata: Arc::new(metadata::Meta::new()),
        pubsub: Arc::new(queue::PubSub::new()),
        translator: Arc::new(sql_translator::SqlTranslator::new(
            sql_translator::TargetDialect::Sqlite,
        )),
        rust_apps: rust_apps_core::RustAppRegistry::default(),
        logger: Arc::new(std::sync::OnceLock::new()),
    }
}

#[tokio::test]
async fn sebrus_logger_app_route_is_mounted() {
    let state = build_state();
    let app = sebrus_logger::SebrusLoggerApp;
    let ctx = AppContext::new(app.name(), state.clone());
    let router = app
        .routes(&ctx, http::router::create_router())
        .with_state(state);

    let response = router
        .oneshot(
            Request::builder()
                .uri("/sebrus_logger/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["app"], "sebrus_logger");
}

#[tokio::test]
async fn sebrus_logger_app_api_method() {
    let state = build_state();
    let registry =
        rust_apps_core::RustAppRegistry::new(vec![Box::new(sebrus_logger::SebrusLoggerApp)]);
    let params = HashMap::from([("name".to_string(), serde_json::json!("Tester"))]);

    let result = registry
        .call_method("sebrus_logger.hello", state, params, None)
        .await
        .unwrap()
        .expect("method should be found");

    assert_eq!(result["message"], "Hello from sebrus_logger: Tester");
}

#[tokio::test]
async fn sebrus_logger_app_workspaces() {
    let app = sebrus_logger::SebrusLoggerApp;
    let workspaces = app.workspaces();
    assert!(!workspaces.is_empty());
    assert_eq!(workspaces[0].name, "Sebrus Logger");
}
