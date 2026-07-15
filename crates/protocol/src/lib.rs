//! Shared message protocol for the AuditReady remote-shell tunnel.
//!
//! Used by both the agent and the broker. Messages are serialized as JSON
//! over the WebSocket for easy debugging; binary PTY data is base64-encoded.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Stable identity for an agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AgentId(pub Uuid);

impl AgentId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for AgentId {
    fn default() -> Self {
        Self::new()
    }
}

/// Identity for one terminal session/channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChannelId(pub Uuid);

impl ChannelId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ChannelId {
    fn default() -> Self {
        Self::new()
    }
}

/// All messages exchanged over the agent↔broker and operator↔broker WebSockets.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TunnelMessage {
    /// Agent → broker: identify and authenticate.
    /// `agent_id` may be omitted; the broker will assign and return one.
    AgentHello {
        #[serde(skip_serializing_if = "Option::is_none")]
        agent_id: Option<AgentId>,
        token: String,
    },

    /// Broker → agent: acceptance response.
    /// When `agent_id` was omitted in `AgentHello`, the assigned id is returned.
    BrokerHello {
        accepted: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        agent_id: Option<AgentId>,
    },

    /// Operator → broker → agent: open a new shell channel.
    ChannelOpen {
        channel_id: ChannelId,
        #[serde(skip_serializing_if = "Option::is_none")]
        command: Option<String>,
    },

    /// Bidirectional: close a channel.
    ChannelClose { channel_id: ChannelId },

    /// Bidirectional: PTY bytes (base64 encoded).
    ChannelData { channel_id: ChannelId, data: String },

    /// Operator → broker → agent: terminal resize.
    ChannelResize {
        channel_id: ChannelId,
        rows: u16,
        cols: u16,
    },

    /// Broker → either side: error notification.
    Error { message: String },
}

impl TunnelMessage {
    /// Encode raw PTY bytes into a `ChannelData` message.
    pub fn channel_data(channel_id: ChannelId, bytes: &[u8]) -> Self {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        Self::ChannelData {
            channel_id,
            data: STANDARD.encode(bytes),
        }
    }

    /// Decode base64 data from a `ChannelData` message, if this is one.
    pub fn decode_channel_data(&self) -> Option<(ChannelId, Vec<u8>)> {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        match self {
            Self::ChannelData { channel_id, data } => {
                STANDARD.decode(data).ok().map(|v| (*channel_id, v))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_hello_round_trips() {
        let msg = TunnelMessage::AgentHello {
            agent_id: Some(AgentId::new()),
            token: "test-token".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let decoded: TunnelMessage = serde_json::from_str(&json).unwrap();
        match (msg, decoded) {
            (
                TunnelMessage::AgentHello {
                    agent_id: id1,
                    token: t1,
                },
                TunnelMessage::AgentHello {
                    agent_id: id2,
                    token: t2,
                },
            ) => {
                assert_eq!(id1, id2);
                assert_eq!(t1, t2);
            }
            _ => panic!("variant mismatch"),
        }
    }

    #[test]
    fn agent_hello_without_agent_id_round_trips() {
        let msg = TunnelMessage::AgentHello {
            agent_id: None,
            token: "test-token".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(!json.contains("agent_id"));
        let decoded: TunnelMessage = serde_json::from_str(&json).unwrap();
        match decoded {
            TunnelMessage::AgentHello { agent_id, token } => {
                assert!(agent_id.is_none());
                assert_eq!(token, "test-token");
            }
            _ => panic!("variant mismatch"),
        }
    }

    #[test]
    fn channel_data_base64_round_trips() {
        let channel_id = ChannelId::new();
        let bytes = b"hello\nworld\x00\x01\x02";
        let msg = TunnelMessage::channel_data(channel_id, bytes.as_slice());
        let (decoded_id, decoded_bytes) = msg.decode_channel_data().unwrap();
        assert_eq!(decoded_id, channel_id);
        assert_eq!(decoded_bytes, bytes);
    }
}
