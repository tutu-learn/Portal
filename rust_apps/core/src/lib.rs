//! SDK for building Frappe apps as Rust crates.
//!
//! Each Rust app implements [`RustApp`] and is registered statically in the
//! runtime. Apps can contribute DocType fixtures, HTTP routes, document hooks,
//! API methods, and scheduled jobs.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use axum::Router;
use dashmap::DashMap;
use serde_json::Value;
use tracing::info;

/// Cache for static Desk assets that change only on deploy.
#[derive(Clone)]
pub struct AssetCache {
    /// Maps bundle name → hashed filename from assets.json, plus the file mtime
    /// we saw when we last loaded it.
    pub bundle_map: Arc<std::sync::RwLock<(HashMap<String, String>, SystemTime)>>,
    /// Concatenated SVG icon sprites, plus the max mtime of the source files.
    pub icon_sprites: Arc<std::sync::RwLock<(String, SystemTime)>>,
}

impl Default for AssetCache {
    fn default() -> Self {
        Self {
            bundle_map: Arc::new(std::sync::RwLock::new((HashMap::new(), SystemTime::UNIX_EPOCH))),
            icon_sprites: Arc::new(std::sync::RwLock::new((String::new(), SystemTime::UNIX_EPOCH))),
        }
    }
}

/// Cache for rendered Desk bootinfo JSON.
#[derive(Clone, Default)]
pub struct BootCache {
    /// (user, cache_key) → (serialized boot JSON, inserted_at)
    pub entries: Arc<DashMap<(String, String), (String, Instant)>>,
}

impl BootCache {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(DashMap::new()),
        }
    }

    /// Maximum age of a cached bootinfo entry before it is ignored.
    const TTL: Duration = Duration::from_secs(5 * 60);

    pub fn get(&self, user: &str, key: &str) -> Option<String> {
        self.entries
            .get(&(user.to_string(), key.to_string()))
            .and_then(|entry| {
                let (json, inserted) = entry.value();
                if inserted.elapsed() <= Self::TTL {
                    Some(json.clone())
                } else {
                    None
                }
            })
    }

    pub fn set(&self, user: &str, key: &str, json: String) {
        self.entries
            .insert((user.to_string(), key.to_string()), (json, Instant::now()));
    }

    /// Remove all cached entries for a user.
    pub fn invalidate_user(&self, user: &str) {
        let prefix = user.to_string();
        self.entries.retain(|(u, _), _| u != &prefix);
    }

    /// Remove every cached entry.
    pub fn invalidate_all(&self) {
        self.entries.clear();
    }
}

pub mod hooks;
pub mod layer;
pub mod logging;

pub use layer::SebrusLoggerLayer;
pub use logging::{log_app_event, log_document_event};

/// Shared runtime state passed to HTTP handlers and Rust apps.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<config::RuntimeConfig>,
    pub site_manager: Arc<config::SiteManager>,
    pub pools: Arc<DashMap<String, orm::DatabasePool>>,
    pub sessions: Arc<session::SessionStore>,
    pub permissions: Arc<permissions::PermissionEngine>,
    pub metadata: Arc<metadata::Meta>,
    pub pubsub: Arc<queue::PubSub>,
    pub translator: Arc<sql_translator::SqlTranslator>,
    pub rust_apps: RustAppRegistry,
    /// Lazy-initialized crash-durable log engine. Apps that provide logging
    /// can set this during `on_startup`; handlers retrieve it with `get()`.
    pub logger: Arc<std::sync::OnceLock<log_engine::LogService>>,
    /// Cached Desk bootinfo JSON per user.
    pub boot_cache: Arc<BootCache>,
    /// Cached static Desk assets (bundle map and icon sprites).
    pub asset_cache: Arc<AssetCache>,
}

/// Context passed to every Rust app during registration and lifecycle hooks.
#[derive(Clone)]
pub struct AppContext {
    pub app_name: &'static str,
    pub state: AppState,
    pub user: Option<String>,
}

impl AppContext {
    pub fn new(app_name: &'static str, state: AppState) -> Self {
        Self {
            app_name,
            state,
            user: None,
        }
    }

    pub fn with_user(mut self, user: Option<String>) -> Self {
        self.user = user;
        self
    }
}

/// A DocType fixture contributed by a Rust app.
///
/// Re-exported from `orm::doctype_sync` so the runtime and apps use the same
/// type.
pub use orm::doctype_sync::DoctypeFixture;

