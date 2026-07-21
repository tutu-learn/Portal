pub mod field_perms;
pub mod query_conditions;
pub mod roles;
pub mod sod;
pub mod user_perms;

use dashmap::DashMap;
use error::Result;
use orm::DatabasePool;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Default TTL for the per-user roles cache. Role changes made outside the
/// native Rust handlers (e.g. through Python controllers) may not invalidate
/// the cache explicitly, so this bounds the stale window.
const ROLES_CACHE_TTL: Duration = Duration::from_secs(60);

/// Map a permission type to the boolean flag on a DocPerm row.
///
/// For the extra Frappe ptypes (`select`, `report`, `export`, `print`,
/// `email`, `share`) the engine falls back to `read` when the explicit flag
/// is not set. `import` falls back to `create`. This mirrors the sensible
/// defaults Frappe uses for actions that are normally available once a role
/// can read (or create) a DocType.
fn ptype_allowed(perm: &DocPerm, ptype: &str) -> bool {
    match ptype {
        "read" => perm.read,
        "write" => perm.write,
        "create" => perm.create,
        "delete" => perm.delete,
        "submit" => perm.submit,
        "cancel" => perm.cancel,
        "amend" => perm.amend,
        "select" => perm.select || perm.read,
        "report" => perm.report || perm.read,
        "export" => perm.export || perm.read,
        "import" => perm.import || perm.create,
        "print" => perm.print || perm.read,
        "email" => perm.email || perm.read,
        "share" => perm.share || perm.read,
        _ => false,
    }
}

#[derive(Debug, Clone)]
pub struct PermissionEngine {
    roles_cache: Arc<DashMap<String, (Vec<String>, Instant)>>,
    perm_cache: Arc<DashMap<String, Vec<DocPerm>>>,
}

#[derive(Debug, Clone)]
pub struct DocPerm {
    pub parent: String,
    pub role: String,
    pub permlevel: i32,
    pub read: bool,
    pub write: bool,
    pub create: bool,
    pub delete: bool,
    pub submit: bool,
    pub cancel: bool,
    pub if_owner: bool,
    pub select: bool,
    pub report: bool,
    pub export: bool,
    pub import: bool,
    pub share: bool,
    pub print: bool,
    pub email: bool,
    pub mask: bool,
    pub amend: bool,
}

impl PermissionEngine {
    pub fn new() -> Self {
        Self {
            roles_cache: Arc::new(DashMap::new()),
            perm_cache: Arc::new(DashMap::new()),
        }
    }

    pub async fn has_permission(
        &self,
        pool: &DatabasePool,
        user: &str,
        doctype: &str,
        ptype: &str,
        doc: Option<&orm::Document>,
    ) -> Result<bool> {
        if user == "Administrator" {
            return Ok(true);
        }

        let roles = self.get_roles(pool, user).await?;
        let perms = self.get_docperms(pool, doctype).await?;

        for perm in perms {
            if !roles.contains(&perm.role) {
                continue;
            }
            let allowed = ptype_allowed(&perm, ptype);
            if allowed {
                // If owner-only permission, check ownership
                if perm.if_owner {
                    if let Some(d) = doc {
                        if d.owner == user {
                            return Ok(true);
                        }
                    }
                    continue;
                }
                return Ok(true);
            }
        }

        // Row-level role permissions denied; check document sharing.
        if let Some(d) = doc {
            if self
                .has_share_permission(pool, user, doctype, &d.name, ptype)
                .await?
            {
                return Ok(true);
            }
        }

        tracing::debug!("permission denied: {} {} {}", user, doctype, ptype);
        Ok(false)
    }

