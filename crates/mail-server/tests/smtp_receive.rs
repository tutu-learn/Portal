//! End-to-end test: send an e-mail to the SebrusMail SMTP inbound server and
//! verify it is stored in the ORM-backed mail store.

use mail_server::store::{MailStore, OrmMailStore, SearchCriteria};
use mail_server::smtp::{start_smtp_server, SmtpServerConfig};
use orm::DatabasePool;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

async fn setup_db() -> (DatabasePool, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let pool = DatabasePool::connect_sqlite(path.to_str().unwrap())
        .await
        .unwrap();

    pool.execute_sql(
        r#"CREATE TABLE "sebrusmail_domain" (
            name TEXT PRIMARY KEY,
            enabled INTEGER,
            dkim_selector TEXT,
            dkim_private_key TEXT,
            catch_all_account TEXT,
            is_default INTEGER
        )"#,
        vec![],
    )
    .await
    .unwrap();

    pool.execute_sql(
        r#"CREATE TABLE "sebrusmail_account" (
            name TEXT PRIMARY KEY,
            email_address TEXT UNIQUE,
            username TEXT,
            domain TEXT,
            password_hash TEXT,
            quota_bytes INTEGER,
            enabled INTEGER,
            is_domain_admin INTEGER
        )"#,
        vec![],
    )
    .await
    .unwrap();

    pool.execute_sql(
        r#"CREATE TABLE "sebrusmail_alias" (
            name TEXT PRIMARY KEY,
            address TEXT,
            domain TEXT,
            destination_accounts TEXT,
            enabled INTEGER
        )"#,
        vec![],
    )
    .await
    .unwrap();

    pool.execute_sql(
        r#"CREATE TABLE "sebrusmail_folder" (
            name TEXT,
            account TEXT,
            uidvalidity INTEGER,
            uidnext INTEGER,
            PRIMARY KEY (name, account)
        )"#,
        vec![],
    )
    .await
    .unwrap();

    pool.execute_sql(
        r#"CREATE TABLE "sebrusmail_message" (
            name TEXT PRIMARY KEY,
            creation TEXT,
            modified TEXT,
            modified_by TEXT,
            owner TEXT,
            docstatus INTEGER DEFAULT 0,
            account TEXT,
            folder TEXT,
            uid INTEGER,
            message_id TEXT,
            subject TEXT,
            "from" TEXT,
            "to" TEXT,
            cc TEXT,
            date TEXT,
            size_bytes INTEGER,
            flags TEXT,
            raw_eml TEXT,
            body_text TEXT,
            is_read INTEGER
        )"#,
        vec![],
    )
    .await
    .unwrap();

    (pool, dir)
}

fn read_response(reader: &mut BufReader<&TcpStream>) -> String {
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    line
}

fn expect(reader: &mut BufReader<&TcpStream>, code: &str) -> String {
    let line = read_response(reader);
    assert!(
        line.starts_with(code),
        "expected {code} response, got: {line}"
    );
    line
}

fn write_crlf(writer: &mut &TcpStream, text: &str) {
    writer.write_all(text.as_bytes()).unwrap();
    writer.write_all(b"\r\n").unwrap();
    writer.flush().unwrap();
}

#[tokio::test]
async fn smtp_receives_email_into_inbox() {
    let (pool, _dir) = setup_db().await;
    let store: Arc<dyn MailStore> = Arc::new(OrmMailStore::new(pool.clone()));

    // Seed a domain and a mailbox.
    pool.execute_sql(
        r#"INSERT INTO "sebrusmail_domain" (name, enabled, is_default)
           VALUES ('example.com', 1, 1)"#,
        vec![],
    )
    .await
    .unwrap();
    pool.execute_sql(
        r#"INSERT INTO "sebrusmail_account" (
            name, email_address, domain, password_hash, quota_bytes, enabled, is_domain_admin
         ) VALUES ('alice@example.com', 'alice@example.com', 'example.com', '', 0, 1, 0)"#,
        vec![],
    )
    .await
    .unwrap();

    // Start the SMTP server on a fixed high port.
    let port = 25252;
    let config = SmtpServerConfig {
        bind_host: "127.0.0.1".into(),
        bind_port: port,
        server_name: "test.local".into(),
        max_message_size: 25 * 1024 * 1024,
    };
    let _handle = start_smtp_server(config, Arc::clone(&store), tokio::runtime::Handle::current())
        .expect("SMTP server should start on fixed port");

    // Wait briefly for the listener thread to come up.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Send an e-mail using raw SMTP.
    let stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to SMTP server");
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut reader = BufReader::new(&stream);
    let mut writer = &stream;

    expect(&mut reader, "220");

    write_crlf(&mut writer, "EHLO test.local");
    loop {
        let line = read_response(&mut reader);
        if line.starts_with("250 ") {
            break;
        }
        assert!(line.starts_with("250-"), "unexpected EHLO response: {line}");
    }

    write_crlf(&mut writer, "MAIL FROM:<sender@example.com>");
    expect(&mut reader, "250");

    write_crlf(&mut writer, "RCPT TO:<alice@example.com>");
    expect(&mut reader, "250");

    write_crlf(&mut writer, "DATA");
    expect(&mut reader, "354");

    let message = b"From: sender@example.com\r\n\
                     To: alice@example.com\r\n\
                     Subject: Hello from SMTP test\r\n\
                     Message-ID: <smtp-test-123@example.com>\r\n\
                     \r\n\
                     This is the message body.\r\n";
    writer.write_all(message).unwrap();
    write_crlf(&mut writer, ".");
    let result = read_response(&mut reader);
    assert!(
        result.starts_with("250"),
        "expected 250 after DATA, got: {result}"
    );

    write_crlf(&mut writer, "QUIT");
    expect(&mut reader, "221");

    // Verify the message landed in the store.
    let uids = store
        .search_messages("alice@example.com", "INBOX", &SearchCriteria::All)
        .await
        .expect("search should succeed");
    assert_eq!(
        uids.len(),
        1,
        "expected one message in INBOX, found {}",
        uids.len()
    );

    let q = mail_server::store::FetchQuery {
        rfc822: true,
        ..Default::default()
    };
    let msgs = store
        .fetch_messages("alice@example.com", "INBOX", &uids, &q)
        .await
        .expect("fetch should succeed");
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].subject, "Hello from SMTP test");
    assert!(String::from_utf8_lossy(&msgs[0].raw_eml).contains("This is the message body."));
}
