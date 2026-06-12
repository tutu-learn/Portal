use crate::pool::DatabasePool;
use error::{Result, RuntimeError};
use tracing::{info, warn};

/// Main entry point: sync metadata tables, create all data tables, insert seed data.
pub async fn sync_all(pool: &DatabasePool) -> Result<()> {
    info!("syncing frappe doctypes");
    sync_metadata(pool).await?;
    sync_data_tables(pool).await?;
    insert_seed_data(pool).await?;
    info!("doctype sync complete");
    Ok(())
}

// ------------------------------------------------------------------
// 1. Metadata sync — create doctype/docfield tables and populate
//    from JSON files in apps/frappe/frappe/*/doctype/
// ------------------------------------------------------------------

async fn sync_metadata(pool: &DatabasePool) -> Result<()> {
    create_metadata_tables(pool).await?;

    let base = std::path::PathBuf::from("apps/frappe/frappe");
    if !base.exists() {
        warn!("frappe app path not found at {}", base.display());
        return Ok(());
    }

    let mut synced = 0usize;
    let mut fields_synced = 0usize;

    let entries = match std::fs::read_dir(&base) {
        Ok(e) => e,
        Err(e) => {
            warn!("failed to read frappe modules dir: {}", e);
            return Ok(());
        }
    };

    for entry in entries.flatten() {
        let doctype_dir = entry.path().join("doctype");
        if !doctype_dir.exists() {
            continue;
        }

        let doctypes = match std::fs::read_dir(&doctype_dir) {
            Ok(d) => d,
            Err(_) => continue,
        };

        for dt_entry in doctypes.flatten() {
            let path = dt_entry.path();
            let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let json_path = path.join(format!("{}.json", fname));
            if !json_path.exists() {
                continue;
            }

            let content = match tokio::fs::read_to_string(&json_path).await {
                Ok(c) => c,
                Err(e) => {
                    warn!("failed to read {}: {}", json_path.display(), e);
                    continue;
                }
            };

            let doc: serde_json::Value = match serde_json::from_str(&content) {
                Ok(d) => d,
                Err(e) => {
                    warn!("failed to parse {}: {}", json_path.display(), e);
                    continue;
                }
            };

            let doctype_name = doc.get("name").and_then(|n| n.as_str()).unwrap_or(fname);

            if let Err(e) = insert_doctype(pool, &doc).await {
                warn!("failed to insert doctype from {}: {}", json_path.display(), e);
                continue;
            }
            synced += 1;

            if let Some(fields) = doc.get("fields").and_then(|f| f.as_array()) {
                for (idx, field) in fields.iter().enumerate() {
                    if let Err(e) = insert_docfield(pool, doctype_name, field, idx).await {
                        warn!("failed to insert docfield for {}: {}", doctype_name, e);
                        continue;
                    }
                    fields_synced += 1;
                }
            }
        }
    }

    info!("synced {} doctypes with {} fields into metadata tables", synced, fields_synced);
    Ok(())
}

