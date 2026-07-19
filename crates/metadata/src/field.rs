use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Field {
    pub fieldname: String,
    pub fieldtype: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub options: Option<String>,
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub reqd: bool,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub in_list_view: bool,
    #[serde(default)]
    pub in_standard_filter: bool,
    /// Frappe permission level for this field. Fields at a permlevel higher
    /// than the user is granted are hidden/read-only.
    #[serde(default)]
    pub permlevel: i32,
}
