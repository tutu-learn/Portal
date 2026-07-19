use async_trait::async_trait;
use chrono::{DateTime, Timelike, Utc};
use error::Result;
use orm::DatabasePool;
use std::collections::HashMap;
use std::str::FromStr;
use tracing::{info, warn};

/// A schedule trigger known to the scheduler. The backend decides what `id`
/// means and what to do when it fires.
#[derive(Debug, Clone)]
pub struct ScheduleTrigger {
    pub id: String,
    /// Cron expression (e.g. "0 9 * * 1") or a Frappe frequency alias
    /// ("hourly", "daily", "weekly", "monthly", "yearly", "all").
    pub cron: String,
}

/// Backend supplied by the runtime. It enumerates triggers and handles them
/// when due.
#[async_trait]
pub trait SchedulerBackend: Send + Sync {
    async fn triggers(&self) -> Result<Vec<ScheduleTrigger>>;
    async fn fire(&self, trigger_id: &str) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct Scheduler;

impl Scheduler {
    pub fn new() -> Self {
        Self
    }

    /// Run the scheduler loop. The backend provides triggers and executes them
    /// when due. `get_pool` is available for backends that need to persist
    /// scheduler state; the scheduler itself no longer holds a long-lived pool
    /// clone so it does not interfere with the runtime pool watchdog.
    pub async fn run<B, F>(&self, backend: &B, _get_pool: F) -> Result<()>
    where
        B: SchedulerBackend,
        F: Fn(&str) -> Option<DatabasePool>,
    {
        info!("scheduler started");
        let mut last_runs: HashMap<String, DateTime<Utc>> = HashMap::new();
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
            let now = Utc::now();
            match backend.triggers().await {
                Ok(triggers) => {
                    for trigger in triggers {
                        let last = last_runs.get(&trigger.id).copied();
                        if is_due(&trigger.cron, now, last) {
                            if let Err(e) = backend.fire(&trigger.id).await {
                                warn!("scheduler trigger {} failed: {}", trigger.id, e);
                            } else {
                                last_runs.insert(trigger.id.clone(), now);
                                info!("scheduler fired {}", trigger.id);
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!("failed to load scheduler triggers: {}", e);
                }
            }
        }
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

/// Map Frappe frequency aliases and 5-field cron expressions to the 7-field
/// format expected by the `cron` crate (sec min hour dom mon dow year).
fn normalize_cron(cron: &str) -> String {
    match cron.trim().to_lowercase().as_str() {
        "hourly" => "0 0 * * * * *".into(),
        "daily" => "0 0 0 * * * *".into(),
        "weekly" => "0 0 0 * * 1 *".into(),
        "monthly" => "0 0 0 1 * * *".into(),
        "yearly" => "0 0 0 1 1 * *".into(),
        "all" => "0 * * * * * *".into(),
        other => {
            let parts: Vec<&str> = other.split_whitespace().collect();
            match parts.len() {
                5 => format!("0 {} *", other),
                6 => format!("0 {}", other),
                7 => other.into(),
                _ => other.into(),
            }
        }
    }
}

/// Find the most recent scheduled minute at or before `now`.
fn previous_scheduled_minute(
    schedule: &cron::Schedule,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let mut t = now
        .with_second(0)?
        .with_nanosecond(0)?;
    // Walk back up to 24 hours to find the previous scheduled minute.
    for _ in 0..1440 {
        if schedule.includes(t) {
            return Some(t);
        }
        t -= chrono::Duration::minutes(1);
    }
    None
}

/// Determine whether a trigger is due now, given its last known run time.
///
/// We find the previous scheduled minute. If that minute is after the last
/// run (or there is no last run), the trigger should fire.
fn is_due(cron: &str, now: DateTime<Utc>, last_run: Option<DateTime<Utc>>) -> bool {
    let normalized = normalize_cron(cron);
    let schedule = match cron::Schedule::from_str(&normalized) {
        Ok(s) => s,
        Err(e) => {
            warn!("invalid cron expression '{}': {}", cron, e);
            return false;
        }
    };

    let Some(scheduled) = previous_scheduled_minute(&schedule, now) else {
        return false;
    };

    match last_run {
        Some(last) => scheduled > last,
        None => now.signed_duration_since(scheduled) <= chrono::Duration::minutes(1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hourly_is_due_after_hour_passes() {
        let now = Utc::now();
        let last_run = now - chrono::Duration::hours(2);
        assert!(is_due("hourly", now, Some(last_run)));
    }

    #[test]
    fn hourly_is_not_due_within_same_hour() {
        let now = Utc::now();
        // Last run was 5 minutes into the current hour, after the scheduled
        // hour mark, so the trigger should not fire again.
        let last_run = now
            .with_minute(5)
            .unwrap()
            .with_second(0)
            .unwrap()
            .with_nanosecond(0)
            .unwrap();
        assert!(!is_due("hourly", now, Some(last_run)));
    }

    #[test]
    fn normalize_frequency_aliases() {
        assert_eq!(normalize_cron("hourly"), "0 0 * * * * *");
        assert_eq!(normalize_cron("daily"), "0 0 0 * * * *");
        assert_eq!(normalize_cron("weekly"), "0 0 0 * * 1 *");
        assert_eq!(normalize_cron("*/5 * * * *"), "0 */5 * * * * *");
    }
}
