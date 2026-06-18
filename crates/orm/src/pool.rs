use crate::document::Document;
use crate::filters::FilterCondition;
use chrono::Utc;
use error::{Result, RuntimeError};
use serde_json::Value;
use sqlx::{Column, Row, TypeInfo};
use std::collections::HashMap;
use tracing::{debug, warn};

#[derive(Debug, Clone)]
pub enum DatabasePool {
    Postgres(sqlx::PgPool),
    Sqlite(sqlx::SqlitePool),
}

impl DatabasePool {
    pub async fn connect_sqlite(url: &str) -> Result<Self> {
        let opts = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(url)
            .create_if_missing(true);
        let pool = sqlx::SqlitePool::connect_with(opts).await?;
        Ok(DatabasePool::Sqlite(pool))
    }

    pub async fn connect_postgres(url: &str) -> Result<Self> {
        let pool = sqlx::PgPool::connect(url).await?;
        Ok(DatabasePool::Postgres(pool))
    }

    pub fn dialect(&self) -> &'static str {
        match self {
            DatabasePool::Postgres(_) => "postgres",
            DatabasePool::Sqlite(_) => "sqlite",
        }
    }

    pub fn placeholder(&self, idx: usize) -> String {
        match self {
            DatabasePool::Postgres(_) => format!("${}", idx),
            DatabasePool::Sqlite(_) => "?".to_string(),
        }
    }

    fn table_name(&self, doctype: &str) -> String {
        let name = doctype.to_lowercase().replace(" ", "_");
        name.strip_prefix("tab").unwrap_or(&name).to_string()
    }

    pub async fn get_doc(&self, doctype: &str, name: &str) -> Result<Document> {
        let table = self.table_name(doctype);
        let sql = format!("SELECT * FROM \"{}\" WHERE name = {}", table, self.placeholder(1));
        debug!("get_doc sql: {}", sql);

        let rows = match self.query_raw(&sql, vec![Value::String(name.into())]).await {
            Ok(r) => r,
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("no such table") || msg.contains("does not exist") {
                    return Err(RuntimeError::NotFound(format!("{} {}", doctype, name)));
                }
                return Err(e);
            }
        };
        let mut m = rows.into_iter().next()
            .ok_or_else(|| RuntimeError::NotFound(format!("{} {}", doctype, name)))?;
        m.insert("doctype".into(), Value::String(doctype.into()));
        m.insert("name".into(), Value::String(name.into()));
        Document::from_map(m)
    }

    pub async fn get_list(
        &self,
        doctype: &str,
        filters: Option<HashMap<String, FilterCondition>>,
        fields: Option<Vec<String>>,
        order_by: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<Document>> {
        let table = self.table_name(doctype);
        let cols = match fields {
            Some(f) if !f.is_empty() => f.join(", "),
            _ => "*".to_string(),
        };

        let mut sql = format!("SELECT {} FROM \"{}\"", cols, table);
        let mut params: Vec<Value> = Vec::new();

        if let Some(filts) = filters {
            if !filts.is_empty() {
                let mut conditions = Vec::new();
                for (k, cond) in filts {
                    let dialect = self.dialect();
                    let base = params.len();
                    let mut offset = 0usize;
                    let (frag, vals) = cond.to_sql(&k, move || {
                        let ph = if dialect == "postgres" {
                            format!("${}", base + offset + 1)
                        } else {
                            "?".to_string()
                        };
                        offset += 1;
                        ph
                    });
                    conditions.push(frag);
                    params.extend(vals);
                }
                sql.push_str(&format!(" WHERE {}", conditions.join(" AND ")));
            }
        }

        if let Some(ob) = order_by {
            sql.push_str(&format!(" ORDER BY {}", ob));
        }

        if let Some(lim) = limit {
            sql.push_str(&format!(" LIMIT {}", lim));
        }

        debug!("get_list sql: {}", sql);
        let rows = match self.query_raw(&sql, params).await {
            Ok(r) => r,
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("no such table") || msg.contains("does not exist") {
                    warn!(doctype = %doctype, table = %table, "table missing in DB, returning empty list");
                    return Ok(vec![]);
                }
                return Err(e);
            }
        };
        let docs: Result<Vec<Document>> = rows.into_iter().map(|mut m| {
            m.insert("doctype".into(), Value::String(doctype.into()));
            Document::from_map(m)
        }).collect();
        docs
    }

    pub async fn save_doc(&self, doc: &Document) -> Result<()> {
        crate::hooks::run_hook("before_save", &doc.doctype, doc).await?;

        let table = self.table_name(&doc.doctype);
        let mut sets = Vec::new();
        let mut params: Vec<Value> = Vec::new();

        for (k, v) in &doc.fields {
            sets.push(format!("{} = {}", k, self.placeholder(params.len() + 1)));
            params.push(v.clone());
        }
        sets.push(format!("modified = {}", self.placeholder(params.len() + 1)));
        params.push(Value::String(doc.modified.to_rfc3339()));

        let sql = format!(
            "UPDATE \"{}\" SET {} WHERE name = {}",
            table,
            sets.join(", "),
            self.placeholder(params.len() + 1)
        );
        params.push(Value::String(doc.name.clone()));

        debug!("save_doc sql: {}", sql);
        self.execute_raw(&sql, params).await?;

        crate::hooks::run_hook("on_update", &doc.doctype, doc).await?;
        Ok(())
    }

    pub async fn insert_doc(&self, doc: &Document) -> Result<String> {
        crate::hooks::run_hook("before_insert", &doc.doctype, doc).await?;

        let table = self.table_name(&doc.doctype);
        let mut cols = vec!["name".to_string(), "owner".to_string(), "creation".to_string(), "modified".to_string(), "docstatus".to_string()];
        let mut params: Vec<Value> = vec![
            Value::String(doc.name.clone()),
            Value::String(doc.owner.clone()),
            Value::String(doc.creation.to_rfc3339()),
            Value::String(doc.modified.to_rfc3339()),
            Value::Number(serde_json::Number::from(doc.docstatus)),
        ];

        for (k, v) in &doc.fields {
            cols.push(k.clone());
            params.push(v.clone());
        }

        let placeholders: Vec<String> = (1..=params.len())
            .map(|i| self.placeholder(i))
            .collect();

        let sql = format!(
            "INSERT INTO \"{}\" ({}) VALUES ({})",
            table,
            cols.join(", "),
            placeholders.join(", ")
        );

        debug!("insert_doc sql: {}", sql);
        self.execute_raw(&sql, params).await?;

        crate::hooks::run_hook("after_insert", &doc.doctype, doc).await?;
        Ok(doc.name.clone())
    }

    pub async fn delete_doc(&self, doctype: &str, name: &str) -> Result<()> {
        let stub_doc = Document::new(doctype, name);
        crate::hooks::run_hook("before_trash", doctype, &stub_doc).await?;

        let table = self.table_name(doctype);
        let sql = format!("DELETE FROM \"{}\" WHERE name = {}", table, self.placeholder(1));
        self.execute_raw(&sql, vec![Value::String(name.into())]).await?;

        crate::hooks::run_hook("after_trash", doctype, &stub_doc).await?;
        Ok(())
    }

    pub async fn exists(&self, doctype: &str, name: &str) -> Result<bool> {
        let table = self.table_name(doctype);
        let sql = format!(
            "SELECT 1 FROM \"{}\" WHERE name = {} LIMIT 1",
            table,
            self.placeholder(1)
        );
        let rows = self.query_raw(&sql, vec![Value::String(name.into())]).await?;
        Ok(!rows.is_empty())
    }

    pub async fn execute_sql(
        &self,
        sql: &str,
        params: Vec<Value>,
    ) -> Result<Vec<HashMap<String, Value>>> {
        self.query_raw(sql, params).await
    }

    pub async fn commit(&self) -> Result<()> {
        // SQLite operates in auto-commit mode unless an explicit transaction
        // was begun via DatabasePool::begin().  Issuing COMMIT without BEGIN
        // raises "cannot commit - no transaction is active", so ignore it for
        // SQLite; writes through execute_sql are already persisted.
        match self {
            DatabasePool::Sqlite(_) => Ok(()),
            DatabasePool::Postgres(_) => {
                self.execute_raw("COMMIT", vec![]).await?;
                Ok(())
            }
        }
    }

    pub async fn rollback(&self) -> Result<()> {
        match self {
            DatabasePool::Sqlite(_) => Ok(()),
            DatabasePool::Postgres(_) => {
                self.execute_raw("ROLLBACK", vec![]).await?;
                Ok(())
            }
        }
    }

    pub async fn begin(&self) -> Result<Transaction<'_>> {
        match self {
            DatabasePool::Postgres(pool) => {
                let tx = pool.begin().await?;
                Ok(Transaction::Postgres(tx))
            }
            DatabasePool::Sqlite(pool) => {
                let tx = pool.begin().await?;
                Ok(Transaction::Sqlite(tx))
            }
        }
    }

    async fn query_raw(
        &self,
        sql: &str,
        params: Vec<Value>,
    ) -> Result<Vec<HashMap<String, Value>>> {
        match self {
            DatabasePool::Postgres(pool) => {
                let mut query = sqlx::query(sql);
                for p in &params {
                    query = bind_postgres(query, p);
                }
                let rows = query.fetch_all(pool).await?;
                Ok(rows.into_iter().map(row_to_map_postgres).collect())
            }
            DatabasePool::Sqlite(pool) => {
                let mut query = sqlx::query(sql);
                for p in &params {
                    query = bind_sqlite(query, p);
                }
                let rows = query.fetch_all(pool).await?;
                Ok(rows.into_iter().map(row_to_map_sqlite).collect())
            }
        }
    }

    async fn execute_raw(&self, sql: &str, params: Vec<Value>) -> Result<()> {
        match self {
            DatabasePool::Postgres(pool) => {
                let mut query = sqlx::query(sql);
                for p in &params {
                    query = bind_postgres(query, p);
                }
                query.execute(pool).await?;
                Ok(())
            }
            DatabasePool::Sqlite(pool) => {
                let mut query = sqlx::query(sql);
                for p in &params {
                    query = bind_sqlite(query, p);
                }
                query.execute(pool).await?;
                Ok(())
            }
        }
    }
}

