//! Private blob storage with unguessable access keys.
//!
//! Apps historically stored uploaded bytes under the site's `private/files`
//! tree keyed by sequential, enumerable IDs (`F-000001`, …) and accepted
//! client-supplied `{id}` storage descriptors without proof of possession.
//! That combination made every blob a confused-deputy target: an attacker who
//! learned (or guessed) someone's blob ID could attach it to their own
//! document and download it through any authorization path that resolved
//! references by ID alone.
//!
//! This module replaces the pattern. `store_blob` writes the bytes to a
//! random, unguessable ID and returns a [`BlobRef`] carrying a separate
//! random 256-bit `key`; the key is written to a sidecar file and is required
//! to open the blob again. A descriptor without the correct key is inert, so
//! forging references no longer grants access. Apps persist the whole
//! `BlobRef` (both fields) in their storage descriptors and pass
//! `validate_storage_descriptor` the client-supplied descriptor before
//! trusting it.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::AppState;

/// Handle to a stored blob. `id` names the file; `key` is the proof of
/// possession required to read it. Persist both (e.g. in a document's
/// `storage` descriptor) — anyone holding the reference may open the blob
/// subject to the app's own authorization rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlobRef {
    pub id: String,
    pub key: String,
}

impl BlobRef {
    /// Embed the reference in a JSON storage descriptor.
    pub fn to_value(&self) -> Value {
        json!({ "id": self.id, "key": self.key })
    }

    /// Parse a descriptor previously produced by [`BlobRef::to_value`] or
    /// `store_blob`'s caller. `None` unless both fields are present strings.
    pub fn from_value(v: &Value) -> Option<BlobRef> {
        Some(BlobRef {
            id: v.get("id")?.as_str()?.to_string(),
            key: v.get("key")?.as_str()?.to_string(),
        })
    }
}

fn token() -> String {
    // Two independent v4 UUIDs joined: 256 bits of randomness, URL-safe.
    format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple())
}

/// Root of the shared private file tree for the (single-site) deployment.
pub fn private_files_root(state: &AppState) -> error::Result<PathBuf> {
    let site = state
        .pools
        .iter()
        .next()
        .map(|e| e.key().clone())
        .ok_or_else(|| error::RuntimeError::Config("no site pool available".into()))?;
    let base = state.config.runtime.sites_path.trim_end_matches('/');
    Ok(PathBuf::from(format!("{base}/{site}/private/files")))
}

fn sidecar(path: &std::path::Path, suffix: &str) -> PathBuf {
    let mut p = path.as_os_str().to_os_string();
    p.push(suffix);
    PathBuf::from(p)
}

fn ct_eq(a: &str, b: &str) -> bool {
    // Constant-time-ish comparison for access keys: no early exit on length
    // or first differing byte.
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut diff = (a.len() ^ b.len()) as u8;
    for i in 0..a.len().min(b.len()) {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

/// Store `bytes` as a blob owned by `owner`, returning its unguessable
/// reference. Overwrites are impossible in practice: the ID is random.
pub fn store_blob(state: &AppState, owner: &str, bytes: &[u8]) -> error::Result<BlobRef> {
    let blob = BlobRef { id: token(), key: token() };
    let dir = private_files_root(state)?;
    std::fs::create_dir_all(&dir).map_err(|e| {
        error::RuntimeError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("create blob dir: {e}"),
        ))
    })?;
    let path = dir.join(&blob.id);
    std::fs::write(&path, bytes).map_err(error::RuntimeError::Io)?;
    std::fs::write(sidecar(&path, ".key"), &blob.key).map_err(error::RuntimeError::Io)?;
    std::fs::write(sidecar(&path, ".owner"), owner).map_err(error::RuntimeError::Io)?;
    Ok(blob)
}

