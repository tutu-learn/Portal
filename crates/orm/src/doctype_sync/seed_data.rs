use crate::doctype_sync::data_tables::add_column_if_missing;
use crate::doctype_sync::helpers::*;
use crate::doctype_sync::{DoctypeFixture, ModuleFixture};
use crate::pool::DatabasePool;
use error::{Result, RuntimeError};
use tracing::{info, warn};

/// Seed data — minimal records needed for the desk to boot.
pub(crate) async fn insert_seed_data(
    pool: &DatabasePool,
    workspace_fixtures: Vec<(String, String, String)>,
    page_fixtures: Vec<(String, String)>,
) -> Result<()> {
    ensure_core_users_and_roles(pool).await?;
    insert_user_types(pool).await?;
    insert_workflow_defaults(pool).await?;
    insert_genders_and_salutations(pool).await?;
    load_workspace_fixtures(pool, workspace_fixtures).await?;
    load_page_fixtures(pool, page_fixtures).await?;
    insert_single_settings(pool).await?;
    info!("seed data inserted");
    Ok(())
}

/// Insert Client Script fixtures contributed by Rust apps.
///
/// These are upserted into the `client_script` table so Desk forms can add
/// custom buttons and handlers. Existing records with the same name are
/// overwritten so fixture changes are applied on every sync.
pub(crate) async fn insert_client_script_fixtures(
    pool: &DatabasePool,
    fixtures: Vec<(String, String)>,
) -> Result<()> {
    if fixtures.is_empty() {
        return Ok(());
    }

    let now_fn = match pool.dialect() {
        "postgres" => "NOW()",
        _ => "datetime('now')",
    };

    for (name, json) in fixtures {
        let doc: serde_json::Value = match serde_json::from_str(&json) {
            Ok(d) => d,
            Err(e) => {
                warn!("failed to parse client script fixture {}: {}", name, e);
                continue;
            }
        };

        let dt = json_str(&doc, "dt");
        let script = json_str(&doc, "script");
        let view = json_str(&doc, "view");
        let module = json_str(&doc, "module");
        let enabled = json_i64(&doc, "enabled");

        if dt.is_empty() || script.is_empty() {
            warn!(
                "skipping client script fixture {}: dt and script are required",
                name
            );
            continue;
        }

        let sql = format!(
            r#"INSERT INTO "client_script" (
                name, creation, modified, modified_by, owner, docstatus,
                dt, script, view, module, enabled
            ) VALUES ({}, {now_fn}, {now_fn}, 'Administrator', 'Administrator', 0,
                      {}, {}, {}, {}, {})
            ON CONFLICT(name) DO UPDATE SET
                creation=EXCLUDED.creation, modified=EXCLUDED.modified, modified_by=EXCLUDED.modified_by,
                owner=EXCLUDED.owner, docstatus=EXCLUDED.docstatus, dt=EXCLUDED.dt,
                script=EXCLUDED.script, view=EXCLUDED.view, module=EXCLUDED.module,
                enabled=EXCLUDED.enabled"#,
            pool.placeholder(1),
            pool.placeholder(2),
            pool.placeholder(3),
            pool.placeholder(4),
            pool.placeholder(5),
            pool.placeholder(6),
        );

        if let Err(e) = pool
            .execute_sql(
                &sql,
                vec![
                    serde_json::Value::String(name),
                    serde_json::Value::String(dt),
                    serde_json::Value::String(script),
                    serde_json::Value::String(if view.is_empty() { "Form".into() } else { view }),
                    serde_json::Value::String(module),
                    serde_json::Value::Number(enabled.into()),
                ],
            )
            .await
        {
            warn!("failed to insert client script fixture: {}", e);
        }
    }

    info!("client script fixtures inserted");
    Ok(())
}

/// Generate a random alphanumeric Administrator password for a new site.
fn generate_admin_password() -> String {
    use rand::Rng;
    rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(24)
        .map(char::from)
        .collect()
}

