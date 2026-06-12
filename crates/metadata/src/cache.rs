use dashmap::DashMap;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct MetaCache {
    inner: Arc<DashMap<String, serde_json::Value>>,
}

impl MetaCache {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(DashMap::new()),
        }
    }

    pub fn get(&self, key: &str) -> Option<serde_json::Value> {
        self.inner.get(key).map(|v| v.clone())
    }

    pub fn set(&self, key: String, value: serde_json::Value) {
        self.inner.insert(key, value);
    }

    pub fn invalidate(&self, key: &str) {
        self.inner.remove(key);
    }
}

impl Default for MetaCache {
    fn default() -> Self {
        Self::new()
    }
}
