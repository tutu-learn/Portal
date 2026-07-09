//! Generic e-mail configuration.
//!
//! `EmailConfig` is the canonical shape used by frontend settings UIs and
//! e-mail operations. It is intentionally decoupled from any particular app
//! (such as Strongroom) or storage backend.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Outgoing SMTP configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SmtpConfig {
    pub host: String,
    pub port: String,
    pub user: String,
    pub pass: String,
    pub from_name: String,
    pub from_email: String,
    #[serde(default = "default_true")]
    pub tls: bool,
}

/// Incoming IMAP configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ImapConfig {
    pub host: String,
    pub port: String,
    pub user: String,
    pub pass: String,
    #[serde(default = "default_true")]
    pub auto_file: bool,
    #[serde(default = "default_true")]
    pub tls: bool,
}

/// Complete e-mail configuration used by e-mail operations.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EmailConfig {
    pub smtp: SmtpConfig,
    pub imap: ImapConfig,
}

fn default_true() -> bool {
    true
}

impl EmailConfig {
    /// Parse the configuration from a JSON payload.
    pub fn from_json(value: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(value)?)
    }

    /// Parse the configuration from a loose `serde_json::Value`.
    pub fn from_value(value: Value) -> anyhow::Result<Self> {
        Ok(serde_json::from_value(value)?)
    }

    /// Return the first configured e-mail address (from or user).
    pub fn email_id(&self) -> String {
        if !self.smtp.from_email.is_empty() {
            return self.smtp.from_email.clone();
        }
        if !self.smtp.user.is_empty() {
            return self.smtp.user.clone();
        }
        if !self.imap.user.is_empty() {
            return self.imap.user.clone();
        }
        String::new()
    }

    /// Return the SMTP port as a `u16`, falling back to 587.
    pub fn smtp_port(&self) -> u16 {
        self.smtp.port.parse().ok().unwrap_or(587)
    }

    /// Return the IMAP port as a `u16`, falling back to 993.
    pub fn imap_port(&self) -> u16 {
        self.imap.port.parse().ok().unwrap_or(993)
    }
}

/// Load configuration from a local settings value.
///
/// Expects a value of the form `{"email": {"smtp": {...}, "imap": {...}}}`
/// or just `{"smtp": {...}, "imap": {...}}`.
pub fn load_config_from_value(local_settings: Value) -> anyhow::Result<EmailConfig> {
    let email_value = local_settings
        .get("email")
        .cloned()
        .unwrap_or(local_settings);
    EmailConfig::from_value(email_value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_camel_case_config() {
        let json = r#"{
            "smtp": {
                "host": "smtp.example.com",
                "port": "587",
                "user": "user@example.com",
                "pass": "secret",
                "fromName": "Strongroom",
                "fromEmail": "noreply@example.com",
                "tls": true
            },
            "imap": {
                "host": "imap.example.com",
                "port": "993",
                "user": "user@example.com",
                "pass": "secret",
                "autoFile": false,
                "tls": true
            }
        }"#;

        let config = EmailConfig::from_json(json).unwrap();
        assert_eq!(config.smtp.host, "smtp.example.com");
        assert_eq!(config.smtp.from_name, "Strongroom");
        assert_eq!(config.smtp.from_email, "noreply@example.com");
        assert!(config.smtp.tls);
        assert!(!config.imap.auto_file);
        assert!(config.imap.tls);
        assert_eq!(config.smtp_port(), 587);
        assert_eq!(config.imap_port(), 993);
    }

    #[test]
    fn roundtrip_serialize_deserialize() {
        let config = EmailConfig {
            smtp: SmtpConfig {
                host: "smtp.example.com".into(),
                port: "587".into(),
                user: "user@example.com".into(),
                pass: "secret".into(),
                from_name: "Strongroom".into(),
                from_email: "noreply@example.com".into(),
                tls: true,
            },
            imap: ImapConfig {
                host: "imap.example.com".into(),
                port: "993".into(),
                user: "user@example.com".into(),
                pass: "secret".into(),
                auto_file: true,
                tls: true,
            },
        };

        let value = serde_json::to_value(&config).unwrap();
        let restored: EmailConfig = serde_json::from_value(value).unwrap();
        assert_eq!(restored.smtp.host, config.smtp.host);
        assert_eq!(restored.imap.auto_file, config.imap.auto_file);
    }
}
