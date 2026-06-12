use error::{Result, RuntimeError};
use crate::TargetDialect;

pub fn rewrite(sql: &str, target: TargetDialect) -> Result<String> {
    match target {
        TargetDialect::Postgres => rewrite_postgres(sql),
        TargetDialect::Sqlite => Ok(sql.to_string()),
    }
}

fn rewrite_postgres(sql: &str) -> Result<String> {
    let mut result = String::with_capacity(sql.len());
    let mut chars = sql.chars().peekable();
    let mut idx = 1usize;

    while let Some(ch) = chars.next() {
        if ch == '?' {
            // Check if it's inside a string literal
            // Simple heuristic: count single quotes so far
            let quote_count = result.chars().filter(|&c| c == '\'').count();
            if quote_count % 2 == 0 {
                result.push_str(&format!("${}", idx));
                idx += 1;
                continue;
            }
        }
        result.push(ch);
    }

    Ok(result)
}
