//! DocType synchronization: Frappe metadata, data tables, seed data, and
//! runtime-injected dynamic fields.

use crate::pool::DatabasePool;
use error::Result;
use tracing::info;

mod data_tables;
mod dynamic_fields;
mod helpers;
mod metadata;
mod permissions;
mod seed_data;

pub use data_tables::add_column_if_missing;
pub(crate) use data_tables::data_table_name;
pub use seed_data::{ensure_core_users_and_roles, hash_user_password};

/// A DocType fixture contributed outside the standard `apps/frappe` tree.
#[derive(Debug, Clone)]
pub struct DoctypeFixture {
    pub module: String,
    pub name: String,
    pub json: String,
    pub app: String,
}

impl DoctypeFixture {
    pub fn new(
        module: impl Into<String>,
        name: impl Into<String>,
        json: impl Into<String>,
    ) -> Self {
        Self {
            module: module.into(),
            name: name.into(),
            json: json.into(),
            app: String::new(),
        }
    }

    pub fn with_app(mut self, app: impl Into<String>) -> Self {
        self.app = app.into();
        self
    }
}

/// A Module fixture contributed by a Rust app.
///
/// Guarantees that the module exists in `module_def` even if the app has no
/// DocType or workspace fixtures for it yet.
#[derive(Debug, Clone)]
pub struct ModuleFixture {
    pub name: String,
    pub app: String,
}

impl ModuleFixture {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            app: String::new(),
        }
    }

    pub fn with_app(mut self, app: impl Into<String>) -> Self {
        self.app = app.into();
        self
    }
}

/// Main entry point: sync metadata tables, create data tables, insert seed data.
pub async fn sync_all(
    pool: &DatabasePool,
    fixtures: Vec<DoctypeFixture>,
    workspace_fixtures: Vec<(String, String, String)>,
    module_fixtures: Vec<ModuleFixture>,
    client_script_fixtures: Vec<(String, String)>,
    page_fixtures: Vec<(String, String)>,
) -> Result<()> {
    info!("syncing frappe doctypes");
    metadata::sync_metadata(pool, fixtures.clone()).await?;
    // Ensure module definitions exist before dynamic field rules are evaluated,
    // because those rules check `module_def.app_name` to decide whether to
    // inject fields (e.g. the Logger tab on User for sebrus_logger).
    seed_data::insert_module_defs(
        pool,
        fixtures.clone(),
        workspace_fixtures.clone(),
        module_fixtures.clone(),
    )
    .await?;
    dynamic_fields::ensure_dynamic_fields(pool).await?;
    data_tables::sync_data_tables(pool).await?;
    dynamic_fields::migrate_legacy_log_viewer_service(pool).await?;
    permissions::ensure_docperm_defaults(pool).await?;
    seed_data::insert_seed_data(pool, workspace_fixtures, page_fixtures)
        .await?;
    seed_data::insert_client_script_fixtures(pool, client_script_fixtures).await?;
    info!("doctype sync complete");
    Ok(())
}
