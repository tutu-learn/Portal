use crate::document::Document;
use error::Result;
use std::sync::{Arc, OnceLock};
use tokio::sync::Mutex;

/// Hook runner called around document lifecycle events.
#[async_trait::async_trait]
pub trait DocHookRunner: Send + Sync + 'static {
    async fn run_hook(&self, event: &str, doctype: &str, doc: &Document) -> Result<()>;
}

static PRIMARY_RUNNER: OnceLock<Arc<dyn DocHookRunner>> = OnceLock::new();
static EXTRA_RUNNERS: OnceLock<Mutex<Vec<Arc<dyn DocHookRunner>>>> = OnceLock::new();

fn extra_runners() -> &'static Mutex<Vec<Arc<dyn DocHookRunner>>> {
    EXTRA_RUNNERS.get_or_init(|| Mutex::new(Vec::new()))
}

// Note: we use tokio::sync::Mutex here because std::sync::MutexGuard is not
// Send, and holding it across an .await would make the public futures of
// DatabasePool (save_doc, insert_doc, delete_doc) !Send, breaking axum
// handlers.

/// Register the primary global document hook runner.
///
/// This should be called once at runtime startup after all apps have been
/// loaded. Subsequent calls are ignored because the underlying `OnceLock` can
/// only be set once.
pub fn set_hook_runner(runner: Option<Arc<dyn DocHookRunner>>) {
    if let Some(r) = runner {
        let _ = PRIMARY_RUNNER.set(r);
    }
}

/// Add an additional hook runner.
///
/// Extra runners are called after the primary runner. This is used by the sync
/// subsystem to capture document mutations without replacing the app-level
/// hook registry.
pub async fn add_hook_runner(runner: Arc<dyn DocHookRunner>) {
    extra_runners().lock().await.push(runner);
}

/// Clear all additional hook runners. Intended for tests.
pub async fn clear_hook_runners() {
    if let Some(runners) = EXTRA_RUNNERS.get() {
        runners.lock().await.clear();
    }
}

pub(crate) async fn run_hook(event: &str, doctype: &str, doc: &Document) -> Result<()> {
    if let Some(runner) = PRIMARY_RUNNER.get() {
        runner.run_hook(event, doctype, doc).await?;
    }
    let runners = extra_runners().lock().await;
    for runner in runners.iter() {
        runner.run_hook(event, doctype, doc).await?;
    }
    Ok(())
}
