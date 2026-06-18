use crate::document::Document;
use error::Result;
use std::sync::{Arc, OnceLock};

/// Hook runner called around document lifecycle events.
#[async_trait::async_trait]
pub trait DocHookRunner: Send + Sync + 'static {
    async fn run_hook(&self, event: &str, doctype: &str, doc: &Document) -> Result<()>;
}

static HOOK_RUNNER: OnceLock<Arc<dyn DocHookRunner>> = OnceLock::new();

/// Register the global document hook runner.
///
/// This should be called once at runtime startup after all apps have been
/// loaded. Passing `None` clears the runner.
pub fn set_hook_runner(runner: Option<Arc<dyn DocHookRunner>>) {
    // OnceLock can only be set once; subsequent calls are ignored.
    if let Some(r) = runner {
        let _ = HOOK_RUNNER.set(r);
    }
}

pub(crate) async fn run_hook(event: &str, doctype: &str, doc: &Document) -> Result<()> {
    if let Some(runner) = HOOK_RUNNER.get() {
        runner.run_hook(event, doctype, doc).await?;
    }
    Ok(())
}
