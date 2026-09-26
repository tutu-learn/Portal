//! redb-backed persistent storage for the sync server.

use crate::proto::ChangeOp;
use crate::store::{SyncStore, TailPage};
use error::{Result, RuntimeError};
use prost::Message;
use redb::{Database, ReadableTable, TableDefinition};
use std::path::Path;
use std::sync::Mutex;

const OPS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("ops");
const FILES_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("files");
const META_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("site_meta");

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SiteMeta {
    head_lsn: u64,
}

/// redb-backed sync store. One `RedbSyncStore` per server instance.
pub struct RedbSyncStore {
    db: Mutex<Database>,
}

impl RedbSyncStore {
    pub fn open(path: &Path) -> Result<Self> {
        let db = Database::create(path).map_err(|e| {
            RuntimeError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("redb open failed: {}", e),
            ))
        })?;
        {
            let tx = db.begin_write().map_err(redb_err)?;
            {
                let _ = tx.open_table(OPS_TABLE).map_err(redb_err)?;
                let _ = tx.open_table(FILES_TABLE).map_err(redb_err)?;
                let _ = tx.open_table(META_TABLE).map_err(redb_err)?;
            }
            tx.commit().map_err(redb_err)?;
        }
        Ok(Self { db: Mutex::new(db) })
    }

    fn meta_key(site_id: &str) -> String {
        site_id.to_string()
    }

    fn op_key(site_id: &str, lsn: u64) -> String {
        format!("{}\0{}", site_id, lsn)
    }

    fn file_key(site_id: &str, hash: &str) -> String {
        format!("{}\0{}", site_id, hash)
    }

    fn read_meta(&self, site_id: &str) -> Result<SiteMeta> {
        let db = self.db.lock().unwrap();
        let tx = db.begin_read().map_err(redb_err)?;
        let table = tx.open_table(META_TABLE).map_err(redb_err)?;
        let value = table.get(&*Self::meta_key(site_id)).map_err(redb_err)?;
        let meta = match value {
            Some(guard) => {
                let bytes = guard.value();
                serde_json::from_slice(bytes).map_err(|e| {
                    RuntimeError::Validation(format!("failed to deserialize site meta: {}", e))
                })?
            }
            None => SiteMeta { head_lsn: 0 },
        };
        Ok(meta)
    }
}

#[async_trait::async_trait]
impl SyncStore for RedbSyncStore {
    async fn append(&self, site_id: &str, mut ops: Vec<ChangeOp>) -> Result<Vec<u64>> {
        let mut meta = self.read_meta(site_id)?;
        let db = self.db.lock().unwrap();
        let tx = db.begin_write().map_err(redb_err)?;
        let mut lsns = Vec::with_capacity(ops.len());
        {
            let mut table = tx.open_table(OPS_TABLE).map_err(redb_err)?;
            for op in &mut ops {
                meta.head_lsn += 1;
                let lsn = meta.head_lsn;
                op.lsn = lsn;
                let bytes = op.encode_to_vec();
                table
                    .insert(&*Self::op_key(site_id, lsn), bytes.as_slice())
                    .map_err(redb_err)?;
                lsns.push(lsn);
            }
        }
        {
            let mut meta_table = tx.open_table(META_TABLE).map_err(redb_err)?;
            let bytes = serde_json::to_vec(&meta).map_err(|e| {
                RuntimeError::Validation(format!("failed to serialize site meta: {}", e))
            })?;
            meta_table
                .insert(&*Self::meta_key(site_id), bytes.as_slice())
                .map_err(redb_err)?;
        }
        tx.commit().map_err(redb_err)?;
        Ok(lsns)
    }

    async fn tail(&self, site_id: &str, after_lsn: u64, max_count: u32) -> Result<TailPage> {
        let meta = self.read_meta(site_id)?;
        let db = self.db.lock().unwrap();
        let tx = db.begin_read().map_err(redb_err)?;
        let table = tx.open_table(OPS_TABLE).map_err(redb_err)?;
        let mut ops = Vec::new();
        for lsn in (after_lsn + 1)..=meta.head_lsn {
            if ops.len() >= max_count as usize {
                break;
            }
            let guard = table.get(&*Self::op_key(site_id, lsn)).map_err(redb_err)?;
            let op = match guard {
                Some(g) => {
                    let bytes = g.value();
                    ChangeOp::decode(bytes).map_err(|e| {
                        RuntimeError::Validation(format!("failed to decode ChangeOp: {}", e))
                    })?
                }
                None => break,
            };
            ops.push(op);
        }
        Ok(TailPage {
            ops,
            head_lsn: meta.head_lsn,
        })
    }

    async fn put_file(&self, site_id: &str, hash: &str, bytes: Vec<u8>) -> Result<()> {
        let db = self.db.lock().unwrap();
        let tx = db.begin_write().map_err(redb_err)?;
        {
            let mut table = tx.open_table(FILES_TABLE).map_err(redb_err)?;
            table
                .insert(&*Self::file_key(site_id, hash), bytes.as_slice())
                .map_err(redb_err)?;
        }
        tx.commit().map_err(redb_err)?;
        Ok(())
    }

    async fn get_file(&self, site_id: &str, hash: &str) -> Result<Option<Vec<u8>>> {
        let db = self.db.lock().unwrap();
        let tx = db.begin_read().map_err(redb_err)?;
        let table = tx.open_table(FILES_TABLE).map_err(redb_err)?;
        let guard = table
            .get(&*Self::file_key(site_id, hash))
            .map_err(redb_err)?;
        let result = guard.map(|g| g.value().to_vec());
        Ok(result)
    }

    async fn head_lsn(&self, site_id: &str) -> Result<u64> {
        let meta = self.read_meta(site_id)?;
        Ok(meta.head_lsn)
    }
}

fn redb_err<E: Into<redb::Error>>(e: E) -> RuntimeError {
    RuntimeError::Io(std::io::Error::new(
        std::io::ErrorKind::Other,
        format!("redb error: {}", e.into()),
    ))
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
    async fn redb_store_persists_ops() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sync.redb");
        {
            let store = RedbSyncStore::open(&path).unwrap();
            let lsns = store
                .append(
                    "localhost",
                    vec![sample_op("C-001", "INSERT"), sample_op("C-002", "UPDATE")],
                )
                .await
                .unwrap();
            assert_eq!(lsns, vec![1, 2]);

            let page = store.tail("localhost", 0, 10).await.unwrap();
            assert_eq!(page.ops.len(), 2);
            assert_eq!(page.head_lsn, 2);
        }

        // Re-open and verify persistence.
        let store2 = RedbSyncStore::open(&path).unwrap();
        assert_eq!(store2.head_lsn("localhost").await.unwrap(), 2);
        let page2 = store2.tail("localhost", 1, 10).await.unwrap();
        assert_eq!(page2.ops.len(), 1);
        assert_eq!(page2.ops[0].name, "C-002");
    }

    #[tokio::test]
    async fn redb_store_persists_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sync.redb");
        {
            let store = RedbSyncStore::open(&path).unwrap();
            store
                .put_file("localhost", "abc123", vec![1, 2, 3])
                .await
                .unwrap();
            let bytes = store
                .get_file("localhost", "abc123")
                .await
                .unwrap()
                .unwrap();
            assert_eq!(bytes, vec![1, 2, 3]);
        }

        let store2 = RedbSyncStore::open(&path).unwrap();
        let bytes2 = store2
            .get_file("localhost", "abc123")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(bytes2, vec![1, 2, 3]);
    }
}
