use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tower_http::cors::CorsLayer;
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

    let config_path = std::env::var("KIFF_RUNTIME_CONFIG").unwrap_or_else(|_| "runtime.toml".into());
    let config = config::RuntimeConfig::from_file(&config_path)?;
    let mut site_manager = config::SiteManager::load(&config.runtime.sites_path).await?;

    // Ensure a default site exists so the Python shim and desk frontend can find
    // a real site database (e.g. sites/localhost/site.db).
    if site_manager.sites().is_empty() {
        info!("no sites found, creating default localhost site");
        site_manager.create_site("localhost")?;
    }

    let site_manager = Arc::new(site_manager);

    // Load Rust app registry before DB setup so we can sync DocType fixtures.
    let rust_app_registry = rust_apps::load_registry();
    info!("loaded {} rust app(s)", rust_app_registry.apps().len());

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
                let workspace_fixtures: Vec<(String, String, String)> = rust_app_registry
                    .all_workspaces()
                    .into_iter()
                    .map(|w| (w.name.to_string(), w.json.to_string(), w.app.to_string()))
                    .collect();
                let module_fixtures = rust_app_registry.all_modules();
                let client_script_fixtures: Vec<(String, String)> = rust_app_registry
                    .all_client_scripts()
                    .into_iter()
                    .map(|c| (c.name.to_string(), c.json.to_string()))
                    .collect();
                if let Err(e) = orm::doctype_sync::sync_all(
                    &p,
                    fixtures,
                    workspace_fixtures,
                    module_fixtures,
                    client_script_fixtures,
                )
                .await
                {
                    error!("doctype sync failed for site {}: {}", name, e);
                }
                // Always ensure the core users and default roles exist, even if
                // the broader doctype sync failed or roles were deleted.
                if let Err(e) = orm::doctype_sync::ensure_core_users_and_roles(&p).await {
                    error!(
                        "failed to ensure core users and roles for site {}: {}",
                        name, e
                    );
                }
                pools.insert(name.clone(), p);
            }
            Err(e) => {
                error!("failed to connect pool for site {}: {}", name, e);
            }
        }
    }

    // Setup Python path — pass the default site's DB info so kiff_core .so can init
    let (default_db_driver, default_db_url) = site_manager
        .sites()
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
        if let Err(e) = kiff_core::init_whitelist() {
            warn!("failed to load Python method whitelist: {}", e);
        }
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
        logger: Arc::new(std::sync::OnceLock::new()),
    };

    // Initialize the shared log engine and start the sink consumer. This used
    // to live in the kiff_logger app; it is now handled by the runtime so
    // that kiff_logger can remain empty while the core crate provides the
    // reusable logging primitives.
    init_log_engine(&config, &app_state).await;

    // Attach runtime state to the registry so document lifecycle hooks can use
    // DB pools and other shared services, then register the hook runner.
    let mut rust_app_registry_for_hooks = rust_app_registry.clone();
    rust_app_registry_for_hooks.set_state(app_state.clone());
    orm::set_hook_runner(Some(Arc::new(rust_app_registry_for_hooks)));

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
    let router = router
        .layer(axum::middleware::from_fn_with_state(
            app_state_for_routes.clone(),
            http::middleware::auth::token_auth_middleware,
        ))
        .layer(CorsLayer::permissive())
        .with_state(app_state_for_routes);
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

async fn init_log_engine(_config: &config::RuntimeConfig, app_state: &rust_apps_core::AppState) {
    // Store the log engine data inside the default site's directory, next to
    // the SQLite database file.
    let data_dir = app_state
        .site_manager
        .sites()
        .iter()
        .next()
        .map(|(_, site)| site.path.join("logengine-data"))
        .unwrap_or_else(|| PathBuf::from("./logengine-data"));

    let commit_interval = Duration::from_secs(30);

    match log_engine::LogService::open_or_create(&data_dir) {
        Ok((service, mut alerts)) => {
            service.spawn_commit_loop(commit_interval);

            // Start the central log sink consumer. Tracing events and document
            // hooks send records to this sink; we forward them into the async
            // log engine here.
            let log_rx = rust_apps_core::logging::init_log_sink();
            rust_apps_core::logging::spawn_log_sink_consumer(service.clone(), log_rx);

            // Forward trigger alerts to tracing for now.
            tokio::spawn(async move {
                while let Some(alert) = alerts.recv().await {
                    tracing::warn!(
                        target: "kiff_logger.alert",
                        trigger = %alert.trigger,
                        service = %alert.record.service,
                        message = %alert.record.message,
                        "trigger fired"
                    );
                }
            });

            // Make the log engine reachable from the Python shim so that
            // virtual DocTypes like Kiff Log Entry can bypass SQL and query
            // logs directly from the Tantivy index.
            kiff_core::init_log_service(service.clone());

            if let Err(_) = app_state.logger.set(service) {
                warn!("log engine already initialized by another task");
            } else {
                info!("log engine ready at {}", data_dir.display());
            }
        }
        Err(e) => {
            warn!("failed to open log engine: {}", e);
        }
    }
}
