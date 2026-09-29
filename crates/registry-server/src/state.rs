use std::{
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use registry_auth::AuthService;
use registry_db::Database;
use registry_events::{
    DeliveryDecision, EventLog, PendingWebhookDelivery, RetryPolicy, WebhookRegistry,
};
use registry_storage::{DynObjectStore, LocalFileStore, MemoryObjectStore, R2ObjectStore};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::task::JoinHandle;

use crate::{AppConfig, AppError, Catalog, Metrics, StorageBackend, UploadManager};

const RUNTIME_SNAPSHOT_KEY: &str = "registry-server";

#[derive(Debug, Serialize, Deserialize)]
struct RuntimeSnapshot {
    version: u32,
    auth: Value,
    catalog: Value,
    uploads: Value,
    events: Value,
    webhooks: Value,
}

#[derive(Clone)]
pub struct AppState {
    pub(crate) sso_attempts: Arc<tokio::sync::Mutex<crate::sso::LoginAttempts>>,
    pub config: Arc<AppConfig>,
    pub store: DynObjectStore,
    pub database: Option<Database>,
    pub auth: Arc<AuthService>,
    pub catalog: Arc<Catalog>,
    pub uploads: Arc<UploadManager>,
    pub events: EventLog,
    pub webhooks: WebhookRegistry,
    pub metrics: Metrics,
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
            StorageBackend::R2 => Arc::new(
                R2ObjectStore::new(
                    config
                        .r2_endpoint
                        .as_deref()
                        .expect("validated R2 endpoint"),
                    config.r2_bucket.as_deref().expect("validated R2 bucket"),
                    config
                        .r2_access_key_id
                        .as_deref()
                        .expect("validated R2 access key"),
                    config
                        .r2_secret_access_key
                        .as_deref()
                        .expect("validated R2 secret key"),
                    &config.r2_region,
                )
                .await?,
            ),
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
        let persisted = match &database {
            Some(database) => database
                .load_snapshot(RUNTIME_SNAPSHOT_KEY)
                .await?
                .map(|value| {
                    serde_json::from_value::<RuntimeSnapshot>(value)
                        .map_err(|error| AppError::State(error.to_string()))
                })
                .transpose()?,
            None => None,
        };
        if persisted
            .as_ref()
            .is_some_and(|snapshot| snapshot.version != 1)
        {
            return Err(AppError::State(
                "unsupported runtime snapshot version".to_owned(),
            ));
        }
        let auth = Arc::new(match persisted.as_ref() {
            Some(snapshot) => AuthService::from_snapshot(
                config.token_issuer.clone(),
                config.token_service.clone(),
                config.token_ttl_seconds,
                snapshot.auth.clone(),
            )
            .map_err(AppError::Auth)?,
            None => AuthService::new(
                config.token_issuer.clone(),
                config.token_service.clone(),
                config.token_ttl_seconds,
            ),
        });
        if !auth.has_users().await {
            if let (Some(username), Some(password)) = (
                config.bootstrap_admin_username.as_deref(),
                config.bootstrap_admin_password.as_deref(),
            ) {
                auth.bootstrap_admin(username, password)
                    .await
                    .map_err(AppError::Auth)?;
            } else if config.environment == crate::AppEnvironment::Production {
                return Err(crate::ConfigError::BootstrapAdminRequired.into());
            }
        }
        let catalog = Arc::new(match persisted.as_ref() {
            Some(snapshot) => {
                Catalog::from_snapshot(snapshot.catalog.clone()).map_err(AppError::Catalog)?
            }
            None => Catalog::default(),
        });
        let upload_ttl = Duration::from_secs(24 * 60 * 60);
        let uploads = Arc::new(match persisted.as_ref() {
            Some(snapshot) => UploadManager::from_snapshot(
                upload_ttl,
                snapshot.uploads.clone(),
                config.storage_backend == StorageBackend::Local,
            )
            .map_err(|error| AppError::State(error.to_string()))?,
            None => UploadManager::new(upload_ttl),
        });
        let events = match persisted.as_ref() {
            Some(snapshot) => EventLog::from_snapshot(snapshot.events.clone())
                .map_err(|error| AppError::State(error.to_string()))?,
            None => EventLog::default(),
        };
        let webhooks = match persisted.as_ref() {
            Some(snapshot) => WebhookRegistry::from_snapshot(snapshot.webhooks.clone())
                .map_err(|error| AppError::State(error.to_string()))?,
            None => WebhookRegistry::default(),
        };
        let state = Self {
            sso_attempts: Arc::new(tokio::sync::Mutex::new(crate::sso::LoginAttempts::default())),
            config: Arc::new(config),
            store,
            database,
            auth,
            catalog,
            uploads,
            events,
            webhooks,
            metrics: Metrics::default(),
            started_at: Instant::now(),
        };
        state.persist_runtime_state().await?;
        Ok(state)
    }

    pub async fn persist_runtime_state(&self) -> Result<(), AppError> {
        let Some(database) = &self.database else {
            return Ok(());
        };
        let snapshot = RuntimeSnapshot {
            version: 1,
            auth: self.auth.snapshot().await.map_err(AppError::Auth)?,
            catalog: self.catalog.snapshot().await.map_err(AppError::Catalog)?,
            uploads: self
                .uploads
                .snapshot()
                .await
                .map_err(|error| AppError::State(error.to_string()))?,
            events: self
                .events
                .snapshot()
                .await
                .map_err(|error| AppError::State(error.to_string()))?,
            webhooks: self
                .webhooks
                .snapshot()
                .await
                .map_err(|error| AppError::State(error.to_string()))?,
        };
        let value =
            serde_json::to_value(snapshot).map_err(|error| AppError::State(error.to_string()))?;
        database
            .save_snapshot(RUNTIME_SNAPSHOT_KEY, &value)
            .await
            .map_err(AppError::Database)
    }

    pub async fn record_event(&self, event: registry_events::RegistryEvent) {
        self.events.record(event.clone()).await;
        let queued = self.webhooks.enqueue(event).await;
        if queued > 0 {
            tracing::debug!(deliveries = queued, "queued webhook deliveries");
        }
    }

    /// Start the single-instance webhook outbox worker. The handle should be kept
    /// alive for the lifetime of the server and is aborted with the Tokio runtime.
    pub fn start_webhook_delivery_worker(&self) -> JoinHandle<()> {
        let state = self.clone();
        tokio::spawn(async move {
            let client = match reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .redirect(reqwest::redirect::Policy::none())
                .user_agent("knotree-registry-webhook/1")
                .build()
            {
                Ok(client) => client,
                Err(error) => {
                    tracing::error!(%error, "webhook HTTP client initialization failed");
                    return;
                }
            };
            let policy = RetryPolicy::default();
            loop {
                let now = now_seconds();
                let deliveries = state.webhooks.due_deliveries(now).await;
                for pending in deliveries {
                    if !state.webhooks.is_enabled(pending.webhook_id).await {
                        state.webhooks.complete(pending.delivery.id).await;
                        continue;
                    }
                    let attempt = pending.attempt.saturating_add(1);
                    let result = send_webhook(&client, &pending, attempt).await;
                    let status = result.as_ref().copied().map_err(|_| "request failed");
                    match policy.next_attempt(attempt, status, now) {
                        DeliveryDecision::Succeeded => {
                            state.webhooks.complete(pending.delivery.id).await;
                            tracing::debug!(
                                webhook_id = %pending.webhook_id,
                                delivery_id = %pending.delivery.id,
                                attempt,
                                "webhook delivered"
                            );
                        }
                        DeliveryDecision::Retry {
                            attempt,
                            not_before,
                            status,
                        } => {
                            state
                                .webhooks
                                .retry(pending.delivery.id, attempt, not_before)
                                .await;
                            tracing::warn!(
                                webhook_id = %pending.webhook_id,
                                delivery_id = %pending.delivery.id,
                                attempt,
                                ?status,
                                not_before,
                                error = result.as_ref().err().map(String::as_str),
                                "webhook delivery will retry"
                            );
                        }
                        DeliveryDecision::Failed { status } => {
                            state.webhooks.complete(pending.delivery.id).await;
                            tracing::error!(
                                webhook_id = %pending.webhook_id,
                                delivery_id = %pending.delivery.id,
                                attempt,
                                ?status,
                                error = result.as_ref().err().map(String::as_str),
                                "webhook delivery failed permanently"
                            );
                        }
                    }
                    if state.database.is_some()
                        && let Err(error) = state.persist_runtime_state().await
                    {
                        tracing::error!(%error, "webhook outbox persistence failed");
                    }
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
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

async fn send_webhook(
    client: &reqwest::Client,
    pending: &PendingWebhookDelivery,
    attempt: u32,
) -> Result<u16, String> {
    let delivery = &pending.delivery;
    let event_kind = serde_json::to_string(&delivery.event.kind)
        .unwrap_or_else(|_| "unknown".to_owned())
        .trim_matches('"')
        .to_owned();
    let response = client
        .post(&pending.url)
        .header("content-type", "application/json")
        .header("x-knotree-event", event_kind)
        .header("x-knotree-delivery", delivery.id.to_string())
        .header("x-knotree-timestamp", delivery.timestamp.to_string())
        .header("x-knotree-signature", &delivery.signature)
        .header("x-knotree-attempt", attempt.to_string())
        .body(delivery.body.clone())
        .send()
        .await
        .map_err(|error| error.to_string())?;
    Ok(response.status().as_u16())
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
