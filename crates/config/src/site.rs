use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteConfig {
    #[serde(default = "default_db_driver")]
    pub db_driver: String,
    #[serde(default = "default_db_url")]
    pub db_url: String,
    #[serde(default)]
    pub encryption_key: String,
    #[serde(default)]
    pub secret_key: String,
    /// Optional host name mapping for multi-site routing. When set, requests
    /// whose `Host` header matches this value are routed to this site.
    #[serde(default)]
    pub host_name: Option<String>,
    #[serde(default)]
    pub mail_server: String,
    #[serde(default = "default_mail_port")]
    pub mail_port: u16,
    #[serde(default)]
    pub mail_login: String,
    #[serde(default = "default_file_size_limit")]
    pub file_size_limit: u64,
}

fn default_db_driver() -> String {
    "sqlite".into()
}
fn default_db_url() -> String {
    "./sites/{site}/site.db".into()
}
fn default_mail_port() -> u16 {
    587
}
fn default_file_size_limit() -> u64 {
    25
}

impl Default for SiteConfig {
    fn default() -> Self {
        Self {
            db_driver: default_db_driver(),
            db_url: default_db_url(),
            encryption_key: String::new(),
            secret_key: String::new(),
            host_name: None,
            mail_server: String::new(),
            mail_port: default_mail_port(),
            mail_login: String::new(),
            file_size_limit: default_file_size_limit(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Site {
    pub name: String,
    pub path: PathBuf,
    pub private: PathBuf,
    pub public: PathBuf,
    pub config: SiteConfig,
}

impl Site {
    pub fn new(name: String, path: PathBuf, config: SiteConfig) -> Self {
        let private = path.join("private");
        let public = path.join("public");
        Self {
            name,
            path,
            private,
            public,
            config,
        }
    }

    pub fn db_url(&self) -> String {
        self.config.db_url.replace("{site}", &self.name)
    }
}
