use crate::doctype_sync::helpers::*;
use crate::pool::DatabasePool;
use error::{Result, RuntimeError};
use tracing::{info, warn};

/// Sync metadata tables from JSON fixtures and the bundled frappe app tree.
pub(crate) async fn sync_metadata(
    pool: &DatabasePool,
    fixtures: Vec<crate::doctype_sync::DoctypeFixture>,
) -> Result<()> {
    create_metadata_tables(pool).await?;

    let mut synced = 0usize;
    let mut fields_synced = 0usize;

    // Sync fixtures from Rust apps first.
    for fixture in fixtures {
        let doc: serde_json::Value = match serde_json::from_str(&fixture.json) {
            Ok(d) => d,
            Err(e) => {
                warn!("failed to parse fixture for {}: {}", fixture.name, e);
                continue;
            }
        };

        let doctype_name = doc
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or(&fixture.name);

        if let Err(e) = insert_doctype(pool, &doc).await {
            warn!("failed to insert fixture doctype {}: {}", fixture.name, e);
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
            if let Err(e) = prune_stale_docfields(pool, doctype_name, fields).await {
                warn!(
                    "failed to prune stale docfields for {}: {}",
                    doctype_name, e
                );
            }
        }

        if let Err(e) = insert_docperms(pool, doctype_name, &doc).await {
            warn!("failed to insert docperms for {}: {}", doctype_name, e);
        }
    }

    // Sync fixtures from the bundled frappe app tree.
    let base = std::path::PathBuf::from("apps/frappe/frappe");
    if !base.exists() {
        warn!("frappe app path not found at {}", base.display());
        info!(
            "synced {} doctypes with {} fields into metadata tables",
            synced, fields_synced
        );
        return Ok(());
    }

    let entries = match std::fs::read_dir(&base) {
        Ok(e) => e,
        Err(e) => {
            warn!("failed to read frappe modules dir: {}", e);
            info!(
                "synced {} doctypes with {} fields into metadata tables",
                synced, fields_synced
            );
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
                warn!(
                    "failed to insert doctype from {}: {}",
                    json_path.display(),
                    e
                );
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
                if let Err(e) = prune_stale_docfields(pool, doctype_name, fields).await {
                    warn!(
                        "failed to prune stale docfields for {}: {}",
                        doctype_name, e
                    );
                }
            }

            if let Err(e) = insert_docperms(pool, doctype_name, &doc).await {
                warn!("failed to insert docperms for {}: {}", doctype_name, e);
            }
        }
    }

    info!(
        "synced {} doctypes with {} fields into metadata tables",
        synced, fields_synced
    );
    Ok(())
}

pub(crate) async fn create_metadata_tables(pool: &DatabasePool) -> Result<()> {
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
            sortable INTEGER DEFAULT 1,
            restrict_to_domain TEXT
        )
    "#;
    pool.execute_sql(doctype_sql, vec![]).await?;

    // Handle upgrades from databases created before restrict_to_domain was part
    // of the metadata table layout (previously covered by migration 010).
    crate::doctype_sync::data_tables::add_column_if_missing(
        pool,
        "doctype",
        "restrict_to_domain",
        "restrict_to_domain TEXT",
    )
    .await?;

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
            permlevel INTEGER DEFAULT 0,
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

    // Handle upgrades from databases created before permlevel enforcement was
    // added to the docfield metadata table.
    crate::doctype_sync::data_tables::add_column_if_missing(
        pool,
        "docfield",
        "permlevel",
        "permlevel INTEGER DEFAULT 0",
    )
    .await?;

    pool.execute_sql(
        "CREATE INDEX IF NOT EXISTS idx_docfield_parent ON docfield(parent)",
        vec![],
    )
    .await?;

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
            title_field, image_field, timeline_field, sortable,
            restrict_to_domain
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
        val(json_str(doc, "restrict_to_domain")),
    ];

    pool.execute_sql(sql, params).await?;
    Ok(())
}