async fn create_metadata_tables(pool: &DatabasePool) -> Result<()> {
    let doctype_sql = r#"
        CREATE TABLE IF NOT EXISTS "doctype" (
            name TEXT PRIMARY KEY,
            creation TEXT,
            modified TEXT,
            modified_by TEXT,
            owner TEXT,
            docstatus INTEGER DEFAULT 0,
            module TEXT,
            autoname TEXT,
            naming_rule TEXT,
            istable INTEGER DEFAULT 0,
            issingle INTEGER DEFAULT 0,
            is_submittable INTEGER DEFAULT 0,
            is_tree INTEGER DEFAULT 0,
            editable_grid INTEGER DEFAULT 0,
            track_changes INTEGER DEFAULT 0,
            track_seen INTEGER DEFAULT 0,
            track_views INTEGER DEFAULT 0,
            engine TEXT,
            sort_field TEXT,
            sort_order TEXT,
            document_type TEXT,
            description TEXT,
            icon TEXT,
            color TEXT,
            read_only INTEGER DEFAULT 0,
            in_create INTEGER DEFAULT 0,
            custom INTEGER DEFAULT 0,
            beta INTEGER DEFAULT 0,
            is_virtual INTEGER DEFAULT 0,
            queue_in_background INTEGER DEFAULT 0,
            default_print_format TEXT,
            search_fields TEXT,
            title_field TEXT,
            image_field TEXT,
            timeline_field TEXT,
            sortable INTEGER DEFAULT 1
        )
    "#;
    pool.execute_sql(doctype_sql, vec![]).await?;

    let docfield_sql = r#"
        CREATE TABLE IF NOT EXISTS "docfield" (
            name TEXT PRIMARY KEY,
            creation TEXT,
            modified TEXT,
            modified_by TEXT,
            owner TEXT,
            docstatus INTEGER DEFAULT 0,
            parent TEXT,
            parentfield TEXT,
            parenttype TEXT,
            idx INTEGER DEFAULT 0,
            fieldname TEXT,
            fieldtype TEXT,
            label TEXT,
            options TEXT,
            reqd INTEGER DEFAULT 0,
            read_only INTEGER DEFAULT 0,
            hidden INTEGER DEFAULT 0,
            in_list_view INTEGER DEFAULT 0,
            in_standard_filter INTEGER DEFAULT 0,
            in_preview INTEGER DEFAULT 0,
            in_global_search INTEGER DEFAULT 0,
            in_filter INTEGER DEFAULT 0,
            bold INTEGER DEFAULT 0,
            italic INTEGER DEFAULT 0,
            no_copy INTEGER DEFAULT 0,
            allow_in_quick_entry INTEGER DEFAULT 0,
            translatable INTEGER DEFAULT 0,
            collapsible INTEGER DEFAULT 0,
            "unique" INTEGER DEFAULT 0,
            set_only_once INTEGER DEFAULT 0,
            remember_last_selected_value INTEGER DEFAULT 0,
            ignore_user_permissions INTEGER DEFAULT 0,
            allow_on_submit INTEGER DEFAULT 0,
            report_hide INTEGER DEFAULT 0,
            search_index INTEGER DEFAULT 0,
            show_dashboard INTEGER DEFAULT 0,
            "default" TEXT,
            depends_on TEXT,
            description TEXT,
            fetch_from TEXT,
            fetch_if_empty INTEGER DEFAULT 0,
            mandatory_depends_on TEXT,
            read_only_depends_on TEXT,
            placeholder TEXT,
            tooltip TEXT,
            is_system_generated INTEGER DEFAULT 0
        )
    "#;
    pool.execute_sql(docfield_sql, vec![]).await?;

    pool.execute_sql(
        "CREATE INDEX IF NOT EXISTS idx_docfield_parent ON docfield(parent)",
        vec![],
    ).await?;

    Ok(())
}

