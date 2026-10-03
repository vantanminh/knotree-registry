use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, header},
    response::{IntoResponse, Redirect, Response},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use registry_events::{EventKind, RegistryEvent};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use url::Url;
use uuid::Uuid;

use crate::{AppConfig, AppEnvironment, AppError, AppState, ConfigError};

#[derive(Clone, Debug)]
pub struct SsoConfig {
    pub issuer: String,
    pub service_origin: String,
    pub client_id: String,
    pub redirect_uri: String,
}

impl SsoConfig {
    pub fn from_env(environment: AppEnvironment) -> Result<Option<Self>, ConfigError> {
        match std::env::var("SSO_ENABLED")
            .unwrap_or_else(|_| "false".into())
            .as_str()
        {
            "false" => return Ok(None),
            "true" => {}
            _ => return Err(ConfigError::Sso("SSO_ENABLED must be true or false")),
        }
        let issuer =
            std::env::var("SSO_ISSUER").unwrap_or_else(|_| "https://accounts.knotree.com".into());
        let config = Self {
            service_origin: accounts_service_origin(&issuer)?,
            issuer,
            client_id: std::env::var("SSO_CLIENT_ID").unwrap_or_else(|_| "knotree-registry".into()),
            redirect_uri: std::env::var("SSO_REDIRECT_URI")
                .unwrap_or_else(|_| "https://registry.knotree.com/api/v1/auth/sso/callback".into()),
        };
        config.validate(environment == AppEnvironment::Production)?;
        Ok(Some(config))
    }
    pub fn validate(&self, production: bool) -> Result<(), ConfigError> {
        for value in [&self.issuer, &self.redirect_uri] {
            let url = Url::parse(value).map_err(|_| ConfigError::Sso("invalid URL"))?;
            let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
            if url.host_str().is_none()
                || !(url.scheme() == "https" || (!production && local && url.scheme() == "http"))
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err(ConfigError::Sso(
                    "URLs require HTTPS and must not include credentials, query or fragment",
                ));
            }
        }
        if Url::parse(&self.issuer)
            .map_err(|_| ConfigError::Sso("invalid issuer"))?
            .path()
            != "/"
            || self.issuer.ends_with('/')
        {
            return Err(ConfigError::Sso(
                "issuer must be an origin without trailing slash",
            ));
        }
        if self.client_id.is_empty() || self.client_id.len() > 128 {
            return Err(ConfigError::Sso("invalid client id"));
        }
        Ok(())
    }
}

fn accounts_service_origin(issuer: &str) -> Result<String, ConfigError> {
    let value = std::env::var("SSO_SERVICE_ORIGIN").unwrap_or_default();
    if value.is_empty() {
        return Ok(issuer.to_string());
    }
    let url = Url::parse(&value).map_err(|_| ConfigError::Sso("invalid service origin"))?;
    if url.scheme() == "http"
        && url.host_str() == Some("knotree-accounts.knotree-accounts.svc.cluster.local")
        && url.port().unwrap_or(80) == 80
        && matches!(url.path(), "" | "/")
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
    {
        return Ok(value);
    }
    Err(ConfigError::Sso(
        "SSO_SERVICE_ORIGIN must be the in-cluster Accounts service",
    ))
}

pub(crate) struct LoginAttempt {
    verifier: String,
    config: SsoConfig,
    expires: Instant,
    return_to: String,
}
pub(crate) type LoginAttempts = HashMap<[u8; 32], LoginAttempt>;

