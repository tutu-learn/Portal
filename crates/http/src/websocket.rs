use crate::middleware::auth::authenticate_request;
use crate::AppState;
use axum::{
    extract::{
        ws::{Message, WebSocketUpgrade},
        Query, State,
    },
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;
use tokio::sync::broadcast;

/// Hard cap on total room subscriptions per connection, bounding the
/// per-connection receiver growth.
const MAX_SUBSCRIPTIONS_PER_CONNECTION: usize = 16;

#[derive(Debug, Deserialize)]
pub struct WsQuery {
    #[serde(default)]
    pub rooms: Option<String>,
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<WsQuery>,
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    // Authenticate BEFORE upgrading; unauthenticated clients get a plain 401.
    let session = match authenticate_request(&state, &headers).await {
        Some(s) => s,
        None => {
            return (StatusCode::UNAUTHORIZED, "authentication required").into_response();
        }
    };

    // The only rooms a connection may ever join are the global room and the
    // authenticated user's own room. Nothing is derived from client-supplied
    // room names beyond membership in this allowlist.
    let allowed: Vec<String> = vec!["global".to_string(), format!("user:{}", session.user)];

    let requested: Vec<String> = query
        .rooms
        .map(|s| s.split(',').map(|x| x.trim().to_string()).collect())
        .unwrap_or_default();

    let mut rooms: Vec<String> = requested
        .into_iter()
        .filter(|r| allowed.contains(r))
        .collect();
    if rooms.is_empty() {
        rooms = allowed.clone();
    }
    rooms.truncate(MAX_SUBSCRIPTIONS_PER_CONNECTION);

    ws.on_upgrade(move |mut socket| async move {
        let mut rooms = rooms;
        let mut receivers: Vec<broadcast::Receiver<String>> = Vec::new();
        for room in &rooms {
            receivers.push(state.pubsub.subscribe(room));
        }

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
                                            let room = room.to_string();
                                            if allowed.contains(&room)
                                                && !rooms.contains(&room)
                                                && rooms.len() < MAX_SUBSCRIPTIONS_PER_CONNECTION
                                            {
                                                receivers.push(state.pubsub.subscribe(&room));
                                                rooms.push(room);
                                            }
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
    .into_response()
}
