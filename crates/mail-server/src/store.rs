//! Mail-store abstraction and ORM-backed implementation.

use async_trait::async_trait;
use chrono::{DateTime, Utc};


pub mod orm;
#[cfg(test)]
mod tests;

pub use orm::OrmMailStore;

/// A hosted mail domain.
#[derive(Debug, Clone)]
pub struct Domain {
    pub name: String,
    pub enabled: bool,
    pub dkim_selector: Option<String>,
    pub dkim_private_key: Option<String>,
    pub catch_all_account: Option<String>,
}

/// A mail account (mailbox owner).
#[derive(Debug, Clone)]
pub struct Account {
    pub email_address: String,
    pub username: Option<String>,
    pub domain: String,
    pub password_hash: String,
    pub quota_bytes: i64,
    pub enabled: bool,
    pub is_domain_admin: bool,
}

/// A mailbox folder.
#[derive(Debug, Clone)]
pub struct Folder {
    pub name: String,
    pub uidvalidity: u32,
    pub uidnext: u32,
    pub exists: u32,
    pub recent: u32,
    pub unseen: u32,
}

/// A stored mail message.
#[derive(Debug, Clone)]
pub struct Message {
    pub uid: u32,
    pub message_id: String,
    pub subject: String,
    pub from: String,
    pub to: String,
    pub cc: String,
    pub date: DateTime<Utc>,
    pub size_bytes: i64,
    pub flags: Vec<String>,
    pub raw_eml: Vec<u8>,
    pub body_text: String,
    pub is_read: bool,
}

/// Query flags for IMAP FETCH.
#[derive(Debug, Clone, Default)]
pub struct FetchQuery {
    pub envelope: bool,
    pub flags: bool,
    pub internal_date: bool,
    pub rfc822: bool,
    pub rfc822_header: bool,
    pub rfc822_size: bool,
    pub rfc822_text: bool,
    pub body_structure: bool,
    pub body: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum MailStoreError {
    #[error("database error: {0}")]
    Database(#[from] error::RuntimeError),
    #[error("account not found: {0}")]
    AccountNotFound(String),
    #[error("domain not found: {0}")]
    DomainNotFound(String),
    #[error("invalid address: {0}")]
    InvalidAddress(String),
    #[error("quota exceeded")]
    QuotaExceeded,
    #[error("authentication failed")]
    AuthFailed,
    #[error("other: {0}")]
    Other(String),
}

impl From<anyhow::Error> for MailStoreError {
    fn from(e: anyhow::Error) -> Self {
        MailStoreError::Other(e.to_string())
    }
}

#[async_trait]
pub trait MailStore: Send + Sync {
    /// Authenticate a user. Accepts `user@domain` or short `username`.
    async fn authenticate(&self, username: &str, password: &str) -> Result<Option<Account>, MailStoreError>;

    /// Resolve a hosted domain.
    async fn resolve_domain(&self, domain: &str) -> Result<Option<Domain>, MailStoreError>;

    /// Resolve a recipient address into a list of target account e-mail addresses.
    /// Expands aliases and catch-all.
    async fn resolve_recipient(&self, address: &str) -> Result<Vec<String>, MailStoreError>;

    /// Append a raw RFC-822 message to a folder, returning the assigned UID.
    async fn append_message(
        &self,
        account_email: &str,
        folder: &str,
        raw: &[u8],
    ) -> Result<u32, MailStoreError>;

    /// List folders for an account.
    async fn list_folders(&self, account_email: &str) -> Result<Vec<Folder>, MailStoreError>;

    /// Get or create a folder and return its current state.
    async fn select_folder(&self, account_email: &str, folder: &str) -> Result<Folder, MailStoreError>;

    /// Fetch messages by UID.
    async fn fetch_messages(
        &self,
        account_email: &str,
        folder: &str,
        uids: &[u32],
        query: &FetchQuery,
    ) -> Result<Vec<Message>, MailStoreError>;

    /// Search messages in a folder. Returns matching UIDs.
    async fn search_messages(
        &self,
        account_email: &str,
        folder: &str,
        criteria: &SearchCriteria,
    ) -> Result<Vec<u32>, MailStoreError>;

    /// Set/replace flags for a set of UIDs.
    async fn set_flags(
        &self,
        account_email: &str,
        folder: &str,
        uids: &[u32],
        flags: &[String],
        mode: FlagMode,
    ) -> Result<(), MailStoreError>;
}

#[derive(Debug, Clone)]
pub enum SearchCriteria {
    All,
    Unseen,
    Recent,
    Uid(Vec<u32>),
    From(String),
    Subject(String),
}

#[derive(Debug, Clone, Copy)]
pub enum FlagMode {
    Replace,
    Add,
    Remove,
}