fn random_token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}
fn hash(value: &str) -> [u8; 32] {
    Sha256::digest(value.as_bytes()).into()
}
fn attempt_key(state: &str, browser: &str) -> [u8; 32] {
    hash(&format!("{state}:{browser}"))
}
fn invalid_login() -> AppError {
    registry_auth::AuthError::InvalidCredentials.into()
}
fn cookie_name(config: &AppConfig) -> &'static str {
    if config.cookie_secure {
        "__Host-kntr-sso"
    } else {
        "kntr-sso"
    }
}
fn cookie(config: &AppConfig, value: &str, age: u64) -> axum::http::HeaderValue {
    format!(
        "{}={value}; Max-Age={age}; Path=/; HttpOnly; SameSite=Lax{}",
        cookie_name(config),
        if config.cookie_secure { "; Secure" } else { "" }
    )
    .parse()
    .expect("generated cookie is valid")
}
fn configured(state: &AppState) -> Result<&SsoConfig, AppError> {
    state
        .config
        .sso
        .as_ref()
        .ok_or(AppError::BadRequest("Accounts SSO is not configured"))
}

pub(crate) async fn configuration(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({"enabled":state.config.sso.is_some()}))
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StartQuery {
    return_to: Option<String>,
    /// `signup` opens Knotree account creation instead of sign-in.
    intent: Option<String>,
}

/// Only same-site paths, so a sign-in can never redirect off Registry.
fn safe_return_to(value: Option<&str>) -> String {
    value
        .filter(|path| {
            path.starts_with('/')
                && !path.starts_with("//")
                && !path.contains('\\')
                && path.len() <= 512
                && !path.chars().any(char::is_control)
                && !path.starts_with("/api/")
        })
        .map(str::to_owned)
        .unwrap_or_else(|| "/".into())
}

pub(crate) async fn start(
    State(state): State<AppState>,
    Query(query): Query<StartQuery>,
) -> Result<Response, AppError> {
    let config = configured(&state)?;
    let state_token = random_token();
    let browser = random_token();
    let verifier = random_token();
    let challenge = URL_SAFE_NO_PAD.encode(hash(&verifier));
    let mut attempts = state.sso_attempts.lock().await;
    attempts.retain(|_, a| a.expires > Instant::now());
    if attempts.len() >= 4096 {
        return Err(AppError::BadRequest("Too many pending sign-in requests"));
    }
    attempts.insert(
        attempt_key(&state_token, &browser),
        LoginAttempt {
            verifier,
            config: config.clone(),
            return_to: safe_return_to(query.return_to.as_deref()),
            expires: Instant::now() + Duration::from_secs(600),
        },
    );
    drop(attempts);
    let mut url =
        Url::parse(&format!("{}/oauth/authorize", config.issuer)).map_err(|_| invalid_login())?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &config.client_id)
        .append_pair("redirect_uri", &config.redirect_uri)
        .append_pair("scope", "openid profile email")
        .append_pair("state", &state_token)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256");
    if query.intent.as_deref() == Some("signup") {
        url.query_pairs_mut().append_pair("screen_hint", "signup");
    }
    let mut response = Redirect::to(url.as_str()).into_response();
    response
        .headers_mut()
        .append(header::SET_COOKIE, cookie(&state.config, &browser, 600));
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    Ok(response)
}

#[derive(Deserialize)]
pub(crate) struct CallbackQuery {
    state: Option<String>,
    code: Option<String>,
    error: Option<String>,
}
#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    token_type: String,
}
#[derive(Deserialize)]
struct Profile {
    sub: String,
    email: String,
    email_verified: bool,
}

