use error::Result;
use tracing::info;

#[derive(Debug, Clone)]
pub struct Scheduler;

impl Scheduler {
    pub fn new() -> Self {
        Self
    }

    /// Run the scheduler loop. Takes no pool: the tick is a stub that never
    /// touches the DB, and holding a long-lived pool clone here would keep a
    /// wedged pool's file descriptors open after the runtime watchdog swaps
    /// it, which on macOS prevents the replacement pool from connecting.
    pub async fn run(&self) -> Result<()> {
        info!("scheduler started");
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
            self.tick().await;
        }
    }

    async fn tick(&self) {
        // TODO: read scheduler_events from hooks.py files and enqueue on frequency
        // For now, just log a heartbeat
        tracing::debug!("scheduler tick");

        // Enqueue any scheduled jobs that are due
        // This is a stub — in production, parse hooks.py scheduler_events
        // and compare against a cron-like schedule
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}
