use std::{
    collections::BTreeSet,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, RawQuery, State},
    http::{
        HeaderMap, HeaderName, HeaderValue, Method, Request, StatusCode,
        header::{
            AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, COOKIE, ETAG, LINK, LOCATION, ORIGIN,
            SET_COOKIE, WWW_AUTHENTICATE,
        },
    },
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{any, get, post},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use hmac::{Hmac, Mac};
use registry_auth::parse_scope;
use registry_core::{
    Action, Digest, OCI_IMAGE_INDEX, RepositoryName, RepositoryScope, validate_manifest,
};
use registry_events::{EventKind, RegistryEvent};
use serde::Deserialize;
use serde_json::json;
use sha2::Sha256;
use tower_http::trace::TraceLayer;
use uuid::Uuid;

use crate::{AppState, PullMode};
use crate::{UploadError, blob_key, manifest_key};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/livez", get(livez))
        .route("/readyz", get(readyz))
        .route("/health/live", get(livez))
        .route("/health/ready", get(readyz))
        .route("/metrics", get(metrics))
        .route("/v2/", get(distribution_version))
        .route("/v2/{*path}", any(oci_route))
        .route("/auth/token", get(token))
        .route("/api/v1/auth/login", post(login))
        .route("/api/v1/auth/logout", post(logout))
        .route("/api/v1/auth/me", get(me))
        .route("/api/v1/auth/tokens", get(list_tokens).post(create_token))
        .route("/api/v1/auth/tokens/{id}/revoke", post(revoke_token))
        .route("/api/v1/overview", get(overview))
        .route("/api/v1/repositories", get(list_repositories))
        .route("/api/v1/repositories/{*repository}", get(repository_detail))
        .route("/api/v1/audit", get(list_audit))
        .route("/api/v1/webhooks", get(list_webhooks).post(create_webhook))
        .route("/api/v1/webhooks/{id}/disable", post(disable_webhook))
        .route("/api/v1/admin/gc", post(run_gc))
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            control_body_limit,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            metrics_middleware,
        ))
        .layer(middleware::from_fn(request_id))
        .layer(TraceLayer::new_for_http())
}

async fn metrics_middleware(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    state.metrics.observe_request();
    let response = next.run(request).await;
    state.metrics.observe_response(response.status().as_u16());
    response
}

async fn control_body_limit(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let is_control_plane = request.uri().path().starts_with("/api/");
    let is_state_change = !matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );
    let csrf_violation = is_control_plane
        && is_state_change
        && request
            .headers()
            .get(ORIGIN)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|origin| !allowed_origin(&state, origin));
    if csrf_violation {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error": "origin_not_allowed"})),
        )
            .into_response();
    }
    let too_large = is_control_plane
        && request
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<usize>().ok())
            .is_some_and(|length| length > state.config.request_body_limit_bytes);
    if too_large {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(json!({"error": "request_too_large"})),
        )
            .into_response();
    }
    next.run(request).await
}

fn allowed_origin(state: &AppState, origin: &str) -> bool {
    if state
        .config
        .control_plane_origins
        .iter()
        .any(|allowed| allowed == origin)
    {
        return true;
    }
    let Ok(origin_url) = url::Url::parse(origin) else {
        return false;
    };
    let Ok(public_url) = url::Url::parse(&state.config.public_url) else {
        return false;
    };
    origin_url.scheme() == public_url.scheme()
        && origin_url.host_str() == public_url.host_str()
        && origin_url.port_or_known_default() == public_url.port_or_known_default()
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

async fn metrics(State(state): State<AppState>) -> impl IntoResponse {
    (
        [(CONTENT_TYPE, "text/plain; version=0.0.4".to_owned())],
        state.metrics.render_prometheus(),
    )
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

async fn distribution_version(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(token) = bearer_token(&headers)
        && state.auth.verify_service_token(token).await.is_ok()
    {
        return version_response();
    }
    challenge_response(&state, None)
}

#[derive(Debug)]
enum OciPath {
    Manifest {
        repository: RepositoryName,
        reference: String,
    },
    Blob {
        repository: RepositoryName,
        digest: String,
    },
    Tags {
        repository: RepositoryName,
    },
    Referrers {
        repository: RepositoryName,
        digest: String,
    },
    UploadStart {
        repository: RepositoryName,
    },
    Upload {
        repository: RepositoryName,
        id: Uuid,
    },
}

async fn oci_route(State(state): State<AppState>, request: Request<Body>) -> Response {
    if !matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::POST | Method::PATCH | Method::PUT | Method::DELETE
    ) {
        return oci_error(
            StatusCode::METHOD_NOT_ALLOWED,
            "UNSUPPORTED",
            "method is not supported",
        );
    }
    let path = request
        .uri()
        .path()
        .strip_prefix("/v2/")
        .unwrap_or_default();
    let Some(parsed) = parse_oci_path(path) else {
        return oci_error(
            StatusCode::NOT_FOUND,
            "NAME_UNKNOWN",
            "resource was not found",
        );
    };
    let (repository, action) = match &parsed {
        OciPath::Manifest { repository, .. } => (
            repository,
            match *request.method() {
                Method::DELETE => Action::Delete,
                Method::PUT => Action::Push,
                _ => Action::Pull,
            },
        ),
        OciPath::Blob { repository, .. }
        | OciPath::Tags { repository }
        | OciPath::Referrers { repository, .. } => (
            repository,
            if request.method() == Method::DELETE {
                Action::Delete
            } else {
                Action::Pull
            },
        ),
        OciPath::UploadStart { repository } | OciPath::Upload { repository, .. } => {
            (repository, Action::Push)
        }
    };
    let Some(token) = bearer_token(request.headers()).map(str::to_owned) else {
        return challenge_response(&state, Some(repository));
    };
    match state.auth.verify_bearer(&token, repository, action).await {
        Ok(_) => {}
        Err(registry_auth::AuthError::NoAccess) => {
            return oci_error(
                StatusCode::FORBIDDEN,
                "DENIED",
                "requested action is not authorized",
            );
        }
        Err(_) => return challenge_response(&state, Some(repository)),
    }
    let head = request.method() == Method::HEAD;
    match parsed {
        OciPath::Manifest {
            repository,
            reference,
        } => {
            if request.method() == Method::DELETE {
                delete_manifest_response(&state, &repository, &reference).await
            } else if request.method() == Method::PUT {
                put_manifest_response(&state, &repository, &reference, request).await
            } else {
                manifest_response(&state, &repository, &reference, head).await
            }
        }
        OciPath::Blob { repository, digest } => {
            blob_response(&state, &repository, &digest, head).await
        }
        OciPath::Tags { repository } => {
            tags_response(
                &state,
                &repository,
                request.uri().query(),
                request.method() == Method::HEAD,
            )
            .await
        }
        OciPath::Referrers { repository, digest } => {
            referrers_response(&state, &repository, &digest).await
        }
        OciPath::UploadStart { repository } => {
            upload_start(&state, repository, &token, request).await
        }
        OciPath::Upload { repository, id } => upload_session(&state, repository, id, request).await,
    }
}

async fn put_manifest_response(
    state: &AppState,
    repository: &RepositoryName,
    reference: &str,
    request: Request<Body>,
) -> Response {
    let content_type = request
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = match to_body(request, state.config.request_body_limit_bytes).await {
        Ok(body) => body,
        Err(response) => return response,
    };
    let info = match validate_manifest(&body, content_type.as_deref()) {
        Ok(info) => info,
        Err(error) => {
            return oci_error(
                StatusCode::BAD_REQUEST,
                "MANIFEST_INVALID",
                &error.to_string(),
            );
        }
    };
    let digest = Digest::sha256(&body);
    if let Ok(requested_digest) = Digest::parse(reference)
        && requested_digest != digest
    {
        return oci_error(
            StatusCode::BAD_REQUEST,
            "DIGEST_INVALID",
            "manifest bytes do not match the digest reference",
        );
    }
    for descriptor in &info.references {
        let has_blob = state
            .catalog
            .blob_visible(repository, &descriptor.digest)
            .await;
        let has_manifest = state
            .catalog
            .resolve_manifest(repository, descriptor.digest.as_str())
            .await
            .is_ok();
        if !has_blob && !has_manifest {
            return oci_error(
                StatusCode::NOT_FOUND,
                "BLOB_UNKNOWN",
                "a manifest descriptor is not available in this repository",
            );
        }
    }
    if let Err(error) = state.store.put(&manifest_key(&digest), body.clone()).await {
        tracing::error!(error = %error, digest = %digest, "manifest object write failed");
        return oci_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "UNKNOWN",
            "manifest storage failed",
        );
    }
    let manifest = match state
        .catalog
        .publish_manifest(repository, reference, &body, content_type.as_deref())
        .await
    {
        Ok(manifest) => manifest,
        Err(error) => {
            let _ = state.store.delete(&manifest_key(&digest)).await;
            return oci_error(
                StatusCode::BAD_REQUEST,
                "MANIFEST_INVALID",
                &error.to_string(),
            );
        }
    };
    let is_tag = Digest::parse(reference).is_err();
    let mut event = RegistryEvent::new(EventKind::ManifestPushed);
    event.repository = Some(repository.to_string());
    event.digest = Some(manifest.digest.to_string());
    if is_tag {
        event.tag = Some(reference.to_owned());
    }
    state.events.record(event).await;
    if is_tag {
        let mut tag_event = RegistryEvent::new(EventKind::TagUpdated);
        tag_event.repository = Some(repository.to_string());
        tag_event.tag = Some(reference.to_owned());
        tag_event.digest = Some(manifest.digest.to_string());
        state.events.record(tag_event).await;
    }
    let mut response = StatusCode::CREATED.into_response();
    response.headers_mut().insert(
        LOCATION,
        HeaderValue::from_str(&format!("/v2/{repository}/manifests/{}", manifest.digest))
            .expect("manifest location is valid"),
    );
    response.headers_mut().insert(
        HeaderName::from_static("docker-content-digest"),
        HeaderValue::from_str(manifest.digest.as_str()).expect("digest is valid"),
    );
    response
        .headers_mut()
        .insert(CONTENT_LENGTH, HeaderValue::from_static("0"));
    add_protocol_headers(&mut response);
    response
}