pub enum Transaction<'a> {
    Postgres(sqlx::Transaction<'a, sqlx::Postgres>),
    Sqlite(sqlx::Transaction<'a, sqlx::Sqlite>),
}

impl<'a> Transaction<'a> {
    pub async fn execute_sql(
        &mut self,
        sql: &str,
        params: Vec<Value>,
    ) -> Result<Vec<HashMap<String, Value>>> {
        match self {
            Transaction::Postgres(tx) => {
                let mut query = sqlx::query(sql);
                for p in &params {
                    query = bind_postgres(query, p);
                }
                let rows = query.fetch_all(&mut **tx).await?;
                Ok(rows.into_iter().map(row_to_map_postgres).collect())
            }
            Transaction::Sqlite(tx) => {
                let mut query = sqlx::query(sql);
                for p in &params {
                    query = bind_sqlite(query, p);
                }
                let rows = query.fetch_all(&mut **tx).await?;
                Ok(rows.into_iter().map(row_to_map_sqlite).collect())
            }
        }
    }

    pub async fn commit(self) -> Result<()> {
        match self {
            Transaction::Postgres(tx) => {
                tx.commit().await?;
                Ok(())
            }
            Transaction::Sqlite(tx) => {
                tx.commit().await?;
                Ok(())
            }
        }
    }

    pub async fn rollback(self) -> Result<()> {
        match self {
            Transaction::Postgres(tx) => {
                tx.rollback().await?;
                Ok(())
            }
            Transaction::Sqlite(tx) => {
                tx.rollback().await?;
                Ok(())
            }
        }
    }
}

fn bind_postgres<'a>(
    query: sqlx::query::Query<'a, sqlx::Postgres, sqlx::postgres::PgArguments>,
    value: &Value,
) -> sqlx::query::Query<'a, sqlx::Postgres, sqlx::postgres::PgArguments> {
    match value {
        Value::Null => query.bind(None::<String>),
        Value::Bool(b) => query.bind(*b),
        Value::Number(n) if n.is_i64() => query.bind(n.as_i64().unwrap()),
        Value::Number(n) if n.is_f64() => query.bind(n.as_f64().unwrap()),
        Value::Number(n) => query.bind(n.as_u64().unwrap() as i64),
        Value::String(s) => query.bind(s.clone()),
        Value::Array(a) => query.bind(serde_json::to_string(a).unwrap_or_default()),
        Value::Object(o) => query.bind(serde_json::to_string(o).unwrap_or_default()),
    }
}

