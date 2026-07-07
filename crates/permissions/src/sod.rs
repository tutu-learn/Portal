//! Separation of Duties engine.
//!
//! Prevents a single user from holding conflicting roles for a given DocType
//! by checking the `__kiff_sod` table. Each row declares that `role_a` and
//! `role_b` may not both be assigned to the same user.

use crate::PermissionEngine;
use error::Result;
use orm::DatabasePool;
use serde_json::Value;

impl PermissionEngine {
    /// Return `Ok(false)` if the user holds both roles of any SOD conflict
    /// rule for `doctype` and `action`. Returns `Ok(true)` otherwise.
    pub async fn check_sod(
        &self,
        pool: &DatabasePool,
        user: &str,
        doctype: &str,
        _action: &str,
    ) -> Result<bool> {
        if user == "Administrator" {
            return Ok(true);
        }

        let roles = self.get_roles(pool, user).await?;
        if roles.len() < 2 {
            return Ok(true);
        }

        let rows = pool
            .execute_sql(
                r#"SELECT role_a, role_b FROM __kiff_sod WHERE doctype = ?"#,
                vec![Value::String(doctype.into())],
            )
            .await?;

        for mut row in rows {
            let a = row
                .remove("role_a")
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default();
            let b = row
                .remove("role_b")
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default();
            if a.is_empty() || b.is_empty() {
                continue;
            }
            let has_a = roles.iter().any(|r| r == &a);
            let has_b = roles.iter().any(|r| r == &b);
            if has_a && has_b {
                tracing::warn!(
                    user = %user,
                    doctype = %doctype,
                    role_a = %a,
                    role_b = %b,
                    "SOD conflict detected"
                );
                return Ok(false);
            }
        }

        Ok(true)
    }
}