fn parse_oci_path(path: &str) -> Option<OciPath> {
    if let Some(repository) = path.strip_suffix("/blobs/uploads/") {
        return Some(OciPath::UploadStart {
            repository: RepositoryName::parse(repository).ok()?,
        });
    }
    if let Some((repository, id)) = path.rsplit_once("/blobs/uploads/") {
        return Some(OciPath::Upload {
            repository: RepositoryName::parse(repository).ok()?,
            id: Uuid::parse_str(id).ok()?,
        });
    }
    if let Some((repository, digest)) = path.rsplit_once("/referrers/") {
        return Some(OciPath::Referrers {
            repository: RepositoryName::parse(repository).ok()?,
            digest: digest.to_owned(),
        });
    }
    if let Some((repository, reference)) = path.rsplit_once("/manifests/") {
        return Some(OciPath::Manifest {
            repository: RepositoryName::parse(repository).ok()?,
            reference: reference.to_owned(),
        });
    }
    if let Some((repository, digest)) = path.rsplit_once("/blobs/") {
        return Some(OciPath::Blob {
            repository: RepositoryName::parse(repository).ok()?,
            digest: digest.to_owned(),
        });
    }
    if let Some(repository) = path.strip_suffix("/tags/list") {
        return Some(OciPath::Tags {
            repository: RepositoryName::parse(repository).ok()?,
        });
    }
    None
}

async fn delete_manifest_response(
    state: &AppState,
    repository: &RepositoryName,
    reference: &str,
) -> Response {
    match state.catalog.delete_manifest(repository, reference).await {
        Ok(manifest) => {
            let mut event = RegistryEvent::new(EventKind::ManifestDeleted);
            event.repository = Some(repository.to_string());
            event.digest = Some(manifest.digest.to_string());
            if Digest::parse(reference).is_err() {
                event.tag = Some(reference.to_owned());
            }
            state.events.record(event).await;
            let mut response = StatusCode::ACCEPTED.into_response();
            response.headers_mut().insert(
                HeaderName::from_static("docker-content-digest"),
                HeaderValue::from_str(manifest.digest.as_str()).expect("digest is valid"),
            );
            add_protocol_headers(&mut response);
            response
        }
        Err(crate::CatalogError::NotFound) => oci_error(
            StatusCode::NOT_FOUND,
            "MANIFEST_UNKNOWN",
            "manifest was not found",
        ),
        Err(_) => oci_error(
            StatusCode::BAD_REQUEST,
            "MANIFEST_INVALID",
            "manifest reference is invalid",
        ),
    }
}

