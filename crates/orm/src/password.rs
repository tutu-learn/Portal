//! Password field persistence matching Frappe's `__auth` architecture.
//!
//! Frappe stores Password fields in two places (see
//! `BaseDocument._save_passwords`):
//!
//! - the real secret lives Fernet-encrypted in `__auth` (encrypted with the
//!   site's `encryption_key`)
//! - the document table keeps a normal text column holding only a dummy
//!   placeholder (`"*" * len(secret)`), so controllers can always read the
//!   attribute (e.g. `if self.new_password:` in User.validate) without ever
//!   seeing the secret
//!
//! The native Rust save path previously copied Password values straight into
//! the data table in plaintext and never wrote `__auth`, which broke OAuth
//! logins (`get_decrypted_password` throwing "Password not found") and leaked
//! secrets through document reads.

use crate::pool::DatabasePool;
use error::{Result, RuntimeError};
use serde_json::Value;
use std::collections::HashMap;
use tracing::warn;

/// Frappe: `"".join(set(pwd)) == "*"` — true when the value consists solely
/// of `*` characters, which is the placeholder stored in the data table and
/// sent back by Desk for an unchanged secret.
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

/// Process Password fields on a document about to be saved, mirroring
/// Frappe's `_save_passwords`. Must be called before `insert_doc`/`save_doc`
/// so the data table only ever receives the dummy placeholder:
///
/// - empty / null values delete any stored secret (Desk cleared the field)
///   and stay empty in the document
/// - dummy values (`*****`) are left untouched (unchanged secret)
/// - anything else is Fernet-encrypted into `__auth` and replaced in the
///   document with `"*" * len(secret)`
///
/// `name` is the document name the row will be saved under (generated before
/// insert for new documents).
pub async fn process_password_fields_for_save(
    pool: &DatabasePool,
    doctype: &str,
    name: &str,
    encryption_key: &str,
    fields: &mut HashMap<String, Value>,
) -> Result<()> {
    for fieldname in password_fields(pool, doctype).await? {
        let Some(value) = fields.get(&fieldname) else {
            continue;
        };
        let secret = value.as_str().unwrap_or_default();
        if secret.is_empty() {
            delete_auth(pool, doctype, name, &fieldname).await?;
        } else if is_dummy_password(secret) {
            // Unchanged placeholder; keep the stored secret.
        } else {
            let encrypted = fernet_encrypt(encryption_key, secret)?;
            upsert_auth(pool, doctype, name, &fieldname, &encrypted).await?;
            fields.insert(fieldname, Value::String("*".repeat(secret.chars().count())));
        }
    }
    Ok(())
}

/// One-time migration for databases written before Password fields were
/// handled: move every plaintext value from the document table's password
/// columns into `__auth` (Fernet-encrypted) and replace the column value
/// with the dummy placeholder. The columns themselves are kept — Frappe
/// schemas include them and controllers expect the attributes to exist.
/// Skips doctypes whose table or column does not exist, and skips entirely
/// when the encryption key is invalid so secrets are never destroyed.
pub async fn migrate_plaintext_password_values(
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
        let mut migrated = 0u32;
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
            // Replace the plaintext in the data table with the dummy,
            // exactly what _save_passwords leaves behind.
            let update = format!(
                r#"UPDATE "{}" SET "{}" = {} WHERE name = {}"#,
                table,
                fieldname,
                pool.placeholder(1),
                pool.placeholder(2)
            );
            pool.execute_sql(
                &update,
                vec![
                    Value::String("*".repeat(secret.chars().count())),
                    Value::String(name),
                ],
            )
            .await?;
            migrated += 1;
        }
        if migrated > 0 {
            warn!(
                "migrated {} plaintext password(s) from {}.{} into __auth",
                migrated, table, fieldname
            );
        }
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
