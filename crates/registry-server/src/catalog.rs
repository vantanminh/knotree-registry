use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use bytes::Bytes;
use registry_core::{Digest, ManifestError, RepositoryName, validate_manifest};
use registry_storage::{DynObjectStore, StorageError};
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
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),
    #[error("catalog state serialization failed: {0}")]
    State(#[from] serde_json::Error),
}

#[derive(Clone, Default)]
pub struct Catalog {
    state: Arc<RwLock<CatalogState>>,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct CatalogState {
    repositories: BTreeMap<String, RepositoryRecord>,
    blob_created_at: BTreeMap<Digest, u64>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct StoredManifest {
    pub digest: Digest,
    pub media_type: String,
    pub size: u64,
    pub references: Vec<registry_core::Descriptor>,
    pub subject: Option<registry_core::Descriptor>,
    pub created_at: u64,
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
struct RepositoryRecord {
    manifests: BTreeMap<Digest, StoredManifest>,
    tags: BTreeMap<String, Digest>,
    blob_refs: BTreeSet<Digest>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct GcReport {
    pub dry_run: bool,
    pub manifests: usize,
    pub blobs: usize,
    pub reclaimed_bytes: u64,
    pub failures: Vec<String>,
}

impl Catalog {
    pub fn from_snapshot(value: serde_json::Value) -> Result<Self, CatalogError> {
        let state = serde_json::from_value(value)?;
        Ok(Self {
            state: Arc::new(RwLock::new(state)),
        })
    }

    pub async fn snapshot(&self) -> Result<serde_json::Value, CatalogError> {
        Ok(serde_json::to_value(&*self.state.read().await)?)
    }

    pub async fn publish_manifest(
        &self,
        repository: &RepositoryName,
        reference: &str,
        raw: &Bytes,
        content_type: Option<&str>,
    ) -> Result<StoredManifest, CatalogError> {
        if !is_digest(reference) && !valid_tag(reference) {
            return Err(CatalogError::InvalidTag);
        }
        let info = validate_manifest(raw, content_type)?;
        let digest = Digest::sha256(raw);
        let created_at = now_seconds();
        let record = StoredManifest {
            digest: digest.clone(),
            media_type: info.media_type,
            size: raw.len() as u64,
            references: info.references.clone(),
            subject: info.subject,
            created_at,
        };
        let referenced_digests = record
            .references
            .iter()
            .map(|descriptor| descriptor.digest.clone())
            .collect::<Vec<_>>();
        let mut state = self.state.write().await;
        for digest in &referenced_digests {
            state
                .blob_created_at
                .entry(digest.clone())
                .or_insert(created_at);
        }
        let repository_record = state
            .repositories
            .entry(repository.to_string())
            .or_default();
        repository_record
            .manifests
            .insert(digest.clone(), record.clone());
        repository_record.blob_refs.extend(referenced_digests);
        if !is_digest(reference) {
            repository_record.tags.insert(reference.to_owned(), digest);
        }
        Ok(record)
    }

    pub async fn resolve_manifest(
        &self,
        repository: &RepositoryName,
        reference: &str,
    ) -> Result<StoredManifest, CatalogError> {
        let state = self.state.read().await;
        let repository_record = state
            .repositories
            .get(repository.as_str())
            .ok_or(CatalogError::NotFound)?;
        let digest = resolve_digest(repository_record, reference)?;
        repository_record
            .manifests
            .get(&digest)
            .cloned()
            .ok_or(CatalogError::NotFound)
    }

    pub async fn delete_manifest(
        &self,
        repository: &RepositoryName,
        reference: &str,
    ) -> Result<StoredManifest, CatalogError> {
        let mut state = self.state.write().await;
        let repository_record = state
            .repositories
            .get_mut(repository.as_str())
            .ok_or(CatalogError::NotFound)?;
        let digest = resolve_digest(repository_record, reference)?;
        let removed = repository_record
            .manifests
            .remove(&digest)
            .ok_or(CatalogError::NotFound)?;
        repository_record.tags.retain(|_, value| value != &digest);
        repository_record.blob_refs = repository_record
            .manifests
            .values()
            .flat_map(|manifest| {
                manifest
                    .references
                    .iter()
                    .map(|descriptor| descriptor.digest.clone())
            })
            .collect();
        Ok(removed)
    }

    pub async fn attach_blob(&self, repository: &RepositoryName, digest: Digest) {
        let mut state = self.state.write().await;
        state
            .repositories
            .entry(repository.to_string())
            .or_default()
            .blob_refs
            .insert(digest.clone());
        state
            .blob_created_at
            .entry(digest)
            .or_insert_with(now_seconds);
    }

    pub async fn blob_visible(&self, repository: &RepositoryName, digest: &Digest) -> bool {
        self.state
            .read()
            .await
            .repositories
            .get(repository.as_str())
            .is_some_and(|record| record.blob_refs.contains(digest))
    }

    pub async fn list_tags(
        &self,
        repository: &RepositoryName,
        last: Option<&str>,
        limit: usize,
    ) -> Result<(Vec<String>, Option<String>), CatalogError> {
        let state = self.state.read().await;
        let repository_record = state
            .repositories
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

    pub async fn referrers(
        &self,
        repository: &RepositoryName,
        subject: &Digest,
    ) -> Result<Vec<StoredManifest>, CatalogError> {
        let state = self.state.read().await;
        let repository_record = state
            .repositories
            .get(repository.as_str())
            .ok_or(CatalogError::NotFound)?;
        Ok(repository_record
            .manifests
            .values()
            .filter(|manifest| {
                manifest
                    .subject
                    .as_ref()
                    .is_some_and(|value| &value.digest == subject)
            })
            .cloned()
            .collect())
    }

    pub async fn garbage_collect(
        &self,
        store: &DynObjectStore,
        grace: Duration,
        dry_run: bool,
    ) -> Result<GcReport, CatalogError> {
        let cutoff = now_seconds().saturating_sub(grace.as_secs());
        let mut state = self.state.write().await;
        let mut live_manifests = BTreeSet::new();
        for repository in state.repositories.values() {
            live_manifests.extend(repository.tags.values().cloned());
        }
        let mut changed = true;
        while changed {
            changed = false;
            let snapshot = live_manifests.clone();
            for repository in state.repositories.values() {
                for digest in &snapshot {
                    if let Some(manifest) = repository.manifests.get(digest) {
                        for descriptor in &manifest.references {
                            if repository.manifests.contains_key(&descriptor.digest)
                                && live_manifests.insert(descriptor.digest.clone())
                            {
                                changed = true;
                            }
                        }
                    }
                }
                for manifest in repository.manifests.values() {
                    if manifest
                        .subject
                        .as_ref()
                        .is_some_and(|subject| live_manifests.contains(&subject.digest))
                        && live_manifests.insert(manifest.digest.clone())
                    {
                        changed = true;
                    }
                }
            }
        }
        let mut live_blobs = BTreeSet::new();
        for repository in state.repositories.values() {
            for digest in &live_manifests {
                if let Some(manifest) = repository.manifests.get(digest) {
                    live_blobs.extend(
                        manifest
                            .references
                            .iter()
                            .map(|descriptor| descriptor.digest.clone()),
                    );
                }
            }
        }
        let manifest_candidates = state
            .repositories
            .values()
            .flat_map(|repository| repository.manifests.values())
            .filter(|manifest| {
                !live_manifests.contains(&manifest.digest) && manifest.created_at <= cutoff
            })
            .map(|manifest| (manifest.digest.clone(), manifest.size))
            .collect::<BTreeMap<_, _>>();
        let blob_candidates = state
            .blob_created_at
            .iter()
            .filter(|(digest, created_at)| !live_blobs.contains(*digest) && **created_at <= cutoff)
            .map(|(digest, _)| digest.clone())
            .collect::<Vec<_>>();
        let mut report = GcReport {
            dry_run,
            manifests: manifest_candidates.len(),
            blobs: blob_candidates.len(),
            reclaimed_bytes: manifest_candidates.values().sum(),
            failures: Vec::new(),
        };
        if dry_run {
            return Ok(report);
        }
        for digest in manifest_candidates.keys() {
            match store.delete(&crate::manifest_key(digest)).await {
                Ok(()) | Err(StorageError::NotFound) => {}
                Err(error) => report.failures.push(format!("manifest {digest}: {error}")),
            }
        }
        for digest in &blob_candidates {
            match store.delete(&crate::blob_key(digest)).await {
                Ok(()) | Err(StorageError::NotFound) => {}
                Err(error) => report.failures.push(format!("blob {digest}: {error}")),
            }
        }
        if report.failures.is_empty() {
            for repository in state.repositories.values_mut() {
                repository
                    .manifests
                    .retain(|digest, _| !manifest_candidates.contains_key(digest));
                repository
                    .tags
                    .retain(|_, digest| !manifest_candidates.contains_key(digest));
                repository
                    .blob_refs
                    .retain(|digest| !blob_candidates.contains(digest));
            }
            state
                .blob_created_at
                .retain(|digest, _| !blob_candidates.contains(digest));
        }
        Ok(report)
    }

    pub async fn manifest_count(&self, repository: &RepositoryName) -> usize {
        self.state
            .read()
            .await
            .repositories
            .get(repository.as_str())
            .map_or(0, |record| record.manifests.len())
    }

    pub async fn repositories(&self) -> Vec<String> {
        self.state
            .read()
            .await
            .repositories
            .keys()
            .cloned()
            .collect()
    }
}

fn resolve_digest(repository: &RepositoryRecord, reference: &str) -> Result<Digest, CatalogError> {
    if let Ok(digest) = Digest::parse(reference) {
        return Ok(digest);
    }
    repository
        .tags
        .get(reference)
        .cloned()
        .ok_or(CatalogError::NotFound)
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

    fn image_manifest(extra: &str) -> Bytes {
        Bytes::from(format!(
            r#"{{"schemaVersion":2,"mediaType":"application/vnd.oci.image.manifest.v1+json","config":{{"mediaType":"application/vnd.oci.image.config.v1+json","digest":"sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","size":0}},"layers":[]}}{extra}"#
        ))
    }

    #[tokio::test]
    async fn gc_preserves_tag_reachable_content_and_removes_orphans() {
        let catalog = Catalog::default();
        let store: DynObjectStore = Arc::new(MemoryObjectStore::default());
        let repository = RepositoryName::parse("team/app").expect("repository");
        let live = image_manifest("");
        let orphan = image_manifest(" ");
        let live_record = catalog
            .publish_manifest(&repository, "latest", &live, None)
            .await
            .expect("live");
        let orphan_record = catalog
            .publish_manifest(
                &repository,
                &Digest::sha256(&orphan).to_string(),
                &orphan,
                None,
            )
            .await
            .expect("orphan");
        store
            .put(&manifest_key(&live_record.digest), live)
            .await
            .expect("live object");
        store
            .put(&manifest_key(&orphan_record.digest), orphan)
            .await
            .expect("orphan object");
        let dry = catalog
            .garbage_collect(&store, Duration::ZERO, true)
            .await
            .expect("dry run");
        assert_eq!(dry.manifests, 1);
        assert_eq!(catalog.manifest_count(&repository).await, 2);
        let report = catalog
            .garbage_collect(&store, Duration::ZERO, false)
            .await
            .expect("gc");
        assert_eq!(report.manifests, 1);
        assert_eq!(catalog.manifest_count(&repository).await, 1);
        assert!(store.get(&manifest_key(&live_record.digest)).await.is_ok());
        assert!(matches!(
            store.get(&manifest_key(&orphan_record.digest)).await,
            Err(StorageError::NotFound)
        ));
    }

    #[tokio::test]
    async fn snapshot_round_trip_preserves_manifest_and_tag_indexes() {
        let catalog = Catalog::default();
        let repository = RepositoryName::parse("team/app").expect("repository");
        let manifest = image_manifest("");
        let published = catalog
            .publish_manifest(&repository, "latest", &manifest, None)
            .await
            .expect("publish");
        let restored =
            Catalog::from_snapshot(catalog.snapshot().await.expect("snapshot")).expect("restore");
        let resolved = restored
            .resolve_manifest(&repository, "latest")
            .await
            .expect("resolve");
        assert_eq!(resolved.digest, published.digest);
        assert_eq!(restored.manifest_count(&repository).await, 1);
    }
}
