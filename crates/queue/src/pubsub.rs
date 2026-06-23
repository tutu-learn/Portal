use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::debug;

#[derive(Debug, Clone)]
pub struct PubSub {
    channels: Arc<dashmap::DashMap<String, broadcast::Sender<String>>>,
}

impl PubSub {
    pub fn new() -> Self {
        Self {
            channels: Arc::new(dashmap::DashMap::new()),
        }
    }

    pub fn subscribe(&self, room: &str) -> broadcast::Receiver<String> {
        let sender = self
            .channels
            .entry(room.into())
            .or_insert_with(|| broadcast::channel(256).0);
        sender.subscribe()
    }

    pub fn publish(&self, room: &str, message: &str) {
        if let Some(sender) = self.channels.get(room) {
            let _ = sender.send(message.into());
        } else {
            debug!("no subscribers for room: {}", room);
        }
    }
}

impl Default for PubSub {
    fn default() -> Self {
        Self::new()
    }
}
