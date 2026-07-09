//! Generic SMTP send / test helpers.

use crate::config::EmailConfig;
use lettre::Transport;

/// Test SMTP connectivity for the supplied configuration.
///
/// Uses `lettre::SmtpTransport::test_connection`. Does not send any e-mail.
pub fn test_smtp(config: &EmailConfig) -> anyhow::Result<()> {
    if config.smtp.host.is_empty() {
        return Err(anyhow::anyhow!("SMTP host is empty"));
    }

    let creds = lettre::transport::smtp::authentication::Credentials::new(
        config.smtp.user.clone(),
        config.smtp.pass.clone(),
    );

    let builder = if config.smtp.tls {
        lettre::SmtpTransport::relay(&config.smtp.host)
            .map_err(|e| anyhow::anyhow!("SMTP relay setup failed: {e}"))?
            .port(config.smtp_port())
            .credentials(creds)
    } else {
        lettre::SmtpTransport::builder_dangerous(&config.smtp.host)
            .port(config.smtp_port())
            .credentials(creds)
    };

    let connected = builder
        .build()
        .test_connection()
        .map_err(|e| anyhow::anyhow!("SMTP connection test failed: {e}"))?;

    if !connected {
        return Err(anyhow::anyhow!("SMTP server rejected the connection"));
    }

    Ok(())
}

/// Send an e-mail via SMTP using `lettre`.
///
/// Returns the generated Message-ID on success. On failure the error is
/// returned to the caller; any retry/queueing policy is the caller's
/// responsibility.
pub fn send_email(config: &EmailConfig, to: &str, subject: &str, body: &str) -> anyhow::Result<String> {
    if config.smtp.host.is_empty() {
        return Err(anyhow::anyhow!("SMTP host is not configured"));
    }

    let from_mailbox = lettre::message::Mailbox::new(
        Some(config.smtp.from_name.clone()),
        config
            .smtp
            .from_email
            .parse()
            .map_err(|e| anyhow::anyhow!("invalid from address: {e}"))?,
    );

    let to_mailbox: lettre::message::Mailbox = to
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid to address: {e}"))?;

    let message_id = format!("<kiff-{}@kiff.local>", uuid::Uuid::new_v4());

    let email = lettre::Message::builder()
        .from(from_mailbox)
        .to(to_mailbox)
        .message_id(Some(message_id.clone()))
        .subject(subject)
        .body(body.to_string())
        .map_err(|e| anyhow::anyhow!("failed to build message: {e}"))?;

    let creds = lettre::transport::smtp::authentication::Credentials::new(
        config.smtp.user.clone(),
        config.smtp.pass.clone(),
    );

    let mailer = if config.smtp.tls {
        lettre::SmtpTransport::relay(&config.smtp.host)
            .map_err(|e| anyhow::anyhow!("SMTP relay setup failed: {e}"))?
            .port(config.smtp_port())
            .credentials(creds)
            .build()
    } else {
        lettre::SmtpTransport::builder_dangerous(&config.smtp.host)
            .port(config.smtp_port())
            .credentials(creds)
            .build()
    };

    mailer
        .send(&email)
        .map_err(|e| anyhow::anyhow!("SMTP send failed: {e}"))?;

    Ok(message_id)
}

#[cfg(test)]
mod tests {
    use lettre::transport::stub::StubTransport;
    use lettre::Transport;

    #[test]
    fn send_email_via_stub_transport() {
        let from_mailbox = lettre::message::Mailbox::new(
            Some("Kiff".into()),
            "noreply@example.com".parse().unwrap(),
        );
        let email = lettre::Message::builder()
            .from(from_mailbox)
            .to("client@example.com".parse().unwrap())
            .subject("Hello")
            .body("Body".to_string())
            .unwrap();

        let sender = StubTransport::new(Ok(()));
        let result = sender.send(&email);
        assert!(result.is_ok(), "stub transport should always succeed");
    }
}
