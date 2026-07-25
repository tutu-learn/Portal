use rust_apps_core::AppState;
use std::collections::HashMap;

/// Queue job executor that dispatches to Rust app methods first, then falls
/// back to whitelisted Python methods.
pub struct RuntimeExecutor {
    state: AppState,
}

impl RuntimeExecutor {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    /// Determine the user under which the job should run. Frappe enqueued jobs
    /// commonly pass the requesting user in kwargs; background jobs default to
    /// Administrator.
    fn job_user(kwargs: &HashMap<String, serde_json::Value>) -> String {
        kwargs
            .get("user")
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_else(|| "Administrator".into())
    }
}

#[async_trait::async_trait]
impl queue::JobExecutor for RuntimeExecutor {
    async fn execute(
        &self,
        method: &str,
        kwargs: &HashMap<String, serde_json::Value>,
    ) -> error::Result<()> {
        let user = Self::job_user(kwargs);

        // Try Rust app API methods first.
        if let Some(result) = self
            .state
            .rust_apps
            .call_method(method, self.state.clone(), kwargs.clone(), Some(user.clone()))
            .await?
        {
            tracing::info!(method = %method, result = %result, "Rust job executed");
            return Ok(());
        }

        // Fall back to Python whitelisted methods.
        let kwargs_json = serde_json::to_value(kwargs).unwrap_or_default();
        kiff_core::call_method_with_user(method, &kwargs_json, Some(&user))?;
        Ok(())
    }
}