    /// Check whether `user` has been granted `ptype` ("read" or "write") on a
    /// specific document via DocShare.
    async fn has_share_permission(
        &self,
        pool: &DatabasePool,
        user: &str,
        doctype: &str,
        name: &str,
        ptype: &str,
    ) -> Result<bool> {
        let flag = match ptype {
            "read" | "select" | "report" | "export" | "print" | "email" | "share" => "read",
            "write" | "create" | "delete" | "submit" | "cancel" | "amend" | "import" => "write",
            _ => return Ok(false),
        };
        let sql = format!(
            r#"SELECT 1 FROM __kiff_docshare
               WHERE share_doctype = {} AND share_name = {}
                 AND (user = {} OR everyone = 1)
                 AND "{}" = 1
               LIMIT 1"#,
            pool.placeholder(1),
            pool.placeholder(2),
            pool.placeholder(3),
            flag
        );
        let rows = pool
            .execute_sql(
                &sql,
                vec![
                    Value::String(doctype.into()),
                    Value::String(name.into()),
                    Value::String(user.into()),
                ],
            )
            .await?;
        Ok(!rows.is_empty())
    }

    pub async fn get_roles(&self, pool: &DatabasePool, user: &str) -> Result<Vec<String>> {
        if let Some(entry) = self.roles_cache.get(user) {
            if entry.1.elapsed() < ROLES_CACHE_TTL {
                return Ok(entry.0.clone());
            }
        }

        let mut roles = if user == "Guest" {
            vec!["Guest".into(), "All".into()]
        } else {
            let mut roles = vec![];

            // Administrator implicitly has every available role, matching Frappe.
            if user == "Administrator" {
                match pool
                    .execute_sql(r#"SELECT name FROM "role" WHERE disabled = 0"#, vec![])
                    .await
                {
                    Ok(rows) => {
                        for mut row in rows {
                            if let Some(role) = row
                                .remove("name")
                                .and_then(|v| v.as_str().map(String::from))
                            {
                                roles.push(role);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            "failed to load roles for Administrator: {}, falling back",
                            e
                        );
                        roles.extend([
                            "Administrator".into(),
                            "System Manager".into(),
                            "All".into(),
                        ]);
                    }
                }
            } else {
                // Read assigned roles from the User child table.
                let sql = format!(
                    r#"SELECT role FROM "has_role" WHERE parenttype = 'User' AND parent = {}"#,
                    pool.placeholder(1)
                );
                if let Ok(rows) = pool
                    .execute_sql(&sql, vec![serde_json::Value::String(user.into())])
                    .await
                {
                    for mut row in rows {
                        if let Some(role) = row
                            .remove("role")
                            .and_then(|v| v.as_str().map(String::from))
                        {
                            roles.push(role);
                        }
                    }
                }
            }

            // Automatic roles.
            for auto in ["All", "Guest"] {
                if !roles.iter().any(|r| r == auto) {
                    roles.push(auto.into());
                }
            }

            // System users implicitly get Desk User.
            let user_type_sql = format!(
                r#"SELECT user_type FROM "user" WHERE name = {}"#,
                pool.placeholder(1)
            );
            if let Ok(rows) = pool
                .execute_sql(&user_type_sql, vec![serde_json::Value::String(user.into())])
                .await
            {
                if let Some(row) = rows.into_iter().next() {
                    if row.get("user_type").and_then(|v| v.as_str()) == Some("System User")
                        && !roles.iter().any(|r| r == "Desk User")
                    {
                        roles.push("Desk User".into());
                    }
                }
            }

            roles
        };

        roles.sort_unstable();
        roles.dedup();
        self.roles_cache
            .insert(user.into(), (roles.clone(), Instant::now()));
        Ok(roles)
    }

    /// Invalidate the cached role list for a specific user.
    pub fn clear_roles_cache(&self, user: &str) {
        self.roles_cache.remove(user);
    }

    /// Invalidate the cached role list for every user.
    pub fn clear_roles_cache_all(&self) {
        self.roles_cache.clear();
    }

    pub async fn get_permission_query_conditions(
        &self,
        pool: &DatabasePool,
        user: &str,
        doctype: &str,
    ) -> Result<Option<String>> {
        if user == "Administrator" {
            return Ok(None);
        }

        let roles = self.get_roles(pool, user).await?;
        let perms = self.get_docperms(pool, doctype).await?;

        // Build OR conditions for each role that has read permission. Track
        // whether any role grants full (non-owner-only) read.
        let mut conditions = Vec::new();
        let mut full_read = false;
        for perm in perms {
            if !roles.contains(&perm.role) || !perm.read {
                continue;
            }
            if perm.if_owner {
                conditions.push(format!("owner = '{}'", user));
            } else {
                full_read = true;
            }
        }

        // Apply User Permission link-field restrictions on top of row-level
        // conditions. This lets administrators restrict users to documents that
        // belong to a specific office, practice area, client, etc.
        let user_perm_conditions = self
            .user_permission_conditions(pool, user, doctype)
            .await?;
        conditions.extend(user_perm_conditions);

        if !full_read && conditions.is_empty() {
            // No read permission at all — return a condition that returns nothing
            return Ok(Some("1 = 0".into()));
        }

        if conditions.is_empty() {
            // Full read and no user-permission restrictions.
            return Ok(None);
        }

        // Deduplicate conditions
        conditions.sort_unstable();
        conditions.dedup();

        if conditions.len() == 1 {
            Ok(Some(conditions.into_iter().next().unwrap()))
        } else {
            Ok(Some(format!("({})", conditions.join(" OR "))))
        }
    }

    pub async fn get_docperms(&self, pool: &DatabasePool, doctype: &str) -> Result<Vec<DocPerm>> {
        let cache_key = format!("{}", doctype);
        if let Some(entry) = self.perm_cache.get(&cache_key) {
            return Ok(entry.clone());
        }

        // Merge permissions from three sources, lowest to highest precedence:
        //   1. Frappe `docperm` (DocType JSON defaults)
        //   2. `__kiff_docperm` (Kiff/Strongroom runtime defaults)
        //   3. Frappe `custom_docperm` (user overrides via Permission Manager)
        // This lets the generic REST layer respect changes made through Frappe's
        // Permission Manager while still falling back to seeded defaults.
        let mut merged: HashMap<(String, String, i32, bool), DocPerm> = HashMap::new();

        let docperm_rows = pool
            .execute_sql(
                r#"
                SELECT parent, role, permlevel, "read", "write", "create", "delete", "submit", "cancel",
                       if_owner, "select", "report", "export", "import", "share", "print", "email", "mask", "amend"
                FROM docperm
                WHERE parenttype = 'DocType' AND parent = ?
                "#,
                vec![serde_json::Value::String(doctype.into())],
            )
            .await
            .unwrap_or_default();
        for row in docperm_rows {
            let perm = Self::row_to_docperm(row);
            merged.insert(
                (
                    perm.parent.clone(),
                    perm.role.clone(),
                    perm.permlevel,
                    perm.if_owner,
                ),
                perm,
            );
        }

        let kiff_rows = pool
            .execute_sql(
                r#"
                SELECT parent, role, permlevel, "read", "write", "create", "delete", "submit", "cancel",
                       if_owner, "select", "report", "export", "import", "share", "print", "email", "mask", "amend"
                FROM __kiff_docperm
                WHERE parent = ? OR parent = '*'
                "#,
                vec![serde_json::Value::String(doctype.into())],
            )
            .await?;
        for row in kiff_rows {
            let perm = Self::row_to_docperm(row);
            merged.insert(
                (
                    perm.parent.clone(),
                    perm.role.clone(),
                    perm.permlevel,
                    perm.if_owner,
                ),
                perm,
            );
        }

        let custom_rows = pool
            .execute_sql(
                r#"
                SELECT parent, role, permlevel, "read", "write", "create", "delete", "submit", "cancel",
                       if_owner, "select", "report", "export", "import", "share", "print", "email", "mask", "amend"
                FROM custom_docperm
                WHERE parent = ?
                "#,
                vec![serde_json::Value::String(doctype.into())],
            )
            .await
            .unwrap_or_default();
        for row in custom_rows {
            let perm = Self::row_to_docperm(row);
            merged.insert(
                (
                    perm.parent.clone(),
                    perm.role.clone(),
                    perm.permlevel,
                    perm.if_owner,
                ),
                perm,
            );
        }

        let perms: Vec<DocPerm> = merged.into_values().collect();
        self.perm_cache.insert(cache_key, perms.clone());
        Ok(perms)
    }

    fn row_to_docperm(mut row: HashMap<String, serde_json::Value>) -> DocPerm {
        DocPerm {
            parent: row
                .remove("parent")
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default(),
            role: row
                .remove("role")
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default(),
            permlevel: row
                .remove("permlevel")
                .and_then(|v| v.as_i64().map(|i| i as i32))
                .unwrap_or(0),
            read: row
                .remove("read")
                .and_then(|v| v.as_i64().map(|i| i != 0))
                .unwrap_or(false),
            write: row
                .remove("write")
                .and_then(|v| v.as_i64().map(|i| i != 0))
                .unwrap_or(false),
            create: row
                .remove("create")
                .and_then(|v| v.as_i64().map(|i| i != 0))
                .unwrap_or(false),
            delete: row
                .remove("delete")
                .and_then(|v| v.as_i64().map(|i| i != 0))
                .unwrap_or(false),
            submit: row
                .remove("submit")
                .and_then(|v| v.as_i64().map(|i| i != 0))
                .unwrap_or(false),
            cancel: row
                .remove("cancel")
                .and_then(|v| v.as_i64().map(|i| i != 0))
                .unwrap_or(false),
            if_owner: row
                .remove("if_owner")
                .and_then(|v| v.as_i64().map(|i| i != 0))
                .unwrap_or(false),
            select: row
                .remove("select")
                .and_then(|v| v.as_i64().map(|i| i != 0))
                .unwrap_or(false),
            report: row
                .remove("report")
                .and_then(|v| v.as_i64().map(|i| i != 0))
                .unwrap_or(false),
            export: row
                .remove("export")
                .and_then(|v| v.as_i64().map(|i| i != 0))
                .unwrap_or(false),
            import: row
                .remove("import")
                .and_then(|v| v.as_i64().map(|i| i != 0))
                .unwrap_or(false),
            share: row
                .remove("share")
                .and_then(|v| v.as_i64().map(|i| i != 0))
                .unwrap_or(false),
            print: row
                .remove("print")
                .and_then(|v| v.as_i64().map(|i| i != 0))
                .unwrap_or(false),
            email: row
                .remove("email")
                .and_then(|v| v.as_i64().map(|i| i != 0))
                .unwrap_or(false),
            mask: row
                .remove("mask")
                .and_then(|v| v.as_i64().map(|i| i != 0))
                .unwrap_or(false),
            amend: row
                .remove("amend")
                .and_then(|v| v.as_i64().map(|i| i != 0))
                .unwrap_or(false),
        }
    }

    pub fn clear_perm_cache(&self, doctype: &str) {
        self.perm_cache.remove(doctype);
    }

    /// Return the set of permlevels for which `user` has `ptype` permission on
    /// `doctype`. Administrator receives all common levels. If no explicit
    /// permission exists, permlevel 0 is assumed allowed so existing setups
    /// keep working.
    pub async fn allowed_permlevels(
        &self,
        pool: &DatabasePool,
        user: &str,
        doctype: &str,
        ptype: &str,
    ) -> Result<std::collections::HashSet<i32>> {
        if user == "Administrator" {
            return Ok((0..=9).collect());
        }

        let roles = self.get_roles(pool, user).await?;
        let perms = self.get_docperms(pool, doctype).await?;
        let mut levels = std::collections::HashSet::new();
        for perm in perms {
            if !roles.contains(&perm.role) {
                continue;
            }
            if ptype_allowed(&perm, ptype) {
                levels.insert(perm.permlevel);
            }
        }
        if levels.is_empty() {
            levels.insert(0);
        }
        Ok(levels)
    }

    /// Filter a document's fields in-place, keeping only those the user is
    /// allowed to read based on permlevel. Standard columns (name, owner,
    /// creation, modified, doctype) are always kept.
    pub fn filter_readable_fields(
        &self,
        doc: &mut orm::Document,
        meta: &metadata::doctype::DocType,
        allowed_levels: &std::collections::HashSet<i32>,
    ) {
        let standard: std::collections::HashSet<&str> = [
            "name", "owner", "creation", "modified", "modified_by", "docstatus", "doctype",
        ]
        .into_iter()
        .collect();

        let allowed_fields: std::collections::HashSet<&str> = meta
            .fields
            .iter()
            .filter(|f| standard.contains(f.fieldname.as_str()) || allowed_levels.contains(&f.permlevel))
            .map(|f| f.fieldname.as_str())
            .collect();

        doc.fields.retain(|k, _| {
            standard.contains(k.as_str()) || allowed_fields.contains(k.as_str())
        });
    }

    /// Return the field names the user is allowed to read/write for `doctype`.
    ///
    /// Standard columns are always allowed: they are not listed in
    /// `meta.fields` (Frappe only stores data fields there), so without them
    /// the permlevel intersection in list/report view rejects requests like
    /// `fields=["name","title"]` with 400 "invalid field: name".
    pub fn allowed_fields(
        &self,
        meta: &metadata::doctype::DocType,
        allowed_levels: &std::collections::HashSet<i32>,
    ) -> std::collections::HashSet<String> {
        let mut out: std::collections::HashSet<String> = meta
            .fields
            .iter()
            .filter(|f| allowed_levels.contains(&f.permlevel))
            .map(|f| f.fieldname.clone())
            .collect();
        for standard in [
            "name",
            "owner",
            "creation",
            "modified",
            "modified_by",
            "docstatus",
            "idx",
            "doctype",
        ] {
            out.insert(standard.to_string());
        }
        out
    }

    /// Check whether all `fields` are writable by the user. Returns the first
    /// disallowed field name, if any. Internal fields (starting with `_`) are
    /// ignored because they are never persisted.
    pub fn check_writable_fields(
        &self,
        meta: &metadata::doctype::DocType,
        allowed_levels: &std::collections::HashSet<i32>,
        fields: &std::collections::HashMap<String, serde_json::Value>,
    ) -> Option<String> {
        for name in fields.keys() {
            if name.starts_with('_') {
                continue;
            }
            let permlevel = meta
                .fields
                .iter()
                .find(|f| f.fieldname == *name)
                .map(|f| f.permlevel)
                .unwrap_or(0);
            if !allowed_levels.contains(&permlevel) {
                return Some(name.clone());
            }
        }
        None
    }
}

impl Default for PermissionEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static DB_COUNTER: AtomicUsize = AtomicUsize::new(0);

    async fn setup_test_db() -> error::Result<orm::DatabasePool> {
        let n = DB_COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = format!("/tmp/kiff_perm_test_{}.db", n);
        let _ = std::fs::remove_file(&path);
        let pool = orm::DatabasePool::connect_sqlite(&path).await?;
        orm::migrations::Migrator::run(&pool).await?;
        Ok(pool)
    }

    async fn create_doctype_table(pool: &orm::DatabasePool, doctype: &str) -> error::Result<()> {
        let table = doctype.to_lowercase().replace(" ", "_");
        let table = table.strip_prefix("tab").unwrap_or(&table);
        let sql = format!(
            r#"CREATE TABLE IF NOT EXISTS "{}" (
                name TEXT PRIMARY KEY,
                owner TEXT NOT NULL DEFAULT 'Administrator',
                creation TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                modified TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                docstatus INTEGER NOT NULL DEFAULT 0,
                title TEXT,
                description TEXT,
                status TEXT
            )"#,
            table
        );
        pool.execute_sql(&sql, vec![]).await?;
        Ok(())
    }

