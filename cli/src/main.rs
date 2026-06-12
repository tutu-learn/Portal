use clap::{Parser, Subcommand};
use tracing::info;

mod commands;

#[derive(Parser)]
#[command(name = "kiff")]
#[command(about = "Kiff runtime CLI — replaces bench")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Init,
    Start,
    NewSite { name: String },
    Migrate,
    Shell,
    Backup,
}

#[tokio::main]
async fn main() -> error::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init => commands::init::run().await,
        Commands::Start => commands::start::run().await,
        Commands::NewSite { name } => commands::new_site::run(&name).await,
        Commands::Migrate => commands::migrate::run().await,
        Commands::Shell => commands::shell::run().await,
        Commands::Backup => commands::backup::run().await,
    }
}
