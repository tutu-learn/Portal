use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, Mutex};
use tokio::task::spawn_blocking;

use crate::engine::LogEngine;
use crate::error::LogResult;
use crate::record::LogRecord;
use crate::trigger::Alert;

/// Async handle to a crash-durable log engine.
///
/// Internally owns a [`LogEngine`] behind a mutex and runs blocking operations
/// on Tokio's blocking thread pool.
#[derive(Clone)]
pub struct LogService {
    engine: Arc<Mutex<LogEngine>>,
}

impl LogService {
    /// Open (or create) a log engine at `dir` and wrap it for async use.
    ///
    /// Returns the service plus an async alert receiver. The receiver is fed by
    /// a background task that polls the synchronous trigger channel returned by
    /// [`LogEngine`].
    pub fn open_or_create(dir: &Path) -> LogResult<(Self, mpsc::Receiver<Alert>)> {
        let (engine, sync_alerts) = LogEngine::open_or_create(dir)?;
        let (alert_tx, alert_rx) = mpsc::channel(1_024);

        let service = Self {
            engine: Arc::new(Mutex::new(engine)),
        };

        // Bridge sync trigger alerts into the async channel.
        tokio::spawn(async move {
            loop {
                let alert = sync_alerts.recv();
                match alert {
                    Ok(a) => {
                        if alert_tx.send(a).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok((service, alert_rx))
    }

    /// Ingest a single log record.
    pub async fn ingest(&self, rec: LogRecord) -> LogResult<()> {
        let engine = self.engine.clone();
        spawn_blocking(move || {
            let mut eng = engine.blocking_lock();
            eng.ingest(rec)
        })
        .await
        .expect("log ingest task panicked")
    }

    /// Ingest multiple log records under a single lock acquisition and a
    /// single WAL flush.
    pub async fn ingest_batch(&self, recs: Vec<LogRecord>) -> LogResult<()> {
        let engine = self.engine.clone();
        spawn_blocking(move || {
            let mut eng = engine.blocking_lock();
            eng.ingest_batch(&recs)
        })
        .await
        .expect("log ingest task panicked")
    }

    /// Commit staged logs and truncate the WAL.
    pub async fn commit(&self) -> LogResult<()> {
        let engine = self.engine.clone();
        spawn_blocking(move || {
            let mut eng = engine.blocking_lock();
            eng.commit()
        })
        .await
        .expect("log commit task panicked")
    }

    /// Return the number of committed (plus staged) log records matching `q`.
    pub async fn count(&self, q: &str) -> LogResult<usize> {
        let engine = self.engine.clone();
        let q = q.to_string();
        spawn_blocking(move || {
            let eng = engine.blocking_lock();
            eng.count(&q)
        })
        .await
        .expect("log count task panicked")
    }

    /// Query committed logs.
    pub async fn query(&self, q: &str, limit: usize) -> LogResult<Vec<LogRecord>> {
        let engine = self.engine.clone();
        let q = q.to_string();
        spawn_blocking(move || {
            let eng = engine.blocking_lock();
            eng.query(&q, limit)
        })
        .await
        .expect("log query task panicked")
    }

    /// Register a trigger.
    pub async fn add_trigger<F>(&self, name: &str, predicate: F)
    where
        F: Fn(&LogRecord) -> bool + Send + Sync + 'static,
    {
        let engine = self.engine.clone();
        let name = name.to_string();
        spawn_blocking(move || {
            let mut eng = engine.blocking_lock();
            eng.add_trigger(&name, predicate);
        })
        .await
        .expect("add trigger task panicked");
    }

    /// Spawn a background task that commits at the given interval.
    pub fn spawn_commit_loop(&self, interval: Duration) {
        let svc = self.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                ticker.tick().await;
                if let Err(e) = svc.commit().await {
                    tracing::error!("log engine auto-commit failed: {}", e);
                }
            }
        });
    }

    /// Delete committed records older than `max_age`.
    pub async fn prune_older_than(&self, max_age: Duration) -> LogResult<usize> {
        let engine = self.engine.clone();
        spawn_blocking(move || {
            let mut eng = engine.blocking_lock();
            eng.prune_older_than(max_age)
        })
        .await
        .expect("log prune task panicked")
    }

    /// Delete all committed records for a single service.
    pub async fn prune_service(&self, service: &str) -> LogResult<usize> {
        let engine = self.engine.clone();
        let service = service.to_string();
        spawn_blocking(move || {
            let mut eng = engine.blocking_lock();
            eng.prune_service(&service)
        })
        .await
        .expect("log prune task panicked")
    }

    /// Spawn a background task that prunes old records at `interval`.
    pub fn spawn_retention_loop(&self, interval: Duration, max_age: Duration) {
        let svc = self.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                ticker.tick().await;
                match svc.prune_older_than(max_age).await {
                    Ok(0) => {}
                    Ok(n) => tracing::info!("log engine retention pruned {} old record(s)", n),
                    Err(e) => tracing::error!("log engine retention failed: {}", e),
                }
            }
        });
    }
}
