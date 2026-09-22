use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("configuration error: {0}")]
    Config(#[from] crate::ConfigError),
    #[error("storage error: {0}")]
    Storage(#[from] registry_storage::StorageError),
    #[error("database error: {0}")]
    Database(#[from] registry_db::DatabaseError),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        tracing::error!(error = %self, "request failed");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "internal_error"})),
        )
            .into_response()
    }
}