async fn insert_doctype(pool: &DatabasePool, doc: &serde_json::Value) -> Result<()> {
    let name = json_str(doc, "name");
    if name.is_empty() {
        return Err(RuntimeError::NotFound("doctype missing name".into()));
    }

    let sql = r#"
        INSERT OR REPLACE INTO "doctype" (
            name, creation, modified, modified_by, owner, docstatus,
            module, autoname, naming_rule, istable, issingle, is_submittable,
            is_tree, editable_grid, track_changes, track_seen, track_views,
            engine, sort_field, sort_order, document_type, description,
            icon, color, read_only, in_create, custom, beta, is_virtual,
            queue_in_background, default_print_format, search_fields,
            title_field, image_field, timeline_field, sortable
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    "#;

    let params = vec![
        val(name),
        val(json_str(doc, "creation")),
        val(json_str(doc, "modified")),
        val(json_str(doc, "modified_by")),
        val(json_str(doc, "owner")),
        num(json_i64(doc, "docstatus")),
        val(json_str(doc, "module")),
        val(json_str(doc, "autoname")),
        val(json_str(doc, "naming_rule")),
        num(json_i64(doc, "istable")),
        num(json_i64(doc, "issingle")),
        num(json_i64(doc, "is_submittable")),
        num(json_i64(doc, "is_tree")),
        num(json_i64(doc, "editable_grid")),
        num(json_i64(doc, "track_changes")),
        num(json_i64(doc, "track_seen")),
        num(json_i64(doc, "track_views")),
        val(json_str(doc, "engine")),
        val(json_str(doc, "sort_field")),
        val(json_str(doc, "sort_order")),
        val(json_str(doc, "document_type")),
        val(json_str(doc, "description")),
        val(json_str(doc, "icon")),
        val(json_str(doc, "color")),
        num(json_i64(doc, "read_only")),
        num(json_i64(doc, "in_create")),
        num(json_i64(doc, "custom")),
        num(json_i64(doc, "beta")),
        num(json_i64(doc, "is_virtual")),
        num(json_i64(doc, "queue_in_background")),
        val(json_str(doc, "default_print_format")),
        val(json_str(doc, "search_fields")),
        val(json_str(doc, "title_field")),
        val(json_str(doc, "image_field")),
        val(json_str(doc, "timeline_field")),
        num(json_i64(doc, "sortable")),
    ];

    pool.execute_sql(sql, params).await?;
    Ok(())
}

async fn insert_docfield(
    pool: &DatabasePool,
    parent: &str,
    field: &serde_json::Value,
    idx: usize,
) -> Result<()> {
    let fieldname = json_str(field, "fieldname");
    let name = if fieldname.is_empty() {
        format!("{}-field-{}", parent, idx)
    } else {
        format!("{}-{}", parent, fieldname)
    };

    let sql = r#"
        INSERT OR REPLACE INTO "docfield" (
            name, creation, modified, modified_by, owner, docstatus,
            parent, parentfield, parenttype, idx, fieldname, fieldtype,
            label, options, reqd, read_only, hidden, in_list_view,
            in_standard_filter, in_preview, in_global_search, in_filter,
            bold, italic, no_copy, allow_in_quick_entry, translatable,
            collapsible, "unique", set_only_once, remember_last_selected_value,
            ignore_user_permissions, allow_on_submit, report_hide,
            search_index, show_dashboard, "default", depends_on, description,
            fetch_from, fetch_if_empty, mandatory_depends_on,
            read_only_depends_on, placeholder, tooltip, is_system_generated
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    "#;

    let params = vec![
        val(name),
        val(json_str(field, "creation")),
        val(json_str(field, "modified")),
        val(json_str(field, "modified_by")),
        val(json_str(field, "owner")),
        num(json_i64(field, "docstatus")),
        val(parent.to_string()),
        val("fields".to_string()),
        val("DocType".to_string()),
        num(idx as i64),
        val(fieldname),
        val(json_str(field, "fieldtype")),
        val(json_str(field, "label")),
        val(json_str(field, "options")),
        num(json_i64(field, "reqd")),
        num(json_i64(field, "read_only")),
        num(json_i64(field, "hidden")),
        num(json_i64(field, "in_list_view")),
        num(json_i64(field, "in_standard_filter")),
        num(json_i64(field, "in_preview")),
        num(json_i64(field, "in_global_search")),
        num(json_i64(field, "in_filter")),
        num(json_i64(field, "bold")),
        num(json_i64(field, "italic")),
        num(json_i64(field, "no_copy")),
        num(json_i64(field, "allow_in_quick_entry")),
        num(json_i64(field, "translatable")),
        num(json_i64(field, "collapsible")),
        num(json_i64(field, "unique")),
        num(json_i64(field, "set_only_once")),
        num(json_i64(field, "remember_last_selected_value")),
        num(json_i64(field, "ignore_user_permissions")),
        num(json_i64(field, "allow_on_submit")),
        num(json_i64(field, "report_hide")),
        num(json_i64(field, "search_index")),
        num(json_i64(field, "show_dashboard")),
        val(json_str(field, "default")),
        val(json_str(field, "depends_on")),
        val(json_str(field, "description")),
        val(json_str(field, "fetch_from")),
        num(json_i64(field, "fetch_if_empty")),
        val(json_str(field, "mandatory_depends_on")),
        val(json_str(field, "read_only_depends_on")),
        val(json_str(field, "placeholder")),
        val(json_str(field, "tooltip")),
        num(json_i64(field, "is_system_generated")),
    ];

    pool.execute_sql(sql, params).await?;
    Ok(())
}

// ------------------------------------------------------------------
// 2. Dynamic data table creation — reads metadata and creates/updates
//    the actual document tables for every doctype.
// ------------------------------------------------------------------

async fn sync_data_tables(pool: &DatabasePool) -> Result<()> {
    info!("syncing data tables from metadata");

    // Read all doctypes from metadata
    let rows = pool.execute_sql(
        "SELECT name, istable FROM \"doctype\"",
        vec![],
    ).await?;

    let mut created = 0usize;
    for row in rows {
        let doctype_name = row.get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let istable = row.get("istable")
            .and_then(|v| v.as_i64())
            .unwrap_or(0) != 0;
        if doctype_name.is_empty() {
            continue;
        }

        // Read fields for this doctype from metadata
        let field_rows = pool.execute_sql(
            "SELECT fieldname, fieldtype FROM \"docfield\" WHERE parent = ? ORDER BY idx",
            vec![serde_json::Value::String(doctype_name.into())],
        ).await?;

        let fields: Vec<(String, String)> = field_rows.into_iter().filter_map(|mut r| {
            let fname = r.remove("fieldname").and_then(|v| v.as_str().map(String::from))?;
            let ftype = r.remove("fieldtype").and_then(|v| v.as_str().map(String::from))?;
            Some((fname, ftype))
        }).collect();

        if let Err(e) = create_data_table(pool, doctype_name, istable, &fields).await {
            warn!("failed to create data table for {}: {}", doctype_name, e);
        } else {
            created += 1;
        }
    }

    info!("created/verified {} data tables", created);
    Ok(())
}

async fn create_data_table(
    pool: &DatabasePool,
    doctype_name: &str,
    istable: bool,
    fields: &[(String, String)],
) -> Result<()> {
    let table = data_table_name(doctype_name);

    let name_col = if istable {
        "name TEXT".to_string()
    } else {
        "name TEXT PRIMARY KEY".to_string()
    };

    let mut expected_cols: Vec<(String, String)> = vec![
        ("name".into(), name_col),
        ("creation".into(), "creation TEXT".into()),
        ("modified".into(), "modified TEXT".into()),
        ("modified_by".into(), "modified_by TEXT".into()),
        ("owner".into(), "owner TEXT".into()),
        ("docstatus".into(), "docstatus INTEGER DEFAULT 0".into()),
    ];

    if istable {
        expected_cols.push(("parent".into(), "parent TEXT".into()));
        expected_cols.push(("parentfield".into(), "parentfield TEXT".into()));
        expected_cols.push(("parenttype".into(), "parenttype TEXT".into()));
        expected_cols.push(("idx".into(), "idx INTEGER DEFAULT 0".into()));
    }

    for (fieldname, fieldtype) in fields {
        if is_ui_or_child_field(fieldtype) {
            continue;
        }
        let col_name = quote_if_reserved(fieldname);
        let sql_type = fieldtype_to_sql(fieldtype);
        expected_cols.push((fieldname.to_string(), format!("{} {}", col_name, sql_type)));
    }

    // Check if table already exists
    let check_sql = format!("PRAGMA table_info(\"{}\")", table);
    let existing = pool.execute_sql(&check_sql, vec![]).await?;

    if existing.is_empty() {
        // Table doesn't exist — create it
        let col_defs: Vec<String> = expected_cols.iter().map(|(_, def)| def.clone()).collect();
        let sql = format!(
            "CREATE TABLE IF NOT EXISTS \"{}\" (\n    {}\n)",
            table,
            col_defs.join(",\n    ")
        );
        pool.execute_sql(&sql, vec![]).await?;
        return Ok(());
    }

    // Table exists — check for missing columns and add them
    let existing_names: Vec<String> = existing.iter().filter_map(|c| {
        c.get("name").and_then(|v| v.as_str()).map(String::from)
    }).collect();

    let mut needs_recreate = false;
    for (col_name, col_def) in &expected_cols {
        if existing_names.contains(&quote_if_reserved(col_name)) || existing_names.contains(col_name) {
            continue;
        }

        // Column missing — try ALTER TABLE ADD COLUMN
        let alter_sql = format!("ALTER TABLE \"{}\" ADD COLUMN {}", table, col_def);
        match pool.execute_sql(&alter_sql, vec![]).await {
            Ok(_) => info!("added column {} to {}", col_name, table),
            Err(e) => {
                warn!("cannot add column {} to {}: {}. table will be recreated.", col_name, table, e);
                needs_recreate = true;
                break;
            }
        }
    }

    if needs_recreate {
        recreate_table_with_migration(pool, &table, &expected_cols).await?;
    }

    Ok(())
}

async fn recreate_table_with_migration(
    pool: &DatabasePool,
    table: &str,
    expected_cols: &[(String, String)],
) -> Result<()> {
    warn!("recreating table {} with migration", table);

    let temp_table = format!("{}__tmp", table);

    // Create temp table with new schema
    let col_defs: Vec<String> = expected_cols.iter().map(|(_, def)| def.clone()).collect();
    let create_sql = format!(
        "CREATE TABLE \"{}\" (\n    {}\n)",
        temp_table,
        col_defs.join(",\n    ")
    );
    pool.execute_sql(&create_sql, vec![]).await?;

    // Copy data from old table, matching columns that exist in both
    let pragma_sql = format!("PRAGMA table_info(\"{}\")", table);
    let old_cols = pool.execute_sql(&pragma_sql, vec![]).await?;
    let old_names: Vec<String> = old_cols.iter().filter_map(|c| {
        c.get("name").and_then(|v| v.as_str()).map(String::from)
    }).collect();

    let common_cols: Vec<String> = expected_cols.iter()
        .map(|(name, _)| name.clone())
        .filter(|name| old_names.contains(name))
        .collect();

    if !common_cols.is_empty() {
        let cols = common_cols.join(", ");
        let copy_sql = format!(
            "INSERT INTO \"{}\" ({}) SELECT {} FROM \"{}\"",
            temp_table, cols, cols, table
        );
        let _ = pool.execute_sql(&copy_sql, vec![]).await;
    }

    // Drop old and rename temp
    pool.execute_sql(&format!("DROP TABLE \"{}\"", table), vec![]).await?;
    pool.execute_sql(&format!("ALTER TABLE \"{}\" RENAME TO \"{}\"", temp_table, table), vec![]).await?;

    info!("table {} migrated successfully", table);
    Ok(())
}

fn data_table_name(doctype: &str) -> String {
    let name = doctype.to_lowercase().replace(" ", "_");
    name.strip_prefix("tab").unwrap_or(&name).to_string()
}

fn is_ui_or_child_field(fieldtype: &str) -> bool {
    matches!(
        fieldtype,
        "Table"
            | "Table MultiSelect"
            | "Section Break"
            | "Column Break"
            | "Tab Break"
            | "Heading"
            | "HTML"
            | "Button"
    )
}

fn fieldtype_to_sql(fieldtype: &str) -> &'static str {
    match fieldtype {
        "Check" | "Int" | "Rating" => "INTEGER DEFAULT 0",
        "Float" | "Currency" | "Percent" => "REAL DEFAULT 0.0",
        _ => "TEXT",
    }
}

fn quote_if_reserved(name: &str) -> String {
    const RESERVED: &[&str] = &[
        "abort", "action", "add", "after", "all", "alter", "analyze", "and", "as", "asc",
        "attach", "autoincrement", "before", "begin", "between", "by", "cascade", "case",
        "cast", "check", "collate", "column", "commit", "conflict", "constraint", "create",
        "cross", "current_date", "current_time", "current_timestamp", "database", "default",
        "deferrable", "deferred", "delete", "desc", "detach", "distinct", "drop", "each",
        "else", "end", "escape", "except", "exclusive", "exists", "explain", "fail", "for",
        "foreign", "from", "full", "glob", "group", "having", "if", "ignore", "immediate",
        "in", "index", "indexed", "initially", "inner", "insert", "instead", "intersect",
        "into", "is", "isnull", "join", "key", "left", "like", "limit", "match", "natural",
        "no", "not", "notnull", "null", "of", "offset", "on", "or", "order", "outer", "plan",
        "pragma", "primary", "query", "raise", "recursive", "references", "regexp", "reindex",
        "release", "rename", "replace", "restrict", "right", "rollback", "row", "savepoint",
        "select", "set", "table", "temp", "temporary", "then", "to", "transaction", "trigger",
        "union", "unique", "update", "using", "vacuum", "values", "view", "virtual", "when",
        "where", "with", "without",
    ];
    let lower = name.to_lowercase();
    if RESERVED.contains(&lower.as_str()) {
        format!("\"{}\"", name)
    } else {
        name.to_string()
    }
}

// ------------------------------------------------------------------
// 3. Seed data — minimal records needed for the desk to boot
// ------------------------------------------------------------------

async fn insert_seed_data(pool: &DatabasePool) -> Result<()> {
    insert_core_users_and_roles(pool).await?;
    insert_module_defs(pool).await?;
    insert_user_types(pool).await?;
    insert_workflow_defaults(pool).await?;
    insert_genders_and_salutations(pool).await?;
    load_workspace_fixtures(pool).await?;
    load_page_fixtures(pool).await?;
    info!("seed data inserted");
    Ok(())
}

async fn insert_core_users_and_roles(pool: &DatabasePool) -> Result<()> {
    // Create __auth table for password storage (matches Frappe's architecture)
    pool.execute_sql(
        r#"CREATE TABLE IF NOT EXISTS "__auth" (
            name TEXT,
            doctype TEXT,
            fieldname TEXT,
            password TEXT,
            PRIMARY KEY (name, doctype, fieldname)
        )"#,
        vec![],
    ).await?;

    // Users
    for (name, first_name, email, enabled) in [
        ("Administrator", "Administrator", "admin@example.com", 1),
        ("Guest", "Guest", "guest@example.com", 1),
    ] {
        let _ = pool.execute_sql(
            r#"INSERT OR REPLACE INTO "user" (name, creation, modified, modified_by, owner, docstatus, first_name, email, enabled)
               VALUES (?, datetime('now'), datetime('now'), 'Administrator', 'Administrator', 0, ?, ?, ?)"#,
            vec![
                serde_json::Value::String(name.into()),
                serde_json::Value::String(first_name.into()),
                serde_json::Value::String(email.into()),
                serde_json::Value::Number(enabled.into()),
            ],
        ).await;
    }

    // Administrator password hash for "admin"
    // Generated with: argon2 hash of "admin"
    let admin_hash = "$argon2id$v=19$m=19456,t=2,p=1$UEWqTMicBrdEJXqPMhP4oA$bR1RecCR37Rw+Spup2ULPNKAZ7H6vZTX4VeqNAfvdkY";
    let _ = pool.execute_sql(
        r#"INSERT OR REPLACE INTO "__auth" (name, doctype, fieldname, password)
           VALUES ('Administrator', 'User', '_password', ?)"#,
        vec![serde_json::Value::String(admin_hash.into())],
    ).await;

    // Roles
    for (role, desk_access) in [
        ("Administrator", 1),
        ("System Manager", 1),
        ("All", 1),
        ("Guest", 0),
        ("Report Manager", 1),
        ("Translator", 1),
    ] {
        let _ = pool.execute_sql(
            r#"INSERT OR REPLACE INTO "role" (name, creation, modified, modified_by, owner, docstatus, role_name, desk_access)
               VALUES (?, datetime('now'), datetime('now'), 'Administrator', 'Administrator', 0, ?, ?)"#,
            vec![
                serde_json::Value::String(role.into()),
                serde_json::Value::String(role.into()),
                serde_json::Value::Number(desk_access.into()),
            ],
        ).await;
    }

    // Has Role links for Administrator
    for role in ["Administrator", "System Manager", "All", "Report Manager", "Translator"] {
        let name = format!("administrator-{}", role.to_lowercase().replace(" ", "-"));
        let _ = pool.execute_sql(
            r#"INSERT OR REPLACE INTO "has_role" (name, creation, modified, modified_by, owner, docstatus, parent, parentfield, parenttype, role)
               VALUES (?, datetime('now'), datetime('now'), 'Administrator', 'Administrator', 0, 'Administrator', 'roles', 'User', ?)"#,
            vec![
                serde_json::Value::String(name),
                serde_json::Value::String(role.into()),
            ],
        ).await;
    }

    Ok(())
}

