pub mod extract;
pub mod handlers;
pub mod middleware;
pub mod router;
pub mod websocket;

use axum::Router;
use dashmap::DashMap;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<config::RuntimeConfig>,
    pub site_manager: Arc<config::SiteManager>,
    pub pools: Arc<DashMap<String, orm::DatabasePool>>,
    pub sessions: Arc<session::SessionStore>,
    pub permissions: Arc<permissions::PermissionEngine>,
    pub metadata: Arc<metadata::Meta>,
    pub pubsub: Arc<queue::PubSub>,
    pub translator: Arc<sql_translator::SqlTranslator>,
}

pub async fn run_server(state: AppState, host: &str, port: u16) -> error::Result<()> {
    let app = router::create_router(state);
    let listener = tokio::net::TcpListener::bind(format!("{}:{}", host, port)).await
        .map_err(error::RuntimeError::Io)?;
    tracing::info!("HTTP server listening on {}:{}", host, port);
    axum::serve(listener, app).await
        .map_err(|e| error::RuntimeError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
    Ok(())
}
