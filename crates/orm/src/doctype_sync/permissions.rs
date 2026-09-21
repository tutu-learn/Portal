use crate::doctype_sync::helpers::*;
use crate::pool::DatabasePool;
use error::Result;
use tracing::info;

/// Ensure every non-table DocType has at least default permissions.
///
/// DocTypes that already have permissions (loaded from JSON or edited by the
/// user) are left untouched. Missing ones receive Administrator/System Manager
/// full access and read-only access for All.
pub(crate) async fn ensure_docperm_defaults(pool: &DatabasePool) -> Result<()> {
    let rows = pool
        .execute_sql(
            r#"SELECT name FROM "doctype"
               WHERE istable = 0
                 AND name NOT IN ('DocType', 'Patch Log', 'Module Def')
                 AND name NOT IN (SELECT DISTINCT parent FROM __kiff_docperm)"#,
            vec![],
        )
        .await?;

    let sql = r#"
        INSERT INTO __kiff_docperm (
            parent, role, permlevel, "read", "write", "create", "delete", "submit", "cancel", if_owner, "mask", "amend"
        ) VALUES (?, ?, 0, ?, ?, ?, ?, ?, ?, 0, 0, 0)
    "#;

    for mut row in rows {
        let name = row
            .remove("name")
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        for (role, r, w, c, d, s, cn) in [
            ("Administrator", 1, 1, 1, 1, 1, 1),
            ("System Manager", 1, 1, 1, 1, 1, 1),
            ("All", 1, 0, 0, 0, 0, 0),
        ] {
            pool.execute_sql(
                sql,
                vec![
                    val(name.clone()),
                    val(role.into()),
                    num(r),
                    num(w),
                    num(c),
                    num(d),
                    num(s),
                    num(cn),
                ],
            )
            .await?;
        }
    }

    info!("docperm defaults ensured");
    Ok(())
}