/// Hash a password with argon2id for storage in `__auth`.
///
/// Used for the seeded Administrator and by apps creating users with a
/// usable login password (e.g. Strongroom's first-boot setup).
pub fn hash_user_password(password: &str) -> Result<String> {
    use argon2::password_hash::{rand_core::OsRng, SaltString};
    use argon2::{Argon2, PasswordHasher};

    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| RuntimeError::Validation(format!("failed to hash admin password: {}", e)))
}

/// Ensure the core users, roles, and Administrator role links exist.
///
/// This is safe to call on every startup: it upserts the default records and
/// leaves manually changed data untouched.
pub async fn ensure_core_users_and_roles(pool: &DatabasePool) -> Result<()> {
    // Create __auth table for password storage (matches Frappe's architecture)
    // Older databases were created without the encrypted column; migrate them.
    add_column_if_missing(
        pool,
        "__auth",
        "encrypted",
        "encrypted INTEGER NOT NULL DEFAULT 0",
    )
    .await?;
    pool.execute_sql(
        r#"CREATE TABLE IF NOT EXISTS "__auth" (
            name TEXT,
            doctype TEXT,
            fieldname TEXT,
            password TEXT,
            encrypted INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (name, doctype, fieldname)
        )"#,
        vec![],
    )
    .await?;

    let now_fn = match pool.dialect() {
        "postgres" => "NOW()",
        _ => "datetime('now')",
    };

    // Users
    for (name, first_name, email, enabled, user_type) in [
        (
            "Administrator",
            "Administrator",
            "admin@example.com",
            1,
            "System User",
        ),
        ("Guest", "Guest", "guest@example.com", 1, "Website User"),
    ] {
        pool.execute_sql(
            &format!(
                r#"INSERT INTO "user" (name, creation, modified, modified_by, owner, docstatus, first_name, email, enabled, user_type)
                   VALUES ({}, {now_fn}, {now_fn}, 'Administrator', 'Administrator', 0, {}, {}, {}, {})
                   ON CONFLICT(name) DO UPDATE SET
                       creation=EXCLUDED.creation, modified=EXCLUDED.modified, modified_by=EXCLUDED.modified_by,
                       owner=EXCLUDED.owner, docstatus=EXCLUDED.docstatus, first_name=EXCLUDED.first_name,
                       email=EXCLUDED.email, enabled=EXCLUDED.enabled, user_type=EXCLUDED.user_type"#,
                pool.placeholder(1),
                pool.placeholder(2),
                pool.placeholder(3),
                pool.placeholder(4),
                pool.placeholder(5),
            ),
            vec![
                serde_json::Value::String(name.into()),
                serde_json::Value::String(first_name.into()),
                serde_json::Value::String(email.into()),
                serde_json::Value::Number(enabled.into()),
                serde_json::Value::String(user_type.into()),
            ],
        ).await?;
    }

    // Administrator password seed. Only inserted on first bootstrap: when a
    // password row already exists we do nothing, so a user-changed password
    // survives restarts and we never print a generated password that isn't
    // actually stored.
    let admin_password_exists = !pool
        .execute_sql(
            r#"SELECT 1 FROM "__auth" WHERE name = 'Administrator' AND doctype = 'User' AND fieldname = 'password' LIMIT 1"#,
            vec![],
        )
        .await?
        .is_empty();

    if !admin_password_exists {
        // The seed password is determined at runtime: KIFF_ADMIN_PASSWORD
        // when set (non-empty), otherwise a random password printed once to
        // stderr. Never log the password itself.
        let admin_password = match std::env::var("KIFF_ADMIN_PASSWORD") {
            Ok(pw) if !pw.is_empty() => pw,
            _ => {
                let pw = generate_admin_password();
                eprintln!(
                    "Generated Administrator password for new site: {} — store it and change it after first login",
                    pw
                );
                warn!("generated a random Administrator password for new site (printed to stderr)");
                pw
            }
        };
        let admin_hash = hash_user_password(&admin_password)?;
        pool.execute_sql(
            &format!(
                r#"INSERT INTO "__auth" (name, doctype, fieldname, password, encrypted)
                   VALUES ('Administrator', 'User', 'password', {}, 0)
                   ON CONFLICT(name, doctype, fieldname) DO NOTHING"#,
                pool.placeholder(1)
            ),
            vec![serde_json::Value::String(admin_hash.into())],
        )
        .await?;
    }

    // Roles
    for (role, desk_access) in [
        ("Administrator", 1),
        ("System Manager", 1),
        ("All", 1),
        ("Guest", 0),
        ("Report Manager", 1),
        ("Translator", 1),
        ("Kiff Logs", 1),
        ("Kiff Logs Admin", 1),
        ("Sebrus Log Rule Admin", 1),
        ("Sebrus Log Rule Viewer", 1),
        ("Sebrus Log Viewer", 1),
        ("Server Admin", 1),
        ("Infrastructure Viewer", 1),
    ] {
        pool.execute_sql(
            &format!(
                r#"INSERT INTO "role" (name, creation, modified, modified_by, owner, docstatus, role_name, desk_access)
                   VALUES ({}, {now_fn}, {now_fn}, 'Administrator', 'Administrator', 0, {}, {})
                   ON CONFLICT(name) DO UPDATE SET
                       creation=EXCLUDED.creation, modified=EXCLUDED.modified, modified_by=EXCLUDED.modified_by,
                       owner=EXCLUDED.owner, docstatus=EXCLUDED.docstatus, role_name=EXCLUDED.role_name,
                       desk_access=EXCLUDED.desk_access"#,
                pool.placeholder(1),
                pool.placeholder(2),
                pool.placeholder(3),
            ),
            vec![
                serde_json::Value::String(role.into()),
                serde_json::Value::String(role.into()),
                serde_json::Value::Number(desk_access.into()),
            ],
        ).await?;
    }

    // Has Role links for Administrator.
    // Remove any stale default links first so this stays idempotent even on
    // databases where the has_role table lacks a unique constraint on `name`.
    let admin_roles = [
        "Administrator",
        "System Manager",
        "All",
        "Report Manager",
        "Translator",
    ];
    let placeholders: Vec<String> = (1..=admin_roles.len())
        .map(|i| pool.placeholder(i))
        .collect();
    let mut delete_params: Vec<serde_json::Value> = admin_roles
        .iter()
        .map(|r| serde_json::Value::String((*r).into()))
        .collect();
    delete_params.push(serde_json::Value::String("Administrator".into()));
    pool.execute_sql(
        &format!(
            r#"DELETE FROM "has_role" WHERE role IN ({}) AND parent = {} AND parenttype = 'User'"#,
            placeholders.join(", "),
            pool.placeholder(admin_roles.len() + 1),
        ),
        delete_params,
    )
    .await?;

    for role in admin_roles {
        let name = format!("administrator-{}", role.to_lowercase().replace(" ", "-"));
        pool.execute_sql(
            &format!(
                r#"INSERT INTO "has_role" (name, creation, modified, modified_by, owner, docstatus, parent, parentfield, parenttype, role)
                   VALUES ({}, {now_fn}, {now_fn}, 'Administrator', 'Administrator', 0, 'Administrator', 'roles', 'User', {})"#,
                pool.placeholder(1),
                pool.placeholder(2),
            ),
            vec![
                serde_json::Value::String(name),
                serde_json::Value::String(role.into()),
            ],
        ).await?;
    }

    info!("core users and roles seeded");
    Ok(())
}

