//! Integration point for Rust Frappe apps.
//!
//! `registered_apps()` is generated at build time from `rust_apps/apps.json`.
//! `kiff_logger` is always loaded because it provides the core logging
//! DocTypes and is not an optional user-installed app.

use rust_apps_core::RustAppRegistry;

/// Build a registry from the statically registered app list, then append
/// `kiff_logger`. `kiff_logger` is always enabled; it provides the core logging
/// DocTypes and is not configurable via `rust_apps/apps.json`.
pub fn load_registry() -> RustAppRegistry {
    let mut apps = crate::registered_apps::registered_apps();
    apps.push(Box::new(kiff_logger::KiffLoggerApp));
    RustAppRegistry::new(apps)
}
