use std::{
    collections::HashMap,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use registry_core::{Action, RepositoryName, RepositoryScope};
use registry_events::{EventKind, RegistryEvent};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use url::Url;
use uuid::Uuid;

use crate::{AppError, AppState};

const CLIENT: &str = "knotree-cloud";
const CALLBACK: &str = "https://cloud.knotree.com/api/v1/auth/knotree-registry/callback";
const CAPACITY: usize = 4096;

#[derive(Clone)]
struct ConsentRequest {
    repository: RepositoryName,
    expected_issuer: String,
    expected_subject: String,
    state: String,
    challenge: String,
    expires: Instant,
}
struct ApprovedCode {
    request: ConsentRequest,
    session_token: String,
    expires: Instant,
}
#[derive(Default)]
pub(crate) struct CloudGrants {
    requests: HashMap<Uuid, ConsentRequest>,
    codes: HashMap<String, ApprovedCode>,
}
impl CloudGrants {
    fn clean(&mut self) {
        let now = Instant::now();
        self.requests.retain(|_, r| r.expires > now);
        self.codes.retain(|_, r| r.expires > now);
    }
}

fn invalid() -> AppError {
    AppError::BadRequest("Cloud authorization is invalid or expired")
}
fn denied() -> AppError {
    registry_auth::AuthError::NoAccess.into()
}
fn no_store(value: serde_json::Value) -> Response {
    let mut response = Json(value).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
}
fn valid_challenge(value: &str) -> bool {
    value.len() == 43
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
}
fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}
fn verifier_valid(value: &str) -> bool {
    (43..=128).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
}

#[derive(Deserialize)]
pub(crate) struct RequestInput {
    client_id: String,
    redirect_uri: String,
    state: String,
    repository: String,
    code_challenge: String,
    code_challenge_method: String,
    expected_issuer: String,
    expected_subject: String,
}
pub(crate) async fn create_request(
    State(state): State<AppState>,
    Json(input): Json<RequestInput>,
) -> Result<Response, AppError> {
    if state.config.sso.is_none() {
        return Err(AppError::BadRequest("Accounts SSO is not configured"));
    }
    if input.client_id != CLIENT
        || input.redirect_uri != CALLBACK
        || state
            .config
            .sso
            .as_ref()
            .is_none_or(|sso| sso.issuer != input.expected_issuer)
        || input.expected_subject.is_empty()
        || input.expected_subject.len() > 1024
        || input.expected_subject.chars().any(char::is_control)
        || input.code_challenge_method != "S256"
        || !valid_challenge(&input.code_challenge)
        || !(32..=128).contains(&input.state.len())
        || !input
            .state
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
    {
        return Err(invalid());
    }
    let repository = RepositoryName::parse(&input.repository).map_err(|_| invalid())?;
    let mut grants = state.cloud_grants.lock().await;
    grants.clean();
    if grants.requests.len() + grants.codes.len() >= CAPACITY {
        return Err(AppError::BadRequest(
            "Too many pending authorization requests",
        ));
    }
    let id = Uuid::new_v4();
    grants.requests.insert(
        id,
        ConsentRequest {
            repository,
            expected_issuer: input.expected_issuer,
            expected_subject: input.expected_subject,
            state: input.state,
            challenge: input.code_challenge,
            expires: Instant::now() + Duration::from_secs(600),
        },
    );
    let origin = Url::parse(&state.config.public_url)
        .map_err(|_| invalid())?
        .origin()
        .ascii_serialization();
    Ok(no_store(
        json!({"request_id":id,"authorization_url":format!("{origin}/cloud/authorize/{id}"),"expires_in":600}),
    ))
}

pub(crate) async fn request_details(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let session =
        crate::routes::cookie_value(&headers).ok_or(registry_auth::AuthError::InvalidSession)?;
    let user = state.auth.session_user(session).await?;
    let identity = state.auth.federated_identity_for_session(session).await?;
    let mut grants = state.cloud_grants.lock().await;
    grants.clean();
    let request = grants.requests.get(&id).ok_or_else(invalid)?;
    if identity
        != (
            request.expected_issuer.clone(),
            request.expected_subject.clone(),
        )
        || !user.can_access_repository(&request.repository)
    {
        return Err(denied());
    }
    Ok(no_store(
        json!({"client":"Knotree Cloud","client_origin":"https://cloud.knotree.com","repository":request.repository,"actions":["pull"],"username":user.username,"expires_in":request.expires.saturating_duration_since(Instant::now()).as_secs(),"credential_lifetime_days":30}),
    ))
}

