use error::Result;

/// Placeholder rewrite is a no-op for SQLite. All placeholders stay as `?`.
pub fn rewrite(sql: &str) -> Result<String> {
    Ok(sql.to_string())
}
