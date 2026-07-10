//! Generic IMAP fetch / test helpers.

use crate::config::EmailConfig;
use native_tls::TlsStream;
use std::collections::HashSet;
use std::net::TcpStream;

/// Unified IMAP session over TLS or a plain TCP stream.
///
/// The `imap` crate exposes `Session<T>` as a generic type, so the TLS and
/// plain variants are incompatible.  This enum lets the callers work with
/// either transport without duplicating the fetch/test logic.
enum Session {
    Tls(imap::Session<TlsStream<TcpStream>>),
    Plain(imap::Session<TcpStream>),
}

impl Session {
    fn select<S: AsRef<str>>(
        &mut self,
        mailbox_name: S,
    ) -> imap::error::Result<imap::types::Mailbox> {
        match self {
            Session::Tls(s) => s.select(mailbox_name),
            Session::Plain(s) => s.select(mailbox_name),
        }
    }

    fn uid_search<S: AsRef<str>>(
        &mut self,
        query: S,
    ) -> imap::error::Result<HashSet<imap::types::Uid>> {
        match self {
            Session::Tls(s) => s.uid_search(query),
            Session::Plain(s) => s.uid_search(query),
        }
    }

    fn uid_fetch<S1: AsRef<str>, S2: AsRef<str>>(
        &mut self,
        uid_set: S1,
        query: S2,
    ) -> imap::error::Result<imap::types::ZeroCopy<Vec<imap::types::Fetch>>> {
        match self {
            Session::Tls(s) => s.uid_fetch(uid_set, query),
            Session::Plain(s) => s.uid_fetch(uid_set, query),
        }
    }

    fn fetch<S1: AsRef<str>, S2: AsRef<str>>(
        &mut self,
        sequence_set: S1,
        query: S2,
    ) -> imap::error::Result<imap::types::ZeroCopy<Vec<imap::types::Fetch>>> {
        match self {
            Session::Tls(s) => s.fetch(sequence_set, query),
            Session::Plain(s) => s.fetch(sequence_set, query),
        }
    }

    fn uid_store<S1: AsRef<str>, S2: AsRef<str>>(
        &mut self,
        uid_set: S1,
        query: S2,
    ) -> imap::error::Result<imap::types::ZeroCopy<Vec<imap::types::Fetch>>> {
        match self {
            Session::Tls(s) => s.uid_store(uid_set, query),
            Session::Plain(s) => s.uid_store(uid_set, query),
        }
    }

    fn logout(&mut self) -> imap::error::Result<()> {
        match self {
            Session::Tls(s) => s.logout(),
            Session::Plain(s) => s.logout(),
        }
    }
}

/// Open a session using the configured transport.
fn connect(config: &EmailConfig) -> anyhow::Result<Session> {
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
        Ok(Session::Tls(session))
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
        Ok(Session::Plain(session))
    }
}

/// Test IMAP connectivity for the supplied configuration.
///
/// Logs in and selects the inbox; does not fetch or alter messages.
pub fn test_imap(config: &EmailConfig) -> anyhow::Result<()> {
    let mut session = connect(config)?;
    session
        .select("INBOX")
        .map_err(|e| anyhow::anyhow!("IMAP select INBOX failed: {e}"))?;
    session
        .logout()
        .map_err(|e| anyhow::anyhow!("IMAP logout failed: {e}"))?;
    Ok(())
}

/// Fetch messages from the IMAP inbox for the supplied configuration.
///
/// First tries to return only unseen messages; if that fails, it falls back to
/// the most recent 50 messages in the inbox.
pub fn fetch_inbox(config: &EmailConfig) -> anyhow::Result<Vec<Vec<u8>>> {
    let session = connect(config)?;
    fetch_messages(session)
}

/// Fetch messages from an already-authenticated IMAP session.
///
/// The session stream may be a plain TCP stream or a TLS-wrapped stream.
fn fetch_messages(mut session: Session) -> anyhow::Result<Vec<Vec<u8>>> {
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

/// Fetch unseen messages from the IMAP inbox together with their UIDs.
///
/// Returns a vector of `(uid, raw_rfc822)` pairs. The UID is required so the
/// caller can mark the message as `\Seen` only after it has been successfully
/// filed.
pub fn fetch_unseen_messages(config: &EmailConfig) -> anyhow::Result<Vec<(u32, Vec<u8>)>> {
    let session = connect(config)?;
    fetch_unseen(session)
}

fn fetch_unseen(mut session: Session) -> anyhow::Result<Vec<(u32, Vec<u8>)>> {
    session
        .select("INBOX")
        .map_err(|e| anyhow::anyhow!("IMAP select INBOX failed: {e}"))?;

    let uids = session
        .uid_search("UNSEEN")
        .map_err(|e| anyhow::anyhow!("IMAP UNSEEN search failed: {e}"))?;

    let mut results = Vec::new();
    for uid in uids {
        match session.uid_fetch(uid.to_string(), "RFC822") {
            Ok(messages) => {
                for msg in messages.iter() {
                    if let Some(body) = msg.body() {
                        results.push((uid, body.to_vec()));
                    }
                }
            }
            Err(e) => tracing::warn!(uid = %uid, error = %e, "failed to fetch unseen message"),
        }
    }

    session
        .logout()
        .map_err(|e| anyhow::anyhow!("IMAP logout failed: {e}"))?;

    Ok(results)
}

/// Mark a single message as `\Seen` using its UID.
pub fn mark_seen(config: &EmailConfig, uid: u32) -> anyhow::Result<()> {
    let mut session = connect(config)?;
    session
        .select("INBOX")
        .map_err(|e| anyhow::anyhow!("IMAP select INBOX failed: {e}"))?;
    session
        .uid_store(uid.to_string(), "+FLAGS (\\Seen)")
        .map_err(|e| anyhow::anyhow!("IMAP mark seen failed: {e}"))?;
    session
        .logout()
        .map_err(|e| anyhow::anyhow!("IMAP logout failed: {e}"))?;
    Ok(())
}
