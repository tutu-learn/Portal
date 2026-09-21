//! Sync action and file-reference helpers used by the local outbox.

/// Mutation kind captured in the sync outbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Insert,
    Update,
    Delete,
    Schema,
}

impl Action {
    pub fn as_str(&self) -> &'static str {
        match self {
            Action::Insert => "INSERT",
            Action::Update => "UPDATE",
            Action::Delete => "DELETE",
            Action::Schema => "SCHEMA",
        }
    }
}

impl std::fmt::Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// File attachment captured with a document change.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FileRef {
    /// SHA-256 of the encrypted blob, hex-encoded.
    pub hash: String,
    /// Size in bytes of the encrypted blob.
    pub size: u64,
    /// Site-relative path, e.g. `private/files/strongroom/invoice.pdf`.
    pub path: String,
    /// Per-file symmetric key, encrypted to the site's public key.
    pub encrypted_key: Vec<u8>,
}
