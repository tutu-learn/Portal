use crate::hooks::HookRegistry;
use queue::{ScheduleTrigger, SchedulerBackend};
use rust_apps_core::{AppContext, AppState};
use tracing::{info, warn};

/// Maps Frappe scheduler frequency aliases to cron expressions understood by
/// the `cron` crate (7-field format: sec min hour dom mon dow year).
fn frequency_cron(freq: &str) -> &str {
    match freq {
        "hourly" => "0 0 * * * * *",
        "daily" => "0 0 0 * * * *",
        "weekly" => "0 0 0 * * 1 *",
        "monthly" => "0 0 0 1 * * *",
        "yearly" => "0 0 0 1 1 * *",
        "all" => "0 * * * * * *",
        other => other,
    }
}

/// Runtime scheduler backend that fires Rust app scheduled jobs and Python
/// scheduler hooks registered in `hooks.py`.
pub struct RuntimeSchedulerBackend {
    app_state: AppState,
    hook_registry: HookRegistry,
}

impl RuntimeSchedulerBackend {
    pub fn new(app_state: AppState, hook_registry: HookRegistry) -> Self {
        Self {
            app_state,
            hook_registry,
        }
    }
}

#[async_trait::async_trait]
impl SchedulerBackend for RuntimeSchedulerBackend {
    async fn triggers(&self) -> error::Result<Vec<ScheduleTrigger>> {
        let mut triggers = Vec::new();

        // Rust app scheduled jobs.
        for app in self.app_state.rust_apps.apps() {
            for job in app.scheduled_jobs() {
                triggers.push(ScheduleTrigger {
                    id: job.name.to_string(),
                    cron: job.cron.to_string(),
                });
            }
        }

        // Python scheduler hooks from Frappe apps.
        // `hooks` stores keys as "scheduler_events:<frequency>".
        for key in self.hook_registry.hooks.keys() {
            if let Some(freq) = key.strip_prefix("scheduler_events:") {
                triggers.push(ScheduleTrigger {
                    id: format!("py:scheduler_events:{}", freq),
                    cron: frequency_cron(freq).to_string(),
                });
            }
        }

        Ok(triggers)
    }

    async fn fire(&self, trigger_id: &str) -> error::Result<()> {
        info!("firing scheduler trigger: {}", trigger_id);

        if let Some(freq) = trigger_id.strip_prefix("py:scheduler_events:") {
            let hooks = self.hook_registry.get_hooks("scheduler_events", Some(freq));
            if hooks.is_empty() {
                return Ok(());
            }
            return self.hook_registry.run_hook("scheduler_events", Some(freq), None).await;
        }

        // Rust scheduled job.
        for app in self.app_state.rust_apps.apps() {
            for job in app.scheduled_jobs() {
                if job.name == trigger_id {
                    let ctx = AppContext::new(app.name(), self.app_state.clone());
                    return (job.handler)(&ctx).await;
                }
            }
        }

        warn!("no scheduled job found for trigger: {}", trigger_id);
        Ok(())
    }
}