fn bind_sqlite<'a>(
    query: sqlx::query::Query<'a, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'a>>,
    value: &Value,
) -> sqlx::query::Query<'a, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'a>> {
    match value {
        Value::Null => query.bind(None::<String>),
        Value::Bool(b) => query.bind(*b),
        Value::Number(n) if n.is_i64() => query.bind(n.as_i64().unwrap()),
        Value::Number(n) if n.is_f64() => query.bind(n.as_f64().unwrap()),
        Value::Number(n) => query.bind(n.as_u64().unwrap() as i64),
        Value::String(s) => query.bind(s.clone()),
        Value::Array(a) => query.bind(serde_json::to_string(a).unwrap_or_default()),
        Value::Object(o) => query.bind(serde_json::to_string(o).unwrap_or_default()),
    }
}

fn row_to_map_postgres(row: sqlx::postgres::PgRow) -> HashMap<String, Value> {
    let mut map = HashMap::new();
    for col in row.columns() {
        let name = col.name().to_string();
        let info = col.type_info().name();
        let val: Value = match info {
            "BOOL" => row.try_get::<bool, _>(name.as_str()).map(Value::Bool).unwrap_or(Value::Null),
            "INT2" | "INT4" | "INT8" => row.try_get::<i64, _>(name.as_str()).map(|v| Value::Number(v.into())).unwrap_or(Value::Null),
            "FLOAT4" | "FLOAT8" => row.try_get::<f64, _>(name.as_str()).map(|v| Value::Number(serde_json::Number::from_f64(v).unwrap_or(0.into()))).unwrap_or(Value::Null),
            "TEXT" | "VARCHAR" | "CHAR" | "NAME" | "UNKNOWN" => row.try_get::<String, _>(name.as_str()).map(Value::String).unwrap_or(Value::Null),
            "TIMESTAMPTZ" | "TIMESTAMP" => row.try_get::<chrono::DateTime<chrono::Utc>, _>(name.as_str()).map(|v| Value::String(v.to_rfc3339())).unwrap_or(Value::Null),
            _ => row.try_get::<String, _>(name.as_str()).map(Value::String).unwrap_or(Value::Null),
        };
        map.insert(name, val);
    }
    map
}