async fn insert_module_defs(pool: &DatabasePool) -> Result<()> {
    for module in ["Core", "Desk", "Website", "Integrations", "Automation", "Printing", "Email", "Geo", "Contacts", "Custom"] {
        let _ = pool.execute_sql(
            r#"INSERT OR REPLACE INTO "module_def" (name, creation, modified, modified_by, owner, docstatus, module_name)
               VALUES (?, datetime('now'), datetime('now'), 'Administrator', 'Administrator', 0, ?)"#,
            vec![
                serde_json::Value::String(module.into()),
                serde_json::Value::String(module.into()),
            ],
        ).await;
    }
    Ok(())
}

async fn insert_user_types(pool: &DatabasePool) -> Result<()> {
    for user_type in ["System User", "Website User"] {
        let _ = pool.execute_sql(
            r#"INSERT OR REPLACE INTO "user_type" (name, creation, modified, modified_by, owner, docstatus, user_type, is_standard)
               VALUES (?, datetime('now'), datetime('now'), 'Administrator', 'Administrator', 0, ?, 1)"#,
            vec![
                serde_json::Value::String(user_type.into()),
                serde_json::Value::String(user_type.into()),
            ],
        ).await;
    }
    Ok(())
}

async fn insert_workflow_defaults(pool: &DatabasePool) -> Result<()> {
    // Workflow States
    for (name, icon, style) in [
        ("Pending", "question-sign", ""),
        ("Approved", "ok-sign", "Success"),
        ("Rejected", "remove", "Danger"),
    ] {
        let _ = pool.execute_sql(
            r#"INSERT OR REPLACE INTO "workflow_state" (name, creation, modified, modified_by, owner, docstatus, workflow_state_name, icon, style)
               VALUES (?, datetime('now'), datetime('now'), 'Administrator', 'Administrator', 0, ?, ?, ?)"#,
            vec![
                serde_json::Value::String(name.into()),
                serde_json::Value::String(name.into()),
                serde_json::Value::String(icon.into()),
                serde_json::Value::String(style.into()),
            ],
        ).await;
    }

    // Workflow Action Master
    for action in ["Approve", "Reject", "Review"] {
        let _ = pool.execute_sql(
            r#"INSERT OR REPLACE INTO "workflow_action_master" (name, creation, modified, modified_by, owner, docstatus, workflow_action_name)
               VALUES (?, datetime('now'), datetime('now'), 'Administrator', 'Administrator', 0, ?)"#,
            vec![
                serde_json::Value::String(action.into()),
                serde_json::Value::String(action.into()),
            ],
        ).await;
    }

    Ok(())
}