#[derive(Deserialize)]
pub(crate) struct DecisionInput {
    allow: bool,
}
pub(crate) async fn decision(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    Json(input): Json<DecisionInput>,
) -> Result<Response, AppError> {
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(denied)?;
    if !crate::routes::allowed_origin(&state, origin) {
        return Err(denied());
    }
    let session =
        crate::routes::cookie_value(&headers).ok_or(registry_auth::AuthError::InvalidSession)?;
    let user = state.auth.session_user(session).await?;
    let identity = state.auth.federated_identity_for_session(session).await?;
    let mut grants = state.cloud_grants.lock().await;
    grants.clean();
    let request = grants.requests.get(&id).ok_or_else(invalid)?;
    if identity
        != (
            request.expected_issuer.clone(),
            request.expected_subject.clone(),
        )
        || !user.can_access_repository(&request.repository)
    {
        return Err(denied());
    }
    let request = grants.requests.remove(&id).ok_or_else(invalid)?;
    let mut redirect = Url::parse(CALLBACK).map_err(|_| invalid())?;
    redirect
        .query_pairs_mut()
        .append_pair("state", &request.state);
    if input.allow {
        let code = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        redirect.query_pairs_mut().append_pair("code", &code);
        grants.codes.insert(
            code,
            ApprovedCode {
                request,
                session_token: session.to_owned(),
                expires: Instant::now() + Duration::from_secs(120),
            },
        );
    } else {
        redirect
            .query_pairs_mut()
            .append_pair("error", "access_denied");
    }
    Ok(no_store(json!({"redirect_url":redirect.as_str()})))
}

