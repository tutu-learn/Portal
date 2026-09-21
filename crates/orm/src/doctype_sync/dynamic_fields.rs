use crate::doctype_sync::helpers::*;
use crate::doctype_sync::metadata::insert_docfield;
use crate::pool::DatabasePool;
use error::Result;
use tracing::{info, warn};

/// Schema-driven dynamic field injection.
///
/// Reads rules from `__kiff_dynamic_field`, evaluates their SQL conditions
/// against the current database/schema, and injects matching fields into
/// `docfield`. A generated Client Script per affected DocType moves the
/// injected fields to their configured target location on Desk forms.
pub(crate) async fn ensure_dynamic_fields(pool: &DatabasePool) -> Result<()> {
    create_dynamic_field_table(pool).await?;
    seed_dynamic_field_rules(pool).await?;

    let rules = pool
        .execute_sql(
            r#"SELECT * FROM "__kiff_dynamic_field" WHERE enabled = 1 ORDER BY idx, name"#,
            vec![],
        )
        .await?;

    for rule in rules {
        let doctype = row_str(&rule, "doctype");
        let fieldname = row_str(&rule, "fieldname");
        if doctype.is_empty() || fieldname.is_empty() {
            continue;
        }

        let condition_sql = row_str(&rule, "condition_sql");
        let applies = if condition_sql.is_empty() {
            true
        } else {
            match pool.execute_sql(&condition_sql, vec![]).await {
                Ok(rows) => !rows.is_empty(),
                Err(e) => {
                    warn!(
                        "dynamic field condition failed for {}.{}: {}",
                        doctype, fieldname, e
                    );
                    false
                }
            }
        };

        if !applies {
            info!(
                "skipping dynamic field {}.{}: condition not met",
                doctype, fieldname
            );
            continue;
        }

        let field = serde_json::json!({
            "fieldname": fieldname,
            "fieldtype": row_str(&rule, "fieldtype"),
            "label": row_str(&rule, "label"),
            "options": row_str(&rule, "options"),
            "description": row_str(&rule, "description"),
            "read_only": 0,
            "hidden": 0,
            "reqd": 0,
            "in_list_view": 0
        });

        let existing_rows = pool
            .execute_sql(
                r#"SELECT idx FROM "docfield" WHERE parent = ?"#,
                vec![val(doctype.clone())],
            )
            .await?;
        let max_idx = existing_rows
            .iter()
            .filter_map(|r| {
                r.get("idx").and_then(|v| {
                    v.as_i64()
                        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                })
            })
            .max()
            .unwrap_or(0);

        let idx = (max_idx as usize) + 1;
        insert_docfield(pool, &doctype, &field, idx).await?;
        info!(
            "ensured dynamic field {} on DocType {} at idx {}",
            fieldname, doctype, idx
        );
    }

    ensure_dynamic_field_client_scripts(pool).await?;
    Ok(())
}

async fn create_dynamic_field_table(pool: &DatabasePool) -> Result<()> {
    let sql = r#"
        CREATE TABLE IF NOT EXISTS "__kiff_dynamic_field" (
            name TEXT PRIMARY KEY,
            doctype TEXT NOT NULL,
            fieldname TEXT NOT NULL,
            fieldtype TEXT NOT NULL DEFAULT 'Data',
            label TEXT NOT NULL,
            options TEXT,
            description TEXT,
            condition_sql TEXT,
            target_field TEXT,
            target_section TEXT,
            idx INTEGER DEFAULT 0,
            enabled INTEGER DEFAULT 1,
            UNIQUE(doctype, fieldname)
        )
    "#;
    pool.execute_sql(sql, vec![]).await?;
    Ok(())
}