pub(crate) async fn insert_module_defs(
    pool: &DatabasePool,
    fixtures: Vec<DoctypeFixture>,
    workspace_fixtures: Vec<(String, String, String)>,
    module_fixtures: Vec<ModuleFixture>,
) -> Result<()> {
    // Ensure the module_def data table has the app_name column.
    // This handles upgrades from databases created before Rust apps contributed modules.
    add_column_if_missing(pool, "module_def", "app_name", "app_name TEXT").await?;

    let mut module_apps: std::collections::BTreeMap<String, String> = [
        ("Core", "frappe"),
        ("Desk", "frappe"),
        ("Website", "frappe"),
        ("Integrations", "frappe"),
        ("Automation", "frappe"),
        ("Printing", "frappe"),
        ("Email", "frappe"),
        ("Geo", "frappe"),
        ("Contacts", "frappe"),
        ("Custom", "frappe"),
    ]
    .iter()
    .map(|(m, a)| (m.to_string(), a.to_string()))
    .collect();

    for fixture in fixtures {
        if !fixture.module.is_empty() {
            module_apps
                .entry(fixture.module)
                .or_insert_with(|| fixture.app.clone());
        }
    }

    for module_fixture in module_fixtures {
        if !module_fixture.name.is_empty() {
            module_apps
                .entry(module_fixture.name)
                .or_insert_with(|| module_fixture.app.clone());
        }
    }

    for (_name, _json, app) in workspace_fixtures {
        let doc: serde_json::Value = match serde_json::from_str(&_json) {
            Ok(d) => d,
            Err(_) => continue,
        };
        if let Some(module) = doc.get("module").and_then(|m| m.as_str()) {
            if !module.is_empty() {
                module_apps
                    .entry(module.to_string())
                    .or_insert_with(|| app.clone());
            }
        }
    }

    for (module, app) in module_apps {
        let app_name = if app.is_empty() {
            "frappe".to_string()
        } else {
            app
        };
        let _ = pool.execute_sql(
            r#"INSERT OR REPLACE INTO "module_def" (name, creation, modified, modified_by, owner, docstatus, module_name, app_name)
               VALUES (?, datetime('now'), datetime('now'), 'Administrator', 'Administrator', 0, ?, ?)"#,
            vec![
                serde_json::Value::String(module.clone()),
                serde_json::Value::String(module),
                serde_json::Value::String(app_name),
            ],
        ).await;
    }
    Ok(())
}

