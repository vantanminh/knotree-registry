//! Durable event and webhook-delivery primitives shared by the server and agent.

use std::{
    collections::{BTreeSet, HashMap},
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

#[derive(Clone, Default)]
pub struct WebhookRegistry {
    endpoints: Arc<RwLock<HashMap<Uuid, WebhookEndpoint>>>,
}

#[derive(Clone)]
struct WebhookEndpoint {
    summary: WebhookSummary,
    secret: String,
}

impl WebhookRegistry {
    pub async fn create(
        &self,
        url: String,
        events: BTreeSet<EventKind>,
    ) -> Result<WebhookCreated, WebhookError> {
        if !(url.starts_with("https://") || url.starts_with("http://")) {
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
        self.endpoints
            .write()
            .await
            .get_mut(&id)
            .map(|endpoint| endpoint.summary.enabled = false)
            .is_some()
    }

    pub async fn signer(&self, id: Uuid) -> Option<WebhookSigner> {
        let endpoint = self.endpoints.read().await.get(&id).cloned()?;
        WebhookSigner::new(endpoint.secret, Duration::from_secs(300)).ok()
    }
}

impl EventLog {
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
        let body = serde_json::to_vec(&event).expect("registry event is serializable");
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
}