async fn referrers_response(
    state: &AppState,
    repository: &RepositoryName,
    value: &str,
) -> Response {
    let subject = match Digest::parse(value) {
        Ok(digest) => digest,
        Err(_) => {
            return oci_error(
                StatusCode::BAD_REQUEST,
                "DIGEST_INVALID",
                "digest is invalid",
            );
        }
    };
    let manifests = match state.catalog.referrers(repository, &subject).await {
        Ok(manifests) => manifests,
        Err(crate::CatalogError::NotFound) => {
            return oci_error(
                StatusCode::NOT_FOUND,
                "NAME_UNKNOWN",
                "repository was not found",
            );
        }
        Err(_) => {
            return oci_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "UNKNOWN",
                "catalog failure",
            );
        }
    };
    let body = serde_json::to_vec(&json!({
        "schemaVersion": 2,
        "mediaType": OCI_IMAGE_INDEX,
        "manifests": manifests.into_iter().map(|manifest| json!({"mediaType": manifest.media_type, "digest": manifest.digest, "size": manifest.size})).collect::<Vec<_>>(),
    })).expect("referrers response is serializable");
    let mut response = content_response(
        StatusCode::OK,
        bytes::Bytes::from(body),
        false,
        "application/json",
        &Digest::sha256(&[]),
        None,
        None,
    );
    response.headers_mut().remove("docker-content-digest");
    response
}

async fn upload_start(
    state: &AppState,
    repository: RepositoryName,
    token: &str,
    request: Request<Body>,
) -> Response {
    let _ = state.uploads.cleanup_expired(&state.store).await;
    let query = request.uri().query().unwrap_or_default();
    if let Some(mount) = query_value(query, "mount")
        && let Ok(digest) = Digest::parse(&mount)
        && let Some(from) =
            query_value(query, "from").and_then(|value| RepositoryName::parse(&value).ok())
        && state
            .auth
            .verify_bearer(token, &from, Action::Pull)
            .await
            .is_ok()
        && state.catalog.blob_visible(&from, &digest).await
    {
        state.catalog.attach_blob(&repository, digest.clone()).await;
        let mut response = StatusCode::CREATED.into_response();
        response.headers_mut().insert(
            LOCATION,
            HeaderValue::from_str(&format!("/v2/{repository}/blobs/{digest}"))
                .expect("location is valid"),
        );
        response.headers_mut().insert(
            HeaderName::from_static("docker-content-digest"),
            HeaderValue::from_str(digest.as_str()).expect("digest is valid"),
        );
        add_protocol_headers(&mut response);
        return response;
    }
    let session = state.uploads.create(repository.clone()).await;
    if let Some(digest_value) = query_value(query, "digest") {
        let digest = match Digest::parse(&digest_value) {
            Ok(digest) => digest,
            Err(_) => {
                return oci_error(
                    StatusCode::BAD_REQUEST,
                    "DIGEST_INVALID",
                    "digest is invalid",
                );
            }
        };
        let body = match to_body(request, state.config.upload_chunk_limit_bytes).await {
            Ok(body) => body,
            Err(response) => return response,
        };
        match state
            .uploads
            .finalize(session.id, digest, body, &state.store)
            .await
        {
            Ok(finalized) => {
                state
                    .catalog
                    .attach_blob(&finalized.repository, finalized.digest.clone())
                    .await;
                finalized_response(&finalized)
            }
            Err(error) => upload_error(error),
        }
    } else {
        upload_progress_response(StatusCode::ACCEPTED, &session)
    }
}

async fn upload_session(
    state: &AppState,
    repository: RepositoryName,
    id: Uuid,
    request: Request<Body>,
) -> Response {
    let status = match state.uploads.status(id).await {
        Ok(status) if status.repository == repository => status,
        Ok(_) => {
            return oci_error(
                StatusCode::NOT_FOUND,
                "BLOB_UPLOAD_UNKNOWN",
                "upload was not found",
            );
        }
        Err(error) => return upload_error(error),
    };
    match *request.method() {
        Method::GET | Method::HEAD => upload_progress_response(StatusCode::NO_CONTENT, &status),
        Method::PATCH => {
            let headers = request.headers().clone();
            let body = match to_body(request, state.config.upload_chunk_limit_bytes).await {
                Ok(body) => body,
                Err(response) => return response,
            };
            let expected = match content_range_start(&headers, status.offset, body.len() as u64) {
                Ok(expected) => expected,
                Err(_) => {
                    return upload_error(UploadError::OffsetMismatch {
                        expected: status.offset,
                        actual: status.offset.saturating_add(1),
                    });
                }
            };
            match state.uploads.append(id, expected, body, &state.store).await {
                Ok(status) => upload_progress_response(StatusCode::ACCEPTED, &status),
                Err(error) => upload_error(error),
            }
        }
        Method::PUT => {
            let digest = match query_value(request.uri().query().unwrap_or_default(), "digest")
                .and_then(|value| Digest::parse(&value).ok())
            {
                Some(digest) => digest,
                None => {
                    return oci_error(
                        StatusCode::BAD_REQUEST,
                        "DIGEST_INVALID",
                        "digest query parameter is required",
                    );
                }
            };
            let body = match to_body(request, state.config.upload_chunk_limit_bytes).await {
                Ok(body) => body,
                Err(response) => return response,
            };
            match state.uploads.finalize(id, digest, body, &state.store).await {
                Ok(finalized) => {
                    state
                        .catalog
                        .attach_blob(&finalized.repository, finalized.digest.clone())
                        .await;
                    finalized_response(&finalized)
                }
                Err(error) => upload_error(error),
            }
        }
        Method::DELETE => match state.uploads.abort(id, &state.store).await {
            Ok(()) => StatusCode::NO_CONTENT.into_response(),
            Err(error) => upload_error(error),
        },
        _ => oci_error(
            StatusCode::METHOD_NOT_ALLOWED,
            "UNSUPPORTED",
            "method is not supported",
        ),
    }
}

async fn to_body(request: Request<Body>, limit: usize) -> Result<bytes::Bytes, Response> {
    axum::body::to_bytes(request.into_body(), limit)
        .await
        .map_err(|_| {
            oci_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "BLOB_UPLOAD_INVALID",
                "upload chunk is too large",
            )
        })
}

fn content_range_start(headers: &HeaderMap, default: u64, body_len: u64) -> Result<u64, ()> {
    let Some(value) = headers.get("content-range") else {
        return Ok(default);
    };
    let value = value.to_str().map_err(|_| ())?;
    let value = value.strip_prefix("bytes ").unwrap_or(value);
    let (start, end) = value.split_once('-').ok_or(())?;
    let start = start.parse::<u64>().map_err(|_| ())?;
    let end = end.parse::<u64>().map_err(|_| ())?;
    if end < start || end - start + 1 != body_len {
        return Err(());
    }
    Ok(start)
}

fn query_value(query: &str, key: &str) -> Option<String> {
    url::form_urlencoded::parse(query.as_bytes())
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.into_owned())
}