/// A Module fixture contributed by a Rust app.
///
/// Re-exported from `orm::doctype_sync` so the runtime and apps use the same
/// type.
pub use orm::doctype_sync::ModuleFixture;

/// A Workspace fixture contributed by a Rust app.
#[derive(Debug, Clone)]
pub struct WorkspaceFixture {
    pub name: &'static str,
    pub json: &'static str,
    pub app: &'static str,
}

impl WorkspaceFixture {
    pub fn new(name: &'static str, json: &'static str) -> Self {
        Self {
            name,
            json,
            app: "",
        }
    }

    pub fn with_app(mut self, app: &'static str) -> Self {
        self.app = app;
        self
    }
}

/// A Page fixture contributed by a Rust app.
///
/// Pages need more than JSON: they may include a controller script, stylesheet,
/// and HTML templates. Keeping the raw parts in memory lets the runtime serve
/// pages even when the app's source tree is not present at runtime.
#[derive(Debug, Clone, Default)]
pub struct PageFixture {
    pub name: String,
    pub json: String,
    pub script: String,
    pub style: String,
    pub templates: HashMap<String, String>,
}

impl PageFixture {
    pub fn new(name: &str, json: &str) -> Self {
        Self {
            name: name.to_string(),
            json: json.to_string(),
            ..Default::default()
        }
    }

    pub fn with_script(mut self, script: &str) -> Self {
        self.script = script.to_string();
        self
    }

    pub fn with_style(mut self, style: &str) -> Self {
        self.style = style.to_string();
        self
    }

    pub fn with_template(mut self, name: &str, content: &str) -> Self {
        self.templates.insert(name.to_string(), content.to_string());
        self
    }
}

/// A Client Script fixture that adds a custom script to a DocType form or list.
#[derive(Debug, Clone, Default)]
pub struct ClientScriptFixture {
    pub name: String,
    pub json: String,
}

impl ClientScriptFixture {
    pub fn new(name: &str, json: &str) -> Self {
        Self {
            name: name.to_string(),
            json: json.to_string(),
        }
    }
}

/// Document lifecycle event kinds supported for Rust hooks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DocEvent {
    BeforeInsert,
    AfterInsert,
    BeforeSave,
    OnUpdate,
    BeforeSubmit,
    OnSubmit,
    BeforeCancel,
    OnCancel,
    BeforeTrash,
    AfterTrash,
    OnChange,
}

impl DocEvent {
    pub fn as_str(&self) -> &'static str {
        match self {
            DocEvent::BeforeInsert => "before_insert",
            DocEvent::AfterInsert => "after_insert",
            DocEvent::BeforeSave => "before_save",
            DocEvent::OnUpdate => "on_update",
            DocEvent::BeforeSubmit => "before_submit",
            DocEvent::OnSubmit => "on_submit",
            DocEvent::BeforeCancel => "before_cancel",
            DocEvent::OnCancel => "on_cancel",
            DocEvent::BeforeTrash => "before_trash",
            DocEvent::AfterTrash => "after_trash",
            DocEvent::OnChange => "on_change",
        }
    }
}

/// A document hook contributed by a Rust app.
pub struct DocHook {
    pub event: DocEvent,
    pub doctype: &'static str,
    pub handler: BoxDocHook,
}

pub type BoxDocHook =
    Box<dyn Fn(&AppContext, &orm::Document) -> HookResult + Send + Sync + 'static>;

pub type HookResult = error::Result<()>;

