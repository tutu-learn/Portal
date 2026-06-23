//! Kiff Logger — Frappe-facing logging app.
//!
//! Provides DocTypes and workspaces for viewing system logs and configuring
//! log backups. The actual logging engine and reusable hooks live in
//! `rust_apps_core` and are initialized by the runtime.

use axum::{routing::get, Router};
use rust_apps_core::{ApiMethod, AppContext, DoctypeFixture, RustApp, WorkspaceFixture};

mod handlers;
mod methods;

pub use handlers::{IngestRequest, IngestResponse, QueryResponse};

pub struct KiffLoggerApp;

#[async_trait::async_trait]
impl RustApp for KiffLoggerApp {
    fn name(&self) -> &'static str {
        "kiff_logger"
    }

    fn version(&self) -> &'static str {
        "0.1.0"
    }

    fn doctypes(&self) -> Vec<DoctypeFixture> {
        vec![
            DoctypeFixture::new(
                "Kiff Logger",
                "Kiff Log Entry",
                include_str!("doctypes/kiff_logger/kiff_log_entry.json"),
            ),
            DoctypeFixture::new(
                "Kiff Logger",
                "Kiff Log Query",
                include_str!("doctypes/kiff_logger/kiff_log_query.json"),
            ),
            DoctypeFixture::new(
                "Kiff Logger",
                "S3 Backup Configuration",
                include_str!("doctypes/kiff_logger/s3_backup_config.json"),
            ),
        ]
    }

    fn workspaces(&self) -> Vec<WorkspaceFixture> {
        vec![WorkspaceFixture::new(
            "Kiff Logger",
            include_str!("workspaces/kiff_logger/kiff_logger.json"),
        )]
    }

    fn routes(
        &self,
        _ctx: &AppContext,
        router: Router<rust_apps_core::AppState>,
    ) -> Router<rust_apps_core::AppState> {
        router
            .route("/kiff_logger/ingest", axum::routing::post(handlers::ingest_handler))
            .route("/kiff_logger/query", get(handlers::query_handler))
    }

    fn api_methods(&self) -> Vec<ApiMethod> {
        vec![
            ApiMethod::new("kiff_logger.ingest", methods::ingest_method),
            ApiMethod::new("kiff_logger.query", methods::query_method),
        ]
    }
}