async fn insert_genders_and_salutations(pool: &DatabasePool) -> Result<()> {
    for gender in ["Male", "Female", "Other", "Transgender", "Genderqueer", "Non-Conforming", "Prefer not to say"] {
        let _ = pool.execute_sql(
            r#"INSERT OR REPLACE INTO "gender" (name, creation, modified, modified_by, owner, docstatus, gender)
               VALUES (?, datetime('now'), datetime('now'), 'Administrator', 'Administrator', 0, ?)"#,
            vec![
                serde_json::Value::String(gender.into()),
                serde_json::Value::String(gender.into()),
            ],
        ).await;
    }

    for salutation in ["Mr", "Ms", "Mx", "Dr", "Mrs", "Madam", "Miss", "Master", "Prof"] {
        let _ = pool.execute_sql(
            r#"INSERT OR REPLACE INTO "salutation" (name, creation, modified, modified_by, owner, docstatus, salutation)
               VALUES (?, datetime('now'), datetime('now'), 'Administrator', 'Administrator', 0, ?)"#,
            vec![
                serde_json::Value::String(salutation.into()),
                serde_json::Value::String(salutation.into()),
            ],
        ).await;
    }

    Ok(())
}

async fn load_workspace_fixtures(pool: &DatabasePool) -> Result<()> {
    let base = std::path::PathBuf::from("apps/frappe/frappe");
    if !base.exists() {
        return Ok(());
    }

    let mut loaded = 0usize;

    let entries = match std::fs::read_dir(&base) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    for entry in entries.flatten() {
        let workspace_dir = entry.path().join("workspace");
        if !workspace_dir.exists() {
            continue;
        }

        let workspaces = match std::fs::read_dir(&workspace_dir) {
            Ok(w) => w,
            Err(_) => continue,
        };

        for ws_entry in workspaces.flatten() {
            let path = ws_entry.path();
            let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let json_path = path.join(format!("{}.json", fname));
            if !json_path.exists() {
                continue;
            }

            let content = match tokio::fs::read_to_string(&json_path).await {
                Ok(c) => c,
                Err(_) => continue,
            };

            let doc: serde_json::Value = match serde_json::from_str(&content) {
                Ok(d) => d,
                Err(_) => continue,
            };

            if let Err(e) = insert_workspace(pool, &doc).await {
                warn!("failed to insert workspace from {}: {}", json_path.display(), e);
                continue;
            }
            loaded += 1;
        }
    }

    if loaded > 0 {
        info!("loaded {} workspace fixtures", loaded);
    }
    Ok(())
}

