//! Anri framework facade.
//!
//! This is the single crate an Anri app repo depends on. It re-exports the
//! app SDK from `rust_apps_core` and the runtime entry points from `runtime`,
//! so an app's `Cargo.toml` only needs:
//!
//! ```toml
//! [dependencies]
//! anri = { git = "https://github.com/<org>/NexsusAG.git", rev = "<pinned commit>" }
//! # or during local development:
//! # anri = { path = "../NexsusAG/crates/anri" }
//! ```
//!
//! and its binary looks like:
//!
//! ```rust,no_run
//! use anri::prelude::*;
//!
//! struct MyApp;
//!
//! #[async_trait::async_trait]
//! impl RustApp for MyApp {
//!     fn name(&self) -> &'static str { "my_app" }
//!     fn version(&self) -> &'static str { "0.1.0" }
//! }
//!
//! #[tokio::main(flavor = "multi_thread", worker_threads = 2)]
//! async fn main() -> anri::Result<()> {
//!     anri::run(vec![Box::new(MyApp)]).await
//! }
//! ```

pub use error;
pub use rust_apps_core;

pub use runtime::{run, run_with_config};

/// Re-exported so apps can write `#[anri::async_trait]` on their `RustApp`
/// impls without adding their own `async-trait` dependency.
pub use async_trait::async_trait;

/// Result type used throughout the framework.
pub type Result<T> = error::Result<T>;

/// Everything an app needs, importable as `use anri::prelude::*;`.
pub mod prelude {
    pub use crate::{async_trait, run, run_with_config, Result};
    pub use rust_apps_core::{
        ApiMethod, AppContext, AppState, ClientScriptFixture, DocEvent, DocHook, DoctypeFixture,
        ModuleFixture, PageFixture, RustApp, RustAppRegistry, ScheduledJob, WorkspaceFixture,
    };
}
