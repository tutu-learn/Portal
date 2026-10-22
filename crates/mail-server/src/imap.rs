//! Minimal IMAP4rev1 server.

use crate::store::{Account, FetchQuery, FlagMode, MailStore, MailStoreError, SearchCriteria};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Configuration for the IMAP listener.
#[derive(Clone, Debug)]
pub struct ImapServerConfig {
    pub bind_host: String,
    pub bind_port: u16,
    pub server_name: String,
}

impl Default for ImapServerConfig {
    fn default() -> Self {
        Self {
            bind_host: "0.0.0.0".into(),
            bind_port: 143,
            server_name: "sebrus.local".into(),
        }
    }
}

/// Handle to the running IMAP server task.
pub struct ImapServerHandle {
    abort: tokio::task::AbortHandle,
}

impl ImapServerHandle {
    pub fn stop(&self) {
        self.abort.abort();
    }
}

/// Start the IMAP server as an async task.
pub async fn start_imap_server(
    config: ImapServerConfig,
    store: Arc<dyn MailStore>,
) -> Result<ImapServerHandle, MailStoreError> {
    let addr = format!("{}:{}", config.bind_host, config.bind_port);
    let listener = TcpListener::bind(&addr)
        .await
        .map_err(|e| MailStoreError::Other(format!("bind IMAP {addr}: {e}")))?;
    tracing::info!("IMAP server listening on {}", addr);

    let task = tokio::spawn(async move {
        loop {
            let (stream, peer) = match listener.accept().await {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!("IMAP accept error: {}", e);
                    continue;
                }
            };
            let store = Arc::clone(&store);
            tokio::spawn(async move {
                if let Err(e) = handle_connection(stream, peer, store).await {
                    tracing::debug!(peer = %peer, error = %e, "IMAP connection closed");
                }
            });
        }
    });

    Ok(ImapServerHandle {
        abort: task.abort_handle(),
    })
}

struct Session {
    account: Option<Account>,
    selected_folder: Option<String>,
    selected_readonly: bool,
}

async fn handle_connection(
    mut stream: TcpStream,
    _peer: std::net::SocketAddr,
    store: Arc<dyn MailStore>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut session = Session {
        account: None,
        selected_folder: None,
        selected_readonly: false,
    };

    send_line(
        &mut stream,
        &format!("* OK {} IMAP4rev1 SebrusMail ready", "sebrus.local"),
    )
    .await?;

    let mut buf = vec![0u8; 8192];
    let mut line_buf = Vec::new();

    loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        line_buf.extend_from_slice(&buf[..n]);

        // Process complete lines.
        loop {
            let Some(pos) = find_crlf(&line_buf) else {
                break;
            };
            let line = line_buf.drain(..pos + 2).collect::<Vec<_>>();
            let line = String::from_utf8_lossy(&line);
            let should_close = process_line(&mut stream, &line, &mut session, &store).await?;
            if should_close {
                return Ok(());
            }
        }
    }

    Ok(())
}

fn find_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\r\n")
}

async fn send_line(stream: &mut TcpStream, line: &str) -> tokio::io::Result<()> {
    stream.write_all(line.as_bytes()).await?;
    stream.write_all(b"\r\n").await?;
    stream.flush().await
}