pub(crate) async fn callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<CallbackQuery>,
) -> Result<Response, AppError> {
    let config = configured(&state)?;
    let state_token = query
        .state
        .filter(|v| v.len() == 64)
        .ok_or_else(invalid_login)?;
    let prefix = format!("{}=", cookie_name(&state.config));
    let browser = headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|h| h.to_str().ok())
        .flat_map(|h| h.split(';'))
        .find_map(|c| c.trim().strip_prefix(&prefix))
        .filter(|v| v.len() == 64)
        .ok_or_else(invalid_login)?;
    // Removing by the combined browser/state key is atomic and single-use.
    // A restart safely invalidates in-flight login attempts; session identities persist.
    let attempt = state
        .sso_attempts
        .lock()
        .await
        .remove(&attempt_key(&state_token, browser))
        .ok_or_else(invalid_login)?;
    if attempt.expires <= Instant::now()
        || attempt.config.issuer != config.issuer
        || attempt.config.client_id != config.client_id
        || attempt.config.redirect_uri != config.redirect_uri
        || query.error.is_some()
    {
        return Err(invalid_login());
    }
    let code = query
        .code
        .filter(|v| !v.is_empty() && v.len() <= 2048)
        .ok_or_else(invalid_login)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| invalid_login())?;
    let tokens = client
        .post(format!("{}/oauth/token", config.service_origin))
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", &config.client_id),
            ("redirect_uri", &config.redirect_uri),
            ("code", &code),
            ("code_verifier", &attempt.verifier),
        ])
        .send()
        .await
        .map_err(|_| invalid_login())?;
    let tokens: TokenResponse = bounded_json(tokens).await?;
    if !tokens.token_type.eq_ignore_ascii_case("bearer") || tokens.access_token.is_empty() {
        return Err(invalid_login());
    }
    let profile = client
        .get(format!("{}/oauth/userinfo", config.service_origin))
        .bearer_auth(&tokens.access_token)
        .send()
        .await
        .map_err(|_| invalid_login())?;
    let profile: Profile = bounded_json(profile).await?;
    if !profile.email_verified
        || !profile.email.contains('@')
        || profile
            .email
            .chars()
            .any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(invalid_login());
    }
    let admin = state
        .config
        .sso_admin_subjects
        .iter()
        .any(|subject| subject == &profile.sub);
    let session = state
        .auth
        .login_federated_as(&config.issuer, &profile.sub, admin)
        .await?;
    let mut event = RegistryEvent::new(EventKind::LoginSucceeded);
    event.actor = Some(session.user.username.clone());
    state.record_event(event).await;
    // Persist before issuing a successful browser session.
    if let Err(error) = state.persist_runtime_state().await {
        let _ = state.auth.logout(&session.token).await;
        return Err(error);
    }
    let mut response = Redirect::to(&attempt.return_to).into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        crate::routes::session_cookie(&state, &session.token, session.expires_at),
    );
    response
        .headers_mut()
        .append(header::SET_COOKIE, cookie(&state.config, "", 0));
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
        .headers_mut()
        .insert("referrer-policy", "no-referrer".parse().unwrap());
    Ok(response)
}

async fn bounded_json<T: serde::de::DeserializeOwned>(
    mut response: reqwest::Response,
) -> Result<T, AppError> {
    if !response.status().is_success() {
        return Err(invalid_login());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| invalid_login())? {
        if bytes.len() + chunk.len() > 16 * 1024 {
            return Err(invalid_login());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| invalid_login())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn return_path_stays_on_registry() {
        assert_eq!(safe_return_to(Some("/repositories/kt-a/app")), "/repositories/kt-a/app");
        assert_eq!(safe_return_to(None), "/");
        for value in [
            "//attacker.example",
            "/\\attacker.example",
            "https://attacker.example",
            "/api/v1/auth/logout",
            "/a\nb",
        ] {
            assert_eq!(safe_return_to(Some(value)), "/", "{value}");
        }
    }
    #[test]
    fn validates_provider_origin_and_https_callbacks() {
        let mut config = SsoConfig {
            issuer: "https://accounts.knotree.com".into(),
            service_origin: "https://accounts.knotree.com".into(),
            client_id: "knotree-registry".into(),
            redirect_uri: "https://registry.knotree.com/api/v1/auth/sso/callback".into(),
        };
        assert!(config.validate(true).is_ok());
        for issuer in [
            "http://accounts.knotree.com",
            "https://user:pass@accounts.knotree.com",
            "https://accounts.knotree.com/path",
            "https://accounts.knotree.com?next=other",
            "https://accounts.knotree.com/",
        ] {
            config.issuer = issuer.into();
            assert!(config.validate(true).is_err());
        }
        config.issuer = "http://localhost:8000".into();
        assert!(config.validate(false).is_ok());
        assert!(config.validate(true).is_err());
    }
}