async fn load_page_fixtures(pool: &DatabasePool) -> Result<()> {
    let base = std::path::PathBuf::from("apps/frappe/frappe");
    if !base.exists() {
        return Ok(());
    }

    let mut loaded = 0usize;

    let entries = match std::fs::read_dir(&base) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    for entry in entries.flatten() {
        let page_dir = entry.path().join("page");
        if !page_dir.exists() {
            continue;
        }

        let pages = match std::fs::read_dir(&page_dir) {
            Ok(p) => p,
            Err(_) => continue,
        };

        for pg_entry in pages.flatten() {
            let path = pg_entry.path();
            let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let json_path = path.join(format!("{}.json", fname));
            if !json_path.exists() {
                continue;
            }

            let content = match tokio::fs::read_to_string(&json_path).await {
                Ok(c) => c,
                Err(_) => continue,
            };

            let doc: serde_json::Value = match serde_json::from_str(&content) {
                Ok(d) => d,
                Err(_) => continue,
            };

            if let Err(e) = insert_page(pool, &doc).await {
                warn!("failed to insert page from {}: {}", json_path.display(), e);
                continue;
            }
            loaded += 1;
        }
    }

    if loaded > 0 {
        info!("loaded {} page fixtures", loaded);
    }
    Ok(())
}

