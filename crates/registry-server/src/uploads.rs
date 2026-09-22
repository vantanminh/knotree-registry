use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use bytes::Bytes;
use registry_core::{Digest, RepositoryName};
use registry_storage::{DynObjectStore, StorageError};
use thiserror::Error;
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

use crate::blob_key;

#[derive(Debug, Error)]
pub enum UploadError {
    #[error("upload session was not found")]
    NotFound,
    #[error("upload session is no longer active")]
    InvalidState,
    #[error("upload session has expired")]
    Expired,
    #[error("upload offset mismatch: expected {expected}, actual {actual}")]
    OffsetMismatch { expected: u64, actual: u64 },
    #[error("uploaded digest does not match: expected {expected}, actual {actual}")]
    DigestMismatch { expected: Digest, actual: Digest },
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),
}

#[derive(Clone, Default)]
pub struct UploadManager {
    sessions: Arc<RwLock<HashMap<Uuid, Arc<UploadEntry>>>>,
    ttl: Duration,
}

struct UploadEntry {
    repository: RepositoryName,
    staging_key: String,
    state: Mutex<UploadState>,
}

struct UploadState {
    offset: u64,
    expires_at: u64,
    status: UploadStatusKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadStatusKind {
    Active,
    Completed,
    Failed,
    Aborted,
}

#[derive(Debug, Clone)]
pub struct UploadStatus {
    pub id: Uuid,
    pub repository: RepositoryName,
    pub staging_key: String,
    pub offset: u64,
    pub expires_at: u64,
    pub status: UploadStatusKind,
}

#[derive(Debug, Clone)]
pub struct FinalizedUpload {
    pub id: Uuid,
    pub repository: RepositoryName,
    pub digest: Digest,
    pub size: u64,
}

impl UploadManager {
    pub fn new(ttl: Duration) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            ttl,
        }
    }

    pub async fn create(&self, repository: RepositoryName) -> UploadStatus {
        let id = Uuid::new_v4();
        let entry = Arc::new(UploadEntry {
            repository,
            staging_key: format!("uploads/{id}/staging"),
            state: Mutex::new(UploadState {
                offset: 0,
                expires_at: now_seconds().saturating_add(self.ttl.as_secs()),
                status: UploadStatusKind::Active,
            }),
        });
        let status = status_for(id, &entry).await;
        self.sessions.write().await.insert(id, entry);
        status
    }

    pub async fn status(&self, id: Uuid) -> Result<UploadStatus, UploadError> {
        let entry = self.entry(id).await?;
        let status = status_for(id, &entry).await;
        if status.status == UploadStatusKind::Active && status.expires_at <= now_seconds() {
            let mut state = entry.state.lock().await;
            state.status = UploadStatusKind::Aborted;
            return Err(UploadError::Expired);
        }
        Ok(status)
    }

    pub async fn append(
        &self,
        id: Uuid,
        expected_offset: u64,
        body: Bytes,
        store: &DynObjectStore,
    ) -> Result<UploadStatus, UploadError> {
        let entry = self.entry(id).await?;
        let mut state = entry.state.lock().await;
        ensure_active(&state)?;
        if state.offset != expected_offset {
            return Err(UploadError::OffsetMismatch {
                expected: state.offset,
                actual: expected_offset,
            });
        }
        let offset = store
            .append(&entry.staging_key, state.offset, body)
            .await
            .map_err(|error| match error {
                StorageError::OffsetMismatch { expected, actual } => {
                    UploadError::OffsetMismatch { expected, actual }
                }
                other => UploadError::Storage(other),
            })?;
        state.offset = offset;
        Ok(UploadStatus {
            id,
            repository: entry.repository.clone(),
            staging_key: entry.staging_key.clone(),
            offset: state.offset,
            expires_at: state.expires_at,
            status: state.status,
        })
    }

    pub async fn finalize(
        &self,
        id: Uuid,
        expected_digest: Digest,
        final_body: Bytes,
        store: &DynObjectStore,
    ) -> Result<FinalizedUpload, UploadError> {
        let entry = self.entry(id).await?;
        let mut state = entry.state.lock().await;
        ensure_active(&state)?;
        if !final_body.is_empty() {
            state.offset = store
                .append(&entry.staging_key, state.offset, final_body)
                .await
                .map_err(|error| match error {
                    StorageError::OffsetMismatch { expected, actual } => {
                        UploadError::OffsetMismatch { expected, actual }
                    }
                    other => UploadError::Storage(other),
                })?;
        }
        store.finalize_append(&entry.staging_key).await?;
        let body = store.get(&entry.staging_key).await?;
        let actual = Digest::sha256(&body);
        if actual != expected_digest {
            state.status = UploadStatusKind::Failed;
            let _ = store.delete(&entry.staging_key).await;
            return Err(UploadError::DigestMismatch {
                expected: expected_digest,
                actual,
            });
        }
        store.put(&blob_key(&actual), body.clone()).await?;
        store.delete(&entry.staging_key).await?;
        state.offset = body.len() as u64;
        state.status = UploadStatusKind::Completed;
        Ok(FinalizedUpload {
            id,
            repository: entry.repository.clone(),
            digest: actual,
            size: body.len() as u64,
        })
    }

    pub async fn abort(&self, id: Uuid, store: &DynObjectStore) -> Result<(), UploadError> {
        let entry = self.entry(id).await?;
        let mut state = entry.state.lock().await;
        if state.status == UploadStatusKind::Active {
            let _ = store.delete(&entry.staging_key).await;
            state.status = UploadStatusKind::Aborted;
        }
        Ok(())
    }

    async fn entry(&self, id: Uuid) -> Result<Arc<UploadEntry>, UploadError> {
        self.sessions
            .read()
            .await
            .get(&id)
            .cloned()
            .ok_or(UploadError::NotFound)
    }
}