/// Write a blob with a caller-chosen ID. Used by seeds and imports that must
/// keep stable IDs across restarts; the key is still random per call, so only
/// use this when the descriptor embedding the key is (re)written at the same
/// time.
pub fn store_blob_with_id(state: &AppState, id: &str, owner: &str, bytes: &[u8]) -> error::Result<BlobRef> {
    let blob = BlobRef { id: id.to_string(), key: token() };
    let dir = private_files_root(state)?;
    std::fs::create_dir_all(&dir).map_err(|e| {
        error::RuntimeError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("create blob dir: {e}"),
        ))
    })?;
    let path = dir.join(&blob.id);
    std::fs::write(&path, bytes).map_err(error::RuntimeError::Io)?;
    std::fs::write(sidecar(&path, ".key"), &blob.key).map_err(error::RuntimeError::Io)?;
    std::fs::write(sidecar(&path, ".owner"), owner).map_err(error::RuntimeError::Io)?;
    Ok(blob)
}

/// True when a file named `id` exists in the blob tree (sidecars excluded
/// from accidental matches by their suffix).
pub fn blob_exists(state: &AppState, id: &str) -> bool {
    private_files_root(state).map(|d| d.join(id).is_file()).unwrap_or(false)
}

/// Owner recorded at store time, when the blob exists.
pub fn blob_owner(state: &AppState, id: &str) -> Option<String> {
    let root = private_files_root(state).ok()?;
    std::fs::read_to_string(sidecar(&root.join(id), ".owner")).ok()
}

/// Open a blob only when the key matches the one recorded at store time.
pub fn open_blob(state: &AppState, id: &str, key: &str) -> error::Result<Vec<u8>> {
    let root = private_files_root(state)?;
    let path = root.join(id);
    let recorded = std::fs::read_to_string(sidecar(&path, ".key"))
        .map_err(|_| error::RuntimeError::NotFound("blob not found.".into()))?;
    if !ct_eq(recorded.trim(), key) {
        return Err(error::RuntimeError::PermissionDenied("invalid blob key.".into()));
    }
    std::fs::read(&path).map_err(|_| error::RuntimeError::NotFound("blob not found.".into()))
}

/// Validate a client-supplied storage descriptor before an app persists it.
///
/// Returns the verified [`BlobRef`] on success. Rejects descriptors that are
/// malformed, name a missing blob, carry the wrong key, or belong to another
/// user (administrators may reference any blob — they can read every
/// document through the app's gates anyway).
///
/// This is the confused-deputy fix: a forged reference to someone else's blob
/// is refused at write time, so no document can ever legitimize reading it.
pub fn validate_storage_descriptor(state: &AppState, user: &str, desc: &Value) -> error::Result<BlobRef> {
    let blob = BlobRef::from_value(desc)
        .ok_or_else(|| error::RuntimeError::Validation("malformed storage descriptor.".into()))?;
    if !blob_exists(state, &blob.id) {
        return Err(error::RuntimeError::Validation("referenced file does not exist.".into()));
    }
    open_blob(state, &blob.id, &blob.key).map_err(|_| {
        error::RuntimeError::Validation("referenced file key does not match.".into())
    })?;
    let owner = blob_owner(state, &blob.id);
    let admin = user == "Administrator";
    if !admin && owner.as_deref() != Some(user) {
        return Err(error::RuntimeError::PermissionDenied(
            "you may only attach files you uploaded yourself.".into(),
        ));
    }
    Ok(blob)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blob_ref_roundtrips_through_value() {
        let r = BlobRef { id: "a".into(), key: "b".into() };
        let v = r.to_value();
        let parsed = BlobRef::from_value(&v).unwrap();
        assert_eq!(parsed.id, "a");
        assert_eq!(parsed.key, "b");
        assert!(BlobRef::from_value(&json!({"id": "x"})).is_none());
    }

    #[test]
    fn ct_eq_compares_content_not_prefix() {
        assert!(ct_eq("abc", "abc"));
        assert!(!ct_eq("abc", "abd"));
        assert!(!ct_eq("abc", "abcd"));
        assert!(!ct_eq("", "a"));
    }
}