fn query_escape(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

fn upload_progress_response(status_code: StatusCode, status: &crate::UploadStatus) -> Response {
    let mut response = status_code.into_response();
    response.headers_mut().insert(
        LOCATION,
        HeaderValue::from_str(&format!(
            "/v2/{}/blobs/uploads/{}",
            status.repository, status.id
        ))
        .expect("location is valid"),
    );
    response.headers_mut().insert(
        HeaderName::from_static("docker-upload-uuid"),
        HeaderValue::from_str(&status.id.to_string()).expect("uuid is valid"),
    );
    response.headers_mut().insert(
        HeaderName::from_static("range"),
        HeaderValue::from_str(&format!("bytes=0-{}", status.offset.saturating_sub(1)))
            .expect("range is valid"),
    );
    add_protocol_headers(&mut response);
    response
}

fn finalized_response(finalized: &crate::FinalizedUpload) -> Response {
    let mut response = StatusCode::CREATED.into_response();
    response.headers_mut().insert(
        LOCATION,
        HeaderValue::from_str(&format!(
            "/v2/{}/blobs/{}",
            finalized.repository, finalized.digest
        ))
        .expect("location is valid"),
    );
    response.headers_mut().insert(
        HeaderName::from_static("docker-content-digest"),
        HeaderValue::from_str(finalized.digest.as_str()).expect("digest is valid"),
    );
    response
        .headers_mut()
        .insert(CONTENT_LENGTH, HeaderValue::from_static("0"));
    add_protocol_headers(&mut response);
    response
}

fn upload_error(error: crate::UploadError) -> Response {
    match error {
        crate::UploadError::OffsetMismatch { .. } => oci_error(
            StatusCode::RANGE_NOT_SATISFIABLE,
            "RANGE_INVALID",
            "upload range is not contiguous",
        ),
        crate::UploadError::DigestMismatch { .. } => oci_error(
            StatusCode::BAD_REQUEST,
            "DIGEST_INVALID",
            "uploaded content digest does not match",
        ),
        crate::UploadError::NotFound | crate::UploadError::Expired => oci_error(
            StatusCode::NOT_FOUND,
            "BLOB_UPLOAD_UNKNOWN",
            "upload was not found",
        ),
        crate::UploadError::InvalidState => oci_error(
            StatusCode::CONFLICT,
            "BLOB_UPLOAD_INVALID",
            "upload is no longer active",
        ),
        crate::UploadError::Storage(_) => oci_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "UNKNOWN",
            "storage failure",
        ),
    }
}

async fn manifest_response(
    state: &AppState,
    repository: &RepositoryName,
    reference: &str,
    head: bool,
) -> Response {
    let manifest = match state.catalog.resolve_manifest(repository, reference).await {
        Ok(manifest) => manifest,
        Err(_) => {
            return oci_error(
                StatusCode::NOT_FOUND,
                "MANIFEST_UNKNOWN",
                "manifest was not found",
            );
        }
    };
    let body = match state.store.get(&manifest_key(&manifest.digest)).await {
        Ok(body) => body,
        Err(registry_storage::StorageError::NotFound) => {
            return oci_error(
                StatusCode::NOT_FOUND,
                "MANIFEST_UNKNOWN",
                "manifest was not found",
            );
        }
        Err(_) => {
            return oci_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "UNKNOWN",
                "storage failure",
            );
        }
    };
    content_response(
        StatusCode::OK,
        body,
        head,
        &manifest.media_type,
        &manifest.digest,
        None,
        None,
    )
}

async fn blob_response(
    state: &AppState,
    repository: &RepositoryName,
    value: &str,
    head: bool,
) -> Response {
    let digest = match Digest::parse(value) {
        Ok(digest) => digest,
        Err(_) => {
            return oci_error(
                StatusCode::BAD_REQUEST,
                "DIGEST_INVALID",
                "digest is invalid",
            );
        }
    };
    if !state.catalog.blob_visible(repository, &digest).await {
        return oci_error(StatusCode::NOT_FOUND, "BLOB_UNKNOWN", "blob was not found");
    }
    if state.config.pull_mode == PullMode::Edge {
        return edge_blob_redirect(state, &digest);
    }
    let metadata = match state.store.head(&blob_key(&digest)).await {
        Ok(metadata) => metadata,
        Err(registry_storage::StorageError::NotFound) => {
            return oci_error(StatusCode::NOT_FOUND, "BLOB_UNKNOWN", "blob was not found");
        }
        Err(_) => {
            return oci_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "UNKNOWN",
                "storage failure",
            );
        }
    };
    let body = if head {
        bytes::Bytes::new()
    } else {
        match state.store.get(&blob_key(&digest)).await {
            Ok(body) => body,
            Err(_) => {
                return oci_error(StatusCode::NOT_FOUND, "BLOB_UNKNOWN", "blob was not found");
            }
        }
    };
    content_response(
        StatusCode::OK,
        body,
        head,
        "application/octet-stream",
        &digest,
        Some(metadata.content_length),
        Some(metadata.etag),
    )
}

fn edge_blob_redirect(state: &AppState, digest: &Digest) -> Response {
    let Some(base_url) = state.config.edge_download_url.as_deref() else {
        return oci_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "BLOB_UNKNOWN",
            "edge pull mode is not configured",
        );
    };
    let Some(secret) = state.config.edge_download_secret.as_deref() else {
        return oci_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "BLOB_UNKNOWN",
            "edge pull mode is not configured",
        );
    };
    let expires_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .saturating_add(60);
    let key = blob_key(digest);
    let canonical = format!("BLOB\n{key}\n{digest}\n{expires_at}");
    let Ok(mut signer) = Hmac::<Sha256>::new_from_slice(secret.as_bytes()) else {
        return oci_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "UNKNOWN",
            "edge grant signer is invalid",
        );
    };
    signer.update(canonical.as_bytes());
    let signature = hex::encode(signer.finalize().into_bytes());
    let separator = if base_url.contains('?') { '&' } else { '?' };
    let location = format!(
        "{base_url}{separator}key={}&digest={}&exp={expires_at}&sig={signature}",
        query_escape(&key),
        query_escape(digest.as_str()),
    );
    let mut response = StatusCode::TEMPORARY_REDIRECT.into_response();
    response.headers_mut().insert(
        LOCATION,
        HeaderValue::from_str(&location).expect("edge location is valid"),
    );
    response.headers_mut().insert(
        HeaderName::from_static("docker-content-digest"),
        HeaderValue::from_str(digest.as_str()).expect("digest is valid"),
    );
    response
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    add_protocol_headers(&mut response);
    response
}

