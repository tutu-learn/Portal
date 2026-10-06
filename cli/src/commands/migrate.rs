use tracing::info;

pub async fn run() -> error::Result<()> {
    let manager = config::SiteManager::load("./sites").await?;
    for (name, site) in manager.sites() {
        info!("migrating site: {}", name);
        let pool = orm::DatabasePool::connect_sqlite(&site.db_url()).await;
        match pool {
            Ok(p) => {
                orm::migrations::Migrator::run(&p.domain_pools()).await?;
                info!("migrations complete for {}", name);
            }
            Err(e) => {
                eprintln!("failed to connect to {}: {}", name, e);
            }
        }
    }
    Ok(())
}
