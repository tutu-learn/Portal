//! Integration point for Rust Frappe apps.
//!
//! The caller supplies the app list: the `anri-runtime` binary builds it from
//! `rust_apps/apps.json` (generated code in `main.rs`), while library
//! consumers pass their own apps to `runtime::run`. `kiff_logger` is always
//! appended because it provides the core logging DocTypes and is not an
//! optional user-installed app.

use rust_apps_core::{RustApp, RustAppRegistry};

/// Build a registry from the given app list, appending `kiff_logger`.
/// `kiff_logger` is always enabled; it provides the core logging DocTypes and
/// is not configurable via `rust_apps/apps.json`.
pub fn registry_from(mut apps: Vec<Box<dyn RustApp>>) -> RustAppRegistry {
    apps.push(Box::new(kiff_logger::KiffLoggerApp));
    RustAppRegistry::new(apps)
}
