//! Integration point for Rust Frappe apps.

use rust_apps_core::RustAppRegistry;

pub use rust_apps_core::RustApp;

/// Build a registry from the statically registered app list.
pub fn load_registry() -> RustAppRegistry {
    RustAppRegistry::new(super::registered_apps::registered_apps())
}
