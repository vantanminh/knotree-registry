use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::{HeaderValue, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
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
