//! Protocol-independent domain types for the registry.

pub mod digest;
pub mod manifest;
pub mod repository;
pub mod scope;

pub use digest::{Digest, DigestError};
pub use manifest::{Descriptor, ManifestError, ManifestInfo, validate_manifest};
pub use repository::{RepositoryError, RepositoryName};
pub use scope::{Action, RepositoryScope};
