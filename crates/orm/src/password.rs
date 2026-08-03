//! Password field persistence matching Frappe's `__auth` architecture.
//!
//! Frappe never stores Password field values in the document table. On save,
//! `Document._save_passwords()` encrypts the value with the site's Fernet
//! `encryption_key` and upserts it into `__auth`, keeping only a dummy
//! `"*****"` placeholder in memory. The native Rust save path previously
//! copied Password fields straight into the document table (in plaintext)
//! and never wrote `__auth`, which broke OAuth logins
//! (`get_decrypted_password` throwing "Password not found") and leaked
//! secrets through document reads.

use crate::document::Document;
use crate::pool::DatabasePool;
use error::{Result, RuntimeError};
use serde_json::Value;
use std::collections::HashMap;
use tracing::warn;

/// Placeholder returned in place of a stored password, mirroring Frappe's
/// dummy password convention.
pub const DUMMY_PASSWORD: &str = "**********";

/// Frappe: `"".join(set(pwd)) == "*"` — true when the value consists solely
/// of `*` characters, which is the placeholder Desk sends back for an
/// unchanged secret.
pub fn is_dummy_password(pwd: &str) -> bool {
    !pwd.is_empty() && pwd.chars().all(|c| c == '*')
}

/// Names of all Password fields defined on a doctype.
pub async fn password_fields(pool: &DatabasePool, doctype: &str) -> Result<Vec<String>> {
    let sql = format!(
        r#"SELECT fieldname FROM "docfield" WHERE parent = {} AND fieldtype = 'Password'"#,
        pool.placeholder(1)
    );
    let rows = pool
        .execute_sql(&sql, vec![Value::String(doctype.into())])
        .await?;
    Ok(rows
        .into_iter()
        .filter_map(|mut r| r.remove("fieldname")?.as_str().map(String::from))
        .collect())
}

fn fernet_encrypt(encryption_key: &str, plaintext: &str) -> Result<String> {
    let fernet = fernet::Fernet::new(encryption_key).ok_or_else(|| {
        RuntimeError::Validation("site encryption_key is not a valid Fernet key".into())
    })?;
    Ok(fernet.encrypt(plaintext.as_bytes()))
}

async fn upsert_auth(
    pool: &DatabasePool,
    doctype: &str,
    name: &str,
    fieldname: &str,
    encrypted: &str,
) -> Result<()> {
    let sql = match pool.dialect() {
        "postgres" => r#"INSERT INTO "__auth" (doctype, name, fieldname, password, encrypted)
                         VALUES ($1, $2, $3, $4, 1)
                         ON CONFLICT (doctype, name, fieldname)
                         DO UPDATE SET password = EXCLUDED.password, encrypted = 1"#
            .to_string(),
        _ => r#"INSERT OR REPLACE INTO "__auth" (doctype, name, fieldname, password, encrypted)
                VALUES (?, ?, ?, ?, 1)"#
            .to_string(),
    };
    pool.execute_sql(
        &sql,
        vec![
            Value::String(doctype.into()),
            Value::String(name.into()),
            Value::String(fieldname.into()),
            Value::String(encrypted.into()),
        ],
    )
    .await?;
    Ok(())
}

async fn delete_auth(pool: &DatabasePool, doctype: &str, name: &str, fieldname: &str) -> Result<()> {
    let sql = format!(
        r#"DELETE FROM "__auth" WHERE doctype = {} AND name = {} AND fieldname = {}"#,
        pool.placeholder(1),
        pool.placeholder(2),
        pool.placeholder(3)
    );
    pool.execute_sql(
        &sql,
        vec![
            Value::String(doctype.into()),
            Value::String(name.into()),
            Value::String(fieldname.into()),
        ],
    )
    .await?;
    Ok(())
}

async fn has_auth(pool: &DatabasePool, doctype: &str, name: &str, fieldname: &str) -> Result<bool> {
    let sql = format!(
        r#"SELECT 1 FROM "__auth" WHERE doctype = {} AND name = {} AND fieldname = {} LIMIT 1"#,
        pool.placeholder(1),
        pool.placeholder(2),
        pool.placeholder(3)
    );
    let rows = pool
        .execute_sql(
            &sql,
            vec![
                Value::String(doctype.into()),
                Value::String(name.into()),
                Value::String(fieldname.into()),
            ],
        )
        .await?;
    Ok(!rows.is_empty())
}

/// Remove Password fields from a document's field map before it is written to
/// the document table, returning the extracted values for
/// [`apply_password_fields`]. Password values must never reach the data table.
pub async fn extract_password_fields(
    pool: &DatabasePool,
    doctype: &str,
    fields: &mut HashMap<String, Value>,
) -> Result<Vec<(String, Value)>> {
    let mut extracted = Vec::new();
    for fieldname in password_fields(pool, doctype).await? {
        if let Some(value) = fields.remove(&fieldname) {
            extracted.push((fieldname, value));
        }
    }
    Ok(extracted)
}

