# Runtime Setup Notes

This document records the framework-level changes required before the Kiff runtime can be built and started.

## 1. Dynamic App Registration from `rust_apps/apps.json`

The runtime should not hardcode which Rust Frappe apps are loaded. Instead, `crates/runtime/build.rs` reads `rust_apps/apps.json` at compile time and generates the app registry.

### How it works

1. `rust_apps/apps.json` lists the apps to load:

   ```json
   {
     "apps": ["sebrus_logger"]
   }
   ```

2. `crates/runtime/build.rs` reads the JSON file and:
   - Generates `registered_apps()` into `OUT_DIR/registered_apps.rs`.
   - Adds/removes the app crates as path dependencies in `crates/runtime/Cargo.toml`.
   - Adds/removes the app crates from the workspace members in the root `Cargo.toml`.

3. `crates/runtime/src/registered_apps.rs` is now a stable stub that includes the generated file:

   ```rust
   include!(concat!(env!("OUT_DIR"), "/registered_apps.rs"));
   ```

### Why two builds are sometimes needed

Cargo cannot add new workspace members or dependencies while a build is already running. When `apps.json` changes and the build script updates the `Cargo.toml` files, the first `cargo build` will emit:

```
warning: apps.json changed; Cargo.toml manifests were updated. Run cargo again to pick up the new app crates.
```

Run the command a second time to compile with the updated manifests.

### Adding a new Rust app

1. Scaffold the app (e.g. `./target/release/kiff new-rust-app my_app`).
2. Add the app name to `rust_apps/apps.json`:

   ```json
   {
     "apps": ["sebrus_logger", "my_app"]
   }
   ```

3. Run `cargo build -p runtime --release` twice (the first run updates `Cargo.toml`, the second compiles the new app).

### Removing an app

1. Remove the app name from `rust_apps/apps.json`.
2. Run `cargo build -p runtime --release` twice.

---

## 2. Test Infrastructure Fix

The HTTP integration tests fail with `503 Service Unavailable` unless the test `AppState` registers a site in `SiteManager` **and** provides a database pool.

### Why

`crates/http/src/site.rs` resolves every request to a site by checking both:

- `AppState.site_manager`
- `AppState.pools`

The original test helper only inserted a pool, so `resolve_site_pool` returned `None` and every handler returned `503`.

### Changes

1. **`crates/config/src/lib.rs`** — add a way to register a site in memory:

   ```rust
   impl SiteManager {
       pub fn register_site(&mut self, site: Site) {
           self.sites.insert(site.name.clone(), site);
       }
   }
   ```

2. **`tests/common/mod.rs`** — register `test_site` in the test helper:

   ```rust
   let site = config::site::Site::new(
       "test_site".into(),
       std::path::PathBuf::from("/tmp/test_site"),
       config::site::SiteConfig::default(),
   );
   let mut site_manager = config::SiteManager::default();
   site_manager.register_site(site);
   ```

### Verification

```bash
cargo test
```

All suites should pass:

- `http`: 21 tests
- `orm`: 9 tests
- `permissions`: 5 tests
- `sql_translator`: 6 tests

---

## 3. Missing Runtime Modules

`crates/runtime/src/main.rs` declares two modules that did not exist in the repository:

- `mod executor;`
- `mod scheduler_backend;`

Without them, `cargo build -p runtime --release` fails.

### Files to Create

#### `crates/runtime/src/executor.rs`

Implements the runtime job executor. It must satisfy `queue::JobExecutor` and dispatch queued jobs to:

1. Rust app `api_methods()` registered in `AppState.rust_apps`.
2. Python whitelisted methods via `kiff_core::call_method(...)`.

