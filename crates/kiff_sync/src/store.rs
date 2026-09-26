//! Persistent storage backend for the sync server.

use crate::proto::ChangeOp;
use error::Result;

/// Abstract storage for a sync server's operation log and file blobs.
#[async_trait::async_trait]
pub trait SyncStore: Send + Sync + 'static {
    /// Append ops and assign monotonic LSNs. Returns the assigned LSNs in the
    /// same order as the input ops.
    async fn append(&self, site_id: &str, ops: Vec<ChangeOp>) -> Result<Vec<u64>>;

    /// Return ops with LSN strictly greater than `after_lsn`, up to
    /// `max_count` items.
    async fn tail(&self, site_id: &str, after_lsn: u64, max_count: u32) -> Result<TailPage>;

    /// Store an encrypted file blob.
    async fn put_file(&self, site_id: &str, hash: &str, bytes: Vec<u8>) -> Result<()>;

    /// Retrieve an encrypted file blob.
    async fn get_file(&self, site_id: &str, hash: &str) -> Result<Option<Vec<u8>>>;

    /// Current head LSN for a site. Returns 0 if the site has no ops.
    async fn head_lsn(&self, site_id: &str) -> Result<u64>;
}

#[derive(Debug, Clone)]
pub struct TailPage {
    pub ops: Vec<ChangeOp>,
    pub head_lsn: u64,
}

/// In-memory implementation useful for unit tests and local spikes.
#[derive(Default, Debug)]
pub struct MemorySyncStore {
    inner: std::sync::Mutex<Inner>,
}

#[derive(Default, Debug)]
struct Inner {
    next_lsn: u64,
    ops: Vec<ChangeOp>,
    files: std::collections::HashMap<(String, String), Vec<u8>>,
}

impl MemorySyncStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait::async_trait]
impl SyncStore for MemorySyncStore {
    async fn append(&self, _site_id: &str, mut ops: Vec<ChangeOp>) -> Result<Vec<u64>> {
        let mut inner = self.inner.lock().unwrap();
        let mut lsns = Vec::with_capacity(ops.len());
        for op in &mut ops {
            inner.next_lsn += 1;
            let lsn = inner.next_lsn;
            op.lsn = lsn;
            lsns.push(lsn);
        }
        inner.ops.extend(ops);
        Ok(lsns)
    }

    async fn tail(&self, _site_id: &str, after_lsn: u64, max_count: u32) -> Result<TailPage> {
        let inner = self.inner.lock().unwrap();
        let ops: Vec<ChangeOp> = inner
            .ops
            .iter()
            .filter(|op| op.lsn > after_lsn)
            .take(max_count as usize)
            .cloned()
            .collect();
        Ok(TailPage {
            ops,
            head_lsn: inner.next_lsn,
        })
    }

    async fn put_file(&self, _site_id: &str, hash: &str, bytes: Vec<u8>) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();
        inner.files.insert(("default".into(), hash.into()), bytes);
        Ok(())
    }

    async fn get_file(&self, _site_id: &str, hash: &str) -> Result<Option<Vec<u8>>> {
        let inner = self.inner.lock().unwrap();
        Ok(inner.files.get(&("default".into(), hash.into())).cloned())
    }

    async fn head_lsn(&self, _site_id: &str) -> Result<u64> {
        let inner = self.inner.lock().unwrap();
        Ok(inner.next_lsn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_op(name: &str, action: &str) -> ChangeOp {
        ChangeOp {
            op_id: uuid::Uuid::new_v4().to_string(),
            site_id: "localhost".into(),
            node_id: "node-a".into(),
            doctype: "Client".into(),
            name: name.into(),
            action: action.into(),
            payload_json: "{\"full_name\":\"Test\"}".into(),
            files: vec![],
            signature: vec![],
            encrypted_payload: vec![],
            lsn: 0,
        }
    }

    #[tokio::test]
    async fn memory_store_assigns_lsns() {
        let store = MemorySyncStore::new();
        let lsns = store
            .append(
                "localhost",
                vec![sample_op("C-001", "INSERT"), sample_op("C-002", "INSERT")],
            )
            .await
            .unwrap();
        assert_eq!(lsns, vec![1, 2]);
        assert_eq!(store.head_lsn("localhost").await.unwrap(), 2);
    }

    #[tokio::test]
    async fn memory_store_tails_after_lsn() {
        let store = MemorySyncStore::new();
        store
            .append(
                "localhost",
                vec![
                    sample_op("C-001", "INSERT"),
                    sample_op("C-002", "UPDATE"),
                    sample_op("C-003", "DELETE"),
                ],
            )
            .await
            .unwrap();

        let page = store.tail("localhost", 1, 10).await.unwrap();
        assert_eq!(page.ops.len(), 2);
        assert_eq!(page.head_lsn, 3);
        assert_eq!(page.ops[0].lsn, 2);
        assert_eq!(page.ops[1].lsn, 3);
    }
}
