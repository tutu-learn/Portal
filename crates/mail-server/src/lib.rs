//! Multi-domain mail server primitives for Sebrus Mail.
//!
//! Provides an ORM-backed mail store, an SMTP inbound server, and a minimal
//! IMAP4rev1 server. All crates used are MIT or Apache-2.0 licensed.

pub mod imap;
pub mod smtp;
pub mod store;

pub use store::{FlagMode, MailStore, MailStoreError, OrmMailStore, SearchCriteria};
