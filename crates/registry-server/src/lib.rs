mod catalog;
mod config;
mod error;
pub mod internal;
mod metrics;
mod routes;
mod sso;
mod state;
mod uploads;

pub use catalog::{
    Catalog, CatalogError, GcReport, RepositorySummary, StorageOverview, StoredManifest, blob_key,
    manifest_key,
};
pub use config::{AppConfig, AppEnvironment, ConfigError, PullMode, StorageBackend};
pub use error::AppError;
pub use metrics::Metrics;
pub use routes::router;
pub use state::{AppState, Readiness};
pub use uploads::{FinalizedUpload, UploadError, UploadManager, UploadStatus, UploadStatusKind};

pub async fn build_state(config: AppConfig) -> Result<AppState, AppError> {
    AppState::initialize(config).await
}

pub use sso::SsoConfig;