async fn seed_dynamic_field_rules(pool: &DatabasePool) -> Result<()> {
    let sql = format!(
        r#"
        INSERT INTO "__kiff_dynamic_field" (
            name, doctype, fieldname, fieldtype, label, options, description,
            condition_sql, target_field, target_section, idx, enabled
        ) VALUES ({}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {})
        ON CONFLICT(name) DO UPDATE SET
            doctype=EXCLUDED.doctype,
            fieldname=EXCLUDED.fieldname,
            fieldtype=EXCLUDED.fieldtype,
            label=EXCLUDED.label,
            options=EXCLUDED.options,
            description=EXCLUDED.description,
            condition_sql=EXCLUDED.condition_sql,
            target_field=EXCLUDED.target_field,
            target_section=EXCLUDED.target_section,
            idx=EXCLUDED.idx,
            enabled=EXCLUDED.enabled
    "#,
        pool.placeholder(1),
        pool.placeholder(2),
        pool.placeholder(3),
        pool.placeholder(4),
        pool.placeholder(5),
        pool.placeholder(6),
        pool.placeholder(7),
        pool.placeholder(8),
        pool.placeholder(9),
        pool.placeholder(10),
        pool.placeholder(11),
        pool.placeholder(12),
    );

    let sebrus_logger_installed =
        r#"SELECT name FROM "module_def" WHERE app_name = 'sebrus_logger'"#;

    let rules = vec![
        (
            "user-logger-tab",
            "User",
            "logger_tab",
            "Tab Break",
            "Logger",
            "",
            "",
            sebrus_logger_installed,
            "",
            "",
            1, // idx: insert tab first
            1,
        ),
        (
            "user-sebrus-log-viewer-service",
            "User",
            "sebrus_log_viewer_service",
            "Data",
            "Sebrus Log Viewer Service",
            "",
            "Service this user may view logs for when assigned the Sebrus Log Viewer role.",
            sebrus_logger_installed,
            "logger_tab",
            "",
            2, // idx: insert field after tab
            1,
        ),
    ];

    for rule in rules {
        let params = vec![
            val(rule.0.to_string()),
            val(rule.1.to_string()),
            val(rule.2.to_string()),
            val(rule.3.to_string()),
            val(rule.4.to_string()),
            val(rule.5.to_string()),
            val(rule.6.to_string()),
            val(rule.7.to_string()),
            val(rule.8.to_string()),
            val(rule.9.to_string()),
            num(rule.10),
            num(rule.11),
        ];
        pool.execute_sql(&sql, params).await?;
    }

    info!("seeded dynamic field rules");
    Ok(())
}

