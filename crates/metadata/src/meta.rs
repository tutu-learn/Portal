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

        let mut doc = crate::doctype::DocType {
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

        // Load DocType-level flags from the synced metadata table.
        if let Ok(rows) = pool
            .execute_sql(
                r#"SELECT module, is_submittable, is_tree, istable, track_changes, track_seen, track_views,
                          autoname, naming_series
                   FROM "doctype" WHERE name = ?"#,
                vec![serde_json::Value::String(doctype.into())],
            )
            .await
        {
            if let Some(mut row) = rows.into_iter().next() {
                if let Some(v) = row.remove("module").and_then(|v| v.as_str().map(String::from)) {
                    doc.module = v;
                }
                doc.is_submittable = json_bool(&row, "is_submittable");
                doc.is_tree = json_bool(&row, "is_tree");
                doc.istable = json_bool(&row, "istable");
                doc.track_changes = json_bool(&row, "track_changes");
                doc.track_seen = json_bool(&row, "track_seen");
                doc.track_views = json_bool(&row, "track_views");
                doc.autoname = row.remove("autoname").and_then(|v| v.as_str().map(String::from));
                doc.naming_series = row
                    .remove("naming_series")
                    .and_then(|v| v.as_str().map(String::from));
            }
        }

        // Load fields from the synced docfield metadata table.
        if let Ok(rows) = pool
            .execute_sql(
                r#"SELECT fieldname, fieldtype, label, options, "default", reqd, read_only,
                          hidden, in_list_view, in_standard_filter, permlevel
                   FROM "docfield" WHERE parent = ? ORDER BY idx"#,
                vec![serde_json::Value::String(doctype.into())],
            )
            .await
        {
            for mut row in rows {
                doc.fields.push(crate::field::Field {
                    fieldname: row
                        .remove("fieldname")
                        .and_then(|v| v.as_str().map(String::from))
                        .unwrap_or_default(),
                    fieldtype: row
                        .remove("fieldtype")
                        .and_then(|v| v.as_str().map(String::from))
                        .unwrap_or_default(),
                    label: row.remove("label").and_then(|v| v.as_str().map(String::from)),
                    options: row.remove("options").and_then(|v| v.as_str().map(String::from)),
                    default: row
                        .remove("default")
                        .and_then(|v| v.as_str().map(String::from)),
                    reqd: json_bool(&row, "reqd"),
                    read_only: json_bool(&row, "read_only"),
                    hidden: json_bool(&row, "hidden"),
                    in_list_view: json_bool(&row, "in_list_view"),
                    in_standard_filter: json_bool(&row, "in_standard_filter"),
                    permlevel: row
                        .remove("permlevel")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0) as i32,
                });
            }
        }

        self.cache.insert(doctype.into(), doc.clone());
        Ok(doc)
    }
}

fn json_bool(row: &std::collections::HashMap<String, serde_json::Value>, key: &str) -> bool {
    row.get(key)
        .and_then(|v| v.as_i64().map(|i| i != 0))
        .or_else(|| row.get(key).and_then(|v| v.as_bool()))
        .unwrap_or(false)
}

impl Default for Meta {
    fn default() -> Self {
        Self::new()
    }
}
