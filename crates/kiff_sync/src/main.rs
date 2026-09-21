//! Kiff sync server binary.
//!
//! Usage:
//!   kiff-sync-server --bind 0.0.0.0:50051 --data-dir ./sync-data --token <token>
//!
//! Or use environment variables:
//!   KIFF_SYNC_BIND=0.0.0.0:50051
//!   KIFF_SYNC_DATA_DIR=./sync-data
//!   KIFF_SYNC_TOKEN=<token>
//!   KIFF_SYNC_TOKENS_FILE=/path/to/tokens.json

use clap::Parser;
use std::path::PathBuf;
use tracing::info;

#[derive(Parser, Debug)]
#[command(name = "kiff-sync-server")]
struct Args {
    #[arg(long, env = "KIFF_SYNC_BIND", default_value = "0.0.0.0:50051")]
    bind: String,
    #[arg(long, env = "KIFF_SYNC_DATA_DIR", default_value = "./sync-data")]
    data_dir: PathBuf,
    #[arg(long, env = "KIFF_SYNC_TOKEN")]
    token: Option<String>,
    #[arg(long, env = "KIFF_SYNC_TOKENS_FILE")]
    tokens_file: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();
    let mut builder = kiff_sync::server::SyncServerBuilder::new(&args.bind, &args.data_dir);

    if let Some(token) = args.token {
        info!("using global site token");
        builder = builder.global_token(token);
    }

    if let Some(path) = args.tokens_file {
        info!("loading per-site tokens from {}", path.display());
        builder = builder.tokens_file(&path)?;
    }

    builder.serve().await
}
