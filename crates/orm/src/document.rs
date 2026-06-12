use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub doctype: String,
    pub name: String,
    pub owner: String,
    #[serde(default = "default_dt")]
    pub creation: DateTime<Utc>,
    #[serde(default = "default_dt")]
    pub modified: DateTime<Utc>,
    #[serde(default)]
    pub docstatus: i32,
    #[serde(flatten)]
    pub fields: HashMap<String, serde_json::Value>,
}

fn default_dt() -> DateTime<Utc> {
    Utc::now()
}

impl Document {
    pub fn new(doctype: impl Into<String>, name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            doctype: doctype.into(),
            name: name.into(),
            owner: "Administrator".into(),
            creation: now,
            modified: now,
            docstatus: 0,
            fields: HashMap::new(),
        }
    }

    pub fn get_field(&self, key: &str) -> Option<&serde_json::Value> {
        self.fields.get(key)
    }

    pub fn set_field(&mut self, key: impl Into<String>, value: impl Into<serde_json::Value>) {
        self.fields.insert(key.into(), value.into());
    }
}