fn ensure_active(state: &UploadState) -> Result<(), UploadError> {
    if state.status != UploadStatusKind::Active {
        return Err(UploadError::InvalidState);
    }
    if state.expires_at <= now_seconds() {
        return Err(UploadError::Expired);
    }
    Ok(())
}

async fn status_for(id: Uuid, entry: &UploadEntry) -> UploadStatus {
    let state = entry.state.lock().await;
    UploadStatus {
        id,
        repository: entry.repository.clone(),
        staging_key: entry.staging_key.clone(),
        offset: state.offset,
        expires_at: state.expires_at,
        status: state.status,
    }
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use registry_storage::MemoryObjectStore;

    #[tokio::test]
    async fn ordered_upload_rejects_wrong_offsets_and_wrong_digests() {
        let store: DynObjectStore = Arc::new(MemoryObjectStore::default());
        let manager = UploadManager::new(Duration::from_secs(300));
        let repository = RepositoryName::parse("team/app").expect("repository");
        let created = manager.create(repository).await;
        assert!(matches!(
            manager
                .append(created.id, 1, Bytes::from_static(b"bad"), &store)
                .await,
            Err(UploadError::OffsetMismatch { .. })
        ));
        let status = manager
            .append(created.id, 0, Bytes::from_static(b"hello"), &store)
            .await
            .expect("append");
        assert_eq!(status.offset, 5);
        let wrong = Digest::sha256(b"nope");
        assert!(matches!(
            manager
                .finalize(created.id, wrong, Bytes::new(), &store)
                .await,
            Err(UploadError::DigestMismatch { .. })
        ));
        let hello_digest = Digest::sha256(b"hello");
        assert!(matches!(
            store.get(&blob_key(&hello_digest)).await,
            Err(StorageError::NotFound)
        ));
    }

    #[tokio::test]
    async fn finalization_promotes_verified_bytes() {
        let store: DynObjectStore = Arc::new(MemoryObjectStore::default());
        let manager = UploadManager::new(Duration::from_secs(300));
        let created = manager
            .create(RepositoryName::parse("team/app").expect("repository"))
            .await;
        manager
            .append(created.id, 0, Bytes::from_static(b"hello"), &store)
            .await
            .expect("append");
        let digest = Digest::sha256(b"hello");
        let finalized = manager
            .finalize(created.id, digest.clone(), Bytes::new(), &store)
            .await
            .expect("finalize");
        assert_eq!(finalized.digest, digest);
        assert_eq!(
            store.get(&blob_key(&digest)).await.expect("blob"),
            Bytes::from_static(b"hello")
        );
    }
}