/// Generate Client Scripts for every DocType that has active dynamic fields.
///
/// The script moves each injected field to immediately after its configured
/// `target_field` so it appears on the Desk form even when it is not part of
/// the static `field_order` in `user.json`.
async fn ensure_dynamic_field_client_scripts(pool: &DatabasePool) -> Result<()> {
    let now_fn = match pool.dialect() {
        "postgres" => "NOW()",
        _ => "datetime('now')",
    };

    let rows = pool
        .execute_sql(
            r#"
                SELECT DISTINCT doctype FROM "__kiff_dynamic_field"
                WHERE enabled = 1 AND target_field IS NOT NULL AND target_field != ''
            "#,
            vec![],
        )
        .await?;

    for row in rows {
        let doctype = row_str(&row, "doctype");
        if doctype.is_empty() {
            continue;
        }

        let rules = pool
            .execute_sql(
                r#"
                    SELECT fieldname, target_field
                    FROM "__kiff_dynamic_field"
                    WHERE enabled = 1 AND doctype = ? AND target_field IS NOT NULL AND target_field != ''
                "#,
                vec![val(doctype.clone())],
            )
            .await?;

        let mut moves = Vec::new();
        for rule in rules {
            let fieldname = row_str(&rule, "fieldname");
            let target = row_str(&rule, "target_field");
            if fieldname.is_empty() || target.is_empty() {
                continue;
            }
            moves.push(format!(
                r#"{{ fieldname: "{}", after: "{}" }}"#,
                fieldname, target
            ));
        }

        if moves.is_empty() {
            continue;
        }

        let moves_json = moves.join(", ");
        let script = format!(
            r#"frappe.ui.form.on("{}", {{
    refresh: function(frm) {{
        const moves = [{}];
        moves.forEach((move) => {{
            const field = frm.fields_dict[move.fieldname];
            const target = frm.fields_dict[move.after];
            if (field && target && target.$wrapper) {{
                field.$wrapper.insertAfter(target.$wrapper);
            }}
        }});
    }}
}});"#,
            doctype, moves_json
        );

        let name = format!(
            "__kiff-dynamic-fields-{}",
            doctype.to_lowercase().replace(' ', "-")
        );
        let sql = format!(
            r#"
            INSERT INTO "client_script" (
                name, creation, modified, modified_by, owner, docstatus,
                dt, script, view, module, enabled
            ) VALUES ({}, {now_fn}, {now_fn}, 'Administrator', 'Administrator', 0,
                      {}, {}, 'Form', 'Core', 1)
            ON CONFLICT(name) DO UPDATE SET
                modified=EXCLUDED.modified,
                dt=EXCLUDED.dt,
                script=EXCLUDED.script,
                view=EXCLUDED.view,
                module=EXCLUDED.module,
                enabled=EXCLUDED.enabled
        "#,
            pool.placeholder(1),
            pool.placeholder(2),
            pool.placeholder(3),
        );

        pool.execute_sql(&sql, vec![val(name), val(doctype.clone()), val(script)])
            .await?;
        info!("ensured dynamic field client script for {}", doctype);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dynamic field rules are seeded, injected into docfield, and a matching
    /// Client Script is generated so the field is visible on Desk forms.
    #[tokio::test]
    async fn dynamic_fields_are_injected_and_client_script_generated() {
        let path = format!("/tmp/orm_dynamic_fields_{}.db", std::process::id());
        let _ = std::fs::remove_file(&path);
        let pool = DatabasePool::connect_sqlite(&path).await.unwrap();

        crate::doctype_sync::metadata::create_metadata_tables(&pool)
            .await
            .unwrap();
        pool.execute_sql(
            r#"CREATE TABLE IF NOT EXISTS "client_script" (
                name TEXT PRIMARY KEY,
                creation TEXT,
                modified TEXT,
                modified_by TEXT,
                owner TEXT,
                docstatus INTEGER DEFAULT 0,
                idx INTEGER DEFAULT 0,
                dt TEXT,
                script TEXT,
                enabled INTEGER DEFAULT 0,
                "view" TEXT,
                module TEXT
            )"#,
            vec![],
        )
        .await
        .unwrap();
        pool.execute_sql(
            r#"CREATE TABLE IF NOT EXISTS "module_def" (
                name TEXT PRIMARY KEY,
                creation TEXT,
                modified TEXT,
                modified_by TEXT,
                owner TEXT,
                docstatus INTEGER DEFAULT 0,
                module_name TEXT,
                app_name TEXT
            )"#,
            vec![],
        )
        .await
        .unwrap();
        pool.execute_sql(
            r#"INSERT INTO "module_def" (name, app_name) VALUES ('SebrusLogger', 'sebrus_logger')"#,
            vec![],
        )
        .await
        .unwrap();

        insert_docfield(
            &pool,
            "User",
            &serde_json::json!({
                "fieldname": "user_type",
                "fieldtype": "Link",
                "label": "User Type"
            }),
            10,
        )
        .await
        .unwrap();

        ensure_dynamic_fields(&pool).await.unwrap();

        let tab_rows = pool
            .execute_sql(
                r#"SELECT fieldname, idx FROM "docfield" WHERE parent = 'User' AND fieldname = 'logger_tab'"#,
                vec![],
            )
            .await
            .unwrap();
        assert_eq!(tab_rows.len(), 1);
        let tab_idx = tab_rows[0]
            .get("idx")
            .and_then(|v| {
                v.as_i64()
                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            })
            .unwrap_or(0);
        assert!(
            tab_idx > 10,
            "logger_tab should be after user_type (idx 10), got {}",
            tab_idx
        );

        let rows = pool
            .execute_sql(
                r#"SELECT fieldname, idx FROM "docfield" WHERE parent = 'User' AND fieldname = 'sebrus_log_viewer_service'"#,
                vec![],
            )
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        let idx = rows[0]
            .get("idx")
            .and_then(|v| {
                v.as_i64()
                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            })
            .unwrap_or(0);
        assert!(
            idx > tab_idx,
            "sebrus_log_viewer_service should be after logger_tab (idx {}), got {}",
            tab_idx,
            idx
        );

        let scripts = pool
            .execute_sql(
                r#"SELECT dt, script FROM "client_script" WHERE name = '__kiff-dynamic-fields-user'"#,
                vec![],
            )
            .await
            .unwrap();
        assert_eq!(scripts.len(), 1);
        let dt = scripts[0].get("dt").and_then(|v| v.as_str()).unwrap();
        assert_eq!(dt, "User");
        let script = scripts[0].get("script").and_then(|v| v.as_str()).unwrap();
        assert!(script.contains("sebrus_log_viewer_service"));
        assert!(script.contains("logger_tab"));

        let _ = std::fs::remove_file(&path);
    }
}
