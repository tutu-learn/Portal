//! Framework-level sync for Kiff.
//!
//! This crate provides the data model, local outbox capture, and server-side
//! storage for keeping SQLite databases and site files in sync across multiple
//! machines via a central server.

pub mod crypto;
pub mod outbox;
pub mod protocol;
pub mod redb_store;
pub mod server;
pub mod store;

/// Generated protobuf types and gRPC service stubs.
pub mod proto {
    tonic::include_proto!("kiff_sync");
}

pub use outbox::{ensure_outbox_tables, register as register_outbox_hook, SyncOutboxHook};
pub use protocol::{Action, FileRef};
pub use redb_store::RedbSyncStore;
pub use store::{MemorySyncStore, SyncStore, TailPage};
