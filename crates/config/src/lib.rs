pub mod site;

use crate::site::{Site, SiteConfig};
use base64::{engine::general_purpose::URL_SAFE, Engine as _};
use error::{Result, RuntimeError};
use rand::RngCore;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// Generate a Fernet-compatible encryption key.
///
/// Frappe expects `encryption_key` to be a base64-url-safe encoded 32-byte
/// value (the same format `cryptography.fernet.Fernet.generate_key()` emits).
/// The previous UUID-hex string was not accepted by Fernet, which broke
/// `get_decrypted_password` / `set_encrypted_password` for all Password fields.
pub fn generate_fernet_key() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE.encode(bytes)
}

/// Returns true if `key` is a valid Fernet key (decodes to 32 bytes).
pub fn is_valid_fernet_key(key: &str) -> bool {
    URL_SAFE.decode(key).map(|b| b.len() == 32).unwrap_or(false)
}

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
            if !config_path.exists() {
                // Directories like sites/assets are build output, not real sites.
                // Only treat directories with an explicit site_config.json as sites.
                continue;
            }
            let content = tokio::fs::read_to_string(&config_path).await
                .map_err(RuntimeError::Io)?;
            let mut config: SiteConfig = serde_json::from_str(&content)
                .map_err(|e| RuntimeError::Config(format!("invalid site_config.json for {}: {}", name, e)))?;

            // Allow operators to pin a stable encryption key from a cluster
            // secret / environment variable. This is essential for stateless
            // deployments where site_config.json may be rebuilt from the image.
            if let Ok(env_key) = std::env::var("FRAPPE_ENCRYPTION_KEY") {
                if !env_key.is_empty() {
                    if is_valid_fernet_key(&env_key) {
                        if config.encryption_key != env_key {
                            info!("using FRAPPE_ENCRYPTION_KEY for site {}", name);
                            config.encryption_key = env_key;
                            let config_json = serde_json::to_string_pretty(&config)
                                .map_err(|e| RuntimeError::Config(format!("failed to serialize site_config.json for {}: {}", name, e)))?;
                            tokio::fs::write(&config_path, config_json).await
                                .map_err(RuntimeError::Io)?;
                        }
                    } else {
                        warn!(
                            "FRAPPE_ENCRYPTION_KEY is set but is not a valid Fernet key; ignoring for site {}",
                            name
                        );
                    }
                }
            }

            // Older sites were created with a UUID-hex encryption_key that is not a
            // valid Fernet key. Regenerate it if invalid; there cannot be any
            // decryptable encrypted data tied to an invalid key.
            if !config.encryption_key.is_empty() && !is_valid_fernet_key(&config.encryption_key) {
                warn!(
                    "site {} has an invalid encryption_key (not Fernet format); regenerating",
                    name
                );
                config.encryption_key = generate_fernet_key();
                let config_json = serde_json::to_string_pretty(&config)
                    .map_err(|e| RuntimeError::Config(format!("failed to serialize site_config.json for {}: {}", name, e)))?;
                tokio::fs::write(&config_path, config_json).await
                    .map_err(RuntimeError::Io)?;
            }

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
            encryption_key: generate_fernet_key(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_fernet_key_is_valid() {
        let key = generate_fernet_key();
        assert!(is_valid_fernet_key(&key));
        assert_eq!(URL_SAFE.decode(&key).unwrap().len(), 32);
    }

    #[test]
    fn uuid_hex_key_is_invalid() {
        // Old sites used a 32-char hex UUID as the encryption key.
        let bad = "ce934cd03e9548828adee38f67d860da";
        assert!(!is_valid_fernet_key(bad));
    }

    #[test]
    fn real_fernet_key_is_valid() {
        // Example produced by Python's Fernet.generate_key().decode().
        let good = "Xf5A9gGQB4qPoaZP_9V4aD3rYrCVAOv1wBD5Dtjln-c=";
        assert!(is_valid_fernet_key(good));
    }
}
