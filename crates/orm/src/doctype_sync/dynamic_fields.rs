use crate::doctype_sync::helpers::*;
use crate::doctype_sync::metadata::insert_docfield;
use crate::pool::DatabasePool;
use error::Result;
use tracing::{debug, info, warn};

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
            "Table MultiSelect",
            "Sebrus Log Viewer Services",
            "User Sebrus Log Viewer Service",
            "Services this user may view logs for when assigned the Sebrus Log Viewer or Sebrus Log Rule Viewer role.",
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

/// One-time migration for the User.sebrus_log_viewer_service field.
///
/// The field was originally a single `Data` value. It is now a
/// `Table MultiSelect` backed by the "User Sebrus Log Viewer Service" child
/// table. Any existing non-empty value on the User record is moved into a
/// single child row so it keeps working after the schema change.
pub(crate) async fn migrate_legacy_log_viewer_service(pool: &DatabasePool) -> Result<()> {
    let user_table = crate::doctype_sync::data_tables::data_table_name("User");
    let child_table =
        crate::doctype_sync::data_tables::data_table_name("User Sebrus Log Viewer Service");

    // If the legacy column does not exist, there is nothing to migrate.
    let legacy_rows = match pool
        .execute_sql(
            &format!(
                r#"SELECT name, sebrus_log_viewer_service FROM "{}" WHERE sebrus_log_viewer_service IS NOT NULL AND sebrus_log_viewer_service != ''"#,
                user_table
            ),
            vec![],
        )
        .await
    {
        Ok(rows) => rows,
        Err(e) => {
            debug!(
                "legacy sebrus_log_viewer_service column not present or not readable: {}",
                e
            );
            return Ok(());
        }
    };

    if legacy_rows.is_empty() {
        return Ok(());
    }

    // Ensure the child table exists before trying to migrate.
    let child_exists = pool
        .execute_sql(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = ?",
            vec![val(child_table.clone())],
        )
        .await
        .map(|rows| !rows.is_empty())
        .unwrap_or(false);
    if !child_exists {
        warn!(
            "cannot migrate legacy sebrus_log_viewer_service values: child table {} does not exist yet",
            child_table
        );
        return Ok(());
    }

    let now = chrono::Utc::now().to_rfc3339();
    let mut migrated = 0usize;
    for row in legacy_rows {
        let user = row_str(&row, "name");
        let service = row_str(&row, "sebrus_log_viewer_service").trim().to_string();
        if user.is_empty() || service.is_empty() {
            continue;
        }

        // Avoid duplicating rows if the migration is rerun.
        let existing = pool
            .execute_sql(
                &format!(
                    r#"SELECT name FROM "{}" WHERE parent = {} AND log_service = {}"#,
                    child_table,
                    pool.placeholder(1),
                    pool.placeholder(2)
                ),
                vec![val(user.clone()), val(service.clone())],
            )
            .await?
            .is_empty();
        if !existing {
            continue;
        }

        let name = uuid::Uuid::new_v4().to_string();
        pool.execute_sql(
            &format!(
                r#"INSERT INTO "{}" (
                    name, creation, modified, modified_by, owner,
                    docstatus, idx, parent, parentfield, parenttype,
                    log_service
                ) VALUES ({}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {})"#,
                child_table,
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
            ),
            vec![
                val(name),
                val(now.clone()),
                val(now.clone()),
                val("Administrator".to_string()),
                val("Administrator".to_string()),
                num(0),
                num(1),
                val(user.clone()),
                val("sebrus_log_viewer_service".to_string()),
                val("User".to_string()),
                val(service),
            ],
        )
        .await?;
        migrated += 1;
    }

    if migrated > 0 {
        // Clear the legacy column values so a future rerun does not duplicate
        // rows. Dropping the column is not necessary and is expensive on SQLite.
        pool.execute_sql(
            &format!(
                r#"UPDATE "{}" SET sebrus_log_viewer_service = '' WHERE sebrus_log_viewer_service IS NOT NULL AND sebrus_log_viewer_service != ''"#,
                user_table
            ),
            vec![],
        )
        .await?;
        info!(
            "migrated {} legacy sebrus_log_viewer_service value(s) to child table",
            migrated
        );
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
                r#"SELECT fieldname, fieldtype, options, idx FROM "docfield" WHERE parent = 'User' AND fieldname = 'sebrus_log_viewer_service'"#,
                vec![],
            )
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        let fieldtype = row_str(&rows[0], "fieldtype");
        assert_eq!(fieldtype, "Table MultiSelect");
        let options = row_str(&rows[0], "options");
        assert_eq!(options, "User Sebrus Log Viewer Service");
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

    /// Legacy single-service values on the User record are moved into the
    /// Table MultiSelect child table on startup.
    #[tokio::test]
    async fn legacy_log_viewer_service_is_migrated_to_child_table() {
        let path = format!("/tmp/orm_migrate_legacy_{}.db", std::process::id());
        let _ = std::fs::remove_file(&path);
        let pool = DatabasePool::connect_sqlite(&path).await.unwrap();

        // Create the legacy User table with the old Data column.
        pool.execute_sql(
            r#"CREATE TABLE "user" (
                name TEXT PRIMARY KEY,
                sebrus_log_viewer_service TEXT
            )"#,
            vec![],
        )
        .await
        .unwrap();
        pool.execute_sql(
            r#"INSERT INTO "user" (name, sebrus_log_viewer_service) VALUES ('test@example.com', 'api-gateway')"#,
            vec![],
        )
        .await
        .unwrap();

        // Create the child table that sync_data_tables would have created.
        pool.execute_sql(
            r#"CREATE TABLE "user_sebrus_log_viewer_service" (
                name TEXT PRIMARY KEY,
                creation TEXT,
                modified TEXT,
                modified_by TEXT,
                owner TEXT,
                docstatus INTEGER DEFAULT 0,
                idx INTEGER DEFAULT 0,
                parent TEXT,
                parentfield TEXT,
                parenttype TEXT,
                log_service TEXT
            )"#,
            vec![],
        )
        .await
        .unwrap();

        migrate_legacy_log_viewer_service(&pool).await.unwrap();

        let children = pool
            .execute_sql(
                r#"SELECT parent, log_service FROM "user_sebrus_log_viewer_service" WHERE parent = 'test@example.com'"#,
                vec![],
            )
            .await
            .unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(row_str(&children[0], "log_service"), "api-gateway");

        let legacy = pool
            .execute_sql(
                r#"SELECT sebrus_log_viewer_service FROM "user" WHERE name = 'test@example.com'"#,
                vec![],
            )
            .await
            .unwrap();
        assert!(legacy.is_empty() || row_str(&legacy[0], "sebrus_log_viewer_service").is_empty());

        let _ = std::fs::remove_file(&path);
    }
}
