//! Storage boundary used by registry protocol handlers.

mod r2;

use std::{
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use async_trait::async_trait;
use bytes::Bytes;
use sha2::{Digest as _, Sha256};
use thiserror::Error;
use tokio::{fs, io::AsyncWriteExt, sync::RwLock};
use uuid::Uuid;

pub use r2::R2ObjectStore;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectMetadata {
    pub key: String,
    pub content_length: u64,
    pub etag: String,
}

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("object not found")]
    NotFound,
    #[error("invalid object key")]
    InvalidKey,
    #[error("upload offset mismatch: expected {expected}, actual {actual}")]
    OffsetMismatch { expected: u64, actual: u64 },
    #[error("storage operation is not supported")]
    Unsupported,
    #[error("R2 storage operation failed: {0}")]
    R2(String),
    #[error("storage I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

#[async_trait]
pub trait ObjectStore: Send + Sync {
    async fn put(&self, key: &str, body: Bytes) -> Result<ObjectMetadata, StorageError>;
    async fn get(&self, key: &str) -> Result<Bytes, StorageError>;
    async fn head(&self, key: &str) -> Result<ObjectMetadata, StorageError>;
    async fn delete(&self, key: &str) -> Result<(), StorageError>;
    async fn append(
        &self,
        key: &str,
        expected_offset: u64,
        body: Bytes,
    ) -> Result<u64, StorageError>;
    async fn finalize_append(&self, key: &str) -> Result<(), StorageError> {
        let _ = key;
        Ok(())
    }
    async fn health(&self) -> Result<(), StorageError>;
}

pub type DynObjectStore = Arc<dyn ObjectStore>;

fn validate_key(key: &str) -> Result<(), StorageError> {
    if key.is_empty() || key.starts_with('/') || key.contains('\\') {
        return Err(StorageError::InvalidKey);
    }
    let path = Path::new(key);
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(StorageError::InvalidKey);
    }
    Ok(())
}

fn etag(body: &[u8]) -> String {
    let digest = Sha256::digest(body);
    format!("\"{}\"", hex::encode(digest))
}

#[derive(Debug, Clone)]
pub struct LocalFileStore {
    root: Arc<PathBuf>,
}

impl LocalFileStore {
    pub async fn new(root: impl Into<PathBuf>) -> Result<Self, StorageError> {
        let root = root.into();
        fs::create_dir_all(&root).await?;
        Ok(Self {
            root: Arc::new(root),
        })
    }

    fn path_for(&self, key: &str) -> Result<PathBuf, StorageError> {
        validate_key(key)?;
        Ok(self.root.join(key))
    }
}

#[async_trait]
impl ObjectStore for LocalFileStore {
    async fn put(&self, key: &str, body: Bytes) -> Result<ObjectMetadata, StorageError> {
        let path = self.path_for(key)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let temporary = path.with_extension(format!("tmp-{}", Uuid::new_v4()));
        let mut file = fs::File::create(&temporary).await?;
        file.write_all(&body).await?;
        file.sync_all().await?;
        drop(file);
        fs::rename(&temporary, &path).await?;
        Ok(ObjectMetadata {
            key: key.to_owned(),
            content_length: body.len() as u64,
            etag: etag(&body),
        })
    }

    async fn get(&self, key: &str) -> Result<Bytes, StorageError> {
        let path = self.path_for(key)?;
        fs::read(path).await.map(Bytes::from).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound
            } else {
                StorageError::Io(error)
            }
        })
    }

    async fn head(&self, key: &str) -> Result<ObjectMetadata, StorageError> {
        let body = self.get(key).await?;
        Ok(ObjectMetadata {
            key: key.to_owned(),
            content_length: body.len() as u64,
            etag: etag(&body),
        })
    }

    async fn delete(&self, key: &str) -> Result<(), StorageError> {
        let path = self.path_for(key)?;
        fs::remove_file(path).await.map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound
            } else {
                StorageError::Io(error)
            }
        })
    }

    async fn append(
        &self,
        key: &str,
        expected_offset: u64,
        body: Bytes,
    ) -> Result<u64, StorageError> {
        let path = self.path_for(key)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let current = match fs::metadata(&path).await {
            Ok(metadata) => metadata.len(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
            Err(error) => return Err(StorageError::Io(error)),
        };
        if current != expected_offset {
            return Err(StorageError::OffsetMismatch {
                expected: expected_offset,
                actual: current,
            });
        }
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await?;
        file.write_all(&body).await?;
        file.sync_data().await?;
        Ok(current + body.len() as u64)
    }

    async fn health(&self) -> Result<(), StorageError> {
        fs::metadata(&*self.root)
            .await
            .map(|_| ())
            .map_err(StorageError::Io)
    }
}

