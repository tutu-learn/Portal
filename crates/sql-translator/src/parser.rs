use error::{Result, RuntimeError};
use sqlparser::ast::Statement;
use sqlparser::dialect::MySqlDialect;
use sqlparser::parser::Parser;

pub fn parse(sql: &str) -> Result<Vec<Statement>> {
    // Pre-process: replace MySQL %s placeholders with ? so sqlparser can handle them
    let preprocessed = sql.replace("%s", "?");
    let dialect = MySqlDialect {};
    Parser::parse_sql(&dialect, &preprocessed)
        .map_err(|e| RuntimeError::SqlTranslation(format!("parse error: {}", e)))
}
