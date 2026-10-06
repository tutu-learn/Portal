use tracing::info;

pub async fn run() -> error::Result<()> {
    let manager = config::SiteManager::load("./sites").await?;
    for (name, site) in manager.sites() {
        let backup_dir = site.private.join("backups");
        std::fs::create_dir_all(&backup_dir)?;
        info!("backing up site: {}", name);

        let src = site.path.join("site.db");
        let dst = backup_dir.join(format!(
            "{}_{}.db",
            name,
            chrono::Utc::now().format("%Y%m%d_%H%M%S")
        ));
        if src.exists() {
            std::fs::copy(&src, &dst)?;
            println!("backup created: {:?}", dst);
        }
    }
    Ok(())
}