async fn tags_response(
    state: &AppState,
    repository: &RepositoryName,
    query: Option<&str>,
    head: bool,
) -> Response {
    let params = url::form_urlencoded::parse(query.unwrap_or_default().as_bytes())
        .into_owned()
        .collect::<std::collections::HashMap<_, _>>();
    let limit = match params.get("n") {
        Some(value) => match value.parse::<usize>() {
            Ok(value) if (1..=1000).contains(&value) => value,
            _ => {
                return oci_error(
                    StatusCode::BAD_REQUEST,
                    "PAGINATION_NUMBER_INVALID",
                    "n must be between 1 and 1000",
                );
            }
        },
        None => 100,
    };
    let last = params.get("last").map(String::as_str);
    let (tags, next) = match state.catalog.list_tags(repository, last, limit).await {
        Ok(result) => result,
        Err(_) => {
            return oci_error(
                StatusCode::NOT_FOUND,
                "NAME_UNKNOWN",
                "repository was not found",
            );
        }
    };
    let body = serde_json::to_vec(&json!({"name": repository.as_str(), "tags": tags}))
        .expect("tag response is serializable");
    let content_length = body.len() as u64;
    let mut response = content_response(
        StatusCode::OK,
        if head {
            bytes::Bytes::new()
        } else {
            bytes::Bytes::from(body)
        },
        head,
        "application/json",
        &Digest::sha256(&[]),
        Some(content_length),
        None,
    );
    if let Some(next) = next {
        let link = format!(
            "</v2/{}/tags/list?n={}&last={}>; rel=\"next\"",
            repository,
            limit,
            url::form_urlencoded::byte_serialize(next.as_bytes()).collect::<String>()
        );
        response.headers_mut().insert(
            LINK,
            HeaderValue::from_str(&link).expect("link header is valid"),
        );
    }
    response.headers_mut().remove("docker-content-digest");
    response
}

fn content_response(
    status: StatusCode,
    body: bytes::Bytes,
    head: bool,
    media_type: &str,
    digest: &Digest,
    content_length: Option<u64>,
    etag: Option<String>,
) -> Response {
    let mut response = Response::new(if head {
        Body::empty()
    } else {
        Body::from(body.clone())
    });
    *response.status_mut() = status;
    let headers = response.headers_mut();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_str(media_type).expect("media type is valid"),
    );
    headers.insert(
        CONTENT_LENGTH,
        HeaderValue::from_str(&content_length.unwrap_or(body.len() as u64).to_string())
            .expect("length is valid"),
    );
    headers.insert(
        HeaderName::from_static("docker-content-digest"),
        HeaderValue::from_str(digest.as_str()).expect("digest is valid"),
    );
    if let Some(etag) = etag {
        headers.insert(ETAG, HeaderValue::from_str(&etag).expect("etag is valid"));
    }
    add_protocol_headers(&mut response);
    response
}

fn version_response() -> Response {
    let mut response = (StatusCode::OK, Json(json!({}))).into_response();
    add_protocol_headers(&mut response);
    response
}

fn challenge_response(state: &AppState, repository: Option<&RepositoryName>) -> Response {
    let realm = format!(
        "{}/auth/token",
        state.config.public_url.trim_end_matches('/')
    );
    let mut value = format!(
        "Bearer realm=\"{realm}\",service=\"{}\"",
        state.config.token_service
    );
    if let Some(repository) = repository {
        value.push_str(&format!(",scope=\"repository:{repository}:pull\""));
    }
    let mut response = (
        StatusCode::UNAUTHORIZED,
        Json(json!({"errors":[{"code":"UNAUTHORIZED","message":"authentication required"}]})),
    )
        .into_response();
    response.headers_mut().insert(
        WWW_AUTHENTICATE,
        HeaderValue::from_str(&value).expect("challenge is valid"),
    );
    add_protocol_headers(&mut response);
    response
}

fn oci_error(status: StatusCode, code: &str, message: &str) -> Response {
    let mut response = (
        status,
        Json(json!({"errors":[{"code":code,"message":message}]})),
    )
        .into_response();
    add_protocol_headers(&mut response);
    response
}

fn add_protocol_headers(response: &mut Response) {
    response.headers_mut().insert(
        HeaderName::from_static("docker-distribution-api-version"),
        HeaderValue::from_static("registry/2.0"),
    );
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}

