use std::sync::Arc;
use tracing::{error, info, warn};

mod hooks;
mod logging;
mod registered_apps;
mod rust_apps;
mod startup;

#[tokio::main]
async fn main() -> error::Result<()> {
    logging::init_tracing();
    info!("kiff runtime starting");

    let config = config::RuntimeConfig::from_file("runtime.toml")?;
    let site_manager = Arc::new(config::SiteManager::load(&config.runtime.sites_path).await?);

    // Load Rust app registry before DB setup so we can sync DocType fixtures.
    let rust_app_registry = rust_apps::load_registry();
    info!(
        "loaded {} rust app(s)",
        rust_app_registry.apps().len()
    );

    // Connect DB pools for all sites
    let pools = Arc::new(dashmap::DashMap::new());
    for (name, site) in site_manager.sites() {
        let pool = match site.config.db_driver.as_str() {
            "postgres" => orm::DatabasePool::connect_postgres(&site.db_url()).await,
            _ => orm::DatabasePool::connect_sqlite(&site.db_url()).await,
        };
        match pool {
            Ok(p) => {
                info!("connected pool for site {}", name);
                // Run migrations for each site
                if let Err(e) = orm::migrations::Migrator::run(&p).await {
                    error!("migrations failed for site {}: {}", name, e);
                } else {
                    info!("migrations complete for site {}", name);
                }
                // Sync Frappe doctypes: metadata tables, dynamic data tables, seed data
                let fixtures = rust_app_registry.all_doctypes();
                if let Err(e) = orm::doctype_sync::sync_all(&p, fixtures).await {
                    error!("doctype sync failed for site {}: {}", name, e);
                }
                pools.insert(name.clone(), p);
            }
            Err(e) => {
                error!("failed to connect pool for site {}: {}", name, e);
            }
        }
    }

    // Setup Python path — pass the default site's DB info so kiff_core .so can init
    let (default_db_driver, default_db_url) = site_manager.sites()
        .iter()
        .next()
        .map(|(_, site)| (site.config.db_driver.clone(), site.db_url()))
        .unwrap_or_else(|| ("sqlite".into(), "".into()));
    startup::setup_python_path_with_db(
        &config.runtime.shim_path,
        &config.runtime.frappe_path,
        &config.runtime.erpnext_path,
        Some(&default_db_driver),
        Some(&default_db_url),
    )?;

    // Load Python hooks
    let mut hook_registry = hooks::HookRegistry::new();
    if let Err(e) = hook_registry.load_from_path(&config.runtime.erpnext_path) {
        warn!("failed to load hooks: {}", e);
    }
    let _hooks = Arc::new(hook_registry);

    // Initialize python-bridge with a default pool and pubsub
    let pubsub = Arc::new(queue::PubSub::new());
    if let Some(pool) = pools.iter().next().map(|e| e.value().clone()) {
        let py_rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("failed to build py runtime");
        kiff_core::init(py_rt, pool);
        kiff_core::init_pubsub(pubsub.clone());
    }

    let app_state = http::AppState {
        config: Arc::new(config.clone()),
        site_manager: site_manager.clone(),
        pools: pools.clone(),
        sessions: Arc::new(session::SessionStore::new()),
        permissions: Arc::new(permissions::PermissionEngine::new()),
        metadata: Arc::new(metadata::Meta::new()),
        pubsub,
        translator: Arc::new(sql_translator::SqlTranslator::default()),
        rust_apps: rust_app_registry.clone(),
    };

    // Register Rust app hooks with the ORM so document lifecycle events invoke them.
    orm::set_hook_runner(Some(Arc::new(rust_app_registry.clone())));

    // Run Rust app startup hooks.
    for app in rust_app_registry.apps() {
        let ctx = rust_apps_core::AppContext::new(app.name(), app_state.clone());
        if let Err(e) = app.on_startup(&ctx).await {
            warn!("startup hook for app {} failed: {}", app.name(), e);
        }
    }

    // Build HTTP server, allowing Rust apps to mount routes before state is applied.
    let app_state_for_routes = app_state.clone();
    let mut router: axum::Router<rust_apps_core::AppState> = http::router::create_router();
    for app in rust_app_registry.apps() {
        let ctx = rust_apps_core::AppContext::new(app.name(), app_state_for_routes.clone());
        router = app.routes(&ctx, router);
    }
    let router = router.with_state(app_state_for_routes);
    let http_future = http::run_server_with_router(router, &config.server.host, config.server.port);

    // Start background workers and scheduler if we have pools
    if let Some(pool) = pools.iter().next().map(|e| e.value().clone()) {
        let worker_short = queue::Worker::new("short");
        let worker_default = queue::Worker::new("default");
        let worker_long = queue::Worker::new("long");
        let scheduler = queue::Scheduler::new();

        let pool2 = pool.clone();
        let pool3 = pool.clone();
        let pool4 = pool.clone();
        let pool5 = pool.clone();

        tokio::select! {
            r = http_future => r?,
            _ = worker_short.run(&pool2) => {},
            _ = worker_default.run(&pool3) => {},
            _ = worker_long.run(&pool4) => {},
            _ = scheduler.run(&pool5) => {},
        }
    } else {
        http_future.await?;
    }

    Ok(())
}
