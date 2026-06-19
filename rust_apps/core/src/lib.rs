//! SDK for building Frappe apps as Rust crates.
//!
//! Each Rust app implements [`RustApp`] and is registered statically in the
//! runtime. Apps can contribute DocType fixtures, HTTP routes, document hooks,
//! API methods, and scheduled jobs.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use axum::Router;
use dashmap::DashMap;
use serde_json::Value;

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
}

/// Context passed to every Rust app during registration and lifecycle hooks.
#[derive(Clone)]
pub struct AppContext {
    pub app_name: &'static str,
    pub state: AppState,
}

impl AppContext {
    pub fn new(app_name: &'static str, state: AppState) -> Self {
        Self { app_name, state }
    }
}

/// A DocType fixture contributed by a Rust app.
///
/// Re-exported from `orm::doctype_sync` so the runtime and apps use the same
/// type.
pub use orm::doctype_sync::DoctypeFixture;

/// A Workspace fixture contributed by a Rust app.
#[derive(Debug, Clone)]
pub struct WorkspaceFixture {
    pub name: &'static str,
    pub json: &'static str,
    pub app: &'static str,
}

impl WorkspaceFixture {
    pub fn new(name: &'static str, json: &'static str) -> Self {
        Self { name, json, app: "" }
    }

    pub fn with_app(mut self, app: &'static str) -> Self {
        self.app = app;
        self
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

pub type BoxApiMethod = Box<
    dyn Fn(AppContext, HashMap<String, Value>) -> MethodResult + Send + Sync + 'static,
>;

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

pub type BoxScheduledJob =
    Box<dyn Fn(&AppContext) -> Pin<Box<dyn Future<Output = error::Result<()>> + Send>> + Send + Sync + 'static>;

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
}

impl RustAppRegistry {
    pub fn new(apps: Vec<Box<dyn RustApp>>) -> Self {
        Self {
            apps: Arc::new(apps),
        }
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

    pub fn all_doc_hooks(&self) -> Vec<DocHook> {
        self.apps
            .iter()
            .flat_map(|app| app.doc_hooks())
            .collect()
    }

    pub fn all_api_methods(&self) -> Vec<ApiMethod> {
        self.apps
            .iter()
            .flat_map(|app| app.api_methods())
            .collect()
    }

    pub fn all_scheduled_jobs(&self) -> Vec<ScheduledJob> {
        self.apps
            .iter()
            .flat_map(|app| app.scheduled_jobs())
            .collect()
    }
}

#[async_trait::async_trait]
impl orm::DocHookRunner for RustAppRegistry {
    async fn run_hook(&self, event: &str, doctype: &str, doc: &orm::Document) -> error::Result<()> {
        for app in self.apps.iter() {
            for hook in app.doc_hooks() {
                if hook.event.as_str() == event && hook.doctype == doctype {
                    let ctx = AppContext::new(app.name(), AppState {
                        config: Arc::new(config::RuntimeConfig::default()),
                        site_manager: Arc::new(config::SiteManager::default()),
                        pools: Arc::new(DashMap::new()),
                        sessions: Arc::new(session::SessionStore::new()),
                        permissions: Arc::new(permissions::PermissionEngine::new()),
                        metadata: Arc::new(metadata::Meta::new()),
                        pubsub: Arc::new(queue::PubSub::new()),
                        translator: Arc::new(sql_translator::SqlTranslator::default()),
                        rust_apps: RustAppRegistry::default(),
                    });
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
    ) -> error::Result<Option<Value>> {
        for app in self.apps.iter() {
            for method in app.api_methods() {
                if method.name == name {
                    let ctx = AppContext::new(app.name(), state);
                    return (method.handler)(ctx, params).await.map(Some);
                }
            }
        }
        Ok(None)
    }
}
