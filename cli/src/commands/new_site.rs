use tracing::info;

pub async fn run(name: &str) -> error::Result<()> {
    info!("creating new site: {}", name);
    let mut manager = config::SiteManager::load("./sites").await?;
    let site = manager.create_site(name)?;

    // Create SQLite database and run migrations
    if site.config.db_driver == "sqlite" {
        let pool = orm::DatabasePool::connect_sqlite(&site.db_url()).await?;
        orm::migrations::Migrator::run(&pool).await?;
        info!("migrations complete for {}", name);
    }

    println!("Site {} created at {:?}", name, site.path);
    Ok(())
}