#[derive(Debug, Default, Clone)]
pub struct MemoryObjectStore {
    objects: Arc<RwLock<std::collections::BTreeMap<String, Bytes>>>,
}

#[async_trait]
impl ObjectStore for MemoryObjectStore {
    async fn put(&self, key: &str, body: Bytes) -> Result<ObjectMetadata, StorageError> {
        validate_key(key)?;
        self.objects
            .write()
            .await
            .insert(key.to_owned(), body.clone());
        Ok(ObjectMetadata {
            key: key.to_owned(),
            content_length: body.len() as u64,
            etag: etag(&body),
        })
    }

    async fn get(&self, key: &str) -> Result<Bytes, StorageError> {
        validate_key(key)?;
        self.objects
            .read()
            .await
            .get(key)
            .cloned()
            .ok_or(StorageError::NotFound)
    }

    async fn head(&self, key: &str) -> Result<ObjectMetadata, StorageError> {
        let body = self.get(key).await?;
        Ok(ObjectMetadata {
            key: key.to_owned(),
            content_length: body.len() as u64,
            etag: etag(&body),
        })
    }

    async fn delete(&self, key: &str) -> Result<(), StorageError> {
        validate_key(key)?;
        self.objects
            .write()
            .await
            .remove(key)
            .map(|_| ())
            .ok_or(StorageError::NotFound)
    }

    async fn append(
        &self,
        key: &str,
        expected_offset: u64,
        body: Bytes,
    ) -> Result<u64, StorageError> {
        validate_key(key)?;
        let mut objects = self.objects.write().await;
        let current = objects.get(key).map_or(0, Bytes::len) as u64;
        if current != expected_offset {
            return Err(StorageError::OffsetMismatch {
                expected: expected_offset,
                actual: current,
            });
        }
        let mut combined = Vec::with_capacity(current as usize + body.len());
        if let Some(existing) = objects.get(key) {
            combined.extend_from_slice(existing);
        }
        combined.extend_from_slice(&body);
        let next = combined.len() as u64;
        objects.insert(key.to_owned(), Bytes::from(combined));
        Ok(next)
    }

    async fn health(&self) -> Result<(), StorageError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn memory_store_round_trips_and_rejects_traversal() {
        let store = MemoryObjectStore::default();
        let metadata = store
            .put("blobs/sha256/example", Bytes::from_static(b"hello"))
            .await
            .expect("put");
        assert_eq!(metadata.content_length, 5);
        assert_eq!(
            store.get("blobs/sha256/example").await.expect("get"),
            Bytes::from_static(b"hello")
        );
        assert!(matches!(
            store.put("../escape", Bytes::new()).await,
            Err(StorageError::InvalidKey)
        ));
        assert_eq!(
            store
                .append("blobs/sha256/example", 5, Bytes::from_static(b" world"))
                .await
                .expect("append"),
            11
        );
        assert!(matches!(
            store
                .append("blobs/sha256/example", 5, Bytes::from_static(b"!"))
                .await,
            Err(StorageError::OffsetMismatch { .. })
        ));
    }

    #[tokio::test]
    async fn local_store_writes_atomically_visible_objects() {
        let root = std::env::temp_dir().join(format!("knotree-registry-{}", Uuid::new_v4()));
        let store = LocalFileStore::new(&root).await.expect("store");
        store
            .put("objects/a", Bytes::from_static(b"data"))
            .await
            .expect("put");
        assert_eq!(
            store.get("objects/a").await.expect("get"),
            Bytes::from_static(b"data")
        );
        store.delete("objects/a").await.expect("delete");
        assert!(matches!(
            store.get("objects/a").await,
            Err(StorageError::NotFound)
        ));
        fs::remove_dir_all(root).await.expect("cleanup");
    }
}
