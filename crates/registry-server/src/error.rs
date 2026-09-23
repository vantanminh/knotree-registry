use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("authentication error: {0}")]
    Auth(#[from] registry_auth::AuthError),
    #[error("configuration error: {0}")]
    Config(#[from] crate::ConfigError),
    #[error("storage error: {0}")]
    Storage(#[from] registry_storage::StorageError),
    #[error("database error: {0}")]
    Database(#[from] registry_db::DatabaseError),
    #[error("catalog error: {0}")]
    Catalog(#[from] crate::CatalogError),
    #[error("bad request: {0}")]
    BadRequest(&'static str),
    #[error("persistent runtime state error: {0}")]
    State(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match &self {
            Self::Auth(registry_auth::AuthError::InvalidCredentials)
            | Self::Auth(registry_auth::AuthError::InvalidSession)
            | Self::Auth(registry_auth::AuthError::CredentialRevoked)
            | Self::Auth(registry_auth::AuthError::CredentialExpired)
            | Self::Auth(registry_auth::AuthError::Bearer)
            | Self::Auth(registry_auth::AuthError::TwoFactorRequired)
            | Self::Auth(registry_auth::AuthError::InvalidTwoFactorCode) => {
                StatusCode::UNAUTHORIZED
            }
            Self::Auth(registry_auth::AuthError::NoAccess) => StatusCode::FORBIDDEN,
            Self::Auth(registry_auth::AuthError::TwoFactorAlreadyEnabled) => StatusCode::CONFLICT,
            Self::Auth(
                registry_auth::AuthError::TwoFactorNotEnabled
                | registry_auth::AuthError::TwoFactorSetupMissing
                | registry_auth::AuthError::WeakPassword
                | registry_auth::AuthError::InvalidScope,
            )
            | Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Catalog(crate::CatalogError::NotFound) => StatusCode::NOT_FOUND,
            Self::Catalog(crate::CatalogError::InvalidTag)
            | Self::Catalog(crate::CatalogError::Manifest(_)) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        if status != StatusCode::INTERNAL_SERVER_ERROR {
            let error = match &self {
                Self::Auth(registry_auth::AuthError::TwoFactorRequired) => "two_factor_required",
                Self::Auth(registry_auth::AuthError::InvalidTwoFactorCode) => "two_factor_invalid",
                Self::Auth(registry_auth::AuthError::WeakPassword) => "weak_password",
                Self::Auth(registry_auth::AuthError::TwoFactorAlreadyEnabled) => {
                    "two_factor_already_enabled"
                }
                Self::Auth(registry_auth::AuthError::TwoFactorNotEnabled) => {
                    "two_factor_not_enabled"
                }
                Self::Auth(registry_auth::AuthError::TwoFactorSetupMissing) => {
                    "two_factor_setup_missing"
                }
                Self::BadRequest("passwords do not match") => "password_mismatch",
                _ if status == StatusCode::FORBIDDEN => "forbidden",
                _ if status == StatusCode::BAD_REQUEST => "bad_request",
                _ => "unauthorized",
            };
            return (status, Json(json!({"error": error}))).into_response();
        }
        tracing::error!(error = %self, "request failed");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "internal_error"})),
        )
            .into_response()
    }
}
