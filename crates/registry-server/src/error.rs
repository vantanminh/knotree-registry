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
            | Self::Auth(registry_auth::AuthError::Bearer) => StatusCode::UNAUTHORIZED,
            Self::Auth(registry_auth::AuthError::NoAccess) => StatusCode::FORBIDDEN,
            Self::Catalog(crate::CatalogError::NotFound) => StatusCode::NOT_FOUND,
            Self::Catalog(crate::CatalogError::InvalidTag)
            | Self::Catalog(crate::CatalogError::Manifest(_)) => StatusCode::BAD_REQUEST,
            Self::BadRequest(_) | Self::Auth(registry_auth::AuthError::InvalidScope) => {
                StatusCode::BAD_REQUEST
            }
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        if status != StatusCode::INTERNAL_SERVER_ERROR {
            let error = if status == StatusCode::FORBIDDEN {
                "forbidden"
            } else if status == StatusCode::BAD_REQUEST {
                "bad_request"
            } else {
                "unauthorized"
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