    #[tokio::test]
    async fn extra_ptypes_follow_read_and_create() -> Result<()> {
        let pool = setup_test_db().await?;
        create_doctype_table(&pool, "PermDoc").await?;

        pool.execute_sql(
            "DELETE FROM __kiff_docperm WHERE parent = '*' AND role = 'All'",
            vec![],
        )
        .await?;

        pool.execute_sql(
            r#"
            INSERT INTO __kiff_docperm (
                "parent", "role", "permlevel", "read", "write", "create", "delete",
                "submit", "cancel", "if_owner", "select", "report", "export", "import",
                "share", "print", "email", "mask", "amend"
            ) VALUES ('PermDoc', 'All', 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0)
            "#,
            vec![],
        )
        .await?;

        let engine = PermissionEngine::new();

        for ptype in ["select", "report", "export", "print", "email", "share"] {
            let allowed = engine
                .has_permission(&pool, "Guest", "PermDoc", ptype, None)
                .await?;
            assert!(allowed, "{} should follow read", ptype);
        }

        let can_import = engine
            .has_permission(&pool, "Guest", "PermDoc", "import", None)
            .await?;
        assert!(
            !can_import,
            "import should not follow read when create is false"
        );

        pool.execute_sql(
            r#"UPDATE __kiff_docperm SET "create" = 1 WHERE parent = 'PermDoc' AND role = 'All'"#,
            vec![],
        )
        .await?;

        let engine = PermissionEngine::new();
        let can_import = engine
            .has_permission(&pool, "Guest", "PermDoc", "import", None)
            .await?;
        assert!(can_import, "import should follow create");

        pool.execute_sql(
            r#"UPDATE __kiff_docperm SET "read" = 0, "select" = 1 WHERE parent = 'PermDoc' AND role = 'All'"#,
            vec![],
        )
        .await?;

        let engine = PermissionEngine::new();
        let can_select = engine
            .has_permission(&pool, "Guest", "PermDoc", "select", None)
            .await?;
        assert!(can_select, "explicit select should be allowed");
        let can_report = engine
            .has_permission(&pool, "Guest", "PermDoc", "report", None)
            .await?;
        assert!(
            !can_report,
            "report should not be allowed when read is false and report is unset"
        );

        Ok(())
    }

