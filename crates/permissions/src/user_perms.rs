//! User-specific permission overrides.
//!
//! Frappe-style User Permission rules restrict the documents a user can see by
//! link-field values (e.g. only Matters for a given Office). This module loads
//! the rules and turns them into SQL conditions.

use crate::PermissionEngine;
use error::Result;
use orm::DatabasePool;
use serde_json::Value;
use std::collections::HashMap;

/// A single User Permission rule.
#[derive(Debug, Clone)]
pub struct UserPermission {
    pub user: String,
    pub allow: String,
    pub for_value: String,
    pub applicable_for: Option<String>,
}

impl PermissionEngine {
    /// Load active User Permission rules for a user.
    pub async fn load_user_permissions(
        &self,
        pool: &DatabasePool,
        user: &str,
    ) -> Result<Vec<UserPermission>> {
        let sql = format!(
            r#"SELECT user, allow, for_value, applicable_for
               FROM "user_permission"
               WHERE user = {} AND IFNULL(is_default, 0) = 0"#,
            pool.placeholder(1)
        );
        let rows = pool
            .execute_sql(&sql, vec![Value::String(user.into())])
            .await?;
        Ok(rows
            .into_iter()
            .filter_map(|mut row| {
                Some(UserPermission {
                    user: row.remove("user")?.as_str()?.to_string(),
                    allow: row.remove("allow")?.as_str()?.to_string(),
                    for_value: row.remove("for_value")?.as_str()?.to_string(),
                    applicable_for: row.remove("applicable_for").and_then(|v| v.as_str().map(String::from)),
                })
            })
            .collect())
    }

    /// Build SQL conditions that restrict `doctype` documents to the link-field
    /// values allowed by the user's User Permission rules.
    ///
    /// Returns an empty vector when no rules apply so callers can skip the
    /// restriction entirely.
    pub async fn user_permission_conditions(
        &self,
        pool: &DatabasePool,
        user: &str,
        doctype: &str,
    ) -> Result<Vec<String>> {
        if user == "Administrator" {
            return Ok(vec![]);
        }

        let rules = self.load_user_permissions(pool, user).await?;
        if rules.is_empty() {
            return Ok(vec![]);
        }

        // Group rules by the DocType they allow (e.g. "Office").
        let mut by_allow: HashMap<String, Vec<String>> = HashMap::new();
        for rule in rules {
            if let Some(ref applicable) = rule.applicable_for {
                if applicable != doctype {
                    continue;
                }
            }
            by_allow
                .entry(rule.allow)
                .or_default()
                .push(rule.for_value);
        }

        let mut conditions = Vec::new();
        for (allowed_doctype, values) in by_allow {
            // Find link fields in the current DocType that point to the allowed DocType.
            let field_sql = format!(
                r#"SELECT fieldname FROM "docfield"
                   WHERE parent = {} AND fieldtype = 'Link' AND options = {}"#,
                pool.placeholder(1),
                pool.placeholder(2)
            );
            let field_rows = pool
                .execute_sql(
                    &field_sql,
                    vec![
                        Value::String(doctype.into()),
                        Value::String(allowed_doctype.clone()),
                    ],
                )
                .await?;

            for mut row in field_rows {
                if let Some(fieldname) = row.remove("fieldname").and_then(|v| v.as_str().map(String::from)) {
                    let escaped: Vec<String> = values
                        .iter()
                        .map(|v| v.replace('\'', "''"))
                        .collect();
                    let cond = if escaped.len() == 1 {
                        format!("\"{}\" = '{}'", fieldname, escaped[0])
                    } else {
                        format!(
                            "\"{}\" IN ({})",
                            fieldname,
                            escaped.iter().map(|v| format!("'{}'", v)).collect::<Vec<_>>().join(", ")
                        )
                    };
                    conditions.push(cond);
                }
            }
        }

        Ok(conditions)
    }
}
