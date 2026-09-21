//! gRPC sync server.

use crate::proto::sync_service_server::{SyncService, SyncServiceServer};
use crate::proto::{
    AppendRequest, AppendResponse, DownloadFileRequest, FileChunk, FileRef,
    TailRequest, TailResponse, UploadFileResponse,
};
use crate::store::{SyncStore, TailPage};
use std::collections::HashMap;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use tokio_stream::{Stream, StreamExt};
use tonic::codegen::tokio_stream;
use tonic::{transport::Server, Request, Response, Status, Streaming};
use tracing::{error, info};

/// Builder for the sync server.
pub struct SyncServerBuilder {
    bind_addr: String,
    data_dir: PathBuf,
    tokens: HashMap<String, String>,
}

impl SyncServerBuilder {
    pub fn new(bind_addr: impl Into<String>, data_dir: impl Into<PathBuf>) -> Self {
        Self {
            bind_addr: bind_addr.into(),
            data_dir: data_dir.into(),
            tokens: HashMap::new(),
        }
    }

    /// Set a single token that is accepted for every site.
    pub fn global_token(mut self, token: impl Into<String>) -> Self {
        self.tokens.insert("*".into(), token.into());
        self
    }

    /// Load per-site tokens from a JSON file mapping `site_id -> token`.
    pub fn tokens_file(mut self, path: &std::path::Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let map: HashMap<String, String> = serde_json::from_str(&content)?;
        self.tokens.extend(map);
        Ok(self)
    }

    pub async fn serve(self) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.data_dir)?;
        let db_path = self.data_dir.join("sync.redb");
        let store = Arc::new(crate::redb_store::RedbSyncStore::open(&db_path)?);
        let auth = AuthLayer {
            tokens: Arc::new(self.tokens),
        };
        let service = SyncServiceImpl { store };
        let svc = SyncServiceServer::with_interceptor(service, move |req| auth.check(req));

        info!("kiff-sync-server listening on {}", self.bind_addr);
        Server::builder()
            .add_service(svc)
            .serve(self.bind_addr.parse()?)
            .await?;
        Ok(())
    }
}

#[derive(Clone)]
struct AuthLayer {
    tokens: Arc<HashMap<String, String>>,
}

impl AuthLayer {
    fn check(&self, req: Request<()>) -> Result<Request<()>, Status> {
        let site_id = req
            .metadata()
            .get("x-site-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let token = req
            .metadata()
            .get("x-site-token")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        let expected = self
            .tokens
            .get(site_id)
            .or_else(|| self.tokens.get("*"))
            .map(|s| s.as_str())
            .unwrap_or("");

        if expected.is_empty() || !constant_time_eq(token, expected) {
            return Err(Status::unauthenticated("invalid site token"));
        }
        Ok(req)
    }
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let diff = AtomicUsize::new(0);
    // Simple byte-by-byte comparison with constant-ish timing for short tokens.
    let max_len = a.len().max(b.len());
    for i in 0..max_len {
        let ca = a.as_bytes().get(i).unwrap_or(&0);
        let cb = b.as_bytes().get(i).unwrap_or(&0);
        diff.fetch_add((*ca != *cb) as usize, Ordering::Relaxed);
    }
    diff.load(Ordering::Relaxed) == 0 && a.len() == b.len()
}

struct SyncServiceImpl<S: SyncStore> {
    store: Arc<S>,
}

#[tonic::async_trait]
impl<S: SyncStore> SyncService for SyncServiceImpl<S> {
    async fn append(
        &self,
        request: Request<AppendRequest>,
    ) -> Result<Response<AppendResponse>, Status> {
        let req = request.into_inner();
        let mut ops = req.ops;
        match self.store.append(&req.site_id, ops.clone()).await {
            Ok(lsns) => {
                for (op, lsn) in ops.iter_mut().zip(&lsns) {
                    op.lsn = *lsn;
                }
                Ok(Response::new(AppendResponse {
                    first_lsn: lsns.first().copied().unwrap_or(0),
                    lsns,
                }))
            }
            Err(e) => {
                error!("append failed: {}", e);
                Err(Status::internal("append failed"))
            }
        }
    }

