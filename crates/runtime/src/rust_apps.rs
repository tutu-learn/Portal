//! Integration point for Rust Frappe apps.
//!
//! Apps are compiled in via `registered_apps.rs`, but which ones are actually
//! enabled is controlled by `rust_apps/apps.json`. This lets operators install
//! and activate Rust apps without editing source code.
//!
//! `kiff_logger` is always loaded because it provides the core logging
//! DocTypes and is not an optional user-installed app.

use rust_apps_core::{RustApp, RustAppRegistry};
use serde::Deserialize;
use std::collections::HashSet;
use tracing::{info, warn};

const APPS_CONFIG_PATH: &str = "rust_apps/apps.json";

#[derive(Debug, Deserialize)]
struct AppsConfig {
    #[serde(default)]
    apps: Vec<String>,
}

/// Build a registry from the statically registered app list, filtered by
/// `rust_apps/apps.json`. If the config file is missing or invalid, all
/// registered apps are enabled (backward-compatible behaviour).
pub fn load_registry() -> RustAppRegistry {
    let registered = super::registered_apps::registered_apps();

    let enabled_names = match load_enabled_apps() {
        Ok(names) => {
            info!("rust_apps/apps.json enables {:?}", names);
            names
        }
        Err(e) => {
            warn!(
                "failed to read {}, enabling all registered Rust apps: {}",
                APPS_CONFIG_PATH, e
            );
            HashSet::new()
        }
    };

    if enabled_names.is_empty() {
        return RustAppRegistry::new(registered);
    }

    let mut filtered: Vec<Box<dyn RustApp>> = registered
        .into_iter()
        .filter(|app| enabled_names.contains(app.name()))
        .collect();

    if filtered.is_empty() {
        warn!(
            "no registered Rust apps matched {}; check the app names",
            APPS_CONFIG_PATH
        );
    }

    // Kiff logger is always enabled; it provides the core logging DocTypes and
    // is not configurable via rust_apps/apps.json.
    filtered.push(Box::new(kiff_logger::KiffLoggerApp));

    RustAppRegistry::new(filtered)
}

fn load_enabled_apps() -> error::Result<HashSet<String>> {
    let content = std::fs::read_to_string(APPS_CONFIG_PATH)?;
    let config: AppsConfig = serde_json::from_str(&content)?;
    Ok(config.apps.into_iter().collect())
}