fn row_to_map_sqlite(row: sqlx::sqlite::SqliteRow) -> HashMap<String, Value> {
    let mut map = HashMap::new();
    for col in row.columns() {
        let name = col.name().to_string();
        let info = col.type_info().name();
        let val: Value = match info {
            "BOOLEAN" => row.try_get::<bool, _>(name.as_str()).map(Value::Bool).unwrap_or(Value::Null),
            "INTEGER" => row.try_get::<i64, _>(name.as_str()).map(|v| Value::Number(v.into())).unwrap_or(Value::Null),
            "REAL" | "DOUBLE" | "FLOAT" => row.try_get::<f64, _>(name.as_str()).map(|v| Value::Number(serde_json::Number::from_f64(v).unwrap_or(0.into()))).unwrap_or(Value::Null),
            "TEXT" | "VARCHAR" | "CHAR" | "NULL" => row.try_get::<String, _>(name.as_str()).map(Value::String).unwrap_or(Value::Null),
            "DATETIME" => row.try_get::<chrono::DateTime<chrono::Utc>, _>(name.as_str()).map(|v| Value::String(v.to_rfc3339())).unwrap_or(Value::Null),
            _ => row.try_get::<String, _>(name.as_str()).map(Value::String).unwrap_or(Value::Null),
        };
        map.insert(name, val);
    }
    map
}

impl Document {
    fn from_map(mut map: HashMap<String, Value>) -> Result<Document> {
        let doctype = map.remove("doctype")
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default();
        let name = map.remove("name")
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default();
        let owner = map.remove("owner")
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_else(|| "Administrator".into());
        let creation = map.remove("creation")
            .and_then(|v| v.as_str().and_then(|s| s.parse().ok()))
            .unwrap_or_else(Utc::now);
        let modified = map.remove("modified")
            .and_then(|v| v.as_str().and_then(|s| s.parse().ok()))
            .unwrap_or_else(Utc::now);
        let docstatus = map.remove("docstatus")
            .and_then(|v| v.as_i64().map(|i| i as i32))
            .unwrap_or(0);

        Ok(Document { doctype, name, owner, creation, modified, docstatus, fields: map })
    }
}
