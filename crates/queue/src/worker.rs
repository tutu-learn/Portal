use crate::job::{Job, JobStatus};
use error::Result;
use orm::DatabasePool;
use tracing::{error, info, warn};

#[derive(Debug, Clone)]
pub struct Worker {
    pub queue: String,
}

impl Worker {
    pub fn new(queue: impl Into<String>) -> Self {
        Self { queue: queue.into() }
    }

    pub async fn run(&self, pool: &DatabasePool) -> Result<()> {
        info!("worker started for queue: {}", self.queue);
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
            match self.dequeue(pool).await {
                Ok(Some(job)) => {
                    if let Err(e) = self.execute(&job, pool).await {
                        error!("job {} failed: {}", job.id, e);
                        let _ = self.mark_failed(pool, &job.id, &format!("{}", e)).await;
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    warn!("dequeue error: {}", e);
                }
            }
        }
    }

    async fn dequeue(&self, pool: &DatabasePool) -> Result<Option<Job>> {
        // Use a transaction to atomically claim a job
        let mut tx = pool.begin().await?;

        let sql = match pool.dialect() {
            "postgres" => r#"
                SELECT id, method, queue, kwargs, status, site, created_at, updated_at
                FROM __kiff_queue
                WHERE queue = $1 AND status = 'queued'
                ORDER BY created_at
                LIMIT 1
                FOR UPDATE SKIP LOCKED
            "#,
            _ => r#"
                SELECT id, method, queue, kwargs, status, site, created_at, updated_at
                FROM __kiff_queue
                WHERE queue = ? AND status = 'queued'
                ORDER BY created_at
                LIMIT 1
            "#,
        };

        let rows = tx.execute_sql(sql, vec![
            serde_json::Value::String(self.queue.clone()),
        ]).await?;

        let row = match rows.into_iter().next() {
            Some(r) => r,
            None => {
                tx.rollback().await?;
                return Ok(None);
            }
        };

        let job = row_to_job(row)?;

        // Mark as running
        let update_sql = match pool.dialect() {
            "postgres" => r#"
                UPDATE __kiff_queue
                SET status = 'running', updated_at = CURRENT_TIMESTAMP
                WHERE id = $1
            "#,
            _ => r#"
                UPDATE __kiff_queue
                SET status = 'running', updated_at = datetime('now')
                WHERE id = ?
            "#,
        };
        tx.execute_sql(update_sql, vec![
            serde_json::Value::String(job.id.clone()),
        ]).await?;

        tx.commit().await?;
        Ok(Some(job))
    }

    async fn execute(&self, job: &Job, pool: &DatabasePool) -> Result<()> {
        info!("executing job {}: {}", job.id, job.method);
        let parts: Vec<&str> = job.method.rsplitn(2, '.').collect();
        if parts.len() != 2 {
            return Err(error::RuntimeError::Validation(format!("invalid method: {}", job.method)));
        }
        let func = parts[0];
        let module = parts[1];

        // TODO: call via PyO3 — for now, log the call
        // The runtime should pass an executor closure to Worker::run
        // that handles the actual Python invocation.
        info!("would call {}.{}({:?})", module, func, job.kwargs);
        let result: error::Result<()> = Ok(());

        match result {
            Ok(()) => self.mark_completed(pool, &job.id).await?,
            Err(e) => self.mark_failed(pool, &job.id, &format!("{}", e)).await?,
        }

        Ok(())
    }

    async fn mark_completed(&self, pool: &DatabasePool, job_id: &str) -> Result<()> {
        let sql = match pool.dialect() {
            "postgres" => r#"
                UPDATE __kiff_queue
                SET status = 'completed', updated_at = CURRENT_TIMESTAMP
                WHERE id = $1
            "#,
            _ => r#"
                UPDATE __kiff_queue
                SET status = 'completed', updated_at = datetime('now')
                WHERE id = ?
            "#,
        };
        pool.execute_sql(sql, vec![serde_json::Value::String(job_id.into())]).await?;
        Ok(())
    }

    async fn mark_failed(&self, pool: &DatabasePool, job_id: &str, error_msg: &str) -> Result<()> {
        let sql = match pool.dialect() {
            "postgres" => r#"
                UPDATE __kiff_queue
                SET status = 'failed', updated_at = CURRENT_TIMESTAMP, error = $2
                WHERE id = $1
            "#,
            _ => r#"
                UPDATE __kiff_queue
                SET status = 'failed', updated_at = datetime('now'), error = ?
                WHERE id = ?
            "#,
        };
        let params = if pool.dialect() == "postgres" {
            vec![
                serde_json::Value::String(job_id.into()),
                serde_json::Value::String(error_msg.into()),
            ]
        } else {
            vec![
                serde_json::Value::String(error_msg.into()),
                serde_json::Value::String(job_id.into()),
            ]
        };
        pool.execute_sql(sql, params).await?;
        Ok(())
    }
}

fn row_to_job(mut row: std::collections::HashMap<String, serde_json::Value>) -> Result<Job> {
    let kwargs_json = row.remove("kwargs")
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| "{}".into());
    let kwargs: std::collections::HashMap<String, serde_json::Value> = serde_json::from_str(&kwargs_json)
        .unwrap_or_default();

    Ok(Job {
        id: row.remove("id").and_then(|v| v.as_str().map(String::from)).unwrap_or_default(),
        method: row.remove("method").and_then(|v| v.as_str().map(String::from)).unwrap_or_default(),
        queue: row.remove("queue").and_then(|v| v.as_str().map(String::from)).unwrap_or_default(),
        kwargs,
        status: match row.remove("status").and_then(|v| v.as_str().map(String::from)).as_deref() {
            Some("running") => JobStatus::Running,
            Some("completed") => JobStatus::Completed,
            Some("failed") => JobStatus::Failed,
            _ => JobStatus::Queued,
        },
        site: row.remove("site").and_then(|v| v.as_str().map(String::from)).unwrap_or_default(),
        created_at: row.remove("created_at")
            .and_then(|v| v.as_str().and_then(|s| s.parse().ok()))
            .unwrap_or_else(chrono::Utc::now),
        updated_at: row.remove("updated_at")
            .and_then(|v| v.as_str().and_then(|s| s.parse().ok()))
            .unwrap_or_else(chrono::Utc::now),
    })
}
