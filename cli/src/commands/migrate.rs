use tracing::info;

pub async fn run() -> error::Result<()> {
    let manager = config::SiteManager::load("./sites").await?;
    for (name, site) in manager.sites() {
        info!("migrating site: {}", name);
        let pool = match site.config.db_driver.as_str() {
            "postgres" => orm::DatabasePool::connect_postgres(&site.db_url()).await,
            _ => orm::DatabasePool::connect_sqlite(&site.db_url()).await,
        };
        match pool {
            Ok(p) => {
                orm::migrations::Migrator::run(&p).await?;
                info!("migrations complete for {}", name);
            }
            Err(e) => {
                eprintln!("failed to connect to {}: {}", name, e);
            }
        }
    }
    Ok(())
}
