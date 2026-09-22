use std::{sync::Arc, time::Instant};

use registry_auth::AuthService;
use registry_db::Database;
use registry_storage::{DynObjectStore, LocalFileStore, MemoryObjectStore};

use crate::{AppConfig, AppError, StorageBackend};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub store: DynObjectStore,
    pub database: Option<Database>,
    pub auth: Arc<AuthService>,
    pub started_at: Instant,
}

#[derive(Debug, serde::Serialize)]
pub struct Readiness {
    pub status: &'static str,
    pub storage: &'static str,
    pub database: &'static str,
}

impl AppState {
    pub async fn initialize(config: AppConfig) -> Result<Self, AppError> {
        let store: DynObjectStore = match config.storage_backend {
            StorageBackend::Local => Arc::new(LocalFileStore::new(&config.storage_root).await?),
            StorageBackend::Memory => Arc::new(MemoryObjectStore::default()),
            StorageBackend::R2 => unreachable!("validated configuration cannot select R2"),
        };
        let database = match config.database_url.as_deref() {
            Some(url) => {
                let database = Database::connect(url, config.database_max_connections).await?;
                database.migrate().await?;
                Some(database)
            }
            None if config.require_database => {
                return Err(crate::ConfigError::MissingDatabase.into());
            }
            None => None,
        };
        let auth = Arc::new(AuthService::new(
            config.token_issuer.clone(),
            config.token_service.clone(),
            config.token_ttl_seconds,
        ));
        if let (Some(username), Some(password)) = (
            config.bootstrap_admin_username.as_deref(),
            config.bootstrap_admin_password.as_deref(),
        ) {
            auth.bootstrap_admin(username, password)
                .await
                .map_err(AppError::Auth)?;
        }
        Ok(Self {
            config: Arc::new(config),
            store,
            database,
            auth,
            started_at: Instant::now(),
        })
    }

    pub async fn readiness(&self) -> Readiness {
        let storage = if self.store.health().await.is_ok() {
            "ok"
        } else {
            "failed"
        };
        let database = match &self.database {
            Some(database) if database.ping().await.is_ok() => "ok",
            Some(_) => "failed",
            None if self.config.require_database => "missing",
            None => "skipped",
        };
        let ready = storage == "ok" && database != "failed" && database != "missing";
        Readiness {
            status: if ready { "ok" } else { "not_ready" },
            storage,
            database,
        }
    }
}
