use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use bytes::Bytes;
use registry_core::{Digest, ManifestError, RepositoryName, validate_manifest};
use thiserror::Error;
use tokio::sync::RwLock;

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("manifest is invalid: {0}")]
    Manifest(#[from] ManifestError),
    #[error("manifest or tag was not found")]
    NotFound,
    #[error("invalid tag")]
    InvalidTag,
}

#[derive(Clone, Default)]
pub struct Catalog {
    repositories: Arc<RwLock<BTreeMap<String, RepositoryRecord>>>,
}

#[derive(Clone)]
pub struct StoredManifest {
    pub digest: Digest,
    pub media_type: String,
    pub size: u64,
    pub references: Vec<registry_core::Descriptor>,
}

#[derive(Clone, Default)]
struct RepositoryRecord {
    manifests: BTreeMap<Digest, StoredManifest>,
    tags: BTreeMap<String, Digest>,
    blob_refs: BTreeSet<Digest>,
}

impl Catalog {
    pub async fn publish_manifest(
        &self,
        repository: &RepositoryName,
        reference: &str,
        raw: &Bytes,
        content_type: Option<&str>,
    ) -> Result<StoredManifest, CatalogError> {
        let info = validate_manifest(raw, content_type)?;
        let digest = Digest::sha256(raw);
        let record = StoredManifest {
            digest: digest.clone(),
            media_type: info.media_type,
            size: raw.len() as u64,
            references: info.references.clone(),
        };
        let mut repositories = self.repositories.write().await;
        let repository_record = repositories.entry(repository.to_string()).or_default();
        repository_record
            .manifests
            .insert(digest.clone(), record.clone());
        repository_record.blob_refs.extend(
            info.references
                .into_iter()
                .map(|descriptor| descriptor.digest),
        );
        if !is_digest(reference) {
            if !valid_tag(reference) {
                return Err(CatalogError::InvalidTag);
            }
            repository_record.tags.insert(reference.to_owned(), digest);
        }
        Ok(record)
    }

    pub async fn resolve_manifest(
        &self,
        repository: &RepositoryName,
        reference: &str,
    ) -> Result<StoredManifest, CatalogError> {
        let repositories = self.repositories.read().await;
        let repository_record = repositories
            .get(repository.as_str())
            .ok_or(CatalogError::NotFound)?;
        let digest = if let Ok(digest) = Digest::parse(reference) {
            digest
        } else {
            repository_record
                .tags
                .get(reference)
                .cloned()
                .ok_or(CatalogError::NotFound)?
        };
        repository_record
            .manifests
            .get(&digest)
            .cloned()
            .ok_or(CatalogError::NotFound)
    }

    pub async fn attach_blob(&self, repository: &RepositoryName, digest: Digest) {
        self.repositories
            .write()
            .await
            .entry(repository.to_string())
            .or_default()
            .blob_refs
            .insert(digest);
    }

    pub async fn blob_visible(&self, repository: &RepositoryName, digest: &Digest) -> bool {
        self.repositories
            .read()
            .await
            .get(repository.as_str())
            .is_some_and(|record| record.blob_refs.contains(digest))
    }

    pub async fn list_tags(
        &self,
        repository: &RepositoryName,
        last: Option<&str>,
        limit: usize,
    ) -> Result<(Vec<String>, Option<String>), CatalogError> {
        let repositories = self.repositories.read().await;
        let repository_record = repositories
            .get(repository.as_str())
            .ok_or(CatalogError::NotFound)?;
        let mut tags = repository_record
            .tags
            .keys()
            .filter(|tag| last.is_none_or(|last| tag.as_str() > last))
            .cloned();
        let mut page = Vec::with_capacity(limit);
        for _ in 0..limit {
            if let Some(tag) = tags.next() {
                page.push(tag);
            } else {
                break;
            }
        }
        let next = tags.next().and_then(|_| page.last().cloned());
        Ok((page, next))
    }

    pub async fn manifest_count(&self, repository: &RepositoryName) -> usize {
        self.repositories
            .read()
            .await
            .get(repository.as_str())
            .map_or(0, |record| record.manifests.len())
    }
}

pub fn manifest_key(digest: &Digest) -> String {
    format!("manifests/{}/{}", digest.algorithm(), digest.encoded())
}

pub fn blob_key(digest: &Digest) -> String {
    format!(
        "blobs/{}/{}/{}",
        digest.algorithm(),
        &digest.encoded()[..2],
        digest.encoded()
    )
}

fn is_digest(value: &str) -> bool {
    Digest::parse(value).is_ok()
}

fn valid_tag(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}
