use rust_apps_core::{AppContext, AppState};
use std::collections::HashMap;
use tracing::{info, warn};

/// Runtime job executor that dispatches queued jobs to registered Rust app
/// methods and falls back to Python whitelisted methods.
pub struct RuntimeExecutor {
    app_state: AppState,
}

impl RuntimeExecutor {
    pub fn new(app_state: AppState) -> Self {
        Self { app_state }
    }
}

#[async_trait::async_trait]
impl queue::JobExecutor for RuntimeExecutor {
    async fn execute(
        &self,
        method: &str,
        kwargs: &HashMap<String, serde_json::Value>,
    ) -> error::Result<()> {
        info!("executing job method: {}", method);

        // Try Rust app API methods first.
        for app in self.app_state.rust_apps.apps() {
            for api_method in app.api_methods() {
                if api_method.name == method {
                    let ctx = AppContext::new(app.name(), self.app_state.clone());
                    let _ = (api_method.handler)(ctx, kwargs.clone()).await?;
                    return Ok(());
                }
            }
        }

        // Fall back to Python whitelisted methods.
        let kwargs_value = serde_json::Value::Object(
            kwargs
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        );
        match tokio::task::spawn_blocking({
            let method = method.to_string();
            let kwargs_value = kwargs_value.clone();
            move || kiff_core::call_method(&method, &kwargs_value)
        })
        .await
        {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => {
                warn!("python job method {} failed: {}", method, e);
                Err(e)
            }
            Err(e) => Err(error::RuntimeError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("job method {} panicked: {}", method, e),
            ))),
        }
    }
}
