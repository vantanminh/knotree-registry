//! In-cluster API for trusted Knotree services (Knotree Cloud).
//!
//! A Knotree account is one identity across services, so Cloud does not ask
//! the user to connect Registry. Cloud calls this API on a cluster-only port
//! and names the account it acts for; Registry derives that account's
//! namespace and never serves anything outside it.
//!
//! Callers authenticate with a projected Kubernetes ServiceAccount token whose
//! audience is this API. Registry verifies it with the TokenReview API and
//! accepts only the configured ServiceAccounts. Tokens are short-lived and
//! rotated by the kubelet, so there is no long-lived shared secret.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use registry_core::RepositoryName;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use subtle_compare::constant_time_eq;
use tokio::sync::Mutex;

use crate::{AppError, AppState, ConfigError};

pub(crate) const DEFAULT_AUDIENCE: &str = "knotree-registry-internal";
pub(crate) const DEFAULT_CALLER: &str =
    "system:serviceaccount:knotree-cloud:knotree-cloud-knotree-api";
pub(crate) const PULL_CREDENTIAL_NAME: &str = "Knotree Cloud image pulls";
const PULL_CREDENTIAL_DAYS: u64 = 90;
const REVIEW_CACHE_SECONDS: u64 = 60;
const REVIEW_CACHE_CAPACITY: usize = 256;
const SA_DIR: &str = "/var/run/secrets/kubernetes.io/serviceaccount";

#[derive(Clone, Debug)]
pub enum InternalAuth {
    /// Verify projected ServiceAccount tokens with the Kubernetes TokenReview API.
    Kubernetes {
        audience: String,
        callers: Vec<String>,
    },
    /// Development only: a static bearer token. Rejected in production.
    DevToken(String),
}

#[derive(Clone, Debug)]
pub struct InternalConfig {
    pub bind_addr: std::net::SocketAddr,
    pub auth: InternalAuth,
}