/// Persist extracted Password values into `__auth`, mirroring Frappe's
/// `_save_passwords`. Call this after the document row has been saved.
///
/// - empty / null values delete any stored secret (Desk cleared the field)
/// - dummy values (`*****`) are skipped (Desk sent back an unchanged secret)
/// - anything else is Fernet-encrypted with the site key and upserted
pub async fn apply_password_fields(
    pool: &DatabasePool,
    doctype: &str,
    name: &str,
    encryption_key: &str,
    extracted: &[(String, Value)],
) -> Result<()> {
    for (fieldname, value) in extracted {
        let secret = value.as_str().unwrap_or_default();
        if secret.is_empty() {
            delete_auth(pool, doctype, name, &fieldname).await?;
        } else if is_dummy_password(secret) {
            // Unchanged placeholder; keep the stored secret.
        } else {
            let encrypted = fernet_encrypt(encryption_key, secret)?;
            upsert_auth(pool, doctype, name, &fieldname, &encrypted).await?;
        }
    }
    Ok(())
}

/// Mask Password fields on a freshly loaded document: fields with a stored
/// secret become the dummy placeholder; anything else (e.g. a legacy
/// plaintext column value) is removed so it never reaches API responses.
pub async fn mask_password_fields(pool: &DatabasePool, doc: &mut Document) -> Result<()> {
    let fields = password_fields(pool, &doc.doctype).await?;
    for fieldname in fields {
        if has_auth(pool, &doc.doctype, &doc.name, &fieldname).await? {
            doc.fields
                .insert(fieldname, Value::String(DUMMY_PASSWORD.into()));
        } else {
            doc.fields.remove(&fieldname);
        }
    }
    Ok(())
}

/// One-time migration for databases created before Password fields were kept
/// out of the data tables: move every plaintext value from the legacy column
/// into `__auth` (Fernet-encrypted), then drop the column. Skips doctypes
/// whose table or column does not exist. When the encryption key is invalid
/// the migration is skipped entirely so secrets are never destroyed.
pub async fn migrate_plaintext_password_columns(
    pool: &DatabasePool,
    encryption_key: &str,
) -> Result<()> {
    if fernet::Fernet::new(encryption_key).is_none() {
        warn!("skipping plaintext password migration: invalid site encryption_key");
        return Ok(());
    }

    let rows = match pool
        .execute_sql(
            r#"SELECT DISTINCT parent, fieldname FROM "docfield" WHERE fieldtype = 'Password'"#,
            vec![],
        )
        .await
    {
        Ok(r) => r,
        Err(e) => {
            // docfield may not exist yet on a brand-new site; nothing to migrate.
            warn!("plaintext password migration skipped: {}", e);
            return Ok(());
        }
    };

    for mut row in rows {
        let (Some(doctype), Some(fieldname)) = (
            row.remove("parent").and_then(|v| v.as_str().map(String::from)),
            row.remove("fieldname").and_then(|v| v.as_str().map(String::from)),
        ) else {
            continue;
        };
        let table = crate::doctype_sync::data_table_name(&doctype);

        if !column_exists(pool, &table, &fieldname).await? {
            continue;
        }

        let select = format!(
            r#"SELECT name, "{}" AS secret FROM "{}" WHERE "{}" IS NOT NULL AND "{}" != ''"#,
            fieldname, table, fieldname, fieldname
        );
        let secrets = pool.execute_sql(&select, vec![]).await?;
        for mut secret_row in secrets {
            let (Some(name), Some(secret)) = (
                secret_row.remove("name").and_then(|v| v.as_str().map(String::from)),
                secret_row.remove("secret").and_then(|v| v.as_str().map(String::from)),
            ) else {
                continue;
            };
            if is_dummy_password(&secret) {
                continue;
            }
            let encrypted = fernet_encrypt(encryption_key, &secret)?;
            upsert_auth(pool, &doctype, &name, &fieldname, &encrypted).await?;
        }

        let alter = format!(r#"ALTER TABLE "{}" DROP COLUMN "{}""#, table, fieldname);
        pool.execute_sql(&alter, vec![]).await?;
        warn!(
            "migrated plaintext password column {}.{} into __auth and dropped it",
            table, fieldname
        );
    }
    Ok(())
}

async fn column_exists(pool: &DatabasePool, table: &str, column: &str) -> Result<bool> {
    match pool.dialect() {
        "postgres" => {
            let rows = pool
                .execute_sql(
                    "SELECT 1 FROM information_schema.columns WHERE table_name = $1 AND column_name = $2",
                    vec![
                        Value::String(table.into()),
                        Value::String(column.into()),
                    ],
                )
                .await?;
            Ok(!rows.is_empty())
        }
        _ => {
            let rows = pool
                .execute_sql(&format!(r#"PRAGMA table_info("{}")"#, table), vec![])
                .await?;
            Ok(rows.into_iter().any(|mut r| {
                r.remove("name")
                    .and_then(|v| v.as_str().map(String::from))
                    .as_deref()
                    == Some(column)
            }))
        }
    }
}
