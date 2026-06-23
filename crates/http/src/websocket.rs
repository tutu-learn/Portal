use crate::AppState;
use axum::{
    extract::{
        ws::{Message, WebSocketUpgrade},
        Query, State,
    },
    response::IntoResponse,
};
use serde::Deserialize;
use std::collections::HashSet;
use tokio::sync::broadcast;

#[derive(Debug, Deserialize)]
pub struct WsQuery {
    #[serde(default)]
    pub rooms: Option<String>,
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<WsQuery>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let rooms: Vec<String> = query
        .rooms
        .map(|s| s.split(',').map(|x| x.trim().to_string()).collect())
        .unwrap_or_else(|| vec!["global".to_string()]);

    ws.on_upgrade(move |mut socket| async move {
        let mut receivers: Vec<broadcast::Receiver<String>> = Vec::new();
        for room in &rooms {
            receivers.push(state.pubsub.subscribe(room));
        }

        // Also subscribe to user-specific room if session is available
        // For now, use a generic user room based on first room
        let user_room = format!("user:{}", rooms.get(0).cloned().unwrap_or_else(|| "anonymous".into()));
        receivers.push(state.pubsub.subscribe(&user_room));

        loop {
            tokio::select! {
                msg = socket.recv() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            // Parse incoming message to see if it's a subscription request
                            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                                if let Some(event) = json.get("event").and_then(|v| v.as_str()) {
                                    if event == "subscribe" {
                                        if let Some(room) = json.get("room").and_then(|v| v.as_str()) {
                                            receivers.push(state.pubsub.subscribe(room));
                                        }
                                    }
                                }
                            }
                        }
                        Some(Ok(Message::Close(_))) | None => break,
                        _ => {}
                    }
                }
                maybe_msg = async {
                    for rx in &mut receivers {
                        if let Ok(msg) = rx.try_recv() {
                            return Some(msg);
                        }
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                    None
                } => {
                    if let Some(msg) = maybe_msg {
                        let _ = socket.send(Message::Text(msg)).await;
                    }
                }
            }
        }
    })
}