    async fn create_frappe_docperm_tables(pool: &orm::DatabasePool) -> Result<()> {
        pool.execute_sql(
            r#"
            CREATE TABLE IF NOT EXISTS "docperm" (
                name TEXT PRIMARY KEY,
                creation TEXT,
                modified TEXT,
                modified_by TEXT,
                owner TEXT,
                docstatus INTEGER DEFAULT 0,
                idx INTEGER DEFAULT 0,
                _user_tags TEXT,
                _comments TEXT,
                _assign TEXT,
                _liked_by TEXT,
                _seen TEXT,
                parent TEXT,
                parentfield TEXT,
                parenttype TEXT,
                role TEXT,
                if_owner INTEGER DEFAULT 0,
                permlevel INTEGER DEFAULT 0,
                "read" INTEGER DEFAULT 0,
                "write" INTEGER DEFAULT 0,
                "create" INTEGER DEFAULT 0,
                "delete" INTEGER DEFAULT 0,
                submit INTEGER DEFAULT 0,
                cancel INTEGER DEFAULT 0,
                amend INTEGER DEFAULT 0,
                report INTEGER DEFAULT 0,
                export INTEGER DEFAULT 0,
                import INTEGER DEFAULT 0,
                share INTEGER DEFAULT 0,
                print INTEGER DEFAULT 0,
                email INTEGER DEFAULT 0,
                "select" INTEGER DEFAULT 0,
                mask INTEGER DEFAULT 0
            )
            "#,
            vec![],
        )
        .await?;
        pool.execute_sql(
            r#"
            CREATE TABLE IF NOT EXISTS "custom_docperm" (
                name TEXT PRIMARY KEY,
                creation TEXT,
                modified TEXT,
                modified_by TEXT,
                owner TEXT,
                docstatus INTEGER DEFAULT 0,
                idx INTEGER DEFAULT 0,
                _user_tags TEXT,
                _comments TEXT,
                _assign TEXT,
                _liked_by TEXT,
                _seen TEXT,
                role TEXT,
                if_owner INTEGER DEFAULT 0,
                permlevel INTEGER DEFAULT 0,
                "read" INTEGER DEFAULT 0,
                "write" INTEGER DEFAULT 0,
                "create" INTEGER DEFAULT 0,
                "delete" INTEGER DEFAULT 0,
                submit INTEGER DEFAULT 0,
                cancel INTEGER DEFAULT 0,
                amend INTEGER DEFAULT 0,
                mask INTEGER DEFAULT 0,
                report INTEGER DEFAULT 0,
                export INTEGER DEFAULT 0,
                import INTEGER DEFAULT 0,
                share INTEGER DEFAULT 0,
                print INTEGER DEFAULT 0,
                email INTEGER DEFAULT 0,
                parent TEXT,
                "select" INTEGER DEFAULT 0
            )
            "#,
            vec![],
        )
        .await?;
        Ok(())
    }

