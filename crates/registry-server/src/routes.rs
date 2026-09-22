use std::collections::BTreeSet;

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, State},
    http::{
        HeaderMap, HeaderValue, Request, StatusCode,
        header::{AUTHORIZATION, COOKIE, SET_COOKIE},
    },
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use registry_auth::parse_scope;
use registry_core::{Action, RepositoryName, RepositoryScope};
use serde::Deserialize;
use serde_json::json;
use tower_http::trace::TraceLayer;
use uuid::Uuid;

use crate::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/livez", get(livez))
        .route("/readyz", get(readyz))
        .route("/health/live", get(livez))
        .route("/health/ready", get(readyz))
        .route("/v2/", get(distribution_version))
        .route("/auth/token", get(token))
        .route("/api/v1/auth/login", post(login))
        .route("/api/v1/auth/logout", post(logout))
        .route("/api/v1/auth/me", get(me))
        .route("/api/v1/auth/tokens", post(create_token))
        .route("/api/v1/auth/tokens/{id}/revoke", post(revoke_token))
        .with_state(state)
        .layer(middleware::from_fn(request_id))
        .layer(TraceLayer::new_for_http())
}

async fn request_id(mut request: Request<Body>, next: Next) -> Response {
    let request_id = request
        .headers()
        .get("x-request-id")
        .cloned()
        .unwrap_or_else(|| {
            HeaderValue::from_str(&Uuid::new_v4().to_string())
                .expect("UUID is a valid header value")
        });
    request
        .headers_mut()
        .insert("x-request-id", request_id.clone());
    let mut response = next.run(request).await;
    response.headers_mut().insert("x-request-id", request_id);
    response
}

async fn index(State(state): State<AppState>) -> impl IntoResponse {
    Json(
        json!({"service": "knotree-registry", "version": env!("CARGO_PKG_VERSION"), "uptime_seconds": state.started_at.elapsed().as_secs()}),
    )
}

async fn livez() -> impl IntoResponse {
    Json(json!({"status": "ok"}))
}