```rust
use rust_apps_core::{AppContext, AppState};
use std::collections::HashMap;
use tracing::{info, warn};

pub struct RuntimeExecutor {
    app_state: AppState,
}

impl RuntimeExecutor {
    pub fn new(app_state: AppState) -> Self {
        Self { app_state }
    }
}

#[async_trait::async_trait]
impl queue::JobExecutor for RuntimeExecutor {
    async fn execute(
        &self,
        method: &str,
        kwargs: &HashMap<String, serde_json::Value>,
    ) -> error::Result<()> {
        info!("executing job method: {}", method);

        // Try Rust app API methods first.
        for app in self.app_state.rust_apps.apps() {
            for api_method in app.api_methods() {
                if api_method.name == method {
                    let ctx = AppContext::new(app.name(), self.app_state.clone());
                    let _ = (api_method.handler)(ctx, kwargs.clone()).await?;
                    return Ok(());
                }
            }
        }

        // Fall back to Python whitelisted methods.
        let kwargs_value = serde_json::Value::Object(
            kwargs.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        );
        match tokio::task::spawn_blocking({
            let method = method.to_string();
            let kwargs_value = kwargs_value.clone();
            move || kiff_core::call_method(&method, &kwargs_value)
        })
        .await
        {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => {
                warn!("python job method {} failed: {}", method, e);
                Err(e)
            }
            Err(e) => Err(error::RuntimeError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("job method {} panicked: {}", method, e),
            ))),
        }
    }
}
```

#### `crates/runtime/src/scheduler_backend.rs`

Implements the runtime scheduler backend. It must satisfy `queue::SchedulerBackend` and handle:

- Rust app `scheduled_jobs()`.
- Python `scheduler_events` from Frappe `hooks.py`.

```rust
use crate::hooks::HookRegistry;
use queue::{ScheduleTrigger, SchedulerBackend};
use rust_apps_core::{AppContext, AppState};
use tracing::{info, warn};

fn frequency_cron(freq: &str) -> &str {
    match freq {
        "hourly" => "0 0 * * * * *",
        "daily" => "0 0 0 * * * *",
        "weekly" => "0 0 0 * * 1 *",
        "monthly" => "0 0 0 1 * * *",
        "yearly" => "0 0 0 1 1 * *",
        "all" => "0 * * * * * *",
        other => other,
    }
}

pub struct RuntimeSchedulerBackend {
    app_state: AppState,
    hook_registry: HookRegistry,
}

impl RuntimeSchedulerBackend {
    pub fn new(app_state: AppState, hook_registry: HookRegistry) -> Self {
        Self {
            app_state,
            hook_registry,
        }
    }
}

#[async_trait::async_trait]
impl SchedulerBackend for RuntimeSchedulerBackend {
    async fn triggers(&self) -> error::Result<Vec<ScheduleTrigger>> {
        let mut triggers = Vec::new();

        for app in self.app_state.rust_apps.apps() {
            for job in app.scheduled_jobs() {
                triggers.push(ScheduleTrigger {
                    id: job.name.to_string(),
                    cron: job.cron.to_string(),
                });
            }
        }

        for key in self.hook_registry.hooks.keys() {
            if let Some(freq) = key.strip_prefix("scheduler_events:") {
                triggers.push(ScheduleTrigger {
                    id: format!("py:scheduler_events:{}", freq),
                    cron: frequency_cron(freq).to_string(),
                });
            }
        }

        Ok(triggers)
    }

    async fn fire(&self, trigger_id: &str) -> error::Result<()> {
        info!("firing scheduler trigger: {}", trigger_id);

        if let Some(freq) = trigger_id.strip_prefix("py:scheduler_events:") {
            return self
                .hook_registry
                .run_hook("scheduler_events", Some(freq), None)
                .await;
        }

        for app in self.app_state.rust_apps.apps() {
            for job in app.scheduled_jobs() {
                if job.name == trigger_id {
                    let ctx = AppContext::new(app.name(), self.app_state.clone());
                    return (job.handler)(&ctx).await;
                }
            }
        }

        warn!("no scheduled job found for trigger: {}", trigger_id);
        Ok(())
    }
}
```

### Verification

```bash
cargo build -p runtime --release
cargo build -p kiff --release
```

---

## 4. Starting the Application

### Build

```bash
cargo build -p runtime --release
cargo build -p kiff --release
```

### Run

```bash
export PYTHONPATH="$(pwd)/.venv/lib/python3.14/site-packages"
./target/release/kiff start
```

The server listens on `0.0.0.0:8000` by default (configured in `runtime.toml`).

### Verify

```bash
curl -I http://localhost:8000/
```

Expected: `307 Temporary Redirect` to `/login?redirect-to=%2F`.

---

## Notes

- The runtime sets up the Python path internally and symlinks `libkiff_core.dylib` into the venv site-packages, but setting `PYTHONPATH` externally matches the documented workflow.
- Background queue workers may log SQLite `disk I/O error` (code 522) warnings while polling `__kiff_queue`. These do not prevent the HTTP server from serving requests.
