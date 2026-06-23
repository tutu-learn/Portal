//! Static list of Rust Frappe apps to load at startup.
//!
//! This file is updated automatically by `kiff new-rust-app <name>` and can also
//! be edited by hand. Each entry must be a `Box<dyn rust_apps_core::RustApp>`.

use rust_apps_core::RustApp;

pub fn registered_apps() -> Vec<Box<dyn RustApp>> {
    vec![Box::new(audit_ready::AuditReadyApp)]
}
