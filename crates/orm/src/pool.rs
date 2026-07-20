use crate::document::Document;
use crate::filters::FilterCondition;
use chrono::Utc;
use error::{Result, RuntimeError};
use serde_json::{json, Value};
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
        use sqlx::sqlite::{SqliteJournalMode, SqliteSynchronous};
        let opts = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(url)
            .create_if_missing(true)
            // WAL mode lets readers proceed while a write is in progress and
            // greatly improves concurrency when multiple agents hit SQLite.
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            // Wait up to 5 seconds instead of failing immediately when the
            // single SQLite writer lock is held by another request.
            .busy_timeout(std::time::Duration::from_secs(5))
            // Larger cache helps metadata-heavy doctype sync and queries.
            .pragma("cache_size", "-64000");
        // SQLite only allows one writer at a time; a large pool mainly creates
        // lock contention. Cap it low by default, but allow override via env.
        let max_connections = std::env::var("KIFF_SQLITE_MAX_CONNECTIONS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3u32);
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            // Always keep one connection open: while the pool holds the DB
            // file, its WAL file cannot legitimately be deleted/recreated,
            // which is what lets the runtime watchdog treat a WAL-inode
            // change as proof of external interference.
            .min_connections(1)
            .max_connections(max_connections)
            .acquire_timeout(std::time::Duration::from_secs(10))
            // Connections must never be recycled: every close-time WAL
            // checkpoint of a sibling connection can delete the shared -wal
            // out from under the rest of the pool (POSIX fcntl locks are
            // per-process, so a closing connection cannot see the others and
            // believes it is the last user of the file). Simply omitting these
            // setters is NOT enough — sqlx defaults to idle_timeout = 10 min
            // and max_lifetime = 30 min, and the resulting expiry close is
            // exactly what repeatedly wedged the pool (observed as the
            // watchdog reporting "WAL file was replaced externally" every
            // ~20 minutes, driven by the libkiff_core.dylib bridge pool
            // recycling its idle connection).
            .max_lifetime(None)
            .idle_timeout(None)
            // A wedged pool retired by the watchdog must likewise never close
            // a connection again: the close-time checkpoint of a split-brain
            // WAL would bake garbage pages into the main DB.
            .connect_with(opts)
            .await?;
        Ok(DatabasePool::Sqlite(pool))
    }

    pub async fn connect_postgres(url: &str) -> Result<Self> {
        let pool = sqlx::PgPool::connect(url).await?;
        Ok(DatabasePool::Postgres(pool))
    }

    /// Close every connection in the pool and wait for outstanding ones to
    /// be returned. Clones of this pool share the same inner pool, so this
    /// closes it for all of them. The runtime watchdog relies on this before
    /// reconnecting: on macOS the POSIX fcntl locks and WAL-index state are
    /// per-process, so while a wedged pool's file descriptors stay open every
    /// new connection from the same process fails with the same corruption
    /// error.
    pub async fn close(&self) {
        match self {
            DatabasePool::Postgres(pool) => pool.close().await,
            DatabasePool::Sqlite(pool) => pool.close().await,
        }
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

    /// Return child-table fields for a DocType as (fieldname, child_doctype).
    async fn get_table_fields(&self, doctype: &str) -> Result<Vec<(String, String)>> {
        let meta_table = self.table_name("DocField");
        let sql = format!(
            r#"SELECT fieldname, options FROM "{}" WHERE parent = {} AND fieldtype IN ('Table', 'Table MultiSelect')"#,
            meta_table,
            self.placeholder(1)
        );
        let rows = self
            .query_raw(&sql, vec![Value::String(doctype.into())])
            .await?;
        Ok(rows
            .into_iter()
            .filter_map(|mut r| {
                let fieldname = r.remove("fieldname")?.as_str()?.to_string();
                let child_doctype = r.remove("options")?.as_str()?.to_string();
                Some((fieldname, child_doctype))
            })
            .collect())
    }

    /// Return the set of valid column identifiers for a DocType data table.
    ///
    /// Includes the standard Frappe columns plus every `fieldname` defined in
    /// the `docfield` metadata table for this DocType. This is used by HTTP
    /// handlers to sanitise `fields` and `order_by` query parameters before
    /// they reach SQL.
    pub async fn get_doctype_columns(
        &self,
        doctype: &str,
    ) -> Result<std::collections::HashSet<String>> {
        let mut cols = std::collections::HashSet::from([
            "name".into(),
            "creation".into(),
            "modified".into(),
            "modified_by".into(),
            "owner".into(),
            "docstatus".into(),
            "idx".into(),
            "parent".into(),
            "parentfield".into(),
            "parenttype".into(),
        ]);

        let meta_table = self.table_name("DocField");
        let sql = format!(
            r#"SELECT fieldname FROM "{}" WHERE parent = {} AND fieldname IS NOT NULL AND fieldname != ''"#,
            meta_table,
            self.placeholder(1)
        );
        match self
            .query_raw(&sql, vec![Value::String(doctype.into())])
            .await
        {
            Ok(rows) => {
                for mut row in rows {
                    if let Some(name) = row
                        .remove("fieldname")
                        .and_then(|v| v.as_str().map(String::from))
                    {
                        cols.insert(name);
                    }
                }
            }
            Err(e) => {
                // If DocField metadata isn't available yet, fall back to the
                // standard columns rather than failing the request.
                warn!(doctype = %doctype, error = %e, "failed to load docfield metadata");
            }
        }

        Ok(cols)
    }

    /// Persist child-table rows for `doc`. Only fields present in `doc.fields`
    /// are processed, matching real Frappe's update_children behaviour.
    async fn save_child_tables(&self, doc: &Document) -> Result<()> {
        let table_fields = self.get_table_fields(&doc.doctype).await?;
        if table_fields.is_empty() {
            return Ok(());
        }

        let now = Utc::now().to_rfc3339();
        let zero = serde_json::Number::from(0i32);

        for (fieldname, child_doctype) in table_fields {
            let Some(value) = doc.fields.get(&fieldname) else {
                continue;
            };
            let rows = match value {
                Value::Array(arr) => arr.clone(),
                _ => continue,
            };

            let child_table = self.table_name(&child_doctype);
            let delete_sql = format!(
                r#"DELETE FROM "{}" WHERE parent = {} AND parentfield = {} AND parenttype = {}"#,
                child_table,
                self.placeholder(1),
                self.placeholder(2),
                self.placeholder(3)
            );
            self.execute_raw(
                &delete_sql,
                vec![
                    Value::String(doc.name.clone()),
                    Value::String(fieldname.clone()),
                    Value::String(doc.doctype.clone()),
                ],
            )
            .await?;

            for (idx, row) in rows.iter().enumerate() {
                let mut obj = match row {
                    Value::Object(o) => o.clone(),
                    _ => continue,
                };

                let mut cols = vec![
                    "name".to_string(),
                    "owner".to_string(),
                    "creation".to_string(),
                    "modified".to_string(),
                    "docstatus".to_string(),
                    "idx".to_string(),
                    "parent".to_string(),
                    "parentfield".to_string(),
                    "parenttype".to_string(),
                ];
                let mut params: Vec<Value> = vec![
                    Value::String(
                        obj.remove("name")
                            .and_then(|v| v.as_str().map(String::from))
                            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                    ),
                    Value::String(doc.owner.clone()),
                    Value::String(now.clone()),
                    Value::String(now.clone()),
                    Value::Number(zero.clone()),
                    Value::Number(serde_json::Number::from((idx + 1) as i64)),
                    Value::String(doc.name.clone()),
                    Value::String(fieldname.clone()),
                    Value::String(doc.doctype.clone()),
                ];

                for (k, v) in obj {
                    if k == "doctype" {
                        continue;
                    }
                    cols.push(k);
                    params.push(v);
                }

                let placeholders: Vec<String> =
                    (1..=params.len()).map(|i| self.placeholder(i)).collect();
                let insert_sql = format!(
                    r#"INSERT INTO "{}" ({}) VALUES ({})"#,
                    child_table,
                    cols.join(", "),
                    placeholders.join(", ")
                );
                self.execute_raw(&insert_sql, params).await?;
            }
        }

        Ok(())
    }

    pub async fn get_doc(&self, doctype: &str, name: &str) -> Result<Document> {
        let table = self.table_name(doctype);
        let sql = format!(
            "SELECT * FROM \"{}\" WHERE name = {}",
            table,
            self.placeholder(1)
        );
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
        let mut m = rows
            .into_iter()
            .next()
            .ok_or_else(|| RuntimeError::NotFound(format!("{} {}", doctype, name)))?;
        m.insert("doctype".into(), Value::String(doctype.into()));
        m.insert("name".into(), Value::String(name.into()));

        let mut doc = Document::from_map(m)?;
        self.load_child_tables(&mut doc, doctype).await?;
        self.add_onload_data(&mut doc, doctype).await?;
        Ok(doc)
    }

    /// Inject `__onload` values that Frappe form controllers normally provide.
    /// The native ORM path does not execute Python controller hooks, so we
    /// reproduce the small set of onload data the Desk UI depends on.
    async fn add_onload_data(&self, doc: &mut Document, doctype: &str) -> Result<()> {
        if !matches!(doctype, "Module Profile" | "User") {
            return Ok(());
        }

        let rows = match self
            .execute_sql(
                r#"SELECT module_name FROM "module_def" ORDER BY module_name"#,
                vec![],
            )
            .await
        {
            Ok(r) => r,
            Err(e) => {
                warn!("failed to load module list for __onload: {}", e);
                return Ok(());
            }
        };

        let modules: Vec<Value> = rows
            .into_iter()
            .filter_map(|mut row| {
                row.remove("module_name")
                    .and_then(|v| v.as_str().map(|s| Value::String(s.into())))
            })
            .collect();

        let onload = json!({ "all_modules": modules });
        doc.set_field("__onload", onload);
        Ok(())
    }

    /// Load child-table rows for `doc` from the database and attach them as
    /// arrays on the parent document, matching Frappe's get_doc behaviour.
    async fn load_child_tables(&self, doc: &mut Document, doctype: &str) -> Result<()> {
        let table_fields = self.get_table_fields(doctype).await?;
        if table_fields.is_empty() {
            return Ok(());
        }

        for (fieldname, child_doctype) in table_fields {
            let child_table = self.table_name(&child_doctype);
            let sql = format!(
                r#"SELECT * FROM "{}" WHERE parent = {} AND parentfield = {} AND parenttype = {} ORDER BY idx"#,
                child_table,
                self.placeholder(1),
                self.placeholder(2),
                self.placeholder(3)
            );
            let params = vec![
                Value::String(doc.name.clone()),
                Value::String(fieldname.clone()),
                Value::String(doctype.into()),
            ];

            let rows = match self.query_raw(&sql, params).await {
                Ok(r) => r,
                Err(e) => {
                    let msg = e.to_string();
                    if msg.contains("no such table") || msg.contains("does not exist") {
                        warn!(
                            doctype = %doctype,
                            field = %fieldname,
                            child_doctype = %child_doctype,
                            "child table missing, skipping"
                        );
                        continue;
                    }
                    return Err(e);
                }
            };

            let children: Vec<Value> = rows
                .into_iter()
                .map(|mut row| {
                    row.insert("doctype".into(), Value::String(child_doctype.clone()));
                    Value::Object(row.into_iter().collect())
                })
                .collect();
            doc.set_field(fieldname, Value::Array(children));
        }

        Ok(())
    }

    pub async fn get_list(
        &self,
        doctype: &str,
        filters: Option<HashMap<String, FilterCondition>>,
        fields: Option<Vec<String>>,
        order_by: Option<(String, bool)>,
        permission_conditions: Option<Vec<String>>,
        limit: Option<usize>,
    ) -> Result<Vec<Document>> {
        let table = self.table_name(doctype);
        let cols = match fields {
            Some(f) if !f.is_empty() => f
                .iter()
                .map(|c| format!("\"{}\"", c.replace('"', "")))
                .collect::<Vec<_>>()
                .join(", "),
            _ => "*".to_string(),
        };

        let mut sql = format!("SELECT {} FROM \"{}\"", cols, table);
        let mut params: Vec<Value> = Vec::new();
        let mut all_conditions: Vec<String> = Vec::new();

        if let Some(filts) = filters {
            if !filts.is_empty() {
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
                    all_conditions.push(frag);
                    params.extend(vals);
                }
            }
        }

        if let Some(conds) = permission_conditions {
            if !conds.is_empty() {
                all_conditions.extend(conds);
            }
        }

        if !all_conditions.is_empty() {
            sql.push_str(&format!(" WHERE {}", all_conditions.join(" AND ")));
        }

        if let Some((field, desc)) = order_by {
            let dir = if desc { "DESC" } else { "ASC" };
            let safe_field = field.replace('"', "");
            sql.push_str(&format!(" ORDER BY \"{}\" {}", safe_field, dir));
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
        let docs: Result<Vec<Document>> = rows
            .into_iter()
            .map(|mut m| {
                m.insert("doctype".into(), Value::String(doctype.into()));
                Document::from_map(m)
            })
            .collect();
        docs
    }

    /// Return the number of rows matching the supplied filters.
    pub async fn count(
        &self,
        doctype: &str,
        filters: Option<HashMap<String, FilterCondition>>,
        permission_conditions: Option<Vec<String>>,
    ) -> Result<usize> {
        if doctype.is_empty() {
            return Ok(0);
        }
        let table = self.table_name(doctype);
        let mut sql = format!("SELECT COUNT(*) FROM \"{}\"", table);
        let mut params: Vec<Value> = Vec::new();
        let mut all_conditions: Vec<String> = Vec::new();

        if let Some(filts) = filters {
            if !filts.is_empty() {
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
                    all_conditions.push(frag);
                    params.extend(vals);
                }
            }
        }

        if let Some(conds) = permission_conditions {
            if !conds.is_empty() {
                all_conditions.extend(conds);
            }
        }

        if !all_conditions.is_empty() {
            sql.push_str(&format!(" WHERE {}", all_conditions.join(" AND ")));
        }

        debug!("count sql: {}", sql);
        let rows = match self.query_raw(&sql, params).await {
            Ok(r) => r,
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("no such table") || msg.contains("does not exist") {
                    warn!(doctype = %doctype, table = %table, "table missing in DB, returning count 0");
                    return Ok(0);
                }
                return Err(e);
            }
        };
        let count = rows
            .into_iter()
            .next()
            .and_then(|mut r| r.remove("COUNT(*)").or_else(|| r.remove("count")))
            .and_then(|v| match v {
                Value::Number(n) => n.as_i64().map(|n| n as usize),
                _ => None,
            })
            .unwrap_or(0);
        Ok(count)
    }

    pub async fn save_doc(&self, doc: &Document) -> Result<()> {
        crate::hooks::run_hook("before_save", &doc.doctype, doc).await?;

        let table = self.table_name(&doc.doctype);
        let table_fields = self.get_table_fields(&doc.doctype).await?;
        let table_field_names: std::collections::HashSet<String> =
            table_fields.iter().map(|(k, _)| k.clone()).collect();

        let mut sets = Vec::new();
        let mut params: Vec<Value> = Vec::new();

        for (k, v) in &doc.fields {
            if table_field_names.contains(k) {
                continue;
            }
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
        self.save_child_tables(doc).await?;

        crate::hooks::run_hook("on_update", &doc.doctype, doc).await?;
        Ok(())
    }

    pub async fn insert_doc(&self, doc: &Document) -> Result<String> {
        crate::hooks::run_hook("before_insert", &doc.doctype, doc).await?;

        let table = self.table_name(&doc.doctype);
        let table_fields = self.get_table_fields(&doc.doctype).await?;
        let table_field_names: std::collections::HashSet<String> =
            table_fields.iter().map(|(k, _)| k.clone()).collect();

        let mut cols = vec![
            "name".to_string(),
            "owner".to_string(),
            "creation".to_string(),
            "modified".to_string(),
            "docstatus".to_string(),
        ];
        let mut params: Vec<Value> = vec![
            Value::String(doc.name.clone()),
            Value::String(doc.owner.clone()),
            Value::String(doc.creation.to_rfc3339()),
            Value::String(doc.modified.to_rfc3339()),
            Value::Number(serde_json::Number::from(doc.docstatus)),
        ];

        for (k, v) in &doc.fields {
            if table_field_names.contains(k) {
                continue;
            }
            cols.push(k.clone());
            params.push(v.clone());
        }

        let placeholders: Vec<String> = (1..=params.len()).map(|i| self.placeholder(i)).collect();

        let sql = format!(
            "INSERT INTO \"{}\" ({}) VALUES ({})",
            table,
            cols.join(", "),
            placeholders.join(", ")
        );

        debug!("insert_doc sql: {}", sql);
        self.execute_raw(&sql, params).await?;
        self.save_child_tables(doc).await?;

        crate::hooks::run_hook("after_insert", &doc.doctype, doc).await?;
        Ok(doc.name.clone())
    }

    pub async fn delete_doc(&self, doctype: &str, name: &str) -> Result<()> {
        let stub_doc = Document::new(doctype, name);
        crate::hooks::run_hook("before_trash", doctype, &stub_doc).await?;

        let table = self.table_name(doctype);
        let sql = format!(
            "DELETE FROM \"{}\" WHERE name = {}",
            table,
            self.placeholder(1)
        );
        self.execute_raw(&sql, vec![Value::String(name.into())])
            .await?;

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
        let rows = self
            .query_raw(&sql, vec![Value::String(name.into())])
            .await?;
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
            "BOOL" => row
                .try_get::<bool, _>(name.as_str())
                .map(Value::Bool)
                .unwrap_or(Value::Null),
            "INT2" | "INT4" | "INT8" => row
                .try_get::<i64, _>(name.as_str())
                .map(|v| Value::Number(v.into()))
                .unwrap_or(Value::Null),
            "FLOAT4" | "FLOAT8" => row
                .try_get::<f64, _>(name.as_str())
                .map(|v| Value::Number(serde_json::Number::from_f64(v).unwrap_or(0.into())))
                .unwrap_or(Value::Null),
            "TEXT" | "VARCHAR" | "CHAR" | "NAME" | "UNKNOWN" => row
                .try_get::<String, _>(name.as_str())
                .map(Value::String)
                .unwrap_or(Value::Null),
            "TIMESTAMPTZ" | "TIMESTAMP" => row
                .try_get::<chrono::DateTime<chrono::Utc>, _>(name.as_str())
                .map(|v| Value::String(v.to_rfc3339()))
                .unwrap_or(Value::Null),
            _ => row
                .try_get::<String, _>(name.as_str())
                .map(Value::String)
                .unwrap_or(Value::Null),
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
            "BOOLEAN" => row
                .try_get::<bool, _>(name.as_str())
                .map(Value::Bool)
                .unwrap_or(Value::Null),
            "INTEGER" => row
                .try_get::<i64, _>(name.as_str())
                .map(|v| Value::Number(v.into()))
                .unwrap_or(Value::Null),
            "REAL" | "DOUBLE" | "FLOAT" => row
                .try_get::<f64, _>(name.as_str())
                .map(|v| Value::Number(serde_json::Number::from_f64(v).unwrap_or(0.into())))
                .unwrap_or(Value::Null),
            "TEXT" | "VARCHAR" | "CHAR" | "NULL" => row
                .try_get::<String, _>(name.as_str())
                .map(Value::String)
                .unwrap_or(Value::Null),
            "DATETIME" => row
                .try_get::<chrono::DateTime<chrono::Utc>, _>(name.as_str())
                .map(|v| Value::String(v.to_rfc3339()))
                .unwrap_or(Value::Null),
            _ => row
                .try_get::<String, _>(name.as_str())
                .map(Value::String)
                .unwrap_or(Value::Null),
        };
        map.insert(name, val);
    }
    map
}

impl Document {
    fn from_map(mut map: HashMap<String, Value>) -> Result<Document> {
        let doctype = map
            .remove("doctype")
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default();
        let name = map
            .remove("name")
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default();
        let owner = map
            .remove("owner")
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_else(|| "Administrator".into());
        let creation = map
            .remove("creation")
            .and_then(|v| v.as_str().and_then(|s| s.parse().ok()))
            .unwrap_or_else(Utc::now);
        let modified = map
            .remove("modified")
            .and_then(|v| v.as_str().and_then(|s| s.parse().ok()))
            .unwrap_or_else(Utc::now);
        let docstatus = map
            .remove("docstatus")
            .and_then(|v| v.as_i64().map(|i| i as i32))
            .unwrap_or(0);

        Ok(Document {
            doctype,
            name,
            owner,
            creation,
            modified,
            docstatus,
            fields: map,
        })
    }
}