/// Returns true if the connection should be closed.
async fn process_line(
    stream: &mut TcpStream,
    line: &str,
    session: &mut Session,
    store: &Arc<dyn MailStore>,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let trimmed = line.trim_end();
    let mut parts = trimmed.split_whitespace();
    let tag = match parts.next() {
        Some(t) => t,
        None => return Ok(false),
    };

    let cmd = parts.next().unwrap_or("").to_ascii_uppercase();
    let args: Vec<&str> = parts.collect();

    match cmd.as_str() {
        "CAPABILITY" => {
            send_line(stream, &format!("* CAPABILITY IMAP4rev1 LOGIN AUTH=PLAIN")).await?;
            ok(stream, tag, "CAPABILITY completed").await?;
        }
        "NOOP" => {
            ok(stream, tag, "NOOP completed").await?;
        }
        "LOGOUT" => {
            send_line(stream, "* BYE SebrusMail logging out").await?;
            ok(stream, tag, "LOGOUT completed").await?;
            return Ok(true);
        }
        "LOGIN" => {
            if args.len() < 2 {
                bad(stream, tag, "LOGIN requires username and password").await?;
            } else {
                let username = unquote(args[0]);
                let password = unquote(args[1]);
                match store.authenticate(&username, &password).await {
                    Ok(Some(account)) => {
                        session.account = Some(account);
                        ok(stream, tag, "LOGIN completed").await?;
                    }
                    _ => no(stream, tag, "Authentication failed").await?,
                }
            }
        }
        "AUTHENTICATE" => {
            if args.first() == Some(&"PLAIN") {
                // Prompt for credentials.
                send_line(stream, "+ ").await?;
                // Read base64 response line.
                // Simple synchronous read within this async function.
                let mut line = String::new();
                // This is a simplified implementation; production should use a proper line reader.
                // We will read the next line from the stream directly.
                let mut byte_buf = [0u8; 1];
                loop {
                    stream.read_exact(&mut byte_buf).await?;
                    if byte_buf[0] == b'\n' {
                        break;
                    }
                    if byte_buf[0] != b'\r' {
                        line.push(byte_buf[0] as char);
                    }
                }
                let decoded = base64_decode(&line).unwrap_or_default();
                let mut creds = decoded.splitn(3, |&b| b == 0);
                let _authz = creds.next();
                let authn = creds
                    .next()
                    .map(|b| String::from_utf8_lossy(b).to_string())
                    .unwrap_or_default();
                let password = creds
                    .next()
                    .map(|b| String::from_utf8_lossy(b).to_string())
                    .unwrap_or_default();
                match store.authenticate(&authn, &password).await {
                    Ok(Some(account)) => {
                        session.account = Some(account);
                        ok(stream, tag, "AUTHENTICATE completed").await?;
                    }
                    _ => no(stream, tag, "Authentication failed").await?,
                }
            } else {
                no(stream, tag, "Unsupported authentication mechanism").await?;
            }
        }
        "SELECT" | "EXAMINE" => {
            if args.is_empty() {
                bad(stream, tag, "SELECT requires a mailbox name").await?;
            } else {
                let folder = unquote(args[0]);
                let readonly = cmd == "EXAMINE";
                match select_folder(stream, tag, session, store, &folder, readonly).await {
                    Ok(_) => {}
                    Err(e) => {
                        no(stream, tag, &format!("SELECT failed: {e}")).await?;
                    }
                }
            }
        }
        "LIST" | "LSUB" => {
            if args.len() < 2 {
                bad(stream, tag, "LIST requires reference and mailbox").await?;
            } else {
                list_folders(stream, tag, session, store, args[1]).await?;
            }
        }
        "STATUS" => {
            if args.len() < 2 {
                bad(stream, tag, "STATUS requires mailbox and items").await?;
            } else {
                status_folder(stream, tag, session, store, unquote(args[0])).await?;
            }
        }
        "FETCH" => {
            if args.len() < 2 {
                bad(stream, tag, "FETCH requires sequence set and items").await?;
            } else {
                let seq_set = args[0];
                let items = &args[1..];
                fetch_cmd(stream, tag, session, store, seq_set, items, false).await?;
            }
        }
        "UID" => {
            if args.is_empty() {
                bad(stream, tag, "UID requires subcommand").await?;
            } else {
                let sub = args[0].to_ascii_uppercase();
                match sub.as_str() {
                    "FETCH" if args.len() >= 3 => {
                        fetch_cmd(stream, tag, session, store, args[1], &args[2..], true).await?;
                    }
                    "STORE" if args.len() >= 3 => {
                        store_flags(stream, tag, session, store, args[1], &args[2..], true).await?;
                    }
                    "SEARCH" => {
                        search_cmd(stream, tag, session, store, &args[1..], true).await?;
                    }
                    _ => bad(stream, tag, "Unknown UID subcommand").await?,
                }
            }
        }
        "STORE" => {
            if args.len() < 3 {
                bad(stream, tag, "STORE requires sequence set, mode, and flags").await?;
            } else {
                store_flags(stream, tag, session, store, args[0], &args[1..], false).await?;
            }
        }
        "SEARCH" => {
            search_cmd(stream, tag, session, store, &args, false).await?;
        }
        "CLOSE" => {
            session.selected_folder = None;
            session.selected_readonly = false;
            ok(stream, tag, "CLOSE completed").await?;
        }
        _ => {
            bad(stream, tag, &format!("Command not recognized: {cmd}")).await?;
        }
    }

    Ok(false)
}

async fn ok(stream: &mut TcpStream, tag: &str, msg: &str) -> tokio::io::Result<()> {
    send_line(stream, &format!("{tag} OK {msg}")).await
}

