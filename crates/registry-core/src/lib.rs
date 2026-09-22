//! Protocol-independent domain types for the registry.

pub mod digest;
pub mod manifest;
pub mod repository;
pub mod scope;

pub use digest::{Digest, DigestError};
pub use manifest::{
    DOCKER_MANIFEST, DOCKER_MANIFEST_LIST, Descriptor, ManifestError, ManifestInfo,
    OCI_ARTIFACT_MANIFEST, OCI_IMAGE_INDEX, OCI_IMAGE_MANIFEST, validate_manifest,
};
pub use repository::{RepositoryError, RepositoryName};
pub use scope::{Action, RepositoryScope};
