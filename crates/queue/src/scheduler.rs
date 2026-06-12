use crate::job::{Job, JobStatus};
use error::Result;
use orm::DatabasePool;
use tracing::{info, warn};

#[derive(Debug, Clone)]
pub struct Scheduler;

impl Scheduler {
    pub fn new() -> Self {
        Self
    }

    pub async fn run(&self, pool: &DatabasePool) -> Result<()> {
        info!("scheduler started");
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
            if let Err(e) = self.tick(pool).await {
                warn!("scheduler tick failed: {}", e);
            }
        }
    }

    async fn tick(&self, pool: &DatabasePool) -> Result<()> {
        // TODO: read scheduler_events from hooks.py files and enqueue on frequency
        // For now, just log a heartbeat
        tracing::debug!("scheduler tick");

        // Enqueue any scheduled jobs that are due
        // This is a stub — in production, parse hooks.py scheduler_events
        // and compare against a cron-like schedule
        Ok(())
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}
