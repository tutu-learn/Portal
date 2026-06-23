use axum::{body::Bytes, extract::Query, http::StatusCode, response::IntoResponse};
use serde::Deserialize;
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct EioQuery {
    pub sid: Option<String>,
    pub transport: Option<String>,
}

const CONTENT_TYPE: &str = "text/plain; charset=UTF-8";

// Engine.IO v4 timing constants (ms)
const PING_INTERVAL_MS: u64 = 25_000;
const PING_TIMEOUT_MS: u64 = 20_000;

/// Engine.IO v4 GET — handles initial handshake and long-poll requests.
pub async fn handle_get(Query(q): Query<EioQuery>) -> impl IntoResponse {
    match q.sid {
        None => {
            // Initial handshake: generate a session id.
            // We do NOT advertise websocket upgrades — polling only.
            let sid = Uuid::new_v4().simple().to_string();
            let body = format!(
                r#"0{{"sid":"{}","upgrades":[],"pingInterval":{},"pingTimeout":{},"maxPayload":1000000}}"#,
                sid, PING_INTERVAL_MS, PING_TIMEOUT_MS,
            );
            (
                StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, CONTENT_TYPE)],
                body,
            )
        }
        Some(_) => {
            // Long-poll: hold the connection open for pingInterval ms, then
            // send a PING (packet type "2") so the client knows we're alive.
            // Without this sleep the client would re-poll thousands of times
            // per second since we have no real data to push.
            tokio::time::sleep(Duration::from_millis(PING_INTERVAL_MS)).await;
            (
                StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, CONTENT_TYPE)],
                "2".to_string(),
            )
        }
    }
}

/// Engine.IO v4 POST — receives packets from the client (CONNECT, PONG, etc.).
pub async fn handle_post(Query(_q): Query<EioQuery>, body: Bytes) -> impl IntoResponse {
    let text = std::str::from_utf8(&body).unwrap_or("");

    // Socket.IO CONNECT packet ("40" or "40/namespace,") — echo acknowledgment.
    if let Some(pos) = text.find("40") {
        let after = &text[pos + 2..];
        let ack = if after.starts_with('/') {
            let end = after.find(',').unwrap_or(after.len());
            format!("40{},", &after[..end])
        } else {
            "40".to_string()
        };
        return (
            StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, CONTENT_TYPE)],
            ack,
        );
    }

    // PONG ("3") or any other packet — acknowledge with noop.
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, CONTENT_TYPE)],
        "6".to_string(),
    )
}
