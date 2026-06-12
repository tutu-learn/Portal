use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Queued,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub method: String,
    pub queue: String,
    #[serde(default)]
    pub kwargs: HashMap<String, serde_json::Value>,
    pub status: JobStatus,
    pub site: String,
    #[serde(default = "Utc::now")]
    pub created_at: DateTime<Utc>,
    #[serde(default = "Utc::now")]
    pub updated_at: DateTime<Utc>,
}

impl Job {
    pub fn new(method: String, queue: String, site: String) -> Self {
        let now = Utc::now();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            method,
            queue,
            kwargs: HashMap::new(),
            status: JobStatus::Queued,
            site,
            created_at: now,
            updated_at: now,
        }
    }
}