async fn readyz(State(state): State<AppState>) -> impl IntoResponse {
    let readiness = state.readiness().await;
    let status = if readiness.status == "ok" {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (status, Json(readiness))
}

async fn distribution_version() -> impl IntoResponse {
    (StatusCode::OK, Json(json!({})))
}

#[derive(Debug, Deserialize)]
struct TokenQuery {
    service: Option<String>,
    scope: Option<String>,
}

async fn token(
    State(state): State<AppState>,
    Query(query): Query<TokenQuery>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, crate::AppError> {
    let service = query
        .service
        .as_deref()
        .ok_or(crate::AppError::BadRequest("service is required"))?;
    let (username, password) =
        basic_credentials(&headers).ok_or(registry_auth::AuthError::InvalidCredentials)?;
    let requested = query
        .scope
        .iter()
        .flat_map(|value| value.split_whitespace())
        .map(parse_scope)
        .collect::<Result<Vec<_>, _>>()?;
    if requested.is_empty() {
        return Err(crate::AppError::BadRequest("scope is required"));
    }
    let minted = state
        .auth
        .mint_token(&username, &password, service, &requested)
        .await?;
    let issued_at = chrono::DateTime::<chrono::Utc>::from_timestamp(minted.issued_at as i64, 0)
        .ok_or(crate::AppError::BadRequest("invalid token time"))?
        .to_rfc3339();
    Ok(Json(json!({
        "token": minted.token,
        "access_token": minted.token,
        "expires_in": minted.expires_in,
        "issued_at": issued_at,
    })))
}

#[derive(Debug, Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

async fn login(
    State(state): State<AppState>,
    Json(input): Json<LoginRequest>,
) -> Result<Response, crate::AppError> {
    let session = state.auth.login(&input.username, &input.password).await?;
    let mut response =
        Json(json!({"user": session.user, "expires_at": session.expires_at})).into_response();
    response.headers_mut().insert(
        SET_COOKIE,
        session_cookie(&state, &session.token, session.expires_at),
    );
    Ok(response)
}

async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, crate::AppError> {
    if let Some(session) = cookie_value(&headers) {
        state.auth.logout(session).await?;
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    response
        .headers_mut()
        .insert(SET_COOKIE, expired_session_cookie(&state));
    Ok(response)
}

async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, crate::AppError> {
    let session = cookie_value(&headers).ok_or(registry_auth::AuthError::InvalidSession)?;
    Ok(Json(state.auth.session_user(session).await?))
}

#[derive(Debug, Deserialize)]
struct CreateTokenRequest {
    name: String,
    scopes: Vec<ScopeInput>,
    expires_at: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ScopeInput {
    repository: String,
    actions: BTreeSet<Action>,
}

async fn create_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateTokenRequest>,
) -> Result<impl IntoResponse, crate::AppError> {
    let session = cookie_value(&headers).ok_or(registry_auth::AuthError::InvalidSession)?;
    let scopes = input
        .scopes
        .into_iter()
        .map(|scope| {
            let repository = RepositoryName::parse(&scope.repository)
                .map_err(|_| registry_auth::AuthError::InvalidScope)?;
            if scope.actions.is_empty() {
                return Err(registry_auth::AuthError::InvalidScope);
            }
            Ok(RepositoryScope {
                repository,
                actions: scope.actions,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(
        state
            .auth
            .create_credential_for_session(session, input.name, scopes, input.expires_at)
            .await?,
    ))
}

async fn revoke_token(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, crate::AppError> {
    let session = cookie_value(&headers).ok_or(registry_auth::AuthError::InvalidSession)?;
    state
        .auth
        .revoke_credential_for_session(session, id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

fn basic_credentials(headers: &HeaderMap) -> Option<(String, String)> {
    let value = headers
        .get(AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Basic ")?;
    let decoded = STANDARD.decode(value).ok()?;
    let credentials = String::from_utf8(decoded).ok()?;
    let (username, password) = credentials.split_once(':')?;
    Some((username.to_owned(), password.to_owned()))
}

fn cookie_value(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(COOKIE)?.to_str().ok()?;
    value
        .split(';')
        .find_map(|part| part.trim().strip_prefix("kntr_session="))
}

fn session_cookie(state: &AppState, token: &str, expires_at: u64) -> HeaderValue {
    let max_age = expires_at.saturating_sub(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    );
    let secure = if state.config.cookie_secure {
        "; Secure"
    } else {
        ""
    };
    HeaderValue::from_str(&format!(
        "kntr_session={token}; Max-Age={max_age}; Path=/; HttpOnly; SameSite=Strict{secure}"
    ))
    .expect("generated session cookie is valid")
}

fn expired_session_cookie(state: &AppState) -> HeaderValue {
    let secure = if state.config.cookie_secure {
        "; Secure"
    } else {
        ""
    };
    HeaderValue::from_str(&format!(
        "kntr_session=; Max-Age=0; Path=/; HttpOnly; SameSite=Strict{secure}"
    ))
    .expect("generated session cookie is valid")
}

#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::util::ServiceExt;

    use super::*;
    use crate::{AppConfig, AppState, StorageBackend};

    fn test_config() -> AppConfig {
        AppConfig {
            bind_addr: "127.0.0.1:0".parse().expect("addr"),
            public_url: "http://localhost:8080".to_owned(),
            database_url: None,
            database_max_connections: 1,
            require_database: false,
            storage_backend: StorageBackend::Memory,
            storage_root: std::env::temp_dir(),
            request_body_limit_bytes: 1024,
            token_issuer: "knotree-registry".to_owned(),
            token_service: "knotree-registry".to_owned(),
            token_ttl_seconds: 300,
            bootstrap_admin_username: None,
            bootstrap_admin_password: None,
            cookie_secure: false,
        }
    }

    #[tokio::test]
    async fn health_and_distribution_routes_are_available() {
        let app = router(AppState::initialize(test_config()).await.expect("state"));
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v2/")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let response = router(AppState::initialize(test_config()).await.expect("state"))
            .oneshot(
                Request::builder()
                    .uri("/livez")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key("x-request-id"));
    }
}
