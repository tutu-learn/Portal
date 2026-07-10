//! Raw RFC-822 e-mail parsing.

use chrono::Utc;
use mail_parser::MimeHeaders;

/// A lightweight representation of a parsed e-mail.
#[derive(Debug, Clone)]
pub struct ParsedEmail {
    pub message_id: String,
    pub subject: String,
    pub sender: String,
    pub recipients: Vec<String>,
    pub body_text: String,
    pub received_at: String,
    pub raw_eml: Vec<u8>,
}

/// A single attachment extracted from a parsed e-mail.
#[derive(Debug, Clone)]
pub struct ParsedAttachment {
    pub filename: String,
    pub content_type: String,
    pub body: Vec<u8>,
}

fn address_to_string(addr: &mail_parser::Address) -> String {
    match addr {
        mail_parser::Address::List(addrs) => addrs
            .iter()
            .filter_map(|a| a.address.as_ref().map(|s| s.to_string()))
            .collect::<Vec<_>>()
            .join(", "),
        mail_parser::Address::Group(groups) => groups
            .iter()
            .flat_map(|g| &g.addresses)
            .filter_map(|a| a.address.as_ref().map(|s| s.to_string()))
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// Parse a raw RFC-822 message into a `ParsedEmail`.
pub fn parse_raw_email(raw: &[u8]) -> Option<ParsedEmail> {
    let msg = mail_parser::MessageParser::default().parse(raw)?;

    let message_id = msg
        .message_id()
        .map(|id| id.to_string())
        .unwrap_or_else(|| format!("kiff-{}@generated", uuid::Uuid::new_v4()));

    let subject = msg.subject().unwrap_or("").to_string();
    let sender = msg.from().map(address_to_string).unwrap_or_default();
    let recipients = msg.to().map(address_to_string).unwrap_or_default();

    let body_text = msg.body_text(0).map(|p| p.to_string()).unwrap_or_default();

    let received_at = msg
        .date()
        .map(|d| d.to_rfc3339())
        .unwrap_or_else(|| Utc::now().to_rfc3339());

    Some(ParsedEmail {
        message_id,
        subject,
        sender,
        recipients: recipients
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        body_text,
        received_at,
        raw_eml: raw.to_vec(),
    })
}

/// Extract attachments from a raw RFC-822 message.
///
/// Attachments are identified by `mail-parser`'s attachment list (MIME parts
/// with `Content-Disposition: attachment` or otherwise marked as attachments).
/// Filenames are sanitised so the returned names are safe to use as local file
/// names.
pub fn parse_attachments(raw: &[u8]) -> Vec<ParsedAttachment> {
    let Some(msg) = mail_parser::MessageParser::default().parse(raw) else {
        return Vec::new();
    };

    msg.attachments()
        .filter_map(|part| {
            let filename = part
                .attachment_name()
                .map(sanitise_filename)
                .unwrap_or_else(|| format!("attachment-{}.bin", uuid::Uuid::new_v4()));
            let content_type = part
                .content_type()
                .map(|ct| {
                    let subtype = ct.subtype().unwrap_or("octet-stream");
                    format!("{}/{}", ct.ctype(), subtype)
                })
                .unwrap_or_else(|| "application/octet-stream".to_string());
            Some(ParsedAttachment {
                filename,
                content_type,
                body: part.contents().to_vec(),
            })
        })
        .collect()
}

fn sanitise_filename(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars().take(128) {
        match ch {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '.' | '-' | '_' | ' ' => out.push(ch),
            _ => out.push('_'),
        }
    }
    if out.is_empty() {
        out = format!("attachment-{}.bin", uuid::Uuid::new_v4());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_EML: &str = r#"From: sender@example.com
To: receiver@example.com
Subject: Test message
Message-ID: <abc123@example.com>
Date: Mon, 01 Jan 2024 12:00:00 +0000
Content-Type: text/plain

This is the body.
"#;

    #[test]
    fn parse_sample_eml() {
        let parsed = parse_raw_email(SAMPLE_EML.as_bytes()).expect("parse succeeded");
        assert_eq!(parsed.message_id, "abc123@example.com");
        assert_eq!(parsed.subject, "Test message");
        assert_eq!(parsed.sender, "sender@example.com");
        assert!(parsed.recipients.contains(&"receiver@example.com".to_string()));
        assert_eq!(parsed.body_text.trim(), "This is the body.");
    }
}
