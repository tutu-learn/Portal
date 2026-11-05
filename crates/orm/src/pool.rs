use crate::document::Document;
use crate::domain::DbDomain;
use crate::domain_pools::DomainPools;
use crate::filters::FilterCondition;
use chrono::Utc;
use error::{Result, RuntimeError};
use serde_json::{json, Value};
use sqlx::{Column, Row, TypeInfo};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tracing::{debug, warn};

/// A database handle that resolves to the correct SQLite pool for the
/// requested table/doctype.
///
/// There are two ways to obtain one:
/// * `DatabasePool::connect_sqlite(url)` creates a single-pool handle for
///   tests, CLI tools, and the Python bridge fallback.
/// * `DomainPools::pool_for(domain)` returns a handle backed by a site's full
///   domain bundle, which is what the runtime and HTTP layer use.
///
/// When the handle comes from a single-pool connection, every domain aliases
/// that one pool. When it comes from a sharded `DomainPools`, doctype-based
/// operations automatically route to `k8s.db`, `telemetry.db`, `state.db`, or
/// `site.db` based on the table name.
#[derive(Debug, Clone)]
pub struct DatabasePool {
    pools: Arc<DomainPools>,
    domain: DbDomain,
}

struct ConnectionOptions {
    max_connections: u32,
    busy_timeout: std::time::Duration,
    acquire_timeout: std::time::Duration,
}

impl Default for ConnectionOptions {
    fn default() -> Self {
        let max_connections = std::env::var("KIFF_SQLITE_MAX_CONNECTIONS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(8u32);
        Self {
            max_connections,
            busy_timeout: std::time::Duration::from_secs(10),
            acquire_timeout: std::time::Duration::from_secs(30),
        }
    }
}

impl ConnectionOptions {
    fn for_domain(domain: DbDomain) -> Self {
        let suffix = match domain {
            DbDomain::Core => "CORE",
            DbDomain::K8s => "K8S",
            DbDomain::Telemetry => "TELEMETRY",
            DbDomain::State => "STATE",
            DbDomain::History => "HISTORY",
        };

        let max_connections = std::env::var(format!("KIFF_SQLITE_MAX_CONNECTIONS_{}", suffix))
            .ok()
            .and_then(|s| s.parse().ok())
            .or_else(|| {
                std::env::var("KIFF_SQLITE_MAX_CONNECTIONS")
                    .ok()
                    .and_then(|s| s.parse().ok())
            })
            .unwrap_or_else(|| match domain {
                DbDomain::Core => 8,
                DbDomain::K8s => 12,
                DbDomain::Telemetry => 8,
                DbDomain::State => 6,
                // Append-only audit trail: low write rate, small pool.
                DbDomain::History => 4,
            });

        let (busy_secs, acquire_secs) = match domain {
            DbDomain::Core => (10, 30),
            DbDomain::K8s => (30, 60),
            DbDomain::Telemetry => (15, 45),
            DbDomain::State => (10, 30),
            DbDomain::History => (10, 30),
        };

        Self {
            max_connections,
            busy_timeout: std::time::Duration::from_secs(busy_secs),
            acquire_timeout: std::time::Duration::from_secs(acquire_secs),
        }
    }
}

impl DatabasePool {
    /// Build a handle backed by `pools` with the supplied default domain.
    pub fn new(pools: Arc<DomainPools>, domain: DbDomain) -> Self {
        Self { pools, domain }
    }

    /// Connect a single SQLite file. Every domain aliases this one pool, so the
    /// handle behaves like the original single-database architecture.
    pub async fn connect_sqlite(url: &str) -> Result<Self> {
        let pool = Self::connect_sqlite_with_options(url, ConnectionOptions::default()).await?;
        Ok(Arc::new(DomainPools::from_core_pool(Self::from_raw(
            pool,
            DbDomain::Core,
        )))
        .core())
    }

    /// Connect to a domain-specific SQLite file inside a site directory.
    pub async fn connect_sqlite_domain(
        site_path: &Path,
        domain: DbDomain,
    ) -> Result<sqlx::SqlitePool> {
        let path = domain.file_path(site_path);
        let path_str = path.to_str().ok_or_else(|| {
            RuntimeError::Config(format!("invalid database path: {:?}", path))
        })?;
        let opts = ConnectionOptions::for_domain(domain);
        Self::connect_sqlite_with_options(path_str, opts).await
    }

