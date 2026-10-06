use tracing::info;

pub async fn run(site: &str) -> error::Result<()> {
    let manager = config::SiteManager::load("./sites").await?;
    let site = manager
        .get(site)
        .ok_or_else(|| error::RuntimeError::Config(format!("site {} not found", site)))?;

    info!("migrating site {} to sharded SQLite", site.name);
    orm::shard_migration::migrate_site_to_sharded(&site.path).await?;
    info!("sharded migration complete for {}", site.name);
    Ok(())
}