async fn no(stream: &mut TcpStream, tag: &str, msg: &str) -> tokio::io::Result<()> {
    send_line(stream, &format!("{tag} NO {msg}")).await
}

async fn bad(stream: &mut TcpStream, tag: &str, msg: &str) -> tokio::io::Result<()> {
    send_line(stream, &format!("{tag} BAD {msg}")).await
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        s[1..s.len() - 1].replace("\\\"", "\"").replace("\\\\", "\\")
    } else {
        s.to_string()
    }
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(s.trim()).ok()
}

async fn select_folder(
    stream: &mut TcpStream,
    tag: &str,
    session: &mut Session,
    store: &Arc<dyn MailStore>,
    folder: &str,
    readonly: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let account = session
        .account
        .as_ref()
        .ok_or_else(|| MailStoreError::AuthFailed)?;
    let folder_state = store.select_folder(&account.email_address, folder).await?;

    send_line(
        stream,
        &format!("* {} EXISTS", folder_state.exists),
    )
    .await?;
    send_line(stream, &format!("* {} RECENT", folder_state.recent)).await?;
    send_line(
        stream,
        &format!(
            "* FLAGS (\\Answered \\Flagged \\Deleted \\Seen \\Draft)"
        ),
    )
    .await?;
    send_line(
        stream,
        &format!(
            "* OK [PERMANENTFLAGS (\\Answered \\Flagged \\Deleted \\Seen \\Draft {})] Flags permitted",
            if readonly { "" } else { "\\*" }
        ),
    )
    .await?;
    send_line(
        stream,
        &format!("* OK [UIDVALIDITY {}] UIDs valid", folder_state.uidvalidity),
    )
    .await?;
    send_line(stream, &format!("* OK [UIDNEXT {}] Predicted next UID", folder_state.uidnext)).await?;

    session.selected_folder = Some(folder.to_string());
    session.selected_readonly = readonly;

    if readonly {
        ok(stream, tag, "EXAMINE completed").await?;
    } else {
        ok(stream, tag, "SELECT completed").await?;
    }
    Ok(())
}

async fn list_folders(
    stream: &mut TcpStream,
    tag: &str,
    session: &Session,
    store: &Arc<dyn MailStore>,
    pattern: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let account = match session.account.as_ref() {
        Some(a) => a,
        None => {
            no(stream, tag, "Not authenticated").await?;
            return Ok(());
        }
    };
    let pattern = unquote(pattern);
    let folders = store.list_folders(&account.email_address).await?;
    for folder in folders {
        let name = folder.name;
        if pattern == "*" || pattern == "%" || name == pattern {
            // Mailbox attributes.
            let attrs = if name.eq_ignore_ascii_case("INBOX") {
                "(\\HasNoChildren)"
            } else {
                "(\\HasNoChildren)"
            };
            send_line(stream, &format!("* LIST {attrs} \"/\" {}", quote(&name))).await?;
        }
    }
    ok(stream, tag, "LIST completed").await?;
    Ok(())
}

async fn status_folder(
    stream: &mut TcpStream,
    tag: &str,
    session: &Session,
    store: &Arc<dyn MailStore>,
    folder: String,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let account = match session.account.as_ref() {
        Some(a) => a,
        None => {
            no(stream, tag, "Not authenticated").await?;
            return Ok(());
        }
    };
    let state = store.select_folder(&account.email_address, &folder).await?;
    send_line(
        stream,
        &format!(
            "* STATUS \"{}\" (MESSAGES {} UIDNEXT {} UIDVALIDITY {} UNSEEN {})",
            quote(&folder),
            state.exists,
            state.uidnext,
            state.uidvalidity,
            state.unseen
        ),
    )
    .await?;
    ok(stream, tag, "STATUS completed").await?;
    Ok(())
}

fn quote(s: &str) -> String {
    if s.contains('"') || s.contains('\\') || s.contains(' ') {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        format!("\"{s}\"")
    }
}