impl InternalConfig {
    pub fn from_env(production: bool) -> Result<Option<Self>, ConfigError> {
        let Some(bind) = std::env::var("INTERNAL_BIND_ADDR")
            .ok()
            .filter(|value| !value.trim().is_empty())
        else {
            return Ok(None);
        };
        let bind_addr = bind.parse()?;
        let auth = match std::env::var("INTERNAL_AUTH")
            .unwrap_or_else(|_| "kubernetes".into())
            .as_str()
        {
            "kubernetes" => InternalAuth::Kubernetes {
                audience: std::env::var("INTERNAL_TOKEN_AUDIENCE")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| DEFAULT_AUDIENCE.into()),
                callers: std::env::var("INTERNAL_ALLOWED_SERVICE_ACCOUNTS")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| DEFAULT_CALLER.into())
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
                    .collect(),
            },
            "dev-token" => {
                InternalAuth::DevToken(std::env::var("INTERNAL_DEV_TOKEN").unwrap_or_default())
            }
            _ => {
                return Err(ConfigError::Internal(
                    "INTERNAL_AUTH must be kubernetes or dev-token",
                ));
            }
        };
        let config = Self { bind_addr, auth };
        config.validate(production)?;
        Ok(Some(config))
    }

    pub fn validate(&self, production: bool) -> Result<(), ConfigError> {
        match &self.auth {
            InternalAuth::Kubernetes { audience, callers } => {
                if audience.is_empty()
                    || callers.is_empty()
                    || callers
                        .iter()
                        .any(|caller| !caller.starts_with("system:serviceaccount:"))
                {
                    return Err(ConfigError::Internal(
                        "internal callers must be system:serviceaccount:<namespace>:<name>",
                    ));
                }
            }
            InternalAuth::DevToken(token) => {
                if production {
                    return Err(ConfigError::Internal(
                        "the internal dev token is not allowed in production",
                    ));
                }
                if token.len() < 32 {
                    return Err(ConfigError::Internal(
                        "INTERNAL_DEV_TOKEN must be at least 32 characters",
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Successful TokenReview results, keyed by the token digest.
#[derive(Default)]
pub(crate) struct ReviewCache {
    entries: HashMap<[u8; 32], (Instant, String)>,
}

#[derive(Clone)]
struct Internal {
    state: AppState,
    auth: InternalAuth,
    cache: Arc<Mutex<ReviewCache>>,
    http: reqwest::Client,
}

pub fn router(state: AppState, config: InternalConfig) -> Router {
    let internal = Internal {
        state,
        auth: config.auth,
        cache: Arc::new(Mutex::new(ReviewCache::default())),
        http: kubernetes_client(),
    };
    Router::new()
        .route("/internal/v1/repositories", get(repositories))
        .route("/internal/v1/repositories/{*repository}", get(tags))
        .route("/internal/v1/manifest", get(manifest))
        .route("/internal/v1/pull-credentials", post(pull_credential))
        .route("/internal/v1/health", get(health))
        .route("/metrics", get(metrics))
        .with_state(internal)
}

/// Per-component readiness for operators. The public `/readyz` only says
/// whether Registry is ready, so storage and database state stay off the
/// internet; this cluster-only port is where they are inspected.
async fn health(State(internal): State<Internal>) -> Response {
    let readiness = internal.state.readiness().await;
    let status = if readiness.status == "ok" {
        axum::http::StatusCode::OK
    } else {
        axum::http::StatusCode::SERVICE_UNAVAILABLE
    };
    let body = json!({
        "status": readiness.status,
        "storage": readiness.storage,
        "database": readiness.database,
        "uptime_seconds": internal.state.started_at.elapsed().as_secs(),
        "version": env!("CARGO_PKG_VERSION"),
    });
    (status, Json(body)).into_response()
}

/// Prometheus counters, scraped inside the cluster only.
async fn metrics(State(internal): State<Internal>) -> Response {
    (
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        internal.state.metrics.render_prometheus(),
    )
        .into_response()
}

fn kubernetes_client() -> reqwest::Client {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none());
    if let Ok(pem) = std::fs::read(PathBuf::from(SA_DIR).join("ca.crt"))
        && let Ok(certificate) = reqwest::Certificate::from_pem(&pem)
    {
        builder = builder.add_root_certificate(certificate);
    }
    builder.build().unwrap_or_default()
}

fn not_found() -> AppError {
    crate::CatalogError::NotFound.into()
}

fn denied() -> AppError {
    registry_auth::AuthError::NoAccess.into()
}

fn unauthenticated() -> AppError {
    registry_auth::AuthError::InvalidCredentials.into()
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .filter(|token| !token.is_empty() && token.len() <= 8192)
}

mod subtle_compare {
    pub fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
        if left.len() != right.len() {
            return false;
        }
        left.iter()
            .zip(right)
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
    }
}

impl Internal {
    /// Authenticates the calling service.
    async fn caller(&self, headers: &HeaderMap) -> Result<(), AppError> {
        let token = bearer(headers).ok_or_else(unauthenticated)?;
        match &self.auth {
            InternalAuth::DevToken(expected) => {
                if constant_time_eq(token.as_bytes(), expected.as_bytes()) {
                    Ok(())
                } else {
                    Err(unauthenticated())
                }
            }
            InternalAuth::Kubernetes { audience, callers } => {
                let key: [u8; 32] = Sha256::digest(token.as_bytes()).into();
                {
                    let mut cache = self.cache.lock().await;
                    let now = Instant::now();
                    cache.entries.retain(|_, (expires, _)| *expires > now);
                    if let Some((_, username)) = cache.entries.get(&key) {
                        return if callers.iter().any(|caller| caller == username) {
                            Ok(())
                        } else {
                            Err(denied())
                        };
                    }
                }
                let username = self.review(token, audience).await?;
                let allowed = callers.iter().any(|caller| caller == &username);
                if !allowed {
                    tracing::warn!(%username, "internal API rejected an unexpected service account");
                    return Err(denied());
                }
                let mut cache = self.cache.lock().await;
                if cache.entries.len() >= REVIEW_CACHE_CAPACITY {
                    cache.entries.clear();
                }
                cache.entries.insert(
                    key,
                    (
                        Instant::now() + Duration::from_secs(REVIEW_CACHE_SECONDS),
                        username,
                    ),
                );
                Ok(())
            }
        }
    }

    /// Asks the API server who the token belongs to, for this audience only.
    async fn review(&self, token: &str, audience: &str) -> Result<String, AppError> {
        let host = std::env::var("KUBERNETES_SERVICE_HOST").map_err(|_| unauthenticated())?;
        let port = std::env::var("KUBERNETES_SERVICE_PORT").unwrap_or_else(|_| "443".into());
        let own_token = std::fs::read_to_string(PathBuf::from(SA_DIR).join("token"))
            .map_err(|_| unauthenticated())?;
        let host = if host.contains(':') {
            format!("[{host}]")
        } else {
            host
        };
        let body = json!({
            "apiVersion": "authentication.k8s.io/v1",
            "kind": "TokenReview",
            "spec": {"token": token, "audiences": [audience]},
        });
        let response = self
            .http
            .post(format!(
                "https://{host}:{port}/apis/authentication.k8s.io/v1/tokenreviews"
            ))
            .bearer_auth(own_token.trim())
            .header(header::CONTENT_TYPE, "application/json")
            .body(body.to_string())
            .send()
            .await
            .map_err(|error| {
                tracing::error!(error = %error, "TokenReview request failed");
                unauthenticated()
            })?;
        if !response.status().is_success() {
            tracing::error!(status = %response.status(), "TokenReview was not accepted");
            return Err(unauthenticated());
        }
        let bytes = response.bytes().await.map_err(|_| unauthenticated())?;
        let review: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| unauthenticated())?;
        let status = &review["status"];
        let audience_ok = status["audiences"]
            .as_array()
            .is_some_and(|values| values.iter().any(|value| value == audience));
        if status["authenticated"] != true || !audience_ok {
            return Err(unauthenticated());
        }
        status["user"]["username"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(unauthenticated)
    }

    /// Authenticates the caller, then resolves the account it acts for.
    async fn account(&self, headers: &HeaderMap) -> Result<registry_auth::UserSummary, AppError> {
        self.caller(headers).await?;
        let sso = self
            .state
            .config
            .sso
            .as_ref()
            .ok_or(AppError::BadRequest("Accounts SSO is not configured"))?;
        let issuer = header_value(headers, "x-knotree-issuer").ok_or_else(denied)?;
        let subject = header_value(headers, "x-knotree-subject").ok_or_else(denied)?;
        if issuer != sso.issuer {
            return Err(denied());
        }
        let user = self.state.auth.federated_user(issuer, subject).await?;
        Ok(user)
    }
}

fn header_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(name)?
        .to_str()
        .ok()
        .filter(|value| !value.is_empty() && value.len() <= 2048)
}

/// Only the account's own namespace, even for Registry administrators.
fn owns(user: &registry_auth::UserSummary, repository: &RepositoryName) -> bool {
    repository
        .as_str()
        .strip_prefix(&user.username)
        .is_some_and(|rest| rest.starts_with('/'))
}

fn no_store(value: serde_json::Value) -> Response {
    let mut response = Json(value).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
}

async fn repositories(
    State(internal): State<Internal>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let user = internal.account(&headers).await?;
    let mut inventory = internal.state.catalog.inventory().await;
    inventory.repositories.retain(|repository| {
        RepositoryName::parse(&repository.name).is_ok_and(|name| owns(&user, &name))
    });
    Ok(no_store(
        json!({"namespace": user.username, "repositories": inventory.repositories}),
    ))
}

async fn tags(
    State(internal): State<Internal>,
    Path(repository): Path<String>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let user = internal.account(&headers).await?;
    let repository = RepositoryName::parse(&repository)
        .map_err(|_| AppError::BadRequest("invalid repository name"))?;
    if !owns(&user, &repository) {
        return Err(not_found());
    }
    Ok(no_store(
        crate::routes::repository_tags_json(&internal.state, &repository).await?,
    ))
}

#[derive(Deserialize)]
struct ManifestQuery {
    repository: String,
    reference: String,
}

async fn manifest(
    State(internal): State<Internal>,
    Query(query): Query<ManifestQuery>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let user = internal.account(&headers).await?;
    let repository = RepositoryName::parse(&query.repository)
        .map_err(|_| AppError::BadRequest("invalid repository name"))?;
    if !owns(&user, &repository) || query.reference.is_empty() || query.reference.len() > 256 {
        return Err(not_found());
    }
    let manifest = internal
        .state
        .catalog
        .resolve_manifest(&repository, &query.reference)
        .await?;
    Ok(no_store(json!({
        "repository": repository.to_string(),
        "reference": query.reference,
        "digest": manifest.digest,
        "media_type": manifest.media_type,
        "size": manifest.size,
    })))
}

#[derive(Deserialize)]
struct PullCredentialInput {
    repository: String,
}

async fn pull_credential(
    State(internal): State<Internal>,
    headers: HeaderMap,
    Json(input): Json<PullCredentialInput>,
) -> Result<Response, AppError> {
    let user = internal.account(&headers).await?;
    let repository = RepositoryName::parse(&input.repository)
        .map_err(|_| AppError::BadRequest("invalid repository name"))?;
    if !owns(&user, &repository) {
        return Err(not_found());
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| AppError::BadRequest("clock error"))?
        .as_secs();
    let credential = internal
        .state
        .auth
        .rotate_repository_pull_credential(
            user.id,
            PULL_CREDENTIAL_NAME.into(),
            repository.clone(),
            Some(now + PULL_CREDENTIAL_DAYS * 86400),
        )
        .await?;
    if let Err(error) = internal.state.persist_runtime_state().await {
        let _ = internal.state.auth.revoke_credential(credential.id).await;
        return Err(error);
    }
    Ok(no_store(json!({
        "username": user.username,
        "secret": credential.secret,
        "credential_id": credential.id,
        "repository": repository.to_string(),
        "expires_at": credential.expires_at,
        "actions": ["pull"],
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    const TOKEN: &str = "internal-dev-token-0123456789abcdef0123";
    const ISSUER: &str = "https://accounts.knotree.com";

    async fn call(
        app: Router,
        method: &str,
        path: &str,
        token: Option<&str>,
        subject: &str,
        body: Option<serde_json::Value>,
    ) -> (StatusCode, serde_json::Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header("x-knotree-issuer", ISSUER)
            .header("x-knotree-subject", subject)
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(token) = token {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let response = app
            .oneshot(
                request
                    .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    #[test]
    fn production_rejects_the_dev_token() {
        let config = InternalConfig {
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            auth: InternalAuth::DevToken(TOKEN.into()),
        };
        assert!(config.validate(true).is_err());
        assert!(config.validate(false).is_ok());
        let short = InternalConfig {
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            auth: InternalAuth::DevToken("short".into()),
        };
        assert!(short.validate(false).is_err());
        let wrong_caller = InternalConfig {
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            auth: InternalAuth::Kubernetes {
                audience: DEFAULT_AUDIENCE.into(),
                callers: vec!["alice".into()],
            },
        };
        assert!(wrong_caller.validate(true).is_err());
    }

    #[tokio::test]
    async fn operator_health_and_metrics_live_on_the_internal_port() {
        let state = AppState::initialize(crate::routes::tests::test_config())
            .await
            .unwrap();
        let app = router(
            state,
            InternalConfig {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                auth: InternalAuth::DevToken(TOKEN.into()),
            },
        );
        let (status, body) = call(app.clone(), "GET", "/internal/v1/health", None, "", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["storage"], "ok");
        assert_eq!(body["database"], "skipped");
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let text = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        assert!(String::from_utf8_lossy(&text).contains("knotree_registry_requests_total"));
    }

    #[tokio::test]
    async fn services_act_only_inside_the_named_accounts_namespace() {
        let mut config = crate::routes::tests::test_config();
        config.sso = Some(crate::SsoConfig {
            issuer: ISSUER.into(),
            service_origin: ISSUER.into(),
            client_id: "knotree-registry".into(),
            redirect_uri: "https://registry.knotree.com/api/v1/auth/sso/callback".into(),
        });
        let state = AppState::initialize(config).await.unwrap();
        let alice = state.auth.federated_user(ISSUER, "alice").await.unwrap();
        let bob = state.auth.federated_user(ISSUER, "bob").await.unwrap();
        let app = router(
            state.clone(),
            InternalConfig {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                auth: InternalAuth::DevToken(TOKEN.into()),
            },
        );
        let alice_repo = format!("{}/app", alice.username);
        let bob_repo = format!("{}/app", bob.username);

        // The caller must authenticate.
        let (status, _) = call(
            app.clone(),
            "GET",
            "/internal/v1/repositories",
            None,
            "alice",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, _) = call(
            app.clone(),
            "GET",
            "/internal/v1/repositories",
            Some("wrong-token-wrong-token-wrong-token-0000"),
            "alice",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let (status, body) = call(
            app.clone(),
            "GET",
            "/internal/v1/repositories",
            Some(TOKEN),
            "alice",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["namespace"], alice.username);

        // Alice cannot read or obtain pull access to Bob's repository.
        let (status, _) = call(
            app.clone(),
            "GET",
            &format!("/internal/v1/repositories/{bob_repo}"),
            Some(TOKEN),
            "alice",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = call(
            app.clone(),
            "POST",
            "/internal/v1/pull-credentials",
            Some(TOKEN),
            "alice",
            Some(json!({"repository": bob_repo})),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        // Her own repository gets a pull-only credential; rotation keeps two.
        let mut issued = Vec::new();
        for _ in 0..3 {
            let (status, body) = call(
                app.clone(),
                "POST",
                "/internal/v1/pull-credentials",
                Some(TOKEN),
                "alice",
                Some(json!({"repository": alice_repo})),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{body}");
            assert_eq!(body["username"], alice.username);
            issued.push(body["secret"].as_str().unwrap().to_owned());
        }
        let pull = registry_core::RepositoryScope {
            repository: RepositoryName::parse(&alice_repo).unwrap(),
            actions: [registry_core::Action::Pull].into_iter().collect(),
        };
        let push = registry_core::RepositoryScope {
            repository: RepositoryName::parse(&alice_repo).unwrap(),
            actions: [registry_core::Action::Push].into_iter().collect(),
        };
        let other = registry_core::RepositoryScope {
            repository: RepositoryName::parse(&format!("{}/other", alice.username)).unwrap(),
            actions: [registry_core::Action::Pull].into_iter().collect(),
        };
        let mint = |secret: &str, scope: &registry_core::RepositoryScope| {
            let state = state.clone();
            let username = alice.username.clone();
            let secret = secret.to_owned();
            let scope = scope.clone();
            async move {
                state
                    .auth
                    .mint_token(&username, &secret, "knotree-registry", &[scope])
                    .await
            }
        };
        assert!(mint(&issued[0], &pull).await.is_err(), "oldest is revoked");
        assert!(
            mint(&issued[1], &pull).await.is_ok(),
            "previous stays valid"
        );
        assert!(mint(&issued[2], &pull).await.is_ok());
        for scope in [&push, &other] {
            assert!(
                mint(&issued[2], scope).await.is_err(),
                "pull-only, one repository"
            );
        }

        // An unknown issuer is never accepted.
        let request = Request::builder()
            .uri("/internal/v1/repositories")
            .header("x-knotree-issuer", "https://evil.example")
            .header("x-knotree-subject", "alice")
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}