async fn insert_user_types(pool: &DatabasePool) -> Result<()> {
    // The User Type DocType names records by the `name` field; there is no
    // `user_type` data column.  These records are required when creating Users.
    for user_type in ["System User", "Website User"] {
        let _ = pool.execute_sql(
            r#"INSERT OR REPLACE INTO "user_type" (name, creation, modified, modified_by, owner, docstatus, is_standard)
               VALUES (?, datetime('now'), datetime('now'), 'Administrator', 'Administrator', 0, 1)"#,
            vec![serde_json::Value::String(user_type.into())],
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
    for gender in [
        "Male",
        "Female",
        "Other",
        "Transgender",
        "Genderqueer",
        "Non-Conforming",
        "Prefer not to say",
    ] {
        let _ = pool.execute_sql(
            r#"INSERT OR REPLACE INTO "gender" (name, creation, modified, modified_by, owner, docstatus, gender)
               VALUES (?, datetime('now'), datetime('now'), 'Administrator', 'Administrator', 0, ?)"#,
            vec![
                serde_json::Value::String(gender.into()),
                serde_json::Value::String(gender.into()),
            ],
        ).await;
    }

    for salutation in [
        "Mr", "Ms", "Mx", "Dr", "Mrs", "Madam", "Miss", "Master", "Prof",
    ] {
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

async fn load_workspace_fixtures(
    pool: &DatabasePool,
    workspace_fixtures: Vec<(String, String, String)>,
) -> Result<()> {
    // Ensure workspace table has columns used by Rust app fixtures.
    add_column_if_missing(pool, "workspace", "app", "app TEXT").await?;
    add_column_if_missing(
        pool,
        "workspace",
        "restrict_to_domain",
        "restrict_to_domain TEXT",
    )
    .await?;

    let mut loaded = 0usize;

    // Insert workspace fixtures contributed by Rust apps first.
    for (name, json, _app) in workspace_fixtures {
        let doc: serde_json::Value = match serde_json::from_str(&json) {
            Ok(d) => d,
            Err(e) => {
                warn!("failed to parse workspace fixture {}: {}", name, e);
                continue;
            }
        };
        if let Err(e) = insert_workspace(pool, &doc).await {
            warn!("failed to insert workspace fixture {}: {}", name, e);
            continue;
        }
        loaded += 1;
    }

    // Then load workspace fixtures from the bundled frappe app tree.
    let base = std::path::PathBuf::from("apps/frappe/frappe");
    if base.exists() {
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
                    warn!(
                        "failed to insert workspace from {}: {}",
                        json_path.display(),
                        e
                    );
                    continue;
                }
                loaded += 1;
            }
        }
    }

    if loaded > 0 {
        info!("loaded {} workspace fixtures", loaded);
    }
    Ok(())
}

async fn load_page_fixtures(
    pool: &DatabasePool,
    rust_page_fixtures: Vec<(String, String)>,
) -> Result<()> {
    let mut loaded = 0usize;

    // Load standard Frappe pages from the upstream app tree.
    let base = std::path::PathBuf::from("apps/frappe/frappe");
    if base.exists() {
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
    }

    // Load Rust app page fixtures so workspace links to native pages pass
    // Frappe's page-permission checks for non-admin users.
    for (name, json) in rust_page_fixtures {
        let doc: serde_json::Value = match serde_json::from_str(&json) {
            Ok(d) => d,
            Err(e) => {
                warn!("failed to parse rust page fixture {}: {}", name, e);
                continue;
            }
        };

        if let Err(e) = insert_page(pool, &doc).await {
            warn!("failed to insert rust page fixture {}: {}", name, e);
            continue;
        }
        loaded += 1;
    }

    if loaded > 0 {
        info!("loaded {} page fixtures", loaded);
    }
    Ok(())
}

async fn insert_single_settings(pool: &DatabasePool) -> Result<()> {
    // System Settings — referenced by bootinfo and many real-Frappe code paths.
    let _ = pool.execute_sql(
        r#"INSERT OR REPLACE INTO "system_settings" (
            name, creation, modified, modified_by, owner, docstatus,
            language, time_zone, date_format, time_format, setup_complete,
            currency, float_precision, currency_precision, rounding_method,
            enable_scheduler, max_report_rows, link_field_results_limit
        ) VALUES (
            'System Settings', datetime('now'), datetime('now'), 'Administrator', 'Administrator', 0,
            'en', 'UTC', 'yyyy-mm-dd', 'HH:mm:ss', 1,
            'USD', 3, 2, 'Banker''s Rounding (legacy)',
            0, 100000, 10
        )"#,
        vec![],
    ).await;

    // Print Settings — required by the form sidebar (allow_print_for_draft).
    let _ = pool
        .execute_sql(
            r#"INSERT OR REPLACE INTO "print_settings" (
            name, creation, modified, modified_by, owner, docstatus,
            allow_print_for_draft, allow_print_for_cancelled, print_style,
            font, font_size, pdf_page_size, send_print_as_pdf,
            repeat_header_footer, with_letterhead, add_draft_heading
        ) VALUES (
            'Print Settings', datetime('now'), datetime('now'), 'Administrator', 'Administrator', 0,
            1, 0, 'Redesign',
            'Default', 9.0, 'A4', 1,
            1, 1, 1
        )"#,
            vec![],
        )
        .await;

    // Dashboard Settings per user — avoids the create-on-demand path on every boot.
    for user in ["Administrator", "Guest"] {
        let _ = pool.execute_sql(
            r#"INSERT OR REPLACE INTO "dashboard_settings" (
                name, creation, modified, modified_by, owner, docstatus,
                chart_config
            ) VALUES (?, datetime('now'), datetime('now'), 'Administrator', 'Administrator', 0, '')"#,
            vec![serde_json::Value::String(user.into())],
        ).await;
    }

    // Notification Settings per user — real Frappe code fetches/creates these on
    // first use and falls over when the shim database doesn't behave like a
    // single persistent SQLite connection. Seed the default users up front.
    for user in ["Administrator", "Guest"] {
        let _ = pool
            .execute_sql(
                r#"INSERT OR REPLACE INTO "notification_settings" (
                name, creation, modified, modified_by, owner, docstatus,
                enabled, enable_email_notifications, enable_email_mention,
                enable_email_assignment, enable_email_share, user, seen
            ) VALUES (
                ?, datetime('now'), datetime('now'), 'Administrator', 'Administrator', 0,
                1, 1, 1, 1, 1, ?, 0
            )"#,
                vec![
                    serde_json::Value::String(user.into()),
                    serde_json::Value::String(user.into()),
                ],
            )
            .await;
    }

    // Language master data — real Frappe resolves the active language to a
    // Language document when booting and when translating.
    let _ = pool
        .execute_sql(
            r#"INSERT OR REPLACE INTO "language" (
            name, creation, modified, modified_by, owner, docstatus,
            language_code, language_name, enabled
        ) VALUES (
            'en', datetime('now'), datetime('now'), 'Administrator', 'Administrator', 0,
            'en', 'English', 1
        )"#,
            vec![],
        )
        .await;

    info!("single settings seeded");
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
            page_name, title, icon, module, standard, system_page, restrict_to_domain
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    "#;

    let restrict_to_domain = doc
        .get("restrict_to_domain")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

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
        val(restrict_to_domain),
    ];

    pool.execute_sql(sql, params).await?;
    Ok(())
}

