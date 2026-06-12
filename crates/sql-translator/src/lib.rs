pub mod functions;
pub mod parser;
pub mod placeholders;
pub mod rewriter;
pub mod tables;

use error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetDialect {
    Postgres,
    Sqlite,
}

#[derive(Debug, Clone)]
pub struct SqlTranslator {
    pub target: TargetDialect,
}

impl SqlTranslator {
    pub fn new(target: TargetDialect) -> Self {
        Self { target }
    }

    pub fn translate(&self, sql: &str) -> Result<String> {
        let mut parsed = parser::parse(sql)?;
        rewriter::rewrite(&mut parsed, self.target)?;
        let output = parsed.into_iter().map(|s| s.to_string()).collect::<Vec<_>>().join("; ");
        let output = placeholders::rewrite(&output, self.target)?;
        Ok(output)
    }
}

impl Default for SqlTranslator {
    fn default() -> Self {
        Self::new(TargetDialect::Sqlite)
    }
}
