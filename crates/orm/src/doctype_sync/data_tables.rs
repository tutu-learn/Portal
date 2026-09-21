use crate::pool::DatabasePool;
use error::Result;
use tracing::{info, warn};

/// Dynamic data table creation — reads metadata and creates/updates the actual
/// document tables for every doctype.
pub(crate) async fn sync_data_tables(pool: &DatabasePool) -> Result<()> {
    info!("syncing data tables from metadata");

    // Read all doctypes from metadata
    let rows = pool
        .execute_sql("SELECT name, istable, is_virtual FROM \"doctype\"", vec![])
        .await?;

    let mut created = 0usize;
    for row in rows {
        let doctype_name = row.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let istable = row.get("istable").and_then(|v| v.as_i64()).unwrap_or(0) != 0;
        let is_virtual = row.get("is_virtual").and_then(|v| v.as_i64()).unwrap_or(0) != 0;
        if doctype_name.is_empty() {
            continue;
        }

        // Skip metadata tables managed manually by create_metadata_tables().
        // Treating them as data tables causes schema mismatches because their
        // JSON definition does not match the metadata table layout.
        if doctype_name == "DocType" || doctype_name == "DocField" {
            continue;
        }

        // Virtual DocTypes are not backed by SQL; their data comes from custom
        // engines (e.g. Kiff Log Entry reads from the Tantivy log engine).
        if is_virtual {
            let table = data_table_name(doctype_name);
            // Drop any stale physical table left over from a previous sync.
            let _ = pool
                .execute_sql(&format!("DROP TABLE IF EXISTS \"{}\"", table), vec![])
                .await;
            continue;
        }

        // Read fields for this doctype from metadata
        let field_rows = pool
            .execute_sql(
                "SELECT fieldname, fieldtype FROM \"docfield\" WHERE parent = ? ORDER BY idx",
                vec![serde_json::Value::String(doctype_name.into())],
            )
            .await?;

        let fields: Vec<(String, String)> = field_rows
            .into_iter()
            .filter_map(|mut r| {
                let fname = r
                    .remove("fieldname")
                    .and_then(|v| v.as_str().map(String::from))?;
                let ftype = r
                    .remove("fieldtype")
                    .and_then(|v| v.as_str().map(String::from))?;
                Some((fname, ftype))
            })
            .collect();

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

    let name_col = "name TEXT PRIMARY KEY".to_string();

    let mut expected_cols: Vec<(String, String)> = vec![
        ("name".into(), name_col),
        ("creation".into(), "creation TEXT".into()),
        ("modified".into(), "modified TEXT".into()),
        ("modified_by".into(), "modified_by TEXT".into()),
        ("owner".into(), "owner TEXT".into()),
        ("docstatus".into(), "docstatus INTEGER DEFAULT 0".into()),
        ("idx".into(), "idx INTEGER DEFAULT 0".into()),
        // Frappe client always requests these internal fields in list views.
        ("_user_tags".into(), "_user_tags TEXT".into()),
        ("_comments".into(), "_comments TEXT".into()),
        ("_assign".into(), "_assign TEXT".into()),
        ("_liked_by".into(), "_liked_by TEXT".into()),
        ("_seen".into(), "_seen TEXT".into()),
    ];

    if istable {
        expected_cols.push(("parent".into(), "parent TEXT".into()));
        expected_cols.push(("parentfield".into(), "parentfield TEXT".into()));
        expected_cols.push(("parenttype".into(), "parenttype TEXT".into()));
    }

    let standard_names: std::collections::HashSet<String> =
        expected_cols.iter().map(|(name, _)| name.clone()).collect();

    for (fieldname, fieldtype) in fields {
        if is_ui_or_child_field(fieldtype) {
            continue;
        }
        // Standard columns (name, idx, etc.) are already added above with the
        // correct SQL types. DocType metadata may repeat them, so skip duplicates.
        if standard_names.contains(fieldname) {
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
    let existing_names: Vec<String> = existing
        .iter()
        .filter_map(|c| c.get("name").and_then(|v| v.as_str()).map(String::from))
        .collect();

    let mut needs_recreate = false;
    for (col_name, col_def) in &expected_cols {
        if existing_names.contains(&quote_if_reserved(col_name))
            || existing_names.contains(col_name)
        {
            continue;
        }

        // Column missing — try ALTER TABLE ADD COLUMN. The `name` column is
        // added without its PRIMARY KEY constraint: neither SQLite nor
        // Postgres can add a PK column to an existing table, and failing here
        // would trigger a destructive recreate of tables that predate the
        // DocType (e.g. the k8s control-plane tables). A plain TEXT column is
        // enough for the ORM to read/write those rows.
        let alter_def = if col_name == "name" {
            "name TEXT"
        } else {
            col_def.as_str()
        };
        let alter_sql = format!("ALTER TABLE \"{}\" ADD COLUMN {}", table, alter_def);
        match pool.execute_sql(&alter_sql, vec![]).await {
            Ok(_) => info!("added column {} to {}", col_name, table),
            Err(e) => {
                warn!(
                    "cannot add column {} to {}: {}. table will be recreated.",
                    col_name, table, e
                );
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
    let old_names: Vec<String> = old_cols
        .iter()
        .filter_map(|c| c.get("name").and_then(|v| v.as_str()).map(String::from))
        .collect();

    let common_cols: Vec<String> = expected_cols
        .iter()
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
    pool.execute_sql(&format!("DROP TABLE \"{}\"", table), vec![])
        .await?;
    pool.execute_sql(
        &format!("ALTER TABLE \"{}\" RENAME TO \"{}\"", temp_table, table),
        vec![],
    )
    .await?;

    info!("table {} migrated successfully", table);
    Ok(())
}

pub(crate) fn data_table_name(doctype: &str) -> String {
    let name = doctype.to_lowercase().replace(" ", "_");
    name.strip_prefix("tab").unwrap_or(&name).to_string()
}

/// Add a column to an existing table when it is not present yet. Also used by
/// Rust apps to evolve their own raw tables (e.g. adding the `name` column a
/// table needs once it becomes DocType-backed).
///
/// Returns `true` when the column was added by this call, so callers can gate
/// one-time backfills on the schema change. A failed ALTER is logged and
/// reported as "not added".
pub async fn add_column_if_missing(
    pool: &DatabasePool,
    table: &str,
    column: &str,
    column_def: &str,
) -> Result<bool> {
    let pragma = format!(r#"PRAGMA table_info("{}")"#, table);
    let rows = pool.execute_sql(&pragma, vec![]).await?;
    let exists = rows.iter().any(|r| {
        r.get("name")
            .and_then(|v| v.as_str())
            .map(|n| n.eq_ignore_ascii_case(column))
            .unwrap_or(false)
    });
    if exists {
        return Ok(false);
    }
    let alter_sql = format!(r#"ALTER TABLE "{}" ADD COLUMN {}"#, table, column_def);
    match pool.execute_sql(&alter_sql, vec![]).await {
        Ok(_) => {
            info!("added column {} to {}", column, table);
            Ok(true)
        }
        Err(e) => {
            warn!("failed to add column {} to {}: {}", column, table, e);
            Ok(false)
        }
    }
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
        "abort",
        "action",
        "add",
        "after",
        "all",
        "alter",
        "analyze",
        "and",
        "as",
        "asc",
        "attach",
        "autoincrement",
        "before",
        "begin",
        "between",
        "by",
        "cascade",
        "case",
        "cast",
        "check",
        "collate",
        "column",
        "commit",
        "conflict",
        "constraint",
        "create",
        "cross",
        "current_date",
        "current_time",
        "current_timestamp",
        "database",
        "default",
        "deferrable",
        "deferred",
        "delete",
        "desc",
        "detach",
        "distinct",
        "drop",
        "each",
        "else",
        "end",
        "escape",
        "except",
        "exclusive",
        "exists",
        "explain",
        "fail",
        "for",
        "foreign",
        "from",
        "full",
        "glob",
        "group",
        "having",
        "if",
        "ignore",
        "immediate",
        "in",
        "index",
        "indexed",
        "initially",
        "inner",
        "insert",
        "instead",
        "intersect",
        "into",
        "is",
        "isnull",
        "join",
        "key",
        "left",
        "like",
        "limit",
        "match",
        "natural",
        "no",
        "not",
        "notnull",
        "null",
        "of",
        "offset",
        "on",
        "or",
        "order",
        "outer",
        "plan",
        "pragma",
        "primary",
        "query",
        "raise",
        "recursive",
        "references",
        "regexp",
        "reindex",
        "release",
        "rename",
        "replace",
        "restrict",
        "right",
        "rollback",
        "row",
        "savepoint",
        "select",
        "set",
        "table",
        "temp",
        "temporary",
        "then",
        "to",
        "transaction",
        "trigger",
        "union",
        "unique",
        "update",
        "using",
        "vacuum",
        "values",
        "view",
        "virtual",
        "when",
        "where",
        "with",
        "without",
    ];
    let lower = name.to_lowercase();
    if RESERVED.contains(&lower.as_str()) {
        format!("\"{}\"", name)
    } else {
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A raw table created before its DocType existed (no `name` column)
    /// must gain a plain TEXT column via ALTER — never a drop-and-recreate.
    /// Neither SQLite nor Postgres can add a PRIMARY KEY column to an
    /// existing table, and recreating would silently drop constraints,
    /// indexes, and the table's identity.
    #[tokio::test]
    async fn existing_table_without_name_column_is_not_recreated() {
        let path = format!("/tmp/orm_doctype_sync_noname_{}.db", std::process::id());
        let _ = std::fs::remove_file(&path);
        let pool = DatabasePool::connect_sqlite(&path).await.unwrap();
        pool.execute_sql(
            r#"CREATE TABLE "k8s_command" (
                id TEXT PRIMARY KEY,
                status TEXT NOT NULL DEFAULT ''
            )"#,
            vec![],
        )
        .await
        .unwrap();
        pool.execute_sql(
            r#"INSERT INTO "k8s_command" (id, status) VALUES ('cmd-1', 'Queued')"#,
            vec![],
        )
        .await
        .unwrap();

        create_data_table(
            &pool,
            "K8s Command",
            false,
            &[
                ("id".to_string(), "Data".to_string()),
                ("status".to_string(), "Data".to_string()),
            ],
        )
        .await
        .unwrap();

        // The row survived (no recreate) and `name` was added as plain TEXT.
        let rows = pool
            .execute_sql(r#"SELECT id, status, name FROM "k8s_command""#, vec![])
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get("id").and_then(|v| v.as_str()), Some("cmd-1"));
        assert_eq!(
            rows[0].get("status").and_then(|v| v.as_str()),
            Some("Queued")
        );
        // The primary key is still `id`: a conflicting insert must fail.
        let dup = pool
            .execute_sql(
                r#"INSERT INTO "k8s_command" (id, status) VALUES ('cmd-1', 'X')"#,
                vec![],
            )
            .await;
        assert!(dup.is_err());

        let _ = std::fs::remove_file(&path);
    }
}
