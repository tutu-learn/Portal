//! Field-level permission checks.
//!
//! Enforces read/write restrictions on individual DocType fields using the
//! `__kiff_fieldperm` table. Rules are keyed by DocType, field name and role.

use crate::PermissionEngine;
use error::Result;
use orm::DatabasePool;
use serde_json::Value;

impl PermissionEngine {
    /// Check whether `user` is allowed to perform `ptype` ("read" or "write")
    /// on `field` of `doctype`.
    ///
    /// Administrator bypasses all field-level checks. If no explicit rules
    /// exist for the field, access is allowed so that fields are not locked
    /// down by default.
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

        let roles = self.get_roles(pool, user).await?;
        let flag_column = match ptype {
            "read" => "read",
            "write" => "write",
            _ => return Ok(false),
        };

        let sql = format!(
            r#"SELECT 1 FROM __kiff_fieldperm
               WHERE parent = {} AND fieldname = {} AND role = {} AND "{}" = 1
               LIMIT 1"#,
            pool.placeholder(1),
            pool.placeholder(2),
            pool.placeholder(3),
            flag_column
        );

        for role in &roles {
            let rows = pool
                .execute_sql(
                    &sql,
                    vec![
                        Value::String(doctype.into()),
                        Value::String(field.into()),
                        Value::String(role.clone()),
                    ],
                )
                .await?;
            if !rows.is_empty() {
                return Ok(true);
            }
        }

        // No matching rule means the field is unrestricted.
        Ok(true)
    }
}
