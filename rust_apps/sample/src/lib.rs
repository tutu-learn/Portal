//! Sample Rust Frappe app demonstrating the `rust_apps` integration.

use axum::{extract::State, routing::get, Json, Router};
use rust_apps_core::{
    ApiMethod, AppContext, DocEvent, DocHook, DoctypeFixture, RustApp, ScheduledJob,
};
use serde_json::Value;

pub struct SampleApp;

#[async_trait::async_trait]
impl RustApp for SampleApp {
    fn name(&self) -> &'static str {
        "sample"
    }

    fn version(&self) -> &'static str {
        "0.1.0"
    }

    fn doctypes(&self) -> Vec<DoctypeFixture> {
        vec![DoctypeFixture::new(
            "Sample",
            "Rust ToDo",
            include_str!("doctypes/todo/todo.json"),
        )]
    }

    fn routes(
        &self,
        _ctx: &AppContext,
        router: Router<rust_apps_core::AppState>,
    ) -> Router<rust_apps_core::AppState> {
        router.route("/sample/health", get(health_handler))
    }

    fn api_methods(&self) -> Vec<ApiMethod> {
        vec![ApiMethod::new("sample.hello", |ctx, params| async move {
            let name = params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("world");
            Ok(Value::Object(serde_json::Map::from_iter([(
                "message".to_string(),
                Value::String(format!("Hello from {}: {}", ctx.app_name, name)),
            )])))
        })]
    }

    fn doc_hooks(&self) -> Vec<DocHook> {
        vec![DocHook::new(DocEvent::AfterInsert, "Rust ToDo", |_ctx, doc| {
            tracing::info!("Rust ToDo created: {} - {:?}", doc.name, doc.fields.get("title"));
            Ok(())
        })]
    }

    fn scheduled_jobs(&self) -> Vec<ScheduledJob> {
        vec![ScheduledJob::new("sample.ping", "*/5 * * * *", |_ctx| async move {
            tracing::info!("sample scheduled ping");
            Ok(())
        })]
    }

    async fn on_startup(&self, ctx: &AppContext) -> error::Result<()> {
        tracing::info!("sample app {} v{} starting up", ctx.app_name, self.version());
        Ok(())
    }
}

async fn health_handler(State(_state): State<rust_apps_core::AppState>) -> Json<Value> {
    Json(serde_json::json!({
        "status": "ok",
        "app": "sample"
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_metadata() {
        let app = SampleApp;
        assert_eq!(app.name(), "sample");
        assert_eq!(app.version(), "0.1.0");
        assert_eq!(app.doctypes().len(), 1);
    }
}