#[derive(Deserialize)]
pub(crate) struct ExchangeInput {
    client_id: String,
    redirect_uri: String,
    code: String,
    code_verifier: String,
}
pub(crate) async fn exchange(
    State(state): State<AppState>,
    Json(input): Json<ExchangeInput>,
) -> Result<Response, AppError> {
    if input.client_id != CLIENT
        || input.redirect_uri != CALLBACK
        || input.code.len() != 64
        || !verifier_valid(&input.code_verifier)
    {
        return Err(invalid());
    }
    let approved = {
        let mut grants = state.cloud_grants.lock().await;
        grants.clean();
        let approved = grants.codes.get(&input.code).ok_or_else(invalid)?;
        if approved.request.challenge != challenge(&input.code_verifier) {
            return Err(invalid());
        }
        grants.codes.remove(&input.code).ok_or_else(invalid)?
    };
    // Confirm the approving session and central identity are still live. No
    // credential is minted until this exchange, so expired codes leave no PAT.
    let user = state.auth.session_user(&approved.session_token).await?;
    let (issuer, subject) = state
        .auth
        .federated_identity_for_session(&approved.session_token)
        .await?;
    if issuer != approved.request.expected_issuer
        || subject != approved.request.expected_subject
        || !user.can_access_repository(&approved.request.repository)
    {
        return Err(denied());
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| invalid())?
        .as_secs();
    let credential = state
        .auth
        .create_credential_for_session(
            &approved.session_token,
            "Knotree Cloud pull authorization".into(),
            vec![RepositoryScope {
                repository: approved.request.repository.clone(),
                actions: [Action::Pull].into_iter().collect(),
            }],
            Some(now + 30 * 86400),
        )
        .await?;
    let mut event = RegistryEvent::new(EventKind::TokenCreated);
    event.actor = Some(user.username.clone());
    event.repository = Some(approved.request.repository.to_string());
    event.metadata = json!({"credential_id":credential.id,"client_id":CLIENT,"actions":["pull"]});
    state.record_event(event).await;
    if let Err(error) = state.persist_runtime_state().await {
        let _ = state.auth.revoke_credential(credential.id).await;
        return Err(error);
    }
    Ok(no_store(
        json!({"username":user.username,"credential":credential.secret,"credential_id":credential.id,"repository":approved.request.repository,"issuer":issuer,"subject":subject,"expires_at":credential.expires_at,"actions":["pull"]}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pkce_is_s256_and_rejects_invalid_verifiers() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert!(verifier_valid(verifier));
        assert_eq!(
            challenge(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert!(valid_challenge(&challenge(verifier)));
        assert!(!verifier_valid("short"));
        assert!(!verifier_valid(&format!("{}\\n", "a".repeat(43))));
    }
}

#[cfg(test)]
mod flow_tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    async fn json_response(
        app: axum::Router,
        path: &str,
        body: serde_json::Value,
        cookie: Option<&str>,
        origin: Option<&str>,
    ) -> (StatusCode, serde_json::Value) {
        let mut request = Request::builder()
            .method("POST")
            .uri(path)
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        if let Some(origin) = origin {
            request = request.header(header::ORIGIN, origin);
        }
        let response = app
            .oneshot(request.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn consent_and_code_exchange_are_scoped_single_use_and_browser_secret_free() {
        let mut config = crate::routes::tests::test_config();
        config.sso = Some(crate::SsoConfig {
            issuer: "https://accounts.knotree.com".into(),
            service_origin: "https://accounts.knotree.com".into(),
            client_id: "knotree-registry".into(),
            redirect_uri: "https://registry.knotree.com/api/v1/auth/sso/callback".into(),
        });
        let state = AppState::initialize(config).await.unwrap();
        let alice = state
            .auth
            .login_federated("https://accounts.knotree.com", "alice")
            .await
            .unwrap();
        let bob = state
            .auth
            .login_federated("https://accounts.knotree.com", "bob")
            .await
            .unwrap();
        let alice_cookie = format!("kntr_session={}", alice.token);
        let bob_cookie = format!("kntr_session={}", bob.token);
        let app = crate::router(state.clone());
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let repository = format!("{}/app", alice.user.username);
        let request = json!({"client_id":CLIENT,"redirect_uri":CALLBACK,"state":"a".repeat(43),"repository":repository,"code_challenge":challenge(verifier),"code_challenge_method":"S256","expected_issuer":"https://accounts.knotree.com","expected_subject":"alice"});
        let mut malicious = request.clone();
        malicious["redirect_uri"] = json!("https://attacker.example/callback");
        assert_eq!(
            json_response(
                app.clone(),
                "/api/v1/cloud-grants/requests",
                malicious,
                None,
                None
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
        let (status, created) = json_response(
            app.clone(),
            "/api/v1/cloud-grants/requests",
            request,
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let decision_path = format!(
            "/api/v1/cloud-grants/requests/{}/decision",
            created["request_id"].as_str().unwrap()
        );
        assert_eq!(
            json_response(
                app.clone(),
                &decision_path,
                json!({"allow":true}),
                Some(&bob_cookie),
                Some("http://localhost:8080")
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            json_response(
                app.clone(),
                &decision_path,
                json!({"allow":true}),
                Some(&alice_cookie),
                Some("https://attacker.example")
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
        let (status, approved) = json_response(
            app.clone(),
            &decision_path,
            json!({"allow":true}),
            Some(&alice_cookie),
            Some("http://localhost:8080"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(approved.get("credential").is_none());
        assert!(
            state
                .auth
                .list_credentials_for_session(&alice.token)
                .await
                .unwrap()
                .is_empty()
        );
        let redirect = Url::parse(approved["redirect_url"].as_str().unwrap()).unwrap();
        assert_eq!(
            redirect.origin().ascii_serialization(),
            "https://cloud.knotree.com"
        );
        let params: HashMap<String, String> = redirect
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        assert_eq!(params["state"], "a".repeat(43));
        let exchange = json!({"client_id":CLIENT,"redirect_uri":CALLBACK,"code":params["code"],"code_verifier":verifier});
        let mut wrong = exchange.clone();
        wrong["code_verifier"] = json!("b".repeat(43));
        assert_eq!(
            json_response(
                app.clone(),
                "/api/v1/cloud-grants/exchange",
                wrong,
                None,
                None
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
        let (status, credential) = json_response(
            app.clone(),
            "/api/v1/cloud-grants/exchange",
            exchange.clone(),
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(credential["subject"], "alice");
        assert_eq!(credential["repository"], repository);
        assert_eq!(credential["actions"], json!(["pull"]));
        let scopes = state
            .auth
            .list_credentials_for_session(&alice.token)
            .await
            .unwrap();
        assert_eq!(scopes.len(), 1);
        assert_eq!(
            scopes[0].scopes[0].actions,
            [Action::Pull]
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>()
        );
        assert_eq!(
            json_response(app, "/api/v1/cloud-grants/exchange", exchange, None, None)
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
    }
}
