//! SMTP inbound server using mailin-embedded.

use crate::store::{MailStore, MailStoreError};
use mailin_embedded::{response, Handler, Response, Server, SslConfig};
use std::net::IpAddr;
use std::sync::Arc;
use tokio::runtime::Handle;

/// Configuration for the SMTP inbound listener.
#[derive(Clone, Debug)]
pub struct SmtpServerConfig {
    pub bind_host: String,
    pub bind_port: u16,
    pub server_name: String,
    pub max_message_size: usize,
}

impl Default for SmtpServerConfig {
    fn default() -> Self {
        Self {
            bind_host: "0.0.0.0".into(),
            bind_port: 25,
            server_name: "sebrus.local".into(),
            max_message_size: 25 * 1024 * 1024,
        }
    }
}

/// A running SMTP server handle.
pub struct SmtpServerHandle {
    thread: Option<std::thread::JoinHandle<()>>,
}

impl SmtpServerHandle {
    pub fn stop(mut self) {
        if let Some(t) = self.thread.take() {
            // mailin-embedded does not expose a clean shutdown; detach the thread.
            let _ = t.join();
        }
    }
}

/// Start the SMTP inbound server in a dedicated thread.
pub fn start_smtp_server(
    config: SmtpServerConfig,
    store: Arc<dyn MailStore>,
    handle: Handle,
) -> Result<SmtpServerHandle, MailStoreError> {
    let addr = format!("{}:{}", config.bind_host, config.bind_port);
    let thread = std::thread::Builder::new()
        .name("sebrus-smtp".into())
        .spawn(move || {
            let handler = SmtpHandler::new(store, handle, config.server_name.clone());
            let mut server = Server::new(handler);
            if let Err(e) = server
                .with_name(&config.server_name)
                .with_ssl(SslConfig::None)
                .and_then(|s| s.with_addr(&addr))
            {
                tracing::error!("failed to configure SMTP server: {}", e);
                return;
            }
            tracing::info!("SMTP server listening on {}", addr);
            if let Err(e) = server.serve() {
                tracing::error!("SMTP server exited: {}", e);
            }
        })
        .map_err(|e| MailStoreError::Other(format!("spawn SMTP thread: {e}")))?;
    Ok(SmtpServerHandle {
        thread: Some(thread),
    })
}

#[derive(Clone)]
struct SmtpHandler {
    store: Arc<dyn MailStore>,
    handle: Handle,
    server_name: String,
    recipients: Vec<String>,
    sender: String,
    buffer: Vec<u8>,
}

impl SmtpHandler {
    fn new(store: Arc<dyn MailStore>, handle: Handle, server_name: String) -> Self {
        Self {
            store,
            handle,
            server_name,
            recipients: Vec::new(),
            sender: String::new(),
            buffer: Vec::new(),
        }
    }

    fn block_on<F>(&self, fut: F) -> F::Output
    where
        F: std::future::Future,
    {
        self.handle.block_on(fut)
    }
}

impl Handler for SmtpHandler {
    fn helo(&mut self, _ip: IpAddr, _domain: &str) -> Response {
        Response::custom(250, self.server_name.clone())
    }

    fn mail(&mut self, _ip: IpAddr, _domain: &str, from: &str) -> Response {
        self.sender = from.to_lowercase();
        self.recipients.clear();
        self.buffer.clear();
        response::OK
    }

    fn rcpt(&mut self, to: &str) -> Response {
        let to = to.to_lowercase();
        match self.block_on(self.store.resolve_recipient(&to)) {
            Ok(targets) if !targets.is_empty() => {
                self.recipients.extend(targets);
                response::OK
            }
            Ok(_) => response::NO_MAILBOX,
            Err(e) => {
                tracing::warn!("recipient resolution failed: {}", e);
                response::INTERNAL_ERROR
            }
        }
    }

    fn data_start(
        &mut self,
        _domain: &str,
        _from: &str,
        _is8bit: bool,
        _to: &[String],
    ) -> Response {
        self.buffer.clear();
        response::START_DATA
    }

    fn data(&mut self, buf: &[u8]) -> Result<(), std::io::Error> {
        self.buffer.extend_from_slice(buf);
        Ok(())
    }

    fn data_end(&mut self) -> Response {
        // Deduplicate recipients; deliver to each target account.
        let mut seen = std::collections::HashSet::new();
        let targets: Vec<String> = self
            .recipients
            .drain(..)
            .filter(|r| seen.insert(r.clone()))
            .collect();

        for account in targets {
            if let Err(e) = self
                .block_on(self.store.append_message(&account, "INBOX", &self.buffer))
            {
                tracing::warn!(account = %account, error = %e, "failed to append message");
                return response::INTERNAL_ERROR;
            }
            tracing::info!(account = %account, "delivered message");
        }

        self.buffer.clear();
        self.sender.clear();
        response::OK
    }

    fn auth_plain(
        &mut self,
        _authorization_id: &str,
        authentication_id: &str,
        password: &str,
    ) -> Response {
        match self.block_on(self.store.authenticate(authentication_id, password)) {
            Ok(Some(_)) => response::AUTH_OK,
            Ok(None) => response::INVALID_CREDENTIALS,
            Err(e) => {
                tracing::warn!("SMTP auth error: {}", e);
                response::INTERNAL_ERROR
            }
        }
    }
}