    /// Construct a single-pool handle directly from a raw `SqlitePool`.
    ///
    /// This is mostly useful for tests and the `from_core_pool` helper; it does
    /// not create domain files and is not watchdog-aware.
    pub(crate) fn from_raw(pool: sqlx::SqlitePool, domain: DbDomain) -> Self {
        Self {
            pools: Arc::new(DomainPools::from_core_pool_inner(pool)),
            domain,
        }
    }

    async fn connect_sqlite_with_options(url: &str, options: ConnectionOptions) -> Result<sqlx::SqlitePool> {
        use sqlx::sqlite::{SqliteJournalMode, SqliteSynchronous};
        let opts = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(url)
            .create_if_missing(true)
            // WAL mode lets readers proceed while a write is in progress and
            // greatly improves concurrency when multiple agents hit SQLite.
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            // Wait instead of failing immediately when the single SQLite writer
            // lock is held by another request.
            .busy_timeout(options.busy_timeout)
            // Per-connection page cache. 16 MB per connection keeps the
            // aggregate cache bounded while still helping metadata-heavy
            // doctype sync and queries.
            .pragma("cache_size", "-16000");
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            // Always keep one connection open: while the pool holds the DB
            // file, its WAL file cannot legitimately be deleted/recreated,
            // which is what lets the runtime watchdog treat a WAL-inode
            // change as proof of external interference.
            .min_connections(1)
            .max_connections(options.max_connections)
            // Give requests a generous window to acquire a connection when the
            // pool is temporarily saturated (e.g. workspace bootinfo + card
            // count queries all arrive together).
            .acquire_timeout(options.acquire_timeout)
            // Connections must never be recycled: every close-time WAL
            // checkpoint of a sibling connection can delete the shared -wal
            // out from under the rest of the pool (POSIX fcntl locks are
            // per-process, so a closing connection cannot see the others and
            // believes it is the last user of the file). Simply omitting these
            // setters is NOT enough — sqlx defaults to idle_timeout = 10 min
            // and max_lifetime = 30 min, and the resulting expiry close is
            // exactly what repeatedly wedged the pool (observed as the
            // watchdog reporting "WAL file was replaced externally" every
            // ~20 minutes, driven by the libanri_core.dylib bridge pool
            // recycling its idle connection).
            .max_lifetime(None)
            .idle_timeout(None)
            // A wedged pool retired by the watchdog must likewise never close
            // a connection again: the close-time checkpoint of a split-brain
            // WAL would bake garbage pages into the main DB.
            .connect_with(opts)
            .await?;
        Ok(pool)
    }

    /// Return a handle whose default domain is `domain`. Doctype-based
    /// operations on the returned handle will use that domain's pool.
    pub fn with_domain(&self, domain: DbDomain) -> Self {
        Self {
            pools: Arc::clone(&self.pools),
            domain,
        }
    }

    /// Return a handle routed to the domain that owns `doctype`.
    pub fn for_doctype(&self, doctype: &str) -> Self {
        self.with_domain(DbDomain::for_table(&table_name(doctype)))
    }

    /// Return a handle routed to the domain that owns table `name`.
    pub fn for_table(&self, name: &str) -> Self {
        self.with_domain(DbDomain::for_table(name))
    }

    /// Return a handle routed to the core (metadata) domain.
    pub fn core(&self) -> Self {
        self.with_domain(DbDomain::Core)
    }

    /// Return the raw `SqlitePool` for the current default domain.
    ///
    /// Callers should prefer the high-level methods that route by doctype; this
    /// is exposed for places that need to hand a raw pool to sqlx (e.g. the
    /// watchdog and `DomainPools::from_core_pool`).
    pub fn into_inner(self) -> sqlx::SqlitePool {
        self.pools.raw_pool_for(self.domain)
    }

    /// Return the underlying `DomainPools` bundle. Useful for the migrator and
    /// other code that needs to operate on every domain.
    pub fn domain_pools(&self) -> Arc<DomainPools> {
        Arc::clone(&self.pools)
    }

    fn pool(&self) -> sqlx::SqlitePool {
        self.pools.raw_pool_for(self.domain)
    }

    /// Close every connection in the current default domain's pool and wait
    /// for outstanding ones to be returned. Clones of this handle share the
    /// same inner pool, so this closes it for all of them. The runtime
    /// watchdog relies on this before reconnecting.
    pub async fn close(&self) {
        self.pool().close().await;
    }

