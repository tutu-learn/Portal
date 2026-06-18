pub mod extract;
pub mod handlers;
pub mod middleware;
pub mod router;
pub mod websocket;

pub use rust_apps_core::AppState;

use axum::Router;

pub async fn run_server(state: AppState, host: &str, port: u16) -> error::Result<()> {
    let app = router::create_router().with_state(state);
    run_server_with_router(app, host, port).await
}

pub async fn run_server_with_router(router: Router, host: &str, port: u16) -> error::Result<()> {
    let listener = tokio::net::TcpListener::bind(format!("{}:{}", host, port)).await
        .map_err(error::RuntimeError::Io)?;
    tracing::info!("HTTP server listening on {}:{}", host, port);
    axum::serve(listener, router).await
        .map_err(|e| error::RuntimeError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
    Ok(())
}
