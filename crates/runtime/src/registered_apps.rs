//! Static list of Rust Frappe apps to load at startup.
//!
//! This file is a stable stub. The actual `registered_apps()` function is
//! generated at build time by `crates/runtime/build.rs` from
//! `rust_apps/apps.json` and included from `OUT_DIR` below.
//!
//! To add or remove an app, edit `rust_apps/apps.json` and rebuild.

include!(concat!(env!("OUT_DIR"), "/registered_apps.rs"));
