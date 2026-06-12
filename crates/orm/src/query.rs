use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct QueryBuilder {
    pub doctype: String,
    pub fields: Vec<String>,
    pub filters: HashMap<String, Value>,
    pub order_by: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

impl QueryBuilder {
    pub fn new(doctype: impl Into<String>) -> Self {
        Self {
            doctype: doctype.into(),
            fields: vec!["*".into()],
            ..Default::default()
        }
    }

    pub fn field(mut self, f: impl Into<String>) -> Self {
        if self.fields.len() == 1 && self.fields[0] == "*" {
            self.fields.clear();
        }
        self.fields.push(f.into());
        self
    }

    pub fn filter(mut self, key: impl Into<String>, value: Value) -> Self {
        self.filters.insert(key.into(), value);
        self
    }

    pub fn order(mut self, by: impl Into<String>) -> Self {
        self.order_by = Some(by.into());
        self
    }

    pub fn limit(mut self, n: usize) -> Self {
        self.limit = Some(n);
        self
    }

    pub fn offset(mut self, n: usize) -> Self {
        self.offset = Some(n);
        self
    }
}
