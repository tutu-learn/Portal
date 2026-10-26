//! End-to-end test for `email::smtp::send_email` against a local SMTP server.

use email::{EmailConfig, SmtpConfig};
use mailin_embedded::{response, Handler, Response, Server, SslConfig};
use std::net::IpAddr;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct RecordingHandler {
    messages: Arc<Mutex<Vec<Vec<u8>>>>,
    sender: String,
    recipients: Vec<String>,
    buffer: Vec<u8>,
}

impl RecordingHandler {
    fn new(messages: Arc<Mutex<Vec<Vec<u8>>>>) -> Self {
        Self {
            messages,
            sender: String::new(),
            recipients: Vec::new(),
            buffer: Vec::new(),
        }
    }
}

impl Handler for RecordingHandler {
    fn helo(&mut self, _ip: IpAddr, _domain: &str) -> Response {
        response::OK
    }

    fn mail(&mut self, _ip: IpAddr, _domain: &str, from: &str) -> Response {
        self.sender = from.to_lowercase();
        self.recipients.clear();
        self.buffer.clear();
        response::OK
    }

    fn rcpt(&mut self, to: &str) -> Response {
        self.recipients.push(to.to_lowercase());
        response::OK
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
        if let Ok(mut msgs) = self.messages.lock() {
            msgs.push(self.buffer.clone());
        }
        self.buffer.clear();
        response::OK
    }

    fn auth_plain(
        &mut self,
        _authorization_id: &str,
        _authentication_id: &str,
        _password: &str,
    ) -> Response {
        response::AUTH_OK
    }
}

fn start_test_smtp_server(port: u16) -> Arc<Mutex<Vec<Vec<u8>>>> {
    let messages = Arc::new(Mutex::new(Vec::new()));
    let handler = RecordingHandler::new(Arc::clone(&messages));

    std::thread::spawn(move || {
        let mut server = Server::new(handler);
        server.with_name("test.relay");
        if server.with_ssl(SslConfig::None).is_err() {
            return;
        }
        if server.with_addr(("127.0.0.1", port)).is_err() {
            return;
        }
        server.serve().ok();
    });

    // Wait for the listener to come up.
    std::thread::sleep(std::time::Duration::from_millis(100));
    messages
}

#[test]
fn send_email_via_smtp_relay() {
    let port = 25253;
    let messages = start_test_smtp_server(port);

    let config = EmailConfig {
        smtp: SmtpConfig {
            host: "127.0.0.1".into(),
            port: port.to_string(),
            user: "".into(),
            pass: "".into(),
            from_email: "sender@example.com".into(),
            from_name: "Test Sender".into(),
            tls: false,
        },
        imap: email::ImapConfig::default(),
    };

    let message_id = email::smtp::send_email(&config, "recipient@example.com", "Hello", "This is the body.")
        .expect("send_email should succeed");

    assert!(!message_id.is_empty());

    // Give the server a moment to process.
    std::thread::sleep(std::time::Duration::from_millis(100));

    let msgs = messages.lock().unwrap();
    assert_eq!(msgs.len(), 1, "expected one message to be relayed");
    let raw = String::from_utf8_lossy(&msgs[0]);
    assert!(raw.contains("From: \"Test Sender\" <sender@example.com>"), "{raw}");
    assert!(raw.contains("To: recipient@example.com"), "{raw}");
    assert!(raw.contains("Subject: Hello"), "{raw}");
    assert!(raw.contains("This is the body."), "{raw}");
}
