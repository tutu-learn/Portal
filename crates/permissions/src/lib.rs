pub mod field_perms;
pub mod query_conditions;
pub mod roles;
pub mod sod;
pub mod user_perms;

use dashmap::DashMap;
use error::{Result, RuntimeError};
use orm::DatabasePool;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct PermissionEngine {
    roles_cache: Arc<DashMap<String, Vec<String>>>,
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
            let allowed = match ptype {
                "read" => perm.read,
                "write" => perm.write,
                "create" => perm.create,
                "delete" => perm.delete,
                "submit" => perm.submit,
                "cancel" => perm.cancel,
                _ => false,
            };
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

        tracing::debug!("permission denied: {} {} {}", user, doctype, ptype);
        Ok(false)
    }

    pub async fn get_roles(&self, pool: &DatabasePool, user: &str) -> Result<Vec<String>> {
        if let Some(entry) = self.roles_cache.get(user) {
            return Ok(entry.clone());
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
        self.roles_cache.insert(user.into(), roles.clone());
        Ok(roles)
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

        // Build OR conditions for each role that has read permission
        let mut conditions = Vec::new();
        for perm in perms {
            if !roles.contains(&perm.role) || !perm.read {
                continue;
            }
            if perm.if_owner {
                conditions.push(format!("owner = '{}'", user));
            } else {
                // Full read permission — no condition needed
                return Ok(None);
            }
        }

        if conditions.is_empty() {
            // No read permission at all — return a condition that returns nothing
            return Ok(Some("1 = 0".into()));
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

    pub async fn check_field_permission(
        &self,
        pool: &DatabasePool,
        user: &str,
        doctype: &str,
        field: &str,
        ptype: &str,
    ) -> Result<bool> {
        if user == "Administrator" {
            return Ok(true);
        }
        // TODO: read field-level permissions from __kiff_fieldperm table
        tracing::debug!(
            "field permission: {} {}.{} {} — allowed",
            user,
            doctype,
            field,
            ptype
        );
        Ok(true)
    }

    pub async fn get_docperms(&self, pool: &DatabasePool, doctype: &str) -> Result<Vec<DocPerm>> {
        let cache_key = format!("{}", doctype);
        if let Some(entry) = self.perm_cache.get(&cache_key) {
            return Ok(entry.clone());
        }

        let sql = r#"
            SELECT parent, role, permlevel, "read", "write", "create", "delete", "submit", "cancel",
                   if_owner, "select", "report", "export", "import", "share", "print", "email", "mask", "amend"
            FROM __kiff_docperm
            WHERE parent = ? OR parent = '*'
        "#;
        let rows = pool
            .execute_sql(sql, vec![serde_json::Value::String(doctype.into())])
            .await?;
        let perms: Vec<DocPerm> = rows
            .into_iter()
            .map(|mut row| DocPerm {
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
            })
            .collect();

        self.perm_cache.insert(cache_key, perms.clone());
        Ok(perms)
    }

    pub fn clear_perm_cache(&self, doctype: &str) {
        self.perm_cache.remove(doctype);
    }
}

impl Default for PermissionEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct SodEngine;

impl SodEngine {
    pub fn new() -> Self {
        Self
    }

    pub async fn check_sod(
        &self,
        _pool: &DatabasePool,
        user: &str,
        doctype: &str,
        action: &str,
    ) -> Result<bool> {
        // TODO: enforce separation of duties rules from __kiff_sod table
        tracing::debug!("SOD check: {} {} {} — allowed", user, doctype, action);
        Ok(true)
    }
}

impl Default for SodEngine {
    fn default() -> Self {
        Self::new()
    }
}
