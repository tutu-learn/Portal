use crate::pool::DatabasePool;
use error::Result;

pub async fn setup_extensions(pool: &DatabasePool) -> Result<()> {
    // Postgres-specific setup like pg_trgm, uuid-ossp, etc.
    // For now, just ensure the connection works.
    pool.execute_sql("SELECT 1", vec![]).await?;
    Ok(())
}
