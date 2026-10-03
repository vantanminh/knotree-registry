//! Durable event and webhook-delivery primitives shared by the server and agent.

use std::{
    collections::{BTreeSet, HashMap, VecDeque},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use thiserror::Error;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    ManifestPushed,
    TagUpdated,
    ManifestDeleted,
    TokenCreated,
    TokenRevoked,
    /// A Cloud integration grant was revoked by its owner.
    GrantRevoked,
    PasswordChanged,
    TwoFactorEnabled,
    TwoFactorDisabled,
    LoginSucceeded,
    LoginFailed,
    GarbageCollection,
    WebhookCreated,
    WebhookDisabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistryEvent {
    pub id: Uuid,
    pub kind: EventKind,
    pub occurred_at: u64,
    pub actor: Option<String>,
    pub repository: Option<String>,
    pub tag: Option<String>,
    pub digest: Option<String>,
    pub metadata: serde_json::Value,
}

impl RegistryEvent {
    pub fn new(kind: EventKind) -> Self {
        Self {
            id: Uuid::new_v4(),
            kind,
            occurred_at: now_seconds(),
            actor: None,
            repository: None,
            tag: None,
            digest: None,
            metadata: serde_json::Value::Object(serde_json::Map::new()),
        }
    }
}

#[derive(Clone, Default)]
pub struct EventLog {
    events: Arc<RwLock<Vec<RegistryEvent>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebhookSummary {
    pub id: Uuid,
    pub url: String,
    pub events: BTreeSet<EventKind>,
    pub enabled: bool,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebhookCreated {
    pub webhook: WebhookSummary,
    pub secret: String,
}

/// Stable ID of the operator-managed Knotree Cloud endpoint.
pub const CLOUD_WEBHOOK_ID: Uuid = Uuid::from_u128(0x3de45586_d014_4c43_ba11_10f019bc94f5);

/// Delivery attempts kept for the dashboard, newest last.
pub const DELIVERY_HISTORY_LIMIT: usize = 100;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryOutcome {
    Delivered,
    Retrying,
    Failed,
}

/// One webhook delivery attempt as shown to operators. Never contains the
/// signing secret or the signed body.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeliveryRecord {
    pub delivery_id: Uuid,
    pub webhook_id: Uuid,
    pub event: EventKind,
    pub repository: Option<String>,
    pub tag: Option<String>,
    pub digest: Option<String>,
    pub attempt: u32,
    pub status: Option<u16>,
    pub outcome: DeliveryOutcome,
    pub error: Option<String>,
    pub at: u64,
}

#[derive(Clone, Default)]
pub struct WebhookRegistry {
    endpoints: Arc<RwLock<HashMap<Uuid, WebhookEndpoint>>>,
    deliveries: Arc<RwLock<HashMap<Uuid, PendingWebhookDelivery>>>,
    history: Arc<RwLock<VecDeque<DeliveryRecord>>>,
}

#[derive(Clone, Serialize, Deserialize)]
struct WebhookEndpoint {
    summary: WebhookSummary,
    secret: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingWebhookDelivery {
    pub webhook_id: Uuid,
    pub url: String,
    pub delivery: WebhookDelivery,
    pub attempt: u32,
    pub next_attempt_at: u64,
}

#[derive(Serialize, Deserialize)]
struct WebhookSnapshot {
    endpoints: Vec<WebhookEndpoint>,
    #[serde(default)]
    deliveries: Vec<PendingWebhookDelivery>,
    #[serde(default)]
    history: Vec<DeliveryRecord>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum WebhookSnapshotInput {
    Current(WebhookSnapshot),
    Legacy(Vec<WebhookEndpoint>),
}

impl WebhookRegistry {
    /// Operator-managed Cloud integration: stable ID and GitHub-supplied signing
    /// secret, applied after restoring persisted state on every startup.
    pub async fn configure_cloud(&self, url: String, secret: String) -> Result<(), WebhookError> {
        let parsed = url::Url::parse(&url).map_err(|_| WebhookError::InvalidUrl)?;
        if parsed.scheme() != "https"
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || secret.len() < 32
            || secret.chars().any(char::is_control)
        {
            return Err(WebhookError::InvalidUrl);
        }
        let id = CLOUD_WEBHOOK_ID;
        let mut endpoints = self.endpoints.write().await;
        let created_at = endpoints
            .get(&id)
            .map(|e| e.summary.created_at)
            .unwrap_or_else(now_seconds);
        for endpoint in endpoints.values_mut() {
            if endpoint.summary.id != id && endpoint.summary.url == url {
                endpoint.summary.enabled = false;
            }
        }
        endpoints.insert(
            id,
            WebhookEndpoint {
                summary: WebhookSummary {
                    id,
                    url,
                    events: [EventKind::TagUpdated, EventKind::GrantRevoked]
                        .into_iter()
                        .collect(),
                    enabled: true,
                    created_at,
                },
                secret,
            },
        );
        Ok(())
    }

    pub fn from_snapshot(value: serde_json::Value) -> Result<Self, serde_json::Error> {
        let snapshot = match serde_json::from_value::<WebhookSnapshotInput>(value)? {
            WebhookSnapshotInput::Current(snapshot) => snapshot,
            WebhookSnapshotInput::Legacy(endpoints) => WebhookSnapshot {
                endpoints,
                deliveries: Vec::new(),
                history: Vec::new(),
            },
        };
        let endpoints = snapshot
            .endpoints
            .into_iter()
            .map(|endpoint| (endpoint.summary.id, endpoint))
            .collect();
        let deliveries = snapshot
            .deliveries
            .into_iter()
            .map(|delivery| (delivery.delivery.id, delivery))
            .collect();
        let mut history: VecDeque<DeliveryRecord> = snapshot.history.into();
        while history.len() > DELIVERY_HISTORY_LIMIT {
            history.pop_front();
        }
        Ok(Self {
            endpoints: Arc::new(RwLock::new(endpoints)),
            deliveries: Arc::new(RwLock::new(deliveries)),
            history: Arc::new(RwLock::new(history)),
        })
    }

    pub async fn snapshot(&self) -> Result<serde_json::Value, serde_json::Error> {
        serde_json::to_value(WebhookSnapshot {
            endpoints: self.endpoints.read().await.values().cloned().collect(),
            deliveries: self.deliveries.read().await.values().cloned().collect(),
            history: self.history.read().await.iter().cloned().collect(),
        })
    }

    pub async fn create(
        &self,
        url: String,
        events: BTreeSet<EventKind>,
    ) -> Result<WebhookCreated, WebhookError> {
        let parsed_url = url::Url::parse(&url);
        if !parsed_url.is_ok_and(|url| {
            matches!(url.scheme(), "https" | "http")
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
        }) {
            return Err(WebhookError::InvalidUrl);
        }
        let id = Uuid::new_v4();
        let secret = format!("whsec_{}_{}", Uuid::new_v4(), Uuid::new_v4());
        let summary = WebhookSummary {
            id,
            url,
            events,
            enabled: true,
            created_at: now_seconds(),
        };
        self.endpoints.write().await.insert(
            id,
            WebhookEndpoint {
                summary: summary.clone(),
                secret: secret.clone(),
            },
        );
        Ok(WebhookCreated {
            webhook: summary,
            secret,
        })
    }

    pub async fn list(&self) -> Vec<WebhookSummary> {
        self.endpoints
            .read()
            .await
            .values()
            .map(|endpoint| endpoint.summary.clone())
            .collect()
    }

    pub async fn disable(&self, id: Uuid) -> bool {
        let disabled = self
            .endpoints
            .write()
            .await
            .get_mut(&id)
            .map(|endpoint| endpoint.summary.enabled = false)
            .is_some();
        if disabled {
            self.deliveries
                .write()
                .await
                .retain(|_, delivery| delivery.webhook_id != id);
        }
        disabled
    }

    pub async fn signer(&self, id: Uuid) -> Option<WebhookSigner> {
        let endpoint = self.endpoints.read().await.get(&id).cloned()?;
        WebhookSigner::new(endpoint.secret, Duration::from_secs(300)).ok()
    }

    /// Add one signed, idempotent outbox entry for every enabled matching endpoint.
    pub async fn enqueue(&self, event: RegistryEvent) -> usize {
        let timestamp = now_seconds();
        let endpoints = self.endpoints.read().await;
        let mut deliveries = self.deliveries.write().await;
        let mut count = 0;
        for endpoint in endpoints.values() {
            if !endpoint.summary.enabled
                || (!endpoint.summary.events.is_empty()
                    && !endpoint.summary.events.contains(&event.kind))
            {
                continue;
            }
            let Ok(signer) = WebhookSigner::new(&endpoint.secret, Duration::from_secs(300)) else {
                continue;
            };
            let delivery = signer.delivery(event.clone(), timestamp);
            let id = delivery.id;
            deliveries.insert(
                id,
                PendingWebhookDelivery {
                    webhook_id: endpoint.summary.id,
                    url: endpoint.summary.url.clone(),
                    delivery,
                    attempt: 0,
                    next_attempt_at: timestamp,
                },
            );
            count += 1;
        }
        count
    }

    pub async fn due_deliveries(&self, now: u64) -> Vec<PendingWebhookDelivery> {
        let endpoints = self.endpoints.read().await;
        self.deliveries
            .read()
            .await
            .values()
            .filter(|delivery| {
                delivery.next_attempt_at <= now
                    && endpoints
                        .get(&delivery.webhook_id)
                        .is_some_and(|endpoint| endpoint.summary.enabled)
            })
            .cloned()
            .collect()
    }

    pub async fn is_enabled(&self, webhook_id: Uuid) -> bool {
        self.endpoints
            .read()
            .await
            .get(&webhook_id)
            .is_some_and(|endpoint| endpoint.summary.enabled)
    }

    pub async fn complete(&self, delivery_id: Uuid) -> bool {
        self.deliveries.write().await.remove(&delivery_id).is_some()
    }

    pub async fn retry(&self, delivery_id: Uuid, attempt: u32, next_attempt_at: u64) -> bool {
        let mut deliveries = self.deliveries.write().await;
        let Some(delivery) = deliveries.get_mut(&delivery_id) else {
            return false;
        };
        delivery.attempt = attempt;
        delivery.next_attempt_at = next_attempt_at;
        true
    }

    /// Remember one delivery attempt for the dashboard, bounded to
    /// [`DELIVERY_HISTORY_LIMIT`] entries.
    pub async fn record_attempt(
        &self,
        pending: &PendingWebhookDelivery,
        attempt: u32,
        status: Option<u16>,
        outcome: DeliveryOutcome,
        error: Option<String>,
        at: u64,
    ) {
        let event = &pending.delivery.event;
        let mut history = self.history.write().await;
        history.push_back(DeliveryRecord {
            delivery_id: pending.delivery.id,
            webhook_id: pending.webhook_id,
            event: event.kind.clone(),
            repository: event.repository.clone(),
            tag: event.tag.clone(),
            digest: event.digest.clone(),
            attempt,
            status,
            outcome,
            error: error.map(|e| e.chars().take(200).collect()),
            at,
        });
        while history.len() > DELIVERY_HISTORY_LIMIT {
            history.pop_front();
        }
    }

    /// Most recent attempts first, optionally for one endpoint.
    pub async fn history(&self, webhook_id: Option<Uuid>, limit: usize) -> Vec<DeliveryRecord> {
        self.history
            .read()
            .await
            .iter()
            .rev()
            .filter(|record| webhook_id.is_none_or(|id| record.webhook_id == id))
            .take(limit)
            .cloned()
            .collect()
    }

    pub async fn pending_for(&self, webhook_id: Uuid) -> usize {
        self.deliveries
            .read()
            .await
            .values()
            .filter(|delivery| delivery.webhook_id == webhook_id)
            .count()
    }

    pub async fn pending_count(&self) -> usize {
        self.deliveries.read().await.len()
    }
}

impl EventLog {
    pub fn from_snapshot(value: serde_json::Value) -> Result<Self, serde_json::Error> {
        let events = serde_json::from_value(value)?;
        Ok(Self {
            events: Arc::new(RwLock::new(events)),
        })
    }

    pub async fn snapshot(&self) -> Result<serde_json::Value, serde_json::Error> {
        serde_json::to_value(&*self.events.read().await)
    }

    pub async fn record(&self, event: RegistryEvent) {
        self.events.write().await.push(event);
    }

    pub async fn recent(&self, limit: usize) -> Vec<RegistryEvent> {
        let events = self.events.read().await;
        events.iter().rev().take(limit).cloned().collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebhookDelivery {
    pub id: Uuid,
    pub event: RegistryEvent,
    pub body: Vec<u8>,
    pub timestamp: u64,
    pub signature: String,
}

#[derive(Serialize)]
struct WebhookPayload<'a> {
    schema_version: u16,
    #[serde(flatten)]
    event: &'a RegistryEvent,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WebhookError {
    #[error("webhook timestamp is outside the replay window")]
    ReplayWindow,
    #[error("webhook signature is invalid")]
    InvalidSignature,
    #[error("webhook secret must not be empty")]
    EmptySecret,
    #[error("webhook URL must be an http(s) URL")]
    InvalidUrl,
}

#[derive(Clone)]
pub struct WebhookSigner {
    secret: Arc<[u8]>,
    replay_window: Duration,
}

impl WebhookSigner {
    pub fn new(secret: impl AsRef<[u8]>, replay_window: Duration) -> Result<Self, WebhookError> {
        if secret.as_ref().is_empty() {
            return Err(WebhookError::EmptySecret);
        }
        Ok(Self {
            secret: Arc::from(secret.as_ref()),
            replay_window,
        })
    }

    pub fn sign(&self, delivery_id: Uuid, timestamp: u64, body: &[u8]) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.secret).expect("non-empty HMAC key");
        mac.update(&canonical_bytes(delivery_id, timestamp, body));
        format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
    }

    pub fn verify(
        &self,
        delivery_id: Uuid,
        timestamp: u64,
        signature: &str,
        body: &[u8],
        now: u64,
    ) -> Result<(), WebhookError> {
        let age = now.abs_diff(timestamp);
        if age > self.replay_window.as_secs() {
            return Err(WebhookError::ReplayWindow);
        }
        let expected = self.sign(delivery_id, timestamp, body);
        if expected.as_bytes().ct_eq(signature.as_bytes()).into() {
            Ok(())
        } else {
            Err(WebhookError::InvalidSignature)
        }
    }

    pub fn delivery(&self, event: RegistryEvent, timestamp: u64) -> WebhookDelivery {
        let id = Uuid::new_v4();
        let body = serde_json::to_vec(&WebhookPayload {
            schema_version: 1,
            event: &event,
        })
        .expect("registry event is serializable");
        let signature = self.sign(id, timestamp, &body);
        WebhookDelivery {
            id,
            event,
            body,
            timestamp,
            signature,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub base_delay: Duration,
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 6,
            base_delay: Duration::from_secs(2),
            max_delay: Duration::from_secs(15 * 60),
        }
    }
}

impl RetryPolicy {
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let exponent = attempt.saturating_sub(1).min(20);
        let multiplier = 1u32 << exponent;
        self.base_delay
            .checked_mul(multiplier)
            .unwrap_or(self.max_delay)
            .min(self.max_delay)
    }

    pub fn next_attempt(
        &self,
        attempt: u32,
        status: Result<u16, &str>,
        now: u64,
    ) -> DeliveryDecision {
        match status {
            Ok(code) if (200..300).contains(&code) => DeliveryDecision::Succeeded,
            Ok(code) if (300..400).contains(&code) => {
                DeliveryDecision::Failed { status: Some(code) }
            }
            Ok(code) if (400..500).contains(&code) && code != 408 && code != 429 => {
                DeliveryDecision::Failed { status: Some(code) }
            }
            Ok(code) if attempt >= self.max_attempts => {
                DeliveryDecision::Failed { status: Some(code) }
            }
            Ok(code) => DeliveryDecision::Retry {
                attempt: attempt + 1,
                not_before: now.saturating_add(self.delay_for_attempt(attempt).as_secs()),
                status: Some(code),
            },
            Err(_error) if attempt >= self.max_attempts => {
                DeliveryDecision::Failed { status: None }
            }
            Err(_) => DeliveryDecision::Retry {
                attempt: attempt + 1,
                not_before: now.saturating_add(self.delay_for_attempt(attempt).as_secs()),
                status: None,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryDecision {
    Succeeded,
    Retry {
        attempt: u32,
        not_before: u64,
        status: Option<u16>,
    },
    Failed {
        status: Option<u16>,
    },
}

fn canonical_bytes(delivery_id: Uuid, timestamp: u64, body: &[u8]) -> Vec<u8> {
    let mut canonical = format!("{timestamp}.{delivery_id}.").into_bytes();
    canonical.extend_from_slice(body);
    canonical
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn cloud_webhook_is_idempotent_and_uses_the_ci_signing_secret() {
        let registry = WebhookRegistry::default();
        let url = "https://cloud.knotree.com/api/v1/public/webhooks/knotree-registry";
        let secret = "x".repeat(32);
        registry
            .configure_cloud(url.into(), secret.clone())
            .await
            .unwrap();
        let initial = registry.list().await;
        registry
            .configure_cloud(url.into(), secret.clone())
            .await
            .unwrap();
        assert_eq!(registry.list().await, initial);
        let restored = WebhookRegistry::from_snapshot(registry.snapshot().await.unwrap()).unwrap();
        let signer = restored.signer(initial[0].id).await.unwrap();
        let expected = WebhookSigner::new(&secret, Duration::from_secs(300)).unwrap();
        let id = Uuid::new_v4();
        assert_eq!(
            signer.sign(id, 100, b"body"),
            expected.sign(id, 100, b"body")
        );
        assert_eq!(
            initial[0].events,
            [EventKind::TagUpdated, EventKind::GrantRevoked]
                .into_iter()
                .collect::<BTreeSet<_>>()
        );
        assert!(
            registry
                .configure_cloud("http://attacker.example".into(), secret)
                .await
                .is_err()
        );
    }

    use super::*;

    #[test]
    fn signatures_bind_delivery_id_timestamp_and_body() {
        let signer = WebhookSigner::new("secret", Duration::from_secs(300)).expect("signer");
        let id = Uuid::new_v4();
        let body = br#"{"event":"pushed"}"#;
        let signature = signer.sign(id, 100, body);
        assert!(signer.verify(id, 100, &signature, body, 200).is_ok());
        assert_eq!(
            signer.verify(id, 100, &signature, br#"{"event":"deleted"}"#, 100),
            Err(WebhookError::InvalidSignature)
        );
        assert_eq!(
            signer.verify(id, 100, &signature, body, 401),
            Err(WebhookError::ReplayWindow)
        );
    }

    #[test]
    fn retry_policy_stops_on_permanent_client_errors() {
        let policy = RetryPolicy::default();
        assert_eq!(
            policy.next_attempt(1, Ok(401), 100),
            DeliveryDecision::Failed { status: Some(401) }
        );
        assert!(matches!(
            policy.next_attempt(1, Ok(429), 100),
            DeliveryDecision::Retry {
                attempt: 2,
                status: Some(429),
                ..
            }
        ));
        assert!(matches!(
            policy.next_attempt(1, Ok(503), 100),
            DeliveryDecision::Retry {
                attempt: 2,
                status: Some(503),
                ..
            }
        ));
        assert_eq!(
            policy.next_attempt(1, Ok(307), 100),
            DeliveryDecision::Failed { status: Some(307) }
        );
        assert_eq!(
            policy.next_attempt(6, Ok(503), 100),
            DeliveryDecision::Failed { status: Some(503) }
        );
        assert!(matches!(
            policy.next_attempt(1, Err("timeout"), 100),
            DeliveryDecision::Retry { attempt: 2, .. }
        ));
    }

    #[tokio::test]
    async fn webhook_registry_reveals_secret_only_on_creation() {
        let registry = WebhookRegistry::default();
        let created = registry
            .create(
                "https://hooks.example.com/registry".to_owned(),
                BTreeSet::new(),
            )
            .await
            .expect("webhook");
        assert!(created.secret.starts_with("whsec_"));
        assert_eq!(registry.list().await.len(), 1);
        assert!(registry.signer(created.webhook.id).await.is_some());
        assert!(registry.disable(created.webhook.id).await);
        assert!(!registry.list().await[0].enabled);
    }

    #[tokio::test]
    async fn webhook_outbox_matches_events_and_survives_snapshot_round_trip() {
        let registry = WebhookRegistry::default();
        let created = registry
            .create(
                "https://hooks.example.com/registry".to_owned(),
                [EventKind::TagUpdated].into_iter().collect(),
            )
            .await
            .expect("webhook");
        let mut event = RegistryEvent::new(EventKind::TagUpdated);
        event.repository = Some("team/app".to_owned());
        assert_eq!(registry.enqueue(event.clone()).await, 1);
        let pending = registry.due_deliveries(now_seconds()).await;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].webhook_id, created.webhook.id);
        assert_eq!(pending[0].delivery.event, event);

        let snapshot = registry.snapshot().await.expect("snapshot");
        let restored = WebhookRegistry::from_snapshot(snapshot).expect("restore");
        assert_eq!(restored.pending_count().await, 1);
        assert!(restored.complete(pending[0].delivery.id).await);
        assert_eq!(restored.pending_count().await, 0);
    }

    #[tokio::test]
    async fn disabling_a_webhook_removes_queued_deliveries() {
        let registry = WebhookRegistry::default();
        let created = registry
            .create(
                "https://hooks.example.com/registry".to_owned(),
                BTreeSet::new(),
            )
            .await
            .expect("webhook");
        assert_eq!(
            registry
                .enqueue(RegistryEvent::new(EventKind::TagUpdated))
                .await,
            1
        );
        assert!(registry.disable(created.webhook.id).await);
        assert_eq!(registry.pending_count().await, 0);
    }

    #[tokio::test]
    async fn webhook_rejects_malformed_and_credential_bearing_urls() {
        let registry = WebhookRegistry::default();
        assert_eq!(
            registry
                .create("https://".to_owned(), BTreeSet::new())
                .await,
            Err(WebhookError::InvalidUrl)
        );
        assert_eq!(
            registry
                .create(
                    "https://user:secret@hooks.example.com/registry".to_owned(),
                    BTreeSet::new()
                )
                .await,
            Err(WebhookError::InvalidUrl)
        );
    }

    #[tokio::test]
    async fn legacy_webhook_snapshots_restore_without_an_outbox() {
        let endpoint = serde_json::json!([]);
        let registry = WebhookRegistry::from_snapshot(endpoint).expect("legacy snapshot");
        assert_eq!(registry.list().await.len(), 0);
        assert_eq!(registry.pending_count().await, 0);
    }

    #[tokio::test]
    async fn delivery_history_is_bounded_and_survives_snapshot() {
        let registry = WebhookRegistry::default();
        registry
            .configure_cloud(
                "https://cloud.knotree.com/api/v1/public/webhooks/knotree-registry".into(),
                "s".repeat(32),
            )
            .await
            .unwrap();
        let mut event = RegistryEvent::new(EventKind::TagUpdated);
        event.repository = Some("team/app".into());
        event.tag = Some("main".into());
        assert_eq!(registry.enqueue(event).await, 1);
        let pending = registry.due_deliveries(now_seconds()).await.remove(0);
        assert_eq!(registry.pending_for(CLOUD_WEBHOOK_ID).await, 1);
        for attempt in 0..(DELIVERY_HISTORY_LIMIT as u32 + 5) {
            registry
                .record_attempt(
                    &pending,
                    attempt,
                    Some(503),
                    DeliveryOutcome::Retrying,
                    None,
                    1,
                )
                .await;
        }
        registry
            .record_attempt(
                &pending,
                999,
                Some(202),
                DeliveryOutcome::Delivered,
                None,
                2,
            )
            .await;
        let history = registry.history(Some(CLOUD_WEBHOOK_ID), 500).await;
        assert_eq!(history.len(), DELIVERY_HISTORY_LIMIT);
        assert_eq!(history[0].outcome, DeliveryOutcome::Delivered);
        assert_eq!(history[0].repository.as_deref(), Some("team/app"));
        let restored = WebhookRegistry::from_snapshot(registry.snapshot().await.unwrap()).unwrap();
        assert_eq!(restored.history(None, 1).await[0].attempt, 999);
        assert!(registry.history(Some(Uuid::new_v4()), 10).await.is_empty());
    }
}
