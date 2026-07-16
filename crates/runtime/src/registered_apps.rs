//! Static list of Rust Frappe apps to load at startup.
//!
//! This file is generated at build time by `crates/runtime/build.rs` from
//! `rust_apps/apps.json`. Do not edit it by hand; update the JSON config
//! instead.
//!
//! If `rust_apps/apps.json` is missing or malformed the generated list falls
//! back to empty, so only `kiff_logger` (appended by `rust_apps.rs`) is loaded.

use rust_apps_core::RustApp;

pub fn registered_apps() -> Vec<Box<dyn RustApp>> {
    vec![
        Box::new(sebrus_logger::SebrusLoggerApp),
    ]
}