    pub fn dialect(&self) -> &'static str {
        "sqlite"
    }

    pub fn placeholder(&self, _idx: usize) -> String {
        "?".to_string()
    }

    fn table_name(&self, doctype: &str) -> String {
        table_name(doctype)
    }

    /// Return child-table fields for a DocType as (fieldname, child_doctype).
    ///
    /// DocField metadata always lives in the core domain, so this queries the
    /// core pool regardless of which domain owns `doctype`.
    async fn get_table_fields(&self, doctype: &str) -> Result<Vec<(String, String)>> {
        let meta = self.core();
        let meta_table = meta.table_name("DocField");
        let sql = format!(
            r#"SELECT fieldname, options FROM "{}" WHERE parent = ? AND fieldtype IN ('Table', 'Table MultiSelect')"#,
            meta_table,
        );
        let rows = meta
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

    /// DocField-defined defaults for a DocType as (fieldname, fieldtype,
    /// default), only for fields with a non-empty `default`.
    ///
    /// DocField metadata always lives in the core domain, so this queries the
    /// core pool regardless of which domain owns `doctype`.
    async fn get_field_defaults(&self, doctype: &str) -> Result<Vec<(String, String, String)>> {
        let meta = self.core();
        let meta_table = meta.table_name("DocField");
        let sql = format!(
            r#"SELECT fieldname, fieldtype, "default" FROM "{}" WHERE parent = ? AND "default" IS NOT NULL AND "default" != ''"#,
            meta_table,
        );
        let rows = meta
            .query_raw(&sql, vec![Value::String(doctype.into())])
            .await?;
        Ok(rows
            .into_iter()
            .filter_map(|mut r| {
                let fieldname = r.remove("fieldname")?.as_str()?.to_string();
                let fieldtype = r.remove("fieldtype")?.as_str()?.to_string();
                let default = r.remove("default")?.as_str()?.to_string();
                Some((fieldname, fieldtype, default))
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

        let meta = self.core();
        let meta_table = meta.table_name("DocField");
        let sql = format!(
            r#"SELECT fieldname FROM "{}" WHERE parent = ? AND fieldname IS NOT NULL AND fieldname != ''"#,
            meta_table,
        );
        match meta
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

        // Virtual child DocTypes (e.g. Frappe's User Session Display) have no
        // physical table, so trying to delete/insert rows fails. Skip them.
        // DocType metadata lives in the core domain regardless of where the
        // parent document is stored.
        let virtual_doctypes: std::collections::HashSet<String> = {
            let meta = self.core();
            let meta_table = meta.table_name("DocType");
            let sql = format!(r#"SELECT name FROM "{}" WHERE is_virtual = 1"#, meta_table);
            let rows = meta.query_raw(&sql, vec![]).await.unwrap_or_default();
            rows.into_iter()
                .filter_map(|mut r| r.remove("name").and_then(|v| v.as_str().map(String::from)))
                .collect()
        };

        let now = Utc::now().to_rfc3339();
        let zero = serde_json::Number::from(0i32);

        for (fieldname, child_doctype) in table_fields {
            if virtual_doctypes.contains(&child_doctype) {
                continue;
            }
            let Some(value) = doc.fields.get(&fieldname) else {
                continue;
            };
            let rows = match value {
                Value::Array(arr) => arr.clone(),
                _ => continue,
            };

            let child_table = self.table_name(&child_doctype);
            let delete_sql = format!(
                r#"DELETE FROM "{}" WHERE parent = ? AND parentfield = ? AND parenttype = ?"#,
                child_table,
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
                    (1..=params.len()).map(|_| "?".to_string()).collect();
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
        let pool = self.for_doctype(doctype);
        let table = pool.table_name(doctype);
        let sql = format!("SELECT * FROM \"{}\" WHERE name = ?", table);
        debug!("get_doc sql: {}", sql);

        let rows = match pool.query_raw(&sql, vec![Value::String(name.into())]).await {
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
        pool.load_child_tables(&mut doc, doctype).await?;
        pool.add_onload_data(&mut doc, doctype).await?;
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
            .core()
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
                r#"SELECT * FROM "{}" WHERE parent = ? AND parentfield = ? AND parenttype = ? ORDER BY idx"#,
                child_table,
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
        let pool = self.for_doctype(doctype);
        let table = pool.table_name(doctype);
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
                    let (frag, vals) = cond.to_sql(&k, || "?".to_string());
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
        let rows = match pool.query_raw(&sql, params).await {
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
        let pool = self.for_doctype(doctype);
        let table = pool.table_name(doctype);
        let mut sql = format!("SELECT COUNT(*) FROM \"{}\"", table);
        let mut params: Vec<Value> = Vec::new();
        let mut all_conditions: Vec<String> = Vec::new();

        if let Some(filts) = filters {
            if !filts.is_empty() {
                for (k, cond) in filts {
                    let (frag, vals) = cond.to_sql(&k, || "?".to_string());
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
        let rows = match pool.query_raw(&sql, params).await {
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

        let pool = self.for_doctype(&doc.doctype);
        let table = pool.table_name(&doc.doctype);
        let table_fields = pool.get_table_fields(&doc.doctype).await?;
        let table_field_names: std::collections::HashSet<String> =
            table_fields.iter().map(|(k, _)| k.clone()).collect();

        let mut sets = Vec::new();
        let mut params: Vec<Value> = Vec::new();

        for (k, v) in &doc.fields {
            if table_field_names.contains(k) {
                continue;
            }
            // Skip internal UI state (`__onload`, `__islocal`, ...): injected
            // by get_doc or the Desk client, never a real column.
            if k.starts_with("__") {
                continue;
            }
            sets.push(format!("{} = ?", k));
            params.push(v.clone());
        }
        sets.push("modified = ?".to_string());
        params.push(Value::String(doc.modified.to_rfc3339()));

        let sql = format!(
            "UPDATE \"{}\" SET {} WHERE name = ?",
            table,
            sets.join(", "),
        );
        params.push(Value::String(doc.name.clone()));

        debug!("save_doc sql: {}", sql);
        pool.execute_raw(&sql, params).await?;
        pool.save_child_tables(doc).await?;

        crate::hooks::run_hook("on_update", &doc.doctype, doc).await?;
        crate::hooks::run_hook("after_save", &doc.doctype, doc).await?;
        Ok(())
    }

    pub async fn insert_doc(&self, doc: &Document) -> Result<String> {
        crate::hooks::run_hook("before_insert", &doc.doctype, doc).await?;

        let pool = self.for_doctype(&doc.doctype);
        let table = pool.table_name(&doc.doctype);
        let table_fields = pool.get_table_fields(&doc.doctype).await?;
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
            // Skip internal UI state (`__onload`, `__islocal`, ...): injected
            // by get_doc or the Desk client, never a real column.
            if k.starts_with("__") {
                continue;
            }
            cols.push(k.clone());
            params.push(v.clone());
        }

        // Apply DocField defaults for unset fields of numeric fieldtypes,
        // mirroring Frappe's insert-time default application (`_set_defaults`)
        // and the default-aware column definitions. Without this an INSERT
        // that omits e.g. `enabled` inherits the SQL column default — 0 on
        // schemas synced before defaults were honored — creating disabled
        // users. Restricted to numeric fieldtypes: a literal default is only
        // safe there (Date defaults like "Today" are expressions, and text
        // columns never had baked defaults, so backfilling them would change
        // behavior for existing apps).
        let field_defaults = self.get_field_defaults(&doc.doctype).await.unwrap_or_else(|e| {
            warn!(
                doctype = %doc.doctype,
                error = %e,
                "failed to load docfield defaults; inserting without backfilled defaults"
            );
            Vec::new()
        });
        let mut extra: Vec<(String, Value)> = Vec::new();
        for (fieldname, fieldtype, default) in field_defaults {
            if !matches!(
                fieldtype.as_str(),
                "Check" | "Int" | "Rating" | "Float" | "Currency" | "Percent"
            ) {
                continue;
            }
            if table_field_names.contains(&fieldname) || cols.iter().any(|c| c == &fieldname) {
                continue;
            }
            extra.push((fieldname, default_value_param(&fieldtype, &default)));
        }

        if !extra.is_empty() {
            let mut cols_with_defaults = cols.clone();
            let mut params_with_defaults = params.clone();
            for (fieldname, value) in &extra {
                cols_with_defaults.push(fieldname.clone());
                params_with_defaults.push(value.clone());
            }
            match pool
                .execute_raw(
                    &insert_sql(&table, &cols_with_defaults),
                    params_with_defaults,
                )
                .await
            {
                Ok(_) => {
                    pool.save_child_tables(doc).await?;
                    crate::hooks::run_hook("after_insert", &doc.doctype, doc).await?;
                    return Ok(doc.name.clone());
                }
                // A default whose column doesn't exist yet (a table that
                // predates the field) must not break inserts that worked
                // before: fall back to the plain insert below. SQLite phrases
                // this two ways depending on where the name is resolved.
                Err(e)
                    if e.to_string().contains("no such column")
                        || e.to_string().contains("has no column named") =>
                {
                    warn!(
                        doctype = %doc.doctype,
                        name = %doc.name,
                        error = %e,
                        "docfield-default backfill hit a missing column; falling back to a plain insert"
                    );
                }
                Err(e) => return Err(e),
            }
        }

        let sql = insert_sql(&table, &cols);
        debug!("insert_doc sql: {}", sql);
        pool.execute_raw(&sql, params).await?;
        pool.save_child_tables(doc).await?;

        crate::hooks::run_hook("after_insert", &doc.doctype, doc).await?;
        Ok(doc.name.clone())
    }

    pub async fn delete_doc(&self, doctype: &str, name: &str) -> Result<()> {
        let stub_doc = Document::new(doctype, name);
        crate::hooks::run_hook("before_trash", doctype, &stub_doc).await?;

        let pool = self.for_doctype(doctype);
        let table = pool.table_name(doctype);
        let sql = format!("DELETE FROM \"{}\" WHERE name = ?", table);
        pool.execute_raw(&sql, vec![Value::String(name.into())])
            .await?;

        crate::hooks::run_hook("after_trash", doctype, &stub_doc).await?;
        Ok(())
    }

    pub async fn exists(&self, doctype: &str, name: &str) -> Result<bool> {
        let pool = self.for_doctype(doctype);
        let table = pool.table_name(doctype);
        let sql = format!("SELECT 1 FROM \"{}\" WHERE name = ? LIMIT 1", table);
        let rows = pool
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
        // was begun via DatabasePool::begin(). Issuing COMMIT without BEGIN
        // raises "cannot commit - no transaction is active", so ignore it.
        Ok(())
    }

    pub async fn rollback(&self) -> Result<()> {
        Ok(())
    }

    pub async fn begin(&self) -> Result<Transaction<'_>> {
        let tx = self.pool().begin().await?;
        Ok(Transaction(tx))
    }

    /// Scan raw SQL for quoted table names and return a handle routed to the
    /// domain that owns the first non-core table found.
    ///
    /// This lets existing raw-SQL callers (e.g. audit_ready's patch_job and
    /// k8s_inventory queries) work transparently when sharding is enabled,
    /// without forcing every call site to call `for_table` explicitly.
    fn routed_for_sql(&self, sql: &str) -> Self {
        let mut domains: Vec<DbDomain> = Vec::new();
        let mut chars = sql.char_indices().peekable();
        while let Some((start, ch)) = chars.next() {
            if ch != '"' {
                continue;
            }
            let mut end = None;
            while let Some((i, c)) = chars.next() {
                if c == '"' {
                    end = Some(i);
                    break;
                }
            }
            let Some(end) = end else { continue };
            let name = &sql[start + 1..end];
            if name.is_empty() || name.starts_with("sqlite_") {
                continue;
            }
            let domain = DbDomain::for_table(name);
            if domain != DbDomain::Core && !domains.contains(&domain) {
                domains.push(domain);
            }
        }

        match domains.as_slice() {
            [] => self.clone(),
            [domain] => self.with_domain(*domain),
            [domain, ..] => {
                // Multiple non-core domains referenced; route to the first one
                // and warn so the caller can be made explicit if needed.
                warn!(
                    sql = %sql,
                    domains = ?domains,
                    "raw SQL references multiple non-core domains; routing to first"
                );
                self.with_domain(*domain)
            }
        }
    }

    async fn query_raw(
        &self,
        sql: &str,
        params: Vec<Value>,
    ) -> Result<Vec<HashMap<String, Value>>> {
        let pool = self.routed_for_sql(sql);
        let mut query = sqlx::query(sql);
        for p in &params {
            query = bind_sqlite(query, p);
        }
        let rows = query.fetch_all(&pool.pool()).await?;
        Ok(rows.into_iter().map(row_to_map_sqlite).collect())
    }

    async fn execute_raw(&self, sql: &str, params: Vec<Value>) -> Result<()> {
        let pool = self.routed_for_sql(sql);
        let mut query = sqlx::query(sql);
        for p in &params {
            query = bind_sqlite(query, p);
        }
        query.execute(&pool.pool()).await?;
        Ok(())
    }
}

fn table_name(doctype: &str) -> String {
    let name = doctype.to_lowercase().replace(" ", "_");
    name.strip_prefix("tab").unwrap_or(&name).to_string()
}

/// Convert a DocField `default` string into a bind parameter matching the
/// fieldtype, mirroring Frappe's insert-time default application.
fn default_value_param(fieldtype: &str, default: &str) -> Value {
    match fieldtype {
        "Check" | "Int" | "Rating" => {
            Value::Number(default.trim().parse::<i64>().unwrap_or(0).into())
        }
        "Float" | "Currency" | "Percent" => Value::Number(
            serde_json::Number::from_f64(default.trim().parse::<f64>().unwrap_or(0.0))
                .unwrap_or_else(|| serde_json::Number::from(0)),
        ),
        _ => Value::String(default.to_string()),
    }
}

/// Build the INSERT statement for `cols` (one `?` placeholder per column).
fn insert_sql(table: &str, cols: &[String]) -> String {
    let placeholders: Vec<String> = (1..=cols.len()).map(|_| "?".to_string()).collect();
    format!(
        "INSERT INTO \"{}\" ({}) VALUES ({})",
        table,
        cols.join(", "),
        placeholders.join(", ")
    )
}

pub struct Transaction<'a>(sqlx::Transaction<'a, sqlx::Sqlite>);

impl<'a> Transaction<'a> {
    pub async fn execute_sql(
        &mut self,
        sql: &str,
        params: Vec<Value>,
    ) -> Result<Vec<HashMap<String, Value>>> {
        let mut query = sqlx::query(sql);
        for p in &params {
            query = bind_sqlite(query, p);
        }
        let rows = query.fetch_all(&mut *self.0).await?;
        Ok(rows.into_iter().map(row_to_map_sqlite).collect())
    }

    pub async fn commit(self) -> Result<()> {
        self.0.commit().await?;
        Ok(())
    }

    pub async fn rollback(self) -> Result<()> {
        self.0.rollback().await?;
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::DbDomain;
    use std::sync::Arc;

    #[tokio::test]
    async fn sharded_pools_route_tables_to_separate_files() {
        // Enable sharding for this test only.
        std::env::set_var("KIFF_SQLITE_SHARDED", "1");

        let dir = tempfile::tempdir().unwrap();
        let pools = Arc::new(DomainPools::connect(dir.path()).await.unwrap());

        // Create a "k8s" table through the routed k8s handle.
        let k8s_pool = pools.pool_for(DbDomain::K8s);
        k8s_pool
            .execute_sql("CREATE TABLE k8s_cluster (name TEXT)", vec![])
            .await
            .unwrap();
        k8s_pool
            .execute_sql("INSERT INTO k8s_cluster VALUES ('c1')", vec![])
            .await
            .unwrap();

        // The k8s table should exist in k8s.db.
        let k8s_rows = k8s_pool
            .execute_sql(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'k8s_cluster'",
                vec![],
            )
            .await
            .unwrap();
        assert_eq!(k8s_rows.len(), 1);

        // The k8s table should NOT exist in the core DB.
        let core_pool = pools.pool_for(DbDomain::Core);
        let core_rows = core_pool
            .execute_sql(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'k8s_cluster'",
                vec![],
            )
            .await
            .unwrap();
        assert!(core_rows.is_empty());

        // A core table should live in site.db.
        core_pool
            .execute_sql("CREATE TABLE core_user (name TEXT)", vec![])
            .await
            .unwrap();
        let core_rows = core_pool
            .execute_sql(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'core_user'",
                vec![],
            )
            .await
            .unwrap();
        assert_eq!(core_rows.len(), 1);

        // Clean up the env var so later tests use the default non-sharded mode.
        std::env::remove_var("KIFF_SQLITE_SHARDED");
    }

    #[tokio::test]
    async fn for_doctype_routes_by_table_name() {
        std::env::set_var("KIFF_SQLITE_SHARDED", "1");

        let dir = tempfile::tempdir().unwrap();
        let pools = Arc::new(DomainPools::connect(dir.path()).await.unwrap());
        let core = pools.core();

        // Getting a handle for a k8s DocType should resolve to the k8s pool.
        let k8s_handle = core.for_doctype("K8s Cluster");
        k8s_handle
            .execute_sql("CREATE TABLE k8s_cluster (name TEXT)", vec![])
            .await
            .unwrap();

        let k8s_rows = k8s_handle
            .execute_sql(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'k8s_cluster'",
                vec![],
            )
            .await
            .unwrap();
        assert_eq!(k8s_rows.len(), 1);

        std::env::remove_var("KIFF_SQLITE_SHARDED");
    }

    #[tokio::test]
    async fn document_history_routes_to_history_domain() {
        std::env::set_var("KIFF_SQLITE_SHARDED", "1");

        let dir = tempfile::tempdir().unwrap();
        let pools = Arc::new(DomainPools::connect(dir.path()).await.unwrap());
        let core = pools.core();

        // The durable document history table must live in its own history.db
        // file so audit writes never hold the site.db writer lock.
        let history_handle = core.for_doctype("Document History");
        history_handle
            .execute_sql("CREATE TABLE document_history (name TEXT)", vec![])
            .await
            .unwrap();

        let history_rows = history_handle
            .execute_sql(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'document_history'",
                vec![],
            )
            .await
            .unwrap();
        assert_eq!(history_rows.len(), 1);
        assert!(dir.path().join("history.db").exists());

        // The core file must NOT carry the table.
        let core_rows = core
            .execute_sql(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'document_history'",
                vec![],
            )
            .await
            .unwrap();
        assert!(core_rows.is_empty());

        std::env::remove_var("KIFF_SQLITE_SHARDED");
    }

    #[tokio::test]
    async fn insert_doc_backfills_docfield_defaults() {
        let path = format!("/tmp/orm_insert_defaults_{}.db", std::process::id());
        let _ = std::fs::remove_file(&path);
        let pool = DatabasePool::connect_sqlite(&path).await.unwrap();

        // Minimal DocField metadata: User.enabled defaults to 1 (the Frappe
        // DocField default behind the "User is disabled" OAuth failure),
        // simultaneous_sessions to 2, and birth_date carries a "Today"
        // expression default that must NOT be backfilled as a literal.
        pool.execute_sql(
            r#"CREATE TABLE "docfield" (parent TEXT, fieldname TEXT, fieldtype TEXT, "default" TEXT, options TEXT)"#,
            vec![],
        )
        .await
        .unwrap();
        pool.execute_sql(
            r#"INSERT INTO "docfield" (parent, fieldname, fieldtype, "default") VALUES
               ('User', 'enabled', 'Check', '1'),
               ('User', 'simultaneous_sessions', 'Int', '2'),
               ('User', 'birth_date', 'Date', 'Today')"#,
            vec![],
        )
        .await
        .unwrap();

        // Data table with the OLD buggy schema: the Check column defaults to 0.
        pool.execute_sql(
            r#"CREATE TABLE "user" (
                name TEXT PRIMARY KEY, owner TEXT, creation TEXT, modified TEXT,
                docstatus INTEGER DEFAULT 0, enabled INTEGER DEFAULT 0,
                simultaneous_sessions INTEGER DEFAULT 0, birth_date TEXT, email TEXT
            )"#,
            vec![],
        )
        .await
        .unwrap();

        // Insert a doc that does NOT set `enabled`: the DocField default must
        // be applied, not the SQL column default (which would create a
        // disabled user — the OAuth "User is disabled" login failure).
        let mut doc = Document::new("User", "u1@example.com");
        doc.set_field("email", "u1@example.com");
        pool.insert_doc(&doc).await.unwrap();

        let rows = pool
            .execute_sql(
                r#"SELECT enabled, simultaneous_sessions, birth_date
                   FROM "user" WHERE name = 'u1@example.com'"#,
                vec![],
            )
            .await
            .unwrap();
        assert_eq!(rows[0].get("enabled").and_then(|v| v.as_i64()), Some(1));
        assert_eq!(
            rows[0]
                .get("simultaneous_sessions")
                .and_then(|v| v.as_i64()),
            Some(2)
        );
        // Date defaults like "Today" are expressions, not literals — the
        // value must stay unset (NULL reads back as "" for TEXT columns in
        // this runtime) rather than storing the string "Today".
        assert!(
            rows[0]
                .get("birth_date")
                .map(|v| v.is_null() || v.as_str() == Some(""))
                .unwrap_or(true),
            "expression defaults must not be backfilled as literals"
        );

        // A caller that sets the field explicitly wins over the default.
        let mut doc2 = Document::new("User", "u2@example.com");
        doc2.set_field("email", "u2@example.com");
        doc2.set_field("enabled", 0);
        pool.insert_doc(&doc2).await.unwrap();
        let rows2 = pool
            .execute_sql(
                r#"SELECT enabled FROM "user" WHERE name = 'u2@example.com'"#,
                vec![],
            )
            .await
            .unwrap();
        assert_eq!(rows2[0].get("enabled").and_then(|v| v.as_i64()), Some(0));

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn insert_doc_falls_back_when_default_column_is_missing() {
        let path = format!("/tmp/orm_insert_defaults_fallback_{}.db", std::process::id());
        let _ = std::fs::remove_file(&path);
        let pool = DatabasePool::connect_sqlite(&path).await.unwrap();

        // DocField metadata knows a field the physical table predates.
        pool.execute_sql(
            r#"CREATE TABLE "docfield" (parent TEXT, fieldname TEXT, fieldtype TEXT, "default" TEXT, options TEXT)"#,
            vec![],
        )
        .await
        .unwrap();
        pool.execute_sql(
            r#"INSERT INTO "docfield" (parent, fieldname, fieldtype, "default")
               VALUES ('User', 'legacy_flag', 'Check', '1')"#,
            vec![],
        )
        .await
        .unwrap();

        // Table WITHOUT the `legacy_flag` column.
        pool.execute_sql(
            r#"CREATE TABLE "user" (
                name TEXT PRIMARY KEY, owner TEXT, creation TEXT, modified TEXT,
                docstatus INTEGER DEFAULT 0, email TEXT
            )"#,
            vec![],
        )
        .await
        .unwrap();

        // Must not fail with "no such column": the backfill is skipped and
        // the insert proceeds exactly as it did before backfilling existed.
        let mut doc = Document::new("User", "u3@example.com");
        doc.set_field("email", "u3@example.com");
        pool.insert_doc(&doc).await.unwrap();

        let rows = pool
            .execute_sql(
                r#"SELECT email FROM "user" WHERE name = 'u3@example.com'"#,
                vec![],
            )
            .await
            .unwrap();
        assert_eq!(
            rows[0].get("email").and_then(|v| v.as_str()),
            Some("u3@example.com")
        );

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn k8s_domain_includes_cluster_and_node_tables() {
        std::env::set_var("KIFF_SQLITE_SHARDED", "1");

        let dir = tempfile::tempdir().unwrap();
        let pools = Arc::new(DomainPools::connect(dir.path()).await.unwrap());
        let core = pools.core();

        // Tables that back the Kubernetes Cluster DocType route to the k8s
        // domain so node projection does not hold the site.db writer lock.
        let k8s_handle = core.for_doctype("Kubernetes Cluster");
        k8s_handle
            .execute_sql(
                "CREATE TABLE kubernetes_cluster (name TEXT)",
                vec![],
            )
            .await
            .unwrap();
        k8s_handle
            .execute_sql(
                "CREATE TABLE kubernetes_control_plane_node (name TEXT)",
                vec![],
            )
            .await
            .unwrap();
        k8s_handle
            .execute_sql(
                "CREATE TABLE kubernetes_worker_node (name TEXT)",
                vec![],
            )
            .await
            .unwrap();

        let k8s_rows = k8s_handle
            .execute_sql(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name IN ('kubernetes_cluster', 'kubernetes_control_plane_node', 'kubernetes_worker_node')",
                vec![],
            )
            .await
            .unwrap();
        assert_eq!(k8s_rows.len(), 3);

        // They should NOT exist in the core DB.
        let core_pool = pools.pool_for(DbDomain::Core);
        let core_rows = core_pool
            .execute_sql(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name IN ('kubernetes_cluster', 'kubernetes_control_plane_node', 'kubernetes_worker_node')",
                vec![],
            )
            .await
            .unwrap();
        assert!(core_rows.is_empty());

        std::env::remove_var("KIFF_SQLITE_SHARDED");
    }
}
