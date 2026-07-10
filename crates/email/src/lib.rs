//! Generic e-mail connectivity for Kiff.
//!
//! Provides SMTP/IMAP configuration, raw message parsing, connectivity tests,
//! and send/fetch operations. Any app-specific behaviour (filing to DocTypes,
/// follow-up reminders, queue method names) belongs in the consuming app.

pub mod config;
pub mod imap;
pub mod parse;
pub mod smtp;

pub use config::{EmailConfig, ImapConfig, SmtpConfig};
pub use parse::{parse_attachments, parse_raw_email, ParsedAttachment, ParsedEmail};

/// Result of testing SMTP and IMAP credentials.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailTestResult {
    pub ok: bool,
    pub smtp_ok: bool,
    pub imap_ok: bool,
    pub message: String,
}

/// Test SMTP and IMAP connectivity for the supplied configuration.
///
/// Both tests run concurrently with a 5-second timeout. No e-mail is sent.
pub async fn test_email_settings(config: &EmailConfig) -> EmailTestResult {
    let timeout = std::time::Duration::from_secs(5);
    let config_clone = config.clone();
    let smtp_handle = tokio::task::spawn_blocking(move || smtp::test_smtp(&config_clone));
    let config_clone = config.clone();
    let imap_handle = tokio::task::spawn_blocking(move || imap::test_imap(&config_clone));

    let smtp_result = tokio::time::timeout(timeout, smtp_handle)
        .await
        .unwrap_or_else(|_| Ok(Err(anyhow::anyhow!("SMTP connection timed out"))))
        .unwrap_or_else(|e| Err(anyhow::anyhow!("SMTP task failed: {e}")));
    let imap_result = tokio::time::timeout(timeout, imap_handle)
        .await
        .unwrap_or_else(|_| Ok(Err(anyhow::anyhow!("IMAP connection timed out"))))
        .unwrap_or_else(|e| Err(anyhow::anyhow!("IMAP task failed: {e}")));

    let smtp_ok = smtp_result.is_ok();
    let imap_ok = imap_result.is_ok();
    let ok = smtp_ok && imap_ok;

    let message = match (smtp_result, imap_result) {
        (Ok(_), Ok(_)) => "SMTP and IMAP connections succeeded.".to_string(),
        (Err(e), Ok(_)) => format!("SMTP failed: {e}"),
        (Ok(_), Err(e)) => format!("IMAP failed: {e}"),
        (Err(se), Err(ie)) => format!("SMTP failed: {se}; IMAP failed: {ie}"),
    };

    EmailTestResult {
        ok,
        smtp_ok,
        imap_ok,
        message,
    }
}
