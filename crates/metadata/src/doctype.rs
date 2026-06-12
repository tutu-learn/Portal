use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocType {
    pub name: String,
    pub module: String,
    #[serde(default)]
    pub fields: Vec<crate::field::Field>,
    #[serde(default)]
    pub permissions: Vec<HashMap<String, serde_json::Value>>,
    #[serde(default)]
    pub autoname: Option<String>,
    #[serde(default)]
    pub naming_series: Option<String>,
    #[serde(default)]
    pub is_submittable: bool,
    #[serde(default)]
    pub is_tree: bool,
    #[serde(default)]
    pub istable: bool,
    #[serde(default)]
    pub editable_grid: bool,
    #[serde(default)]
    pub track_changes: bool,
    #[serde(default)]
    pub track_seen: bool,
    #[serde(default)]
    pub track_views: bool,
}
