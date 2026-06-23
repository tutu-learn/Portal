use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// A single log record.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LogRecord {
    /// Unix timestamp in milliseconds.
    pub timestamp: i64,
    /// Log level, e.g. INFO / WARN / ERROR.
    pub level: String,
    /// Originating service or component.
    pub service: String,
    /// Free-text log message.
    pub message: String,
    /// Optional structured fields.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub fields: HashMap<String, serde_json::Value>,
}

impl LogRecord {
    pub fn new(level: &str, service: &str, message: &str) -> Self {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        Self {
            timestamp: ts,
            level: level.into(),
            service: service.into(),
            message: message.into(),
            fields: HashMap::new(),
        }
    }

    pub fn with_field<K: Into<String>, V: Into<serde_json::Value>>(
        mut self,
        key: K,
        value: V,
    ) -> Self {
        self.fields.insert(key.into(), value.into());
        self
    }
}