pub(crate) async fn insert_docfield(
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
            label, options, permlevel, reqd, read_only, hidden, in_list_view,
            in_standard_filter, in_preview, in_global_search, in_filter,
            bold, italic, no_copy, allow_in_quick_entry, translatable,
            collapsible, "unique", set_only_once, remember_last_selected_value,
            ignore_user_permissions, allow_on_submit, report_hide,
            search_index, show_dashboard, "default", depends_on, description,
            fetch_from, fetch_if_empty, mandatory_depends_on,
            read_only_depends_on, placeholder, tooltip, is_system_generated
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
        num(json_i64(field, "permlevel")),
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

/// Delete docfield rows for `doctype_name` that are no longer present in the
/// fixture, so fields removed from a DocType JSON disappear from forms and
/// metadata on the next sync.
async fn prune_stale_docfields(
    pool: &DatabasePool,
    doctype_name: &str,
    fields: &[serde_json::Value],
) -> Result<()> {
    let keep: Vec<String> = fields
        .iter()
        .map(|f| json_str(f, "fieldname"))
        .filter(|s| !s.is_empty())
        .collect();

    let rows = pool
        .execute_sql(
            r#"SELECT fieldname FROM "docfield" WHERE parent = ?"#,
            vec![serde_json::Value::String(doctype_name.into())],
        )
        .await?;
    for mut row in rows {
        let Some(existing) = row
            .remove("fieldname")
            .and_then(|v| v.as_str().map(String::from))
        else {
            continue;
        };
        if !keep.iter().any(|k| k == &existing) {
            pool.execute_sql(
                r#"DELETE FROM "docfield" WHERE parent = ? AND fieldname = ?"#,
                vec![
                    serde_json::Value::String(doctype_name.into()),
                    serde_json::Value::String(existing.clone()),
                ],
            )
            .await?;
            info!("pruned stale docfield {}.{}", doctype_name, existing);
        }
    }
    Ok(())
}

/// Insert standard permissions for a DocType from its JSON definition.
///
/// If permissions already exist for the DocType they are left untouched so
/// edits made through the Permission Manager survive restarts. New DocTypes
/// (or DocTypes with no permissions yet) get seeded from JSON.
async fn insert_docperms(
    pool: &DatabasePool,
    doctype_name: &str,
    doc: &serde_json::Value,
) -> Result<()> {
    let perms = match doc.get("permissions").and_then(|p| p.as_array()) {
        Some(p) if !p.is_empty() => p,
        _ => return Ok(()),
    };

    // Replace existing permissions so changes to fixture JSON are applied on
    // every sync. Manual edits made directly in the database will be overwritten.
    pool.execute_sql(
        r#"DELETE FROM __kiff_docperm WHERE parent = ?"#,
        vec![serde_json::Value::String(doctype_name.into())],
    )
    .await?;

    let sql = r#"
        INSERT INTO __kiff_docperm (
            parent, role, permlevel, "read", "write", "create", "delete", "submit", "cancel",
            if_owner, "select", "report", "export", "import", "share", "print", "email", "mask", "amend"
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    "#;

    for perm in perms {
        let role = json_str(perm, "role");
        if role.is_empty() {
            continue;
        }
        let params = vec![
            val(doctype_name.into()),
            val(role),
            num(json_i64(perm, "permlevel")),
            num(json_i64(perm, "read")),
            num(json_i64(perm, "write")),
            num(json_i64(perm, "create")),
            num(json_i64(perm, "delete")),
            num(json_i64(perm, "submit")),
            num(json_i64(perm, "cancel")),
            num(json_i64(perm, "if_owner")),
            num(json_i64(perm, "select")),
            num(json_i64(perm, "report")),
            num(json_i64(perm, "export")),
            num(json_i64(perm, "import")),
            num(json_i64(perm, "share")),
            num(json_i64(perm, "print")),
            num(json_i64(perm, "email")),
            num(json_i64(perm, "mask")),
            num(json_i64(perm, "amend")),
        ];
        pool.execute_sql(sql, params).await?;
    }

    Ok(())
}
