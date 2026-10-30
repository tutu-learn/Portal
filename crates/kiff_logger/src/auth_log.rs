//! Authentication event records persisted to the site database.
//!
//! Login success, failed login, and logout events are stored as
//! `Authentication Log` rows via the ORM. The Tantivy log engine only keeps
//! raw records for a few days; this table is the durable access evidence
//! used for ISO 27001 A.8.15/A.8.16 and access reviews.
//!
//! Rows are written exclusively by the auth handlers. No role holds
//! create/write/delete permission, so the trail cannot be forged or edited
//! through the REST API or Desk.

use orm::Document;

pub const AUTHENTICATION_LOG_DOCTYPE: &str = "Authentication Log";

#[derive(Debug, Clone, Copy)]
pub enum AuthenticationEvent {
    LoginSuccess,
    LoginFailed,
    Logout,
}

impl AuthenticationEvent {
    pub fn as_str(&self) -> &'static str {
        match self {
            AuthenticationEvent::LoginSuccess => "Login Success",
            AuthenticationEvent::LoginFailed => "Login Failed",
            AuthenticationEvent::Logout => "Logout",
        }
    }
}

/// Best-effort insert of an authentication event. Recording must never
/// break authentication itself, so failures are traced and swallowed.
pub async fn record_authentication_event(
    pool: &orm::DatabasePool,
    user: &str,
    event: AuthenticationEvent,
    ip: Option<&str>,
    user_agent: Option<&str>,
) {
    let now = chrono::Utc::now();
    let name = format!(
        "AUTH-{}-{}",
        now.format("%Y%m%d%H%M%S"),
        rand::random::<u32>()
    );

    let mut doc = Document::new(AUTHENTICATION_LOG_DOCTYPE, name);
    doc.set_field("user", user);
    doc.set_field("event", event.as_str());
    doc.set_field("timestamp", now.to_rfc3339());
    if let Some(ip) = ip {
        doc.set_field("ip_address", ip);
    }
    if let Some(ua) = user_agent {
        doc.set_field("user_agent", ua);
    }

    if let Err(e) = pool.insert_doc(&doc).await {
        tracing::warn!(
            user = %user,
            event = %event.as_str(),
            "failed to record authentication event: {}",
            e
        );
    }
}
