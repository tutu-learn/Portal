pub mod site;

use crate::site::{Site, SiteConfig};
use error::{Result, RuntimeError};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DatabaseConfig {
    #[serde(default = "default_driver")]
    pub driver: String,
    #[serde(default = "default_url")]
    pub url: String,
}

fn default_driver() -> String { "sqlite".into() }
fn default_url() -> String { "./sites/{site}/site.db".into() }

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_workers")]
    pub workers: usize,
}

fn default_host() -> String { "0.0.0.0".into() }
fn default_port() -> u16 { 8000 }
fn default_workers() -> usize { 4 }

#[derive(Debug, Clone, Deserialize, Default)]
pub struct QueueConfig {
    #[serde(default = "default_short")]
    pub short_workers: usize,
    #[serde(default = "default_default")]
    pub default_workers: usize,
    #[serde(default = "default_long")]
    pub long_workers: usize,
}

fn default_short() -> usize { 2 }
fn default_default() -> usize { 2 }
fn default_long() -> usize { 1 }

#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeSection {
    pub frappe_path: String,
    pub erpnext_path: String,
    pub shim_path: String,
    pub sites_path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeConfig {
    pub runtime: RuntimeSection,
    #[serde(default)]
    pub database: DatabaseConfig,
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub queue: QueueConfig,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            runtime: RuntimeSection {
                frappe_path: "./apps/frappe".into(),
                erpnext_path: "".into(),
                shim_path: "./python".into(),
                sites_path: "./sites".into(),
            },
            database: DatabaseConfig {
                driver: "sqlite".into(),
                url: "./sites/{site}/site.db".into(),
            },
            server: ServerConfig {
                host: "0.0.0.0".into(),
                port: 8000,
                workers: 4,
            },
            queue: QueueConfig {
                short_workers: 2,
                default_workers: 2,
                long_workers: 1,
            },
        }
    }
}

impl RuntimeConfig {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: RuntimeConfig = toml::from_str(&content)
            .map_err(|e| RuntimeError::Config(format!("failed to parse runtime.toml: {}", e)))?;
        Ok(config)
    }
}

#[derive(Debug, Clone, Default)]
pub struct SiteManager {
    sites: HashMap<String, Site>,
    sites_path: PathBuf,
}

impl SiteManager {
    pub async fn load<P: AsRef<Path>>(sites_path: P) -> Result<Self> {
        let sites_path = sites_path.as_ref().to_path_buf();
        let mut sites = HashMap::new();

        if !sites_path.exists() {
            std::fs::create_dir_all(&sites_path)?;
        }

        let mut entries = tokio::fs::read_dir(&sites_path).await
            .map_err(RuntimeError::Io)?;

        while let Some(entry) = entries.next_entry().await.map_err(RuntimeError::Io)? {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let config_path = path.join("site_config.json");
            let config = if config_path.exists() {
                let content = tokio::fs::read_to_string(&config_path).await
                    .map_err(RuntimeError::Io)?;
                serde_json::from_str(&content)
                    .map_err(|e| RuntimeError::Config(format!("invalid site_config.json for {}: {}", name, e)))?
            } else {
                warn!("site {} missing site_config.json, using defaults", name);
                SiteConfig::default()
            };

            let site = Site::new(name.clone(), path, config);
            sites.insert(name, site);
        }

        info!("loaded {} sites from {:?}", sites.len(), sites_path);
        Ok(Self { sites, sites_path })
    }

    pub fn get(&self, name: &str) -> Option<&Site> {
        self.sites.get(name)
    }

    pub fn sites(&self) -> &HashMap<String, Site> {
        &self.sites
    }

    pub fn sites_path(&self) -> &Path {
        &self.sites_path
    }

    pub fn create_site(&mut self, name: &str) -> Result<Site> {
        let site_path = self.sites_path.join(name);
        if site_path.exists() {
            return Err(RuntimeError::Validation(format!("site {} already exists", name)));
        }

        std::fs::create_dir_all(&site_path)?;
        std::fs::create_dir_all(site_path.join("private/files"))?;
        std::fs::create_dir_all(site_path.join("private/backups"))?;
        std::fs::create_dir_all(site_path.join("private/logs"))?;
        std::fs::create_dir_all(site_path.join("public/files"))?;

        let config = SiteConfig {
            db_driver: "sqlite".into(),
            db_url: format!("./sites/{}/site.db", name),
            encryption_key: uuid::Uuid::new_v4().to_string().replace("-", ""),
            secret_key: uuid::Uuid::new_v4().to_string().replace("-", ""),
            ..Default::default()
        };

        let config_json = serde_json::to_string_pretty(&config)?;
        std::fs::write(site_path.join("site_config.json"), config_json)?;

        let site = Site::new(name.to_string(), site_path, config);
        self.sites.insert(name.to_string(), site.clone());
        Ok(site)
    }
}