async fn insert_workspace(pool: &DatabasePool, doc: &serde_json::Value) -> Result<()> {
    let name = json_str(doc, "name");
    if name.is_empty() {
        return Ok(());
    }

    // The seeded workspace fixtures reference Dashboard Charts and Number Cards
    // that don't exist in this minimal runtime, which crashes the desk on load.
    // Keep the workspace shell and navigation links, but drop all widget blocks.
    let empty_content = serde_json::Value::String("[]".to_string());
    let content = doc
        .get("content")
        .filter(|v| !v.as_str().map_or(true, |s| s.trim().is_empty()))
        .unwrap_or(&empty_content);

    let sql = r#"
        INSERT OR REPLACE INTO "workspace" (
            name, creation, modified, modified_by, owner, docstatus,
            label, title, icon, public, is_hidden, content, sequence_id, module, parent_page, for_user, app, type, restrict_to_domain
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
        val(content.as_str().unwrap_or("[]").to_string()),
        serde_json::Value::Number(
            serde_json::Number::from_f64(json_f64(doc, "sequence_id")).unwrap_or(0.into()),
        ),
        val(json_str(doc, "module")),
        val(json_str(doc, "parent_page")),
        val(json_str(doc, "for_user")),
        val(json_str(doc, "app")),
        val(json_str(doc, "type")),
        json_str_or_null(doc, "restrict_to_domain"),
    ];

    pool.execute_sql(sql, params).await?;

    // Insert all workspace child tables. Widget blocks (charts, shortcuts, etc.)
    // are kept empty when fixtures don't supply them, which lets Frappe's
    // workspace loader treat them as empty lists instead of None.
    let child_mappings = [
        ("links", "workspace_link"),
        ("charts", "workspace_chart"),
        ("shortcuts", "workspace_shortcut"),
        ("quick_lists", "workspace_quick_list"),
        ("number_cards", "workspace_number_card"),
        ("custom_blocks", "workspace_custom_block"),
        ("roles", "has_role"),
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
    let col_names: Vec<String> = cols
        .iter()
        .filter_map(|c| c.get("name").and_then(|v| v.as_str()).map(String::from))
        .collect();

    if col_names.is_empty() {
        warn!("child table {} does not exist, skipping", table);
        return Ok(());
    }

    // Remove stale child rows so re-syncs don't accumulate duplicates.
    let _ = pool
        .execute_sql(
            &format!(
                r#"DELETE FROM "{}" WHERE parent = ? AND parenttype = ? AND parentfield = ?"#,
                table
            ),
            vec![
                serde_json::Value::String(parent.into()),
                serde_json::Value::String(parenttype.into()),
                serde_json::Value::String(parentfield.into()),
            ],
        )
        .await;

    for (idx, row) in rows.iter().enumerate() {
        let mut values: std::collections::HashMap<String, serde_json::Value> =
            std::collections::HashMap::new();

        // Standard fields
        values.insert(
            "name".into(),
            val(format!("{}-{}-{}", parent, parentfield, idx)),
        );
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
            insert_vals
                .iter()
                .map(|_| "?".to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );

        if let Err(e) = pool.execute_sql(&sql, insert_vals).await {
            warn!("failed to insert child row into {}: {}", table, e);
        }
    }

    Ok(())
}