async fn token(
    State(state): State<AppState>,
    RawQuery(raw_query): RawQuery,
    headers: HeaderMap,
) -> Result<impl IntoResponse, crate::AppError> {
    let mut service = None;
    let mut scopes = Vec::new();
    for (key, value) in url::form_urlencoded::parse(raw_query.unwrap_or_default().as_bytes()) {
        match key.as_ref() {
            "service" => service = Some(value.into_owned()),
            "scope" => scopes.push(value.into_owned()),
            _ => {}
        }
    }
    let service = service
        .as_deref()
        .ok_or(crate::AppError::BadRequest("service is required"))?;
    let (username, password) =
        basic_credentials(&headers).ok_or(registry_auth::AuthError::InvalidCredentials)?;
    let requested = scopes
        .iter()
        .flat_map(|value| value.split_whitespace())
        .map(parse_scope)
        .collect::<Result<Vec<_>, _>>()?;
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
    let session = match state.auth.login(&input.username, &input.password).await {
        Ok(session) => session,
        Err(error) => {
            let mut event = RegistryEvent::new(EventKind::LoginFailed);
            event.actor = Some(input.username.clone());
            state.events.record(event).await;
            return Err(error.into());
        }
    };
    let mut event = RegistryEvent::new(EventKind::LoginSucceeded);
    event.actor = Some(input.username.clone());
    state.events.record(event).await;
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

async fn overview(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, crate::AppError> {
    let session = cookie_value(&headers).ok_or(registry_auth::AuthError::InvalidSession)?;
    let user = state.auth.session_user(session).await?;
    let repositories = state.catalog.repositories().await;
    let credentials = state.auth.list_credentials_for_session(session).await?;
    Ok(Json(json!({
        "user": user,
        "repository_count": repositories.len(),
        "repositories": repositories,
        "active_token_count": credentials.iter().filter(|credential| credential.revoked_at.is_none()).count(),
    })))
}

async fn list_repositories(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, crate::AppError> {
    let session = cookie_value(&headers).ok_or(registry_auth::AuthError::InvalidSession)?;
    let _ = state.auth.session_user(session).await?;
    let repositories = state
        .catalog
        .repositories()
        .await
        .into_iter()
        .map(|name| json!({"name": name, "visibility": "private"}))
        .collect::<Vec<_>>();
    Ok(Json(json!({"repositories": repositories})))
}

async fn repository_detail(
    State(state): State<AppState>,
    Path(repository): Path<String>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, crate::AppError> {
    let session = cookie_value(&headers).ok_or(registry_auth::AuthError::InvalidSession)?;
    let _ = state.auth.session_user(session).await?;
    let repository = RepositoryName::parse(&repository)
        .map_err(|_| crate::AppError::BadRequest("invalid repository name"))?;
    let (tags, _) = state.catalog.list_tags(&repository, None, 1000).await?;
    let mut entries = Vec::with_capacity(tags.len());
    for tag in tags {
        let manifest = state.catalog.resolve_manifest(&repository, &tag).await?;
        entries.push(json!({
            "tag": tag,
            "digest": manifest.digest,
            "media_type": manifest.media_type,
            "size": manifest.size,
            "created_at": manifest.created_at,
        }));
    }
    Ok(Json(
        json!({"name": repository, "visibility": "private", "tags": entries}),
    ))
}

#[derive(Debug, Deserialize)]
struct AuditQuery {
    limit: Option<usize>,
}

async fn list_audit(
    State(state): State<AppState>,
    Query(query): Query<AuditQuery>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, crate::AppError> {
    let session = cookie_value(&headers).ok_or(registry_auth::AuthError::InvalidSession)?;
    let _ = state.auth.session_user(session).await?;
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    Ok(Json(json!({"events": state.events.recent(limit).await})))
}

#[derive(Debug, Deserialize)]
struct CreateWebhookRequest {
    url: String,
    #[serde(default)]
    events: BTreeSet<EventKind>,
}

async fn list_webhooks(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, crate::AppError> {
    let session = cookie_value(&headers).ok_or(registry_auth::AuthError::InvalidSession)?;
    let _ = state.auth.session_user(session).await?;
    Ok(Json(json!({"webhooks": state.webhooks.list().await})))
}

async fn create_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateWebhookRequest>,
) -> Result<impl IntoResponse, crate::AppError> {
    let session = cookie_value(&headers).ok_or(registry_auth::AuthError::InvalidSession)?;
    let actor = state.auth.session_user(session).await?;
    if !actor.is_admin {
        return Err(registry_auth::AuthError::NoAccess.into());
    }
    let created = state
        .webhooks
        .create(input.url, input.events)
        .await
        .map_err(|_| crate::AppError::BadRequest("webhook URL must be an http(s) URL"))?;
    let mut event = RegistryEvent::new(EventKind::WebhookCreated);
    event.actor = Some(actor.username);
    event.metadata = json!({"webhook_id": created.webhook.id, "url": created.webhook.url});
    state.events.record(event).await;
    Ok(Json(created))
}

async fn disable_webhook(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, crate::AppError> {
    let session = cookie_value(&headers).ok_or(registry_auth::AuthError::InvalidSession)?;
    let actor = state.auth.session_user(session).await?;
    if !actor.is_admin {
        return Err(registry_auth::AuthError::NoAccess.into());
    }
    if !state.webhooks.disable(id).await {
        return Err(crate::AppError::Catalog(crate::CatalogError::NotFound));
    }
    let mut event = RegistryEvent::new(EventKind::WebhookDisabled);
    event.actor = Some(actor.username);
    event.metadata = json!({"webhook_id": id});
    state.events.record(event).await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
struct CreateTokenRequest {
    name: String,
    scopes: Vec<ScopeInput>,
    expires_at: Option<u64>,
}

async fn list_tokens(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, crate::AppError> {
    let session = cookie_value(&headers).ok_or(registry_auth::AuthError::InvalidSession)?;
    Ok(Json(
        json!({"tokens": state.auth.list_credentials_for_session(session).await?}),
    ))
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
    let actor = state.auth.session_user(session).await?;
    let token_name = input.name.clone();
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
    let created = state
        .auth
        .create_credential_for_session(session, input.name, scopes, input.expires_at)
        .await?;
    let mut event = RegistryEvent::new(EventKind::TokenCreated);
    event.actor = Some(actor.username);
    event.metadata = json!({"name": token_name, "credential_id": created.id});
    state.events.record(event).await;
    Ok(Json(created))
}

async fn revoke_token(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, crate::AppError> {
    let session = cookie_value(&headers).ok_or(registry_auth::AuthError::InvalidSession)?;
    let actor = state.auth.session_user(session).await?;
    state
        .auth
        .revoke_credential_for_session(session, id)
        .await?;
    let mut event = RegistryEvent::new(EventKind::TokenRevoked);
    event.actor = Some(actor.username);
    event.metadata = json!({"credential_id": id});
    state.events.record(event).await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
struct GcRequest {
    #[serde(default)]
    dry_run: bool,
    grace_seconds: Option<u64>,
}

async fn run_gc(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<GcRequest>,
) -> Result<impl IntoResponse, crate::AppError> {
    let session = cookie_value(&headers).ok_or(registry_auth::AuthError::InvalidSession)?;
    let user = state.auth.session_user(session).await?;
    if !user.is_admin {
        return Err(registry_auth::AuthError::NoAccess.into());
    }
    let report = state
        .catalog
        .garbage_collect(
            &state.store,
            std::time::Duration::from_secs(input.grace_seconds.unwrap_or(7 * 24 * 60 * 60)),
            input.dry_run,
        )
        .await?;
    let mut event = RegistryEvent::new(EventKind::GarbageCollection);
    event.actor = Some(user.username);
    event.metadata = serde_json::to_value(&report).unwrap_or_else(|_| json!({}));
    state.events.record(event).await;
    Ok(Json(report))
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
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use bytes::Bytes;
    use registry_auth::parse_scope;
    use registry_core::{Digest, RepositoryName};
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
            upload_chunk_limit_bytes: 1024 * 1024,
            token_issuer: "knotree-registry".to_owned(),
            token_service: "knotree-registry".to_owned(),
            token_ttl_seconds: 300,
            bootstrap_admin_username: None,
            bootstrap_admin_password: None,
            cookie_secure: false,
            r2_endpoint: None,
            r2_bucket: None,
            r2_access_key_id: None,
            r2_secret_access_key: None,
            r2_region: "auto".to_owned(),
            pull_mode: PullMode::Proxy,
            edge_download_url: None,
            edge_download_secret: None,
            control_plane_origins: Vec::new(),
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
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(response.headers().contains_key("www-authenticate"));
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

    #[tokio::test]
    async fn token_endpoint_accepts_repeated_scope_parameters() {
        let state = AppState::initialize(test_config()).await.expect("state");
        let user = state
            .auth
            .bootstrap_admin("admin", "correct horse battery staple")
            .await
            .expect("bootstrap");
        let credential = state
            .auth
            .issue_credential_for_user(
                user.id,
                "docker".to_owned(),
                vec![parse_scope("repository:team/app:pull,push").expect("scope")],
                None,
            )
            .await
            .expect("credential");
        let basic = base64::engine::general_purpose::STANDARD
            .encode(format!("admin:{}", credential.secret));
        let response = router(state)
            .oneshot(
                Request::builder()
                    .uri("/auth/token?service=knotree-registry&scope=repository%3Ateam%2Fapp%3Apull&scope=repository%3Ateam%2Fapp%3Apull%2Cpush")
                    .header("authorization", format!("Basic {basic}"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    }

    #[tokio::test]
    async fn control_plane_session_and_token_lifecycle_is_cookie_scoped() {
        let state = AppState::initialize(test_config()).await.expect("state");
        state
            .auth
            .bootstrap_admin("admin", "correct horse battery staple")
            .await
            .expect("bootstrap");

        let response = router(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/login")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"username":"admin","password":"correct horse battery staple"}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let cookie = response
            .headers()
            .get("set-cookie")
            .expect("session cookie")
            .to_str()
            .expect("cookie header")
            .split(';')
            .next()
            .expect("cookie pair")
            .to_owned();

        let response = router(state.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/v1/overview")
                    .header("cookie", &cookie)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);

        let response = router(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/tokens")
                    .header("content-type", "application/json")
                    .header("cookie", &cookie)
                    .body(Body::from(
                        r#"{"name":"ci","scopes":[{"repository":"team/app","actions":["pull","push"]}]}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let created: serde_json::Value = serde_json::from_slice(
            &to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("body"),
        )
        .expect("credential json");
        let credential_id = created["id"].as_str().expect("credential id").to_owned();
        assert!(created["secret"].as_str().is_some());

        let response = router(state.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/v1/auth/tokens")
                    .header("cookie", &cookie)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let listed = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        assert!(String::from_utf8_lossy(&listed).contains("ci"));

        let response = router(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/auth/tokens/{credential_id}/revoke"))
                    .header("cookie", &cookie)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        let response = router(state.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/v1/audit?limit=10")
                    .header("cookie", &cookie)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let audit = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        assert!(String::from_utf8_lossy(&audit).contains("token_created"));

        let response = router(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/webhooks")
                    .header("content-type", "application/json")
                    .header("cookie", &cookie)
                    .body(Body::from(
                        r#"{"url":"https://hooks.example.com/registry","events":["manifest_pushed"]}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let webhook: serde_json::Value = serde_json::from_slice(
            &to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("body"),
        )
        .expect("webhook json");
        let webhook_id = webhook["webhook"]["id"]
            .as_str()
            .expect("webhook id")
            .to_owned();
        assert!(webhook["secret"].as_str().is_some());

        let response = router(state.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/v1/webhooks")
                    .header("cookie", &cookie)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        let listed_webhooks = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        assert!(!String::from_utf8_lossy(&listed_webhooks).contains("whsec_"));

        let response = router(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/webhooks/{webhook_id}/disable"))
                    .header("cookie", &cookie)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn edge_pull_redirect_is_signed_after_repository_authorization() {
        let mut config = test_config();
        config.pull_mode = PullMode::Edge;
        config.edge_download_url = Some("https://blobs.example.com/v1/blob".to_owned());
        config.edge_download_secret = Some("edge-secret".to_owned());
        let state = AppState::initialize(config).await.expect("state");
        let user = state
            .auth
            .bootstrap_admin("admin", "correct horse battery staple")
            .await
            .expect("bootstrap");
        let credential = state
            .auth
            .issue_credential_for_user(
                user.id,
                "pull".to_owned(),
                vec![parse_scope("repository:team/app:pull").expect("scope")],
                None,
            )
            .await
            .expect("credential");
        let bearer = state
            .auth
            .mint_token(
                "admin",
                &credential.secret,
                "knotree-registry",
                &[parse_scope("repository:team/app:pull").expect("scope")],
            )
            .await
            .expect("bearer")
            .token;
        let repository = RepositoryName::parse("team/app").expect("repository");
        let digest = Digest::sha256(b"edge blob");
        state.catalog.attach_blob(&repository, digest.clone()).await;

        let response = router(state)
            .oneshot(
                Request::builder()
                    .uri(format!("/v2/team/app/blobs/{digest}"))
                    .header("authorization", format!("Bearer {bearer}"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT);
        assert!(
            response.headers()["location"]
                .to_str()
                .expect("location")
                .contains("sig=")
        );
        assert_eq!(response.headers()["docker-content-digest"], digest.as_str());
    }

    #[tokio::test]
    async fn manifest_put_requires_push_and_preserves_exact_bytes() {
        let state = AppState::initialize(test_config()).await.expect("state");
        let user = state
            .auth
            .bootstrap_admin("admin", "correct horse battery staple")
            .await
            .expect("bootstrap");
        let credential = state
            .auth
            .issue_credential_for_user(
                user.id,
                "push".to_owned(),
                vec![parse_scope("repository:team/app:pull,push").expect("scope")],
                None,
            )
            .await
            .expect("credential");
        let bearer = state
            .auth
            .mint_token(
                "admin",
                &credential.secret,
                "knotree-registry",
                &[parse_scope("repository:team/app:pull,push").expect("scope")],
            )
            .await
            .expect("bearer")
            .token;
        let repository = RepositoryName::parse("team/app").expect("repository");
        let config = Bytes::from_static(b"config");
        let config_digest = Digest::sha256(&config);
        state
            .store
            .put(&blob_key(&config_digest), config.clone())
            .await
            .expect("config object");
        state
            .catalog
            .attach_blob(&repository, config_digest.clone())
            .await;
        let raw = Bytes::from(format!(
            r#"{{"schemaVersion":2,"mediaType":"application/vnd.oci.image.manifest.v1+json","config":{{"mediaType":"application/vnd.oci.image.config.v1+json","digest":"{config_digest}","size":6}},"layers":[]}}"#
        ));
        let digest = Digest::sha256(&raw);
        let response = router(state.clone())
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/v2/team/app/manifests/latest")
                    .header("authorization", format!("Bearer {bearer}"))
                    .header("content-type", "application/vnd.oci.image.manifest.v1+json")
                    .body(Body::from(raw.clone()))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(response.headers()["docker-content-digest"], digest.as_str());
        assert_eq!(
            state
                .store
                .get(&manifest_key(&digest))
                .await
                .expect("manifest"),
            raw
        );

        let wrong_digest = Digest::sha256(b"different");
        let response = router(state)
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/v2/team/app/manifests/{wrong_digest}"))
                    .header("authorization", format!("Bearer {bearer}"))
                    .header("content-type", "application/vnd.oci.image.manifest.v1+json")
                    .body(Body::from(
                        r#"{"schemaVersion":2,"mediaType":"application/vnd.oci.image.manifest.v1+json","config":{"mediaType":"application/vnd.oci.image.config.v1+json","digest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","size":0},"layers":[]}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn authorized_pull_preserves_manifest_bytes_and_headers() {
        let state = AppState::initialize(test_config()).await.expect("state");
        let user = state
            .auth
            .bootstrap_admin("admin", "correct horse battery staple")
            .await
            .expect("bootstrap");
        let credential = state
            .auth
            .issue_credential_for_user(
                user.id,
                "pull".to_owned(),
                vec![parse_scope("repository:team/app:pull").expect("scope")],
                None,
            )
            .await
            .expect("credential");
        let bearer = state
            .auth
            .mint_token(
                "admin",
                &credential.secret,
                "knotree-registry",
                &[parse_scope("repository:team/app:pull").expect("scope")],
            )
            .await
            .expect("bearer")
            .token;
        let repository = RepositoryName::parse("team/app").expect("repository");
        let raw = Bytes::from_static(
            br#"{"schemaVersion":2,"mediaType":"application/vnd.oci.image.manifest.v1+json","config":{"mediaType":"application/vnd.oci.image.config.v1+json","digest":"sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","size":0},"layers":[]}"#,
        );
        let manifest = state
            .catalog
            .publish_manifest(&repository, "latest", &raw, None)
            .await
            .expect("manifest");
        state
            .store
            .put(&manifest_key(&manifest.digest), raw.clone())
            .await
            .expect("manifest object");

        let response = router(state.clone())
            .oneshot(
                Request::builder()
                    .uri("/v2/team/app/manifests/latest")
                    .header("authorization", format!("Bearer {bearer}"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()["docker-content-digest"],
            manifest.digest.as_str()
        );
        assert_eq!(
            response.headers()["content-type"],
            "application/vnd.oci.image.manifest.v1+json"
        );
        assert_eq!(
            to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("body"),
            raw
        );

        let response = router(state)
            .oneshot(
                Request::builder()
                    .method("HEAD")
                    .uri("/v2/team/app/manifests/latest")
                    .header("authorization", format!("Bearer {bearer}"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-length"], raw.len().to_string());
        assert!(
            to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("body")
                .is_empty()
        );
        assert_eq!(Digest::sha256(&raw), manifest.digest);
    }

    #[tokio::test]
    async fn chunked_upload_publishes_only_after_digest_verification() {
        let state = AppState::initialize(test_config()).await.expect("state");
        let user = state
            .auth
            .bootstrap_admin("admin", "correct horse battery staple")
            .await
            .expect("bootstrap");
        let credential = state
            .auth
            .issue_credential_for_user(
                user.id,
                "push".to_owned(),
                vec![parse_scope("repository:team/app:pull,push").expect("scope")],
                None,
            )
            .await
            .expect("credential");
        let bearer = state
            .auth
            .mint_token(
                "admin",
                &credential.secret,
                "knotree-registry",
                &[parse_scope("repository:team/app:pull,push").expect("scope")],
            )
            .await
            .expect("bearer")
            .token;

        let response = router(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v2/team/app/blobs/uploads/")
                    .header("authorization", format!("Bearer {bearer}"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let location = response.headers()["location"]
            .to_str()
            .expect("location")
            .to_owned();

        let response = router(state.clone())
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(&location)
                    .header("authorization", format!("Bearer {bearer}"))
                    .header("content-range", "bytes 0-4")
                    .body(Body::from("hello"))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert_eq!(response.headers()["range"], "bytes=0-4");

        let digest = Digest::sha256(b"hello");
        let response = router(state.clone())
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("{location}?digest={digest}"))
                    .header("authorization", format!("Bearer {bearer}"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(response.headers()["docker-content-digest"], digest.as_str());
        assert_eq!(
            state.store.get(&blob_key(&digest)).await.expect("blob"),
            Bytes::from_static(b"hello")
        );
        assert!(
            state
                .catalog
                .blob_visible(
                    &RepositoryName::parse("team/app").expect("repository"),
                    &digest
                )
                .await
        );
    }

    #[tokio::test]
    async fn referrers_and_delete_use_manifest_relationships() {
        let state = AppState::initialize(test_config()).await.expect("state");
        let user = state
            .auth
            .bootstrap_admin("admin", "correct horse battery staple")
            .await
            .expect("bootstrap");
        let credential = state
            .auth
            .issue_credential_for_user(
                user.id,
                "maintainer".to_owned(),
                vec![parse_scope("repository:team/app:pull,delete").expect("scope")],
                None,
            )
            .await
            .expect("credential");
        let bearer = state
            .auth
            .mint_token(
                "admin",
                &credential.secret,
                "knotree-registry",
                &[parse_scope("repository:team/app:pull,delete").expect("scope")],
            )
            .await
            .expect("bearer")
            .token;
        let repository = RepositoryName::parse("team/app").expect("repository");
        let base = Bytes::from_static(
            br#"{"schemaVersion":2,"mediaType":"application/vnd.oci.image.manifest.v1+json","config":{"mediaType":"application/vnd.oci.image.config.v1+json","digest":"sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","size":0},"layers":[]}"#,
        );
        let base_record = state
            .catalog
            .publish_manifest(&repository, "latest", &base, None)
            .await
            .expect("base manifest");
        state
            .store
            .put(&manifest_key(&base_record.digest), base)
            .await
            .expect("base object");
        let referrer = Bytes::from(format!(
            r#"{{"schemaVersion":2,"mediaType":"application/vnd.oci.artifact.manifest.v1+json","artifactType":"application/example.sbom","blobs":[],"subject":{{"mediaType":"application/vnd.oci.image.manifest.v1+json","digest":"{}","size":0}}}}"#,
            base_record.digest
        ));
        let referrer_digest = Digest::sha256(&referrer);
        let referrer_record = state
            .catalog
            .publish_manifest(&repository, &referrer_digest.to_string(), &referrer, None)
            .await
            .expect("referrer");
        state
            .store
            .put(&manifest_key(&referrer_record.digest), referrer)
            .await
            .expect("referrer object");

        let response = router(state.clone())
            .oneshot(
                Request::builder()
                    .uri(format!("/v2/team/app/referrers/{}", base_record.digest))
                    .header("authorization", format!("Bearer {bearer}"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        assert!(String::from_utf8_lossy(&body).contains(referrer_record.digest.as_str()));

        let response = router(state.clone())
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/v2/team/app/manifests/latest")
                    .header("authorization", format!("Bearer {bearer}"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert!(matches!(
            state.catalog.resolve_manifest(&repository, "latest").await,
            Err(crate::CatalogError::NotFound)
        ));
    }
}
