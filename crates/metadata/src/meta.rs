use dashmap::DashMap;
use error::Result;
use orm::DatabasePool;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct Meta {
    cache: Arc<DashMap<String, crate::doctype::DocType>>,
}

impl Meta {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(DashMap::new()),
        }
    }

    pub async fn get(&self, pool: &DatabasePool, doctype: &str) -> Result<crate::doctype::DocType> {
        if let Some(entry) = self.cache.get(doctype) {
            return Ok(entry.clone());
        }
        // TODO: load from database DocType table
        let doc = crate::doctype::DocType {
            name: doctype.into(),
            module: "Core".into(),
            fields: vec![],
            permissions: vec![],
            autoname: None,
            naming_series: None,
            is_submittable: false,
            is_tree: false,
            istable: false,
            editable_grid: false,
            track_changes: false,
            track_seen: false,
            track_views: false,
        };
        self.cache.insert(doctype.into(), doc.clone());
        Ok(doc)
    }
}

impl Default for Meta {
    fn default() -> Self {
        Self::new()
    }
}