    async fn tail(
        &self,
        request: Request<TailRequest>,
    ) -> Result<Response<TailResponse>, Status> {
        let req = request.into_inner();
        match self.store.tail(&req.site_id, req.after_lsn, req.max_count).await {
            Ok(TailPage { ops, head_lsn }) => Ok(Response::new(TailResponse { ops, head_lsn })),
            Err(e) => {
                error!("tail failed: {}", e);
                Err(Status::internal("tail failed"))
            }
        }
    }

    async fn upload_file(
        &self,
        request: Request<Streaming<FileChunk>>,
    ) -> Result<Response<UploadFileResponse>, Status> {
        let mut stream = request.into_inner();
        let mut site_id = String::new();
        let mut hash = String::new();
        let mut buffer: Vec<u8> = Vec::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            if site_id.is_empty() {
                site_id = chunk.site_id;
                hash = chunk.hash;
            }
            buffer.extend_from_slice(&chunk.data);
        }

        if site_id.is_empty() || hash.is_empty() {
            return Err(Status::invalid_argument("missing site_id or hash"));
        }

        match self.store.put_file(&site_id, &hash, buffer).await {
            Ok(()) => Ok(Response::new(UploadFileResponse { hash })),
            Err(e) => {
                error!("upload_file failed: {}", e);
                Err(Status::internal("upload failed"))
            }
        }
    }

    type DownloadFileStream =
        Pin<Box<dyn Stream<Item = Result<FileChunk, Status>> + Send + 'static>>;

    async fn download_file(
        &self,
        request: Request<DownloadFileRequest>,
    ) -> Result<Response<Self::DownloadFileStream>, Status> {
        let req = request.into_inner();
        let bytes = match self.store.get_file(&req.site_id, &req.hash).await {
            Ok(Some(b)) => b,
            Ok(None) => return Err(Status::not_found("file not found")),
            Err(e) => {
                error!("download_file failed: {}", e);
                return Err(Status::internal("download failed"));
            }
        };

        let site_id = req.site_id;
        let hash = req.hash;
        let stream = async_stream::try_stream! {
            const CHUNK: usize = 64 * 1024;
            for (i, chunk) in bytes.chunks(CHUNK).enumerate() {
                yield FileChunk {
                    site_id: if i == 0 { site_id.clone() } else { String::new() },
                    hash: if i == 0 { hash.clone() } else { String::new() },
                    data: chunk.to_vec(),
                };
            }
        };
        Ok(Response::new(Box::pin(stream) as Self::DownloadFileStream))
    }
}

/// Convert a crate-level [`crate::protocol::FileRef`] into a protobuf `FileRef`.
pub fn file_ref_to_proto(f: &crate::protocol::FileRef) -> FileRef {
    FileRef {
        hash: f.hash.clone(),
        size: f.size,
        path: f.path.clone(),
        encrypted_key: f.encrypted_key.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::MemorySyncStore;

    fn sample_op(name: &str, action: &str) -> crate::proto::ChangeOp {
        crate::proto::ChangeOp {
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
    async fn service_append_and_tail() {
        let store = Arc::new(MemorySyncStore::new());
        let service = SyncServiceImpl { store };

        let append_req = Request::new(AppendRequest {
            site_id: "localhost".into(),
            ops: vec![
                sample_op("C-001", "INSERT"),
                sample_op("C-002", "INSERT"),
            ],
        });
        let resp = service.append(append_req).await.unwrap().into_inner();
        assert_eq!(resp.first_lsn, 1);
        assert_eq!(resp.lsns, vec![1, 2]);

        let tail_req = Request::new(TailRequest {
            site_id: "localhost".into(),
            after_lsn: 0,
            max_count: 10,
        });
        let tail = service.tail(tail_req).await.unwrap().into_inner();
        assert_eq!(tail.ops.len(), 2);
        assert_eq!(tail.head_lsn, 2);
        assert_eq!(tail.ops[0].lsn, 1);
        assert_eq!(tail.ops[1].lsn, 2);
    }


}