impl DocHook {
    pub fn new<F>(event: DocEvent, doctype: &'static str, handler: F) -> Self
    where
        F: Fn(&AppContext, &orm::Document) -> HookResult + Send + Sync + 'static,
    {
        Self {
            event,
            doctype,
            handler: Box::new(handler),
        }
    }
}

/// An API method contributed by a Rust app.
///
/// Methods are addressable as `<app_name>.<method_name>` or simply
/// `<method_name>` if the app registers a unique name.
pub struct ApiMethod {
    pub name: &'static str,
    pub handler: BoxApiMethod,
}

pub type BoxApiMethod =
    Box<dyn Fn(AppContext, HashMap<String, Value>) -> MethodResult + Send + Sync + 'static>;

pub type MethodResult = Pin<Box<dyn Future<Output = error::Result<Value>> + Send + 'static>>;

impl ApiMethod {
    pub fn new<F, Fut>(name: &'static str, handler: F) -> Self
    where
        F: Fn(AppContext, HashMap<String, Value>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = error::Result<Value>> + Send + 'static,
    {
        Self {
            name,
            handler: Box::new(move |ctx, params| Box::pin(handler(ctx, params))),
        }
    }
}

/// A scheduled job contributed by a Rust app.
pub struct ScheduledJob {
    pub name: &'static str,
    /// Cron expression, e.g. "0 9 * * 1" (Monday 9am).
    pub cron: &'static str,
    pub handler: BoxScheduledJob,
}

pub type BoxScheduledJob = Box<
    dyn Fn(&AppContext) -> Pin<Box<dyn Future<Output = error::Result<()>> + Send>>
        + Send
        + Sync
        + 'static,
>;

impl ScheduledJob {
    pub fn new<F, Fut>(name: &'static str, cron: &'static str, handler: F) -> Self
    where
        F: Fn(&AppContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = error::Result<()>> + Send + 'static,
    {
        Self {
            name,
            cron,
            handler: Box::new(move |ctx| Box::pin(handler(ctx))),
        }
    }
}

/// Trait implemented by every Rust Frappe app.
///
/// All methods have default no-op implementations so apps only override what
/// they need.
#[async_trait::async_trait]
pub trait RustApp: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn version(&self) -> &'static str;

    /// Register HTTP routes on the shared Axum router.
    ///
    /// The router is parameterized with `AppState`; state is applied by the
    /// runtime after all apps have contributed their routes. Handlers can
    /// extract `State<AppState>` as usual.
    fn routes(&self, _ctx: &AppContext, router: Router<AppState>) -> Router<AppState> {
        router
    }

    /// Return DocType JSON fixtures to sync into the metadata tables.
    fn doctypes(&self) -> Vec<DoctypeFixture> {
        vec![]
    }

    /// Return Workspace JSON fixtures to sync into the metadata tables.
    fn workspaces(&self) -> Vec<WorkspaceFixture> {
        vec![]
    }

    /// Return Module fixtures to guarantee modules exist in `module_def`
    /// even when the app has no DocType or workspace for them yet.
    fn modules(&self) -> Vec<ModuleFixture> {
        vec![]
    }

    /// Return Page fixtures for desk pages served by this app.
    fn pages(&self) -> Vec<PageFixture> {
        vec![]
    }

    /// Return Client Script fixtures to inject into Desk forms/lists.
    fn client_scripts(&self) -> Vec<ClientScriptFixture> {
        vec![]
    }

    /// Register Rust handlers for document lifecycle events.
    fn doc_hooks(&self) -> Vec<DocHook> {
        vec![]
    }

    /// Register scheduled jobs.
    fn scheduled_jobs(&self) -> Vec<ScheduledJob> {
        vec![]
    }

    /// Register API methods callable via `/api/method/:method`.
    fn api_methods(&self) -> Vec<ApiMethod> {
        vec![]
    }

    /// Called after `AppState` is constructed but before the HTTP server starts.
    async fn on_startup(&self, _ctx: &AppContext) -> error::Result<()> {
        Ok(())
    }

    /// Called on graceful shutdown.
    async fn on_shutdown(&self, _ctx: &AppContext) -> error::Result<()> {
        Ok(())
    }
}

/// Registry of all statically registered Rust apps.
#[derive(Clone, Default)]
pub struct RustAppRegistry {
    apps: Arc<Vec<Box<dyn RustApp>>>,
    /// Runtime state available after the HTTP layer is built. Stored here so
    /// document lifecycle hooks can access DB pools and other shared state.
    state: Option<Arc<AppState>>,
}

impl RustAppRegistry {
    pub fn new(apps: Vec<Box<dyn RustApp>>) -> Self {
        Self {
            apps: Arc::new(apps),
            state: None,
        }
    }

    /// Attach the shared runtime state so hooks can use pools, permissions,
    /// etc. This must be called before `set_hook_runner` registers the runner.
    pub fn set_state(&mut self, state: AppState) {
        self.state = Some(Arc::new(state));
    }

    pub fn apps(&self) -> &[Box<dyn RustApp>] {
        &self.apps
    }

    pub fn get(&self, name: &str) -> Option<&Box<dyn RustApp>> {
        self.apps.iter().find(|app| app.name() == name)
    }

    pub fn all_doctypes(&self) -> Vec<DoctypeFixture> {
        self.apps
            .iter()
            .flat_map(|app| {
                let app_name = app.name();
                app.doctypes().into_iter().map(move |mut f| {
                    if f.app.is_empty() {
                        f.app = app_name.to_string();
                    }
                    f
                })
            })
            .collect()
    }

    pub fn all_workspaces(&self) -> Vec<WorkspaceFixture> {
        self.apps
            .iter()
            .flat_map(|app| {
                let app_name = app.name();
                app.workspaces().into_iter().map(move |mut f| {
                    if f.app.is_empty() {
                        f.app = app_name;
                    }
                    f
                })
            })
            .collect()
    }

    pub fn all_modules(&self) -> Vec<ModuleFixture> {
        self.apps
            .iter()
            .flat_map(|app| {
                let app_name = app.name();
                app.modules().into_iter().map(move |mut f| {
                    if f.app.is_empty() {
                        f.app = app_name.to_string();
                    }
                    f
                })
            })
            .collect()
    }

    pub fn all_pages(&self) -> Vec<PageFixture> {
        self.apps.iter().flat_map(|app| app.pages()).collect()
    }

    pub fn all_client_scripts(&self) -> Vec<ClientScriptFixture> {
        self.apps
            .iter()
            .flat_map(|app| app.client_scripts())
            .collect()
    }

    pub fn all_doc_hooks(&self) -> Vec<DocHook> {
        self.apps.iter().flat_map(|app| app.doc_hooks()).collect()
    }

    pub fn all_api_methods(&self) -> Vec<ApiMethod> {
        self.apps.iter().flat_map(|app| app.api_methods()).collect()
    }

    pub fn all_scheduled_jobs(&self) -> Vec<ScheduledJob> {
        self.apps
            .iter()
            .flat_map(|app| app.scheduled_jobs())
            .collect()
    }
}

/// DocTypes whose writes should invalidate the per-user Desk bootinfo cache.
const BOOT_INVALIDATING_DOCTYPES: &[&str] = &[
    "User",
    "Role",
    "Has Role",
    "DocPerm",
    "Workspace",
    "Module Def",
    "Property Setter",
    "Custom Field",
    "Client Script",
    "Navbar Settings",
    "Notification Settings",
    "Letter Head",
    "Desktop Settings",
    "DocType",
    "DocField",
];

#[async_trait::async_trait]
impl orm::DocHookRunner for RustAppRegistry {
    async fn run_hook(&self, event: &str, doctype: &str, doc: &orm::Document) -> error::Result<()> {
        // Invalidate the Desk bootinfo cache when config/permission/user data
        // changes. Global changes clear the whole cache; per-user changes
        // (e.g. User.desk_properties) clear only that user.
        if matches!(event, "after_insert" | "on_update" | "after_trash")
            && BOOT_INVALIDATING_DOCTYPES.contains(&doctype)
        {
            if let Some(state) = self.state.as_ref() {
                if doctype == "User" {
                    state.boot_cache.invalidate_user(&doc.name);
                } else {
                    state.boot_cache.invalidate_all();
                }
            }
        }

        for app in self.apps.iter() {
            for hook in app.doc_hooks() {
                if hook.event.as_str() == event && hook.doctype == doctype {
                    let state = self
                        .state
                        .as_ref()
                        .map(|s| (**s).clone())
                        .unwrap_or_else(|| AppState {
                            config: Arc::new(config::RuntimeConfig::default()),
                            site_manager: Arc::new(config::SiteManager::default()),
                            pools: Arc::new(DashMap::new()),
                            sessions: Arc::new(session::SessionStore::new()),
                            permissions: Arc::new(permissions::PermissionEngine::new()),
                            metadata: Arc::new(metadata::Meta::new()),
                            pubsub: Arc::new(queue::PubSub::new()),
                            translator: Arc::new(sql_translator::SqlTranslator::default()),
                            rust_apps: RustAppRegistry::default(),
                            logger: Arc::new(std::sync::OnceLock::new()),
                            boot_cache: Arc::new(BootCache::new()),
                            asset_cache: Arc::new(AssetCache::default()),
                        });
                    let ctx = AppContext::new(app.name(), state);
                    (hook.handler)(&ctx, doc)?;
                }
            }
        }
        Ok(())
    }
}

impl RustAppRegistry {
    /// Find and invoke a Rust API method by name.
    ///
    /// Returns `Ok(None)` if no Rust method with this name is registered,
    /// allowing the caller to fall back to another dispatcher (e.g. Python).
    pub async fn call_method(
        &self,
        name: &str,
        state: AppState,
        params: HashMap<String, Value>,
        user: Option<String>,
    ) -> error::Result<Option<Value>> {
        for app in self.apps.iter() {
            for method in app.api_methods() {
                if method.name == name {
                    let ctx = AppContext::new(app.name(), state).with_user(user);
                    return (method.handler)(ctx, params).await.map(Some);
                }
            }
        }
        Ok(None)
    }
}

/// Ensure the `User.home_page` field and its backing data column exist.
///
/// Some Portal/Frappe checkouts omit this field. We inject it into `docfield`
/// and the `user` table so the framework-wide property setters below always
/// have something to apply to.
pub async fn ensure_user_home_page_field(pool: &orm::DatabasePool) -> error::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();

    // Inject the DocField metadata if it is missing.
    let existing_field = pool
        .execute_sql(
            r#"SELECT 1 FROM "docfield" WHERE parent = 'User' AND fieldname = 'home_page' LIMIT 1"#,
            vec![],
        )
        .await?;

    if existing_field.is_empty() {
        pool.execute_sql(
            r#"
            INSERT INTO "docfield" (
                name, creation, modified, modified_by, owner, docstatus,
                parent, parentfield, parenttype, idx, fieldname, fieldtype, label,
                options, description, permlevel, reqd, read_only, hidden, in_list_view,
                in_standard_filter, in_preview, in_global_search, in_filter,
                bold, italic, no_copy, allow_in_quick_entry, translatable,
                collapsible, "unique", set_only_once, remember_last_selected_value,
                ignore_user_permissions, allow_on_submit, report_hide, search_index,
                show_dashboard, "default", depends_on, fetch_from, fetch_if_empty,
                mandatory_depends_on, read_only_depends_on, placeholder, tooltip,
                is_system_generated
            ) VALUES (
                'User-home_page', ?, ?, 'Administrator', 'Administrator', 0,
                'User', 'fields', 'DocType', 90, 'home_page', 'Data', 'Home Page', '',
                'Route the user lands on after login (e.g. /desk/crm). Leave blank to use the default desk.',
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, '', '', 0, '', '', '', '', 0
            )
            ON CONFLICT(name) DO UPDATE SET
                modified=EXCLUDED.modified, fieldtype=EXCLUDED.fieldtype, label=EXCLUDED.label,
                description=EXCLUDED.description, hidden=EXCLUDED.hidden
            "#,
            vec![now.clone().into(), now.clone().into()],
        )
        .await?;
        info!("ensured User.home_page field in docfield metadata");
    }

    // Ensure the backing column exists in the user data table.
    let column_exists = match pool.dialect() {
        "postgres" => {
            let rows = pool
                .execute_sql(
                    r#"SELECT 1 FROM information_schema.columns
                       WHERE table_name = 'user' AND column_name = 'home_page'"#,
                    vec![],
                )
                .await?;
            !rows.is_empty()
        }
        _ => {
            let rows = pool
                .execute_sql(r#"PRAGMA table_info("user")"#, vec![])
                .await?;
            rows.into_iter().any(|mut r| {
                r.remove("name")
                    .and_then(|v| v.as_str().map(String::from))
                    .as_deref()
                    == Some("home_page")
            })
        }
    };

    if !column_exists {
        pool.execute_sql(
            r#"ALTER TABLE "user" ADD COLUMN "home_page" TEXT"#,
            vec![],
        )
        .await?;
        info!("added User.home_page column to user data table");
    }

    Ok(())
}

/// Seed framework-wide Property Setters that should exist on every site.
/// This runs during runtime startup for each site, before Rust app hooks.
///
/// If `home_page_default` is provided, it is also set as the default value for
/// `User.home_page` so new users are created with that landing page.
pub async fn seed_framework_property_setters(
    pool: &orm::DatabasePool,
    home_page_default: Option<&str>,
) -> error::Result<()> {
    ensure_user_home_page_field(pool).await?;

    let now = chrono::Utc::now().to_rfc3339();

    // Always unhide User.home_page.
    let hidden_sql = r#"
        INSERT INTO "property_setter" (
            name, creation, modified, modified_by, owner, docstatus,
            doctype_or_field, doc_type, field_name, property, property_type, value
        ) VALUES (
            'User-home_page-hidden', ?, ?, 'Administrator', 'Administrator', 0,
            'DocField', 'User', 'home_page', 'hidden', 'Check', '0'
        )
        ON CONFLICT(name) DO UPDATE SET
            modified=EXCLUDED.modified, value=EXCLUDED.value
    "#;
    pool.execute_sql(hidden_sql, vec![now.clone().into(), now.clone().into()])
        .await?;
    info!("seeded framework property setter: User.home_page hidden=0");

    // Optionally seed a default value for User.home_page.
    if let Some(default) = home_page_default {
        let default_sql = r#"
            INSERT INTO "property_setter" (
                name, creation, modified, modified_by, owner, docstatus,
                doctype_or_field, doc_type, field_name, property, property_type, value
            ) VALUES (
                'User-home_page-default', ?, ?, 'Administrator', 'Administrator', 0,
                'DocField', 'User', 'home_page', 'default', 'Data', ?
            )
            ON CONFLICT(name) DO UPDATE SET
                modified=EXCLUDED.modified, value=EXCLUDED.value
        "#;
        pool.execute_sql(
            default_sql,
            vec![
                now.clone().into(),
                now.into(),
                serde_json::Value::String(default.to_string()),
            ],
        )
        .await?;
        info!(
            "seeded framework property setter: User.home_page default={}",
            default
        );
    }

    Ok(())
}

/// Back-fill `User.home_page` for existing users when a custom default is
/// configured.
///
/// New users automatically inherit the value via the Property Setter default,
/// but records created before that setter existed (or on sites where the field
/// was missing entirely) are updated on every startup so the setting "sticks".
/// Only blank values are overwritten, preserving any user-specific choice.
pub async fn seed_framework_user_home_page_defaults(
    pool: &orm::DatabasePool,
    default: Option<&str>,
) -> error::Result<()> {
    let Some(default) = default else {
        return Ok(());
    };
    if default.is_empty() {
        return Ok(());
    }

    // Only touch records that are actually blank so user-specific choices are
    // preserved.
    let blank_users = pool
        .execute_sql(
            r#"SELECT name FROM "user" WHERE COALESCE("home_page", '') = ''"#,
            vec![],
        )
        .await?;

    if blank_users.is_empty() {
        return Ok(());
    }

    let now = chrono::Utc::now().to_rfc3339();
    pool.execute_sql(
        r#"
        UPDATE "user"
        SET "home_page" = ?, modified = ?, modified_by = 'Administrator'
        WHERE COALESCE("home_page", '') = ''
        "#,
        vec![
            serde_json::Value::String(default.to_string()),
            serde_json::Value::String(now),
        ],
    )
    .await?;

    info!(
        "back-filled {} existing User record(s) with home_page default={}",
        blank_users.len(),
        default
    );

    Ok(())
}

/// Seed framework-wide User permissions so every user can read their own
/// User record (and therefore see fields like `home_page` on their profile).
pub async fn seed_framework_user_permissions(pool: &orm::DatabasePool) -> error::Result<()> {
    // Check whether the own-record read permission already exists.
    let existing = pool
        .execute_sql(
            r#"SELECT 1 FROM __kiff_docperm
               WHERE parent = 'User' AND role = 'All' AND permlevel = 0 AND if_owner = 1
               LIMIT 1"#,
            vec![],
        )
        .await?;

    if existing.is_empty() {
        pool.execute_sql(
            r#"
            INSERT INTO __kiff_docperm (
                parent, role, permlevel, "read", "write", "create", "delete",
                "submit", "cancel", if_owner, "select", "report", "export", "import",
                "share", "print", "email", "mask", "amend"
            ) VALUES (
                'User', 'All', 0, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0
            )
            "#,
            vec![],
        )
        .await?;
        info!("seeded framework user permission: All can read own User record");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boot_cache_stores_and_retrieves() {
        let cache = BootCache::new();
        assert!(cache.get("alice", "key1").is_none());
        cache.set("alice", "key1", "boot-json".to_string());
        assert_eq!(cache.get("alice", "key1").as_deref(), Some("boot-json"));
    }

    #[test]
    fn boot_cache_invalidates_per_user() {
        let cache = BootCache::new();
        cache.set("alice", "k", "a".to_string());
        cache.set("bob", "k", "b".to_string());
        cache.invalidate_user("alice");
        assert!(cache.get("alice", "k").is_none());
        assert_eq!(cache.get("bob", "k").as_deref(), Some("b"));
    }

    #[test]
    fn boot_cache_invalidates_all() {
        let cache = BootCache::new();
        cache.set("alice", "k", "a".to_string());
        cache.set("bob", "k", "b".to_string());
        cache.invalidate_all();
        assert!(cache.get("alice", "k").is_none());
        assert!(cache.get("bob", "k").is_none());
    }
}
