//! `anri-runtime` binary: starts the Anri runtime with the in-tree Rust apps
//! generated from `rust_apps/apps.json` (see `crates/runtime/build.rs`).
//! External app repos call `runtime::run` (usually via the `anri` facade)
//! with their own app list instead.

mod registered_apps;

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> error::Result<()> {
    runtime::run(registered_apps::registered_apps()).await
}
