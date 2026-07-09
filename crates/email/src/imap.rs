//! Generic IMAP fetch / test helpers.

use crate::config::EmailConfig;
use std::net::TcpStream;

/// Test IMAP connectivity for the supplied configuration.
///
/// Logs in and selects the inbox; does not fetch or alter messages.
pub fn test_imap(config: &EmailConfig) -> anyhow::Result<()> {
    let host = config.imap.host.clone();
    let port = config.imap_port();
    let user = config.imap.user.clone();
    let pass = config.imap.pass.clone();

    if host.is_empty() || user.is_empty() {
        return Err(anyhow::anyhow!("IMAP host or user is empty"));
    }

    if config.imap.tls {
        let tls = native_tls::TlsConnector::builder()
            .build()
            .map_err(|e| anyhow::anyhow!("TLS setup failed: {e}"))?;
        let mut session = imap::connect((host.as_str(), port), host.as_str(), &tls)
            .map_err(|e| anyhow::anyhow!("IMAP TLS connect failed: {e}"))?
            .login(&user, &pass)
            .map_err(|(e, _)| anyhow::anyhow!("IMAP login failed: {e}"))?;
        session
            .select("INBOX")
            .map_err(|e| anyhow::anyhow!("IMAP select INBOX failed: {e}"))?;
        session
            .logout()
            .map_err(|e| anyhow::anyhow!("IMAP logout failed: {e}"))?;
    } else {
        let stream = TcpStream::connect((host.as_str(), port))
            .map_err(|e| anyhow::anyhow!("IMAP plain connect failed: {e}"))?;
        let mut client = imap::Client::new(stream);
        client
            .read_greeting()
            .map_err(|e| anyhow::anyhow!("IMAP greeting failed: {e}"))?;
        let mut session = client
            .login(&user, &pass)
            .map_err(|(e, _)| anyhow::anyhow!("IMAP login failed: {e}"))?;
        session
            .select("INBOX")
            .map_err(|e| anyhow::anyhow!("IMAP select INBOX failed: {e}"))?;
        session
            .logout()
            .map_err(|e| anyhow::anyhow!("IMAP logout failed: {e}"))?;
    }

    Ok(())
}

/// Fetch messages from the IMAP inbox for the supplied configuration.
///
/// First tries to return only unseen messages; if that fails, it falls back to
/// the most recent 50 messages in the inbox.
pub fn fetch_inbox(config: &EmailConfig) -> anyhow::Result<Vec<Vec<u8>>> {
    let host = config.imap.host.clone();
    let port = config.imap_port();
    let user = config.imap.user.clone();
    let pass = config.imap.pass.clone();

    if host.is_empty() || user.is_empty() {
        return Err(anyhow::anyhow!("IMAP host or user is empty"));
    }

    if config.imap.tls {
        let tls = native_tls::TlsConnector::builder()
            .build()
            .map_err(|e| anyhow::anyhow!("TLS setup failed: {e}"))?;
        let session = imap::connect((host.as_str(), port), host.as_str(), &tls)
            .map_err(|e| anyhow::anyhow!("IMAP TLS connect failed: {e}"))?
            .login(&user, &pass)
            .map_err(|(e, _)| anyhow::anyhow!("IMAP login failed: {e}"))?;
        fetch_messages(session)
    } else {
        let stream = TcpStream::connect((host.as_str(), port))
            .map_err(|e| anyhow::anyhow!("IMAP plain connect failed: {e}"))?;
        let mut client = imap::Client::new(stream);
        client
            .read_greeting()
            .map_err(|e| anyhow::anyhow!("IMAP greeting failed: {e}"))?;
        let session = client
            .login(&user, &pass)
            .map_err(|(e, _)| anyhow::anyhow!("IMAP login failed: {e}"))?;
        fetch_messages(session)
    }
}

/// Fetch messages from an already-authenticated IMAP session.
///
/// The session stream may be a plain TCP stream or a TLS-wrapped stream.
fn fetch_messages<T>(mut session: imap::Session<T>) -> anyhow::Result<Vec<Vec<u8>>>
where
    T: std::io::Read + std::io::Write,
{
    session
        .select("INBOX")
        .map_err(|e| anyhow::anyhow!("IMAP select INBOX failed: {e}"))?;

    let mut results = Vec::new();

    match session.uid_search("UNSEEN") {
        Ok(uids) if !uids.is_empty() => {
            for uid in uids {
                match session.uid_fetch(uid.to_string(), "RFC822") {
                    Ok(messages) => {
                        for msg in messages.iter() {
                            if let Some(body) = msg.body() {
                                results.push(body.to_vec());
                            }
                        }
                    }
                    Err(e) => tracing::warn!(uid = %uid, error = %e, "failed to fetch unseen message"),
                }
            }
        }
        _ => {
            // Fall back to the most recent messages when UNSEEN is unsupported
            // or returns nothing.
            match session.fetch("1:50", "RFC822") {
                Ok(messages) => {
                    for msg in messages.iter() {
                        if let Some(body) = msg.body() {
                            results.push(body.to_vec());
                        }
                    }
                }
                Err(e) => {
                    session
                        .logout()
                        .map_err(|le| anyhow::anyhow!("IMAP logout failed: {le}"))?;
                    return Err(anyhow::anyhow!("IMAP fetch failed: {e}"));
                }
            }
        }
    }

    session
        .logout()
        .map_err(|e| anyhow::anyhow!("IMAP logout failed: {e}"))?;

    Ok(results)
}