fn parse_seq_set(seq_set: &str, exists: u32) -> Result<Vec<u32>, MailStoreError> {
    let mut out = Vec::new();
    for part in seq_set.split(',') {
        let part = part.trim();
        if part == "*" {
            if exists > 0 {
                out.push(exists);
            }
        } else if let Some((a, b)) = part.split_once(':') {
            let start = if a == "*" { exists } else { a.parse::<u32>().unwrap_or(1) };
            let end = if b == "*" { exists } else { b.parse::<u32>().unwrap_or(exists) };
            for uid in start.min(end)..=start.max(end) {
                out.push(uid);
            }
        } else {
            out.push(part.parse::<u32>().map_err(|_| MailStoreError::Other("bad sequence".into()))?);
        }
    }
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

fn parse_fetch_items(items: &[&str]) -> FetchQuery {
    let mut q = FetchQuery::default();
    for item in items {
        let item = item.to_ascii_uppercase();
        match item.as_str() {
            "ALL" => {
                q.envelope = true;
                q.flags = true;
                q.internal_date = true;
                q.rfc822_size = true;
            }
            "FAST" => {
                q.flags = true;
                q.internal_date = true;
                q.rfc822_size = true;
            }
            "FULL" => {
                q.envelope = true;
                q.flags = true;
                q.internal_date = true;
                q.rfc822_size = true;
                q.body = true;
            }
            "ENVELOPE" => q.envelope = true,
            "FLAGS" => q.flags = true,
            "INTERNALDATE" => q.internal_date = true,
            "RFC822" => q.rfc822 = true,
            "RFC822.HEADER" => q.rfc822_header = true,
            "RFC822.SIZE" => q.rfc822_size = true,
            "RFC822.TEXT" => q.rfc822_text = true,
            "BODYSTRUCTURE" => q.body_structure = true,
            "BODY" => q.body = true,
            "BODY.PEEK[]" => q.rfc822 = true,
            _ => {}
        }
    }
    q
}

async fn fetch_cmd(
    stream: &mut TcpStream,
    tag: &str,
    session: &Session,
    store: &Arc<dyn MailStore>,
    seq_set: &str,
    items: &[&str],
    uid_cmd: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (account, folder) = match (&session.account, &session.selected_folder) {
        (Some(a), Some(f)) => (a, f),
        _ => {
            no(stream, tag, "No folder selected").await?;
            return Ok(());
        }
    };

    let folder_state = store.select_folder(&account.email_address, folder).await?;
    let mut uids = parse_seq_set(seq_set, folder_state.exists)?;
    if uid_cmd {
        // seq_set already UIDs.
    } else {
        // Map sequence numbers to UIDs. Simplification: sequence n == nth UID.
        // For a real implementation, maintain an ordered UID list.
        let all = store
            .search_messages(&account.email_address, folder, &SearchCriteria::All)
            .await?;
        let mut mapped = Vec::new();
        for seq in &uids {
            if let Some(uid) = all.get((seq.saturating_sub(1)) as usize) {
                mapped.push(*uid);
            }
        }
        uids = mapped;
    }

    let query = parse_fetch_items(items);
    let messages = store
        .fetch_messages(&account.email_address, folder, &uids, &query)
        .await?;

    for msg in messages {
        let mut parts: Vec<String> = Vec::new();
        if query.flags {
            let flags = msg
                .flags
                .iter()
                .map(|f| format!("\\{}", &f[1..])) // \Seen -> \Seen
                .collect::<Vec<_>>()
                .join(" ");
            parts.push(format!("FLAGS ({flags})"));
        }
        if query.envelope {
            parts.push(format!(
                "ENVELOPE (\"{}\" \"{}\" ((NIL NIL \"{}\" \"{}\")) ((NIL NIL \"{}\" \"{}\")) ((NIL NIL \"{}\" \"{}\")) ((NIL NIL \"{}\" \"{}\")) NIL NIL \"{}\" \"{}\")",
                msg.date.format("%d-%b-%Y %H:%M:%S %z"),
                quote_envelope_string(&msg.subject),
                local_part(&msg.from),
                domain_part(&msg.from),
                local_part(&msg.from),
                domain_part(&msg.from),
                local_part(&msg.to),
                domain_part(&msg.to),
                local_part(&msg.to),
                domain_part(&msg.to),
                quote_envelope_string(&msg.message_id),
                quote_envelope_string(&msg.message_id),
            ));
        }
        if query.internal_date {
            parts.push(format!(
                "INTERNALDATE \"{}\"",
                msg.date.format("%d-%b-%Y %H:%M:%S %z")
            ));
        }
        if query.rfc822_size {
            parts.push(format!("RFC822.SIZE {}", msg.size_bytes));
        }
        if query.rfc822 {
            let body = String::from_utf8_lossy(&msg.raw_eml);
            parts.push(format!("RFC822 {{{}}}", body.len()));
            // Send body literal on its own line.
            send_line(stream, &format!("* {} FETCH ({})", msg.uid, parts.join(" "))).await?;
            send_literal(stream, &body).await?;
            continue;
        }

        if parts.is_empty() {
            parts.push(format!("UID {}", msg.uid));
        }
        send_line(stream, &format!("* {} FETCH ({})", msg.uid, parts.join(" "))).await?;
    }

    ok(stream, tag, "FETCH completed").await?;
    Ok(())
}

async fn send_literal(stream: &mut TcpStream, data: &str) -> tokio::io::Result<()> {
    stream.write_all(data.as_bytes()).await?;
    stream.write_all(b"\r\n").await?;
    stream.flush().await
}

fn quote_envelope_string(s: &str) -> String {
    if s.is_empty() {
        "NIL".into()
    } else {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

fn local_part(addr: &str) -> String {
    addr.split('@').next().unwrap_or("").to_string()
}

fn domain_part(addr: &str) -> String {
    addr.split('@').nth(1).unwrap_or("").to_string()
}

async fn store_flags(
    stream: &mut TcpStream,
    tag: &str,
    session: &mut Session,
    store: &Arc<dyn MailStore>,
    seq_set: &str,
    args: &[&str],
    uid_cmd: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if session.selected_readonly {
        no(stream, tag, "Mailbox is read-only").await?;
        return Ok(());
    }
    let (account, folder) = match (&session.account, &session.selected_folder) {
        (Some(a), Some(f)) => (a, f),
        _ => {
            no(stream, tag, "No folder selected").await?;
            return Ok(());
        }
    };

    if args.len() < 2 {
        bad(stream, tag, "STORE requires mode and flags").await?;
        return Ok(());
    }

    let mode_str = args[0].to_ascii_uppercase();
    let mode = match mode_str.as_str() {
        "+FLAGS" | "+FLAGS.SILENT" => FlagMode::Add,
        "-FLAGS" | "-FLAGS.SILENT" => FlagMode::Remove,
        "FLAGS" | "FLAGS.SILENT" => FlagMode::Replace,
        _ => {
            bad(stream, tag, "Invalid store mode").await?;
            return Ok(());
        }
    };

    let flags: Vec<String> = args[1..]
        .iter()
        .map(|s| s.trim_matches(|c| c == '(' || c == ')').to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let folder_state = store.select_folder(&account.email_address, folder).await?;
    let mut uids = parse_seq_set(seq_set, folder_state.exists)?;
    if !uid_cmd {
        // Map sequence numbers to UIDs (simplified).
        let all = store
            .search_messages(&account.email_address, folder, &SearchCriteria::All)
            .await?;
        let mut mapped = Vec::new();
        for seq in &uids {
            if let Some(uid) = all.get((seq.saturating_sub(1)) as usize) {
                mapped.push(*uid);
            }
        }
        uids = mapped;
    }

    store
        .set_flags(&account.email_address, folder, &uids, &flags, mode)
        .await?;

    ok(stream, tag, "STORE completed").await?;
    Ok(())
}

async fn search_cmd(
    stream: &mut TcpStream,
    tag: &str,
    session: &Session,
    store: &Arc<dyn MailStore>,
    args: &[&str],
    uid_cmd: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (account, folder) = match (&session.account, &session.selected_folder) {
        (Some(a), Some(f)) => (a, f),
        _ => {
            no(stream, tag, "No folder selected").await?;
            return Ok(());
        }
    };

    let criteria = parse_search(args);
    let uids = store
        .search_messages(&account.email_address, folder, &criteria)
        .await?;
    let response = if uids.is_empty() {
        "* SEARCH".to_string()
    } else {
        format!("* SEARCH {}", uids.iter().map(|u| u.to_string()).collect::<Vec<_>>().join(" "))
    };
    send_line(stream, &response).await?;

    if uid_cmd {
        ok(stream, tag, "UID SEARCH completed").await?;
    } else {
        ok(stream, tag, "SEARCH completed").await?;
    }
    Ok(())
}

fn parse_search(args: &[&str]) -> SearchCriteria {
    let joined = args
        .iter()
        .map(|s| s.to_ascii_uppercase())
        .collect::<Vec<_>>()
        .join(" ");
    if joined.contains("UNSEEN") {
        SearchCriteria::Unseen
    } else if joined.contains("RECENT") {
        SearchCriteria::Recent
    } else {
        SearchCriteria::All
    }
}
