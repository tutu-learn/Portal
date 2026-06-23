use crate::pool::DatabasePool;
use error::Result;

pub async fn setup_pragmas(pool: &DatabasePool) -> Result<()> {
    pool.execute_sql("PRAGMA foreign_keys = ON", vec![]).await?;
    pool.execute_sql("PRAGMA journal_mode = WAL", vec![])
        .await?;
    Ok(())
}