    #[tokio::test]
    async fn custom_docperm_overrides_kiff_docperm() -> Result<()> {
        let pool = setup_test_db().await?;
        create_doctype_table(&pool, "MergeDoc").await?;
        create_frappe_docperm_tables(&pool).await?;

        // Kiff default: Guest can read but not create.
        pool.execute_sql(
            r#"
            INSERT INTO __kiff_docperm (
                "parent", "role", "permlevel", "read", "write", "create", "delete",
                "submit", "cancel", "if_owner", "select", "report", "export", "import",
                "share", "print", "email", "mask", "amend"
            ) VALUES ('MergeDoc', 'All', 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0)
            "#,
            vec![],
        )
        .await?;

        let engine = PermissionEngine::new();
        let can_create_before = engine
            .has_permission(&pool, "Guest", "MergeDoc", "create", None)
            .await?;
        assert!(!can_create_before, "Kiff default should deny create");

        // Frappe custom_docperm override: Guest can create.
        pool.execute_sql(
            r#"
            INSERT INTO custom_docperm (
                name, parent, role, permlevel, "read", "write", "create", "delete"
            ) VALUES ('merge-all', 'MergeDoc', 'All', 0, 1, 0, 1, 0)
            "#,
            vec![],
        )
        .await?;

        let engine = PermissionEngine::new();
        let can_create_after = engine
            .has_permission(&pool, "Guest", "MergeDoc", "create", None)
            .await?;
        assert!(
            can_create_after,
            "custom_docperm override should grant create"
        );

        Ok(())
    }

    #[test]
    fn allowed_fields_includes_standard_columns() {
        // DocType JSON only lists data fields; standard columns (name,
        // modified, ...) are implicit. List/report views request them
        // explicitly (`fields=["name","title"]`), so they must always be in
        // the allowed set regardless of permlevel.
        let meta: metadata::doctype::DocType = serde_json::from_value(serde_json::json!({
            "name": "Infrastructure Server",
            "module": "Infrastructure",
            "fields": [
                { "fieldname": "title", "fieldtype": "Data" },
                { "fieldname": "admin_note", "fieldtype": "Data", "permlevel": 3 }
            ]
        }))
        .expect("meta deserializes");

        let engine = PermissionEngine::new();
        let levels: std::collections::HashSet<i32> = [0].into_iter().collect();
        let allowed = engine.allowed_fields(&meta, &levels);

        assert!(allowed.contains("name"));
        assert!(allowed.contains("owner"));
        assert!(allowed.contains("modified"));
        assert!(allowed.contains("docstatus"));
        assert!(allowed.contains("idx"));
        assert!(allowed.contains("title"));
        assert!(
            !allowed.contains("admin_note"),
            "permlevel-3 field must stay hidden at level 0"
        );
    }
}
