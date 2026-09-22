mod config;
mod error;
mod routes;
mod state;

pub use config::{AppConfig, ConfigError, StorageBackend};
pub use error::AppError;
pub use routes::router;
pub use state::{AppState, Readiness};

pub async fn build_state(config: AppConfig) -> Result<AppState, AppError> {
    AppState::initialize(config).await
}