async fn insert_page(pool: &DatabasePool, doc: &serde_json::Value) -> Result<()> {
    let name = json_str(doc, "name");
    if name.is_empty() {
        return Ok(());
    }

    let sql = r#"
        INSERT OR REPLACE INTO "page" (
            name, creation, modified, modified_by, owner, docstatus,
            page_name, title, icon, module, standard, system_page
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    "#;

    let params = vec![
        val(name),
        val(json_str(doc, "creation")),
        val(json_str(doc, "modified")),
        val(json_str(doc, "modified_by")),
        val(json_str(doc, "owner")),
        num(json_i64(doc, "docstatus")),
        val(json_str(doc, "page_name")),
        val(json_str(doc, "title")),
        val(json_str(doc, "icon")),
        val(json_str(doc, "module")),
        val(json_str(doc, "standard")),
        num(json_i64(doc, "system_page")),
    ];

    pool.execute_sql(sql, params).await?;
    Ok(())
}

async fn insert_workspace(pool: &DatabasePool, doc: &serde_json::Value) -> Result<()> {
    let name = json_str(doc, "name");
    if name.is_empty() {
        return Ok(());
    }

    let sql = r#"
        INSERT OR REPLACE INTO "workspace" (
            name, creation, modified, modified_by, owner, docstatus,
            label, title, icon, public, is_hidden, content, sequence_id, module, parent_page, for_user
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    "#;

    let params = vec![
        val(name.clone()),
        val(json_str(doc, "creation")),
        val(json_str(doc, "modified")),
        val(json_str(doc, "modified_by")),
        val(json_str(doc, "owner")),
        num(json_i64(doc, "docstatus")),
        val(json_str(doc, "label")),
        val(json_str(doc, "title")),
        val(json_str(doc, "icon")),
        num(json_i64(doc, "public")),
        num(json_i64(doc, "is_hidden")),
        val(json_str(doc, "content")),
        serde_json::Value::Number(serde_json::Number::from_f64(json_f64(doc, "sequence_id")).unwrap_or(0.into())),
        val(json_str(doc, "module")),
        val(json_str(doc, "parent_page")),
        val(json_str(doc, "for_user")),
    ];

    pool.execute_sql(sql, params).await?;

    // Insert child table rows (links, charts, shortcuts, etc.)
    let child_mappings = [
        ("links", "workspace_link"),
        ("charts", "workspace_chart"),
        ("shortcuts", "workspace_shortcut"),
        ("number_cards", "workspace_number_card"),
        ("quick_lists", "workspace_quick_list"),
        ("custom_blocks", "workspace_custom_block"),
    ];

    for (fieldname, table) in child_mappings {
        if let Some(rows) = doc.get(fieldname).and_then(|v| v.as_array()) {
            insert_child_rows(pool, table, &name, "Workspace", fieldname, rows).await?;
        }
    }

    Ok(())
}

async fn insert_child_rows(
    pool: &DatabasePool,
    table: &str,
    parent: &str,
    parenttype: &str,
    parentfield: &str,
    rows: &Vec<serde_json::Value>,
) -> Result<()> {
    // Discover columns from the child table schema
    let pragma = format!(r#"PRAGMA table_info("{}")"#, table);
    let cols = pool.execute_sql(&pragma, vec![]).await?;
    let col_names: Vec<String> = cols.iter().filter_map(|c| {
        c.get("name").and_then(|v| v.as_str()).map(String::from)
    }).collect();

    if col_names.is_empty() {
        warn!("child table {} does not exist, skipping", table);
        return Ok(());
    }

    for (idx, row) in rows.iter().enumerate() {
        let mut values: std::collections::HashMap<String, serde_json::Value> = std::collections::HashMap::new();

        // Standard fields
        values.insert("name".into(), val(format!("{}-{}-{}", parent, parentfield, idx)));
        values.insert("creation".into(), val("datetime('now')".into())); // will be literal in sql
        values.insert("modified".into(), val("datetime('now')".into()));
        values.insert("modified_by".into(), val("Administrator".into()));
        values.insert("owner".into(), val("Administrator".into()));
        values.insert("docstatus".into(), num(0));
        values.insert("parent".into(), val(parent.into()));
        values.insert("parenttype".into(), val(parenttype.into()));
        values.insert("parentfield".into(), val(parentfield.into()));
        values.insert("idx".into(), num(idx as i64));

        // Fields from the JSON row
        if let Some(obj) = row.as_object() {
            for (k, v) in obj {
                values.insert(k.clone(), v.clone());
            }
        }

        // Build INSERT with only columns that exist in the table
        let mut insert_cols: Vec<String> = Vec::new();
        let mut insert_vals: Vec<serde_json::Value> = Vec::new();
        for col in &col_names {
            if let Some(v) = values.get(col) {
                insert_cols.push(format!("\"{}\"", col));
                insert_vals.push(v.clone());
            }
        }

        let sql = format!(
            r#"INSERT OR REPLACE INTO "{}" ({}) VALUES ({})"#,
            table,
            insert_cols.join(", "),
            insert_vals.iter().map(|_| "?".to_string()).collect::<Vec<_>>().join(", ")
        );

        if let Err(e) = pool.execute_sql(&sql, insert_vals).await {
            warn!("failed to insert child row into {}: {}", table, e);
        }
    }

    Ok(())
}

// ------------------------------------------------------------------
// Helpers
// ------------------------------------------------------------------

fn json_str(doc: &serde_json::Value, key: &str) -> String {
    doc.get(key).and_then(|v| v.as_str()).unwrap_or("").to_string()
}

fn json_i64(doc: &serde_json::Value, key: &str) -> i64 {
    doc.get(key)
        .and_then(|v| v.as_i64())
        .or_else(|| doc.get(key).and_then(|v| v.as_str()).and_then(|s| s.parse().ok()))
        .unwrap_or(0)
}

fn json_f64(doc: &serde_json::Value, key: &str) -> f64 {
    doc.get(key)
        .and_then(|v| v.as_f64())
        .or_else(|| doc.get(key).and_then(|v| v.as_i64()).map(|i| i as f64))
        .or_else(|| doc.get(key).and_then(|v| v.as_str()).and_then(|s| s.parse().ok()))
        .unwrap_or(0.0)
}

fn val(s: String) -> serde_json::Value {
    serde_json::Value::String(s)
}

fn num(n: i64) -> serde_json::Value {
    serde_json::Value::Number(n.into())
}
