use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use aws_config::BehaviorVersion;
use aws_credential_types::{Credentials, provider::SharedCredentialsProvider};
use aws_sdk_s3::{
    Client,
    primitives::ByteStream,
    types::{CompletedMultipartUpload, CompletedPart},
};
use bytes::Bytes;
use sha2::{Digest as _, Sha256};
use tokio::sync::Mutex;

use crate::{ObjectMetadata, ObjectStore, StorageError};

const PART_SIZE: usize = 16 * 1024 * 1024;

#[derive(Clone)]
pub struct R2ObjectStore {
    client: Client,
    bucket: String,
    uploads: Arc<Mutex<HashMap<String, MultipartState>>>,
}

struct MultipartState {
    upload_id: String,
    next_part_number: i32,
    offset: u64,
    tail: Vec<u8>,
    parts: Vec<CompletedPart>,
}

impl R2ObjectStore {
    pub async fn new(
        endpoint: &str,
        bucket: &str,
        access_key_id: &str,
        secret_access_key: &str,
        region: &str,
    ) -> Result<Self, StorageError> {
        if endpoint.is_empty()
            || bucket.is_empty()
            || access_key_id.is_empty()
            || secret_access_key.is_empty()
        {
            return Err(StorageError::R2(
                "R2 endpoint, bucket and credentials are required".to_owned(),
            ));
        }
        let credentials = Credentials::new(
            access_key_id,
            secret_access_key,
            None,
            None,
            "knotree-registry",
        );
        let sdk_config = aws_config::defaults(BehaviorVersion::latest())
            .region(aws_sdk_s3::config::Region::new(region.to_owned()))
            .endpoint_url(endpoint)
            .credentials_provider(SharedCredentialsProvider::new(credentials))
            .load()
            .await;
        let config = aws_sdk_s3::config::Builder::from(&sdk_config)
            .force_path_style(true)
            .build();
        Ok(Self {
            client: Client::from_conf(config),
            bucket: bucket.to_owned(),
            uploads: Arc::new(Mutex::new(HashMap::new())),
        })
    }
}

#[async_trait]
impl ObjectStore for R2ObjectStore {
    async fn put(&self, key: &str, body: Bytes) -> Result<ObjectMetadata, StorageError> {
        let output = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(ByteStream::from(body.clone()))
            .send()
            .await
            .map_err(|error| StorageError::R2(error.to_string()))?;
        let etag = output
            .e_tag()
            .map(str::to_owned)
            .unwrap_or_else(|| quoted_sha256(&body));
        Ok(ObjectMetadata {
            key: key.to_owned(),
            content_length: body.len() as u64,
            etag,
        })
    }

    async fn get(&self, key: &str) -> Result<Bytes, StorageError> {
        let output = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|error| StorageError::R2(error.to_string()))?;
        output
            .body
            .collect()
            .await
            .map(|body| body.into_bytes())
            .map_err(|error| StorageError::R2(error.to_string()))
    }

    async fn head(&self, key: &str) -> Result<ObjectMetadata, StorageError> {
        let output = self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|error| StorageError::R2(error.to_string()))?;
        Ok(ObjectMetadata {
            key: key.to_owned(),
            content_length: output.content_length().unwrap_or_default().max(0) as u64,
            etag: output.e_tag().unwrap_or_default().to_owned(),
        })
    }

    async fn delete(&self, key: &str) -> Result<(), StorageError> {
        if let Some(state) = self.uploads.lock().await.remove(key) {
            self.client
                .abort_multipart_upload()
                .bucket(&self.bucket)
                .key(key)
                .upload_id(state.upload_id)
                .send()
                .await
                .map_err(|error| StorageError::R2(error.to_string()))?;
        }
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map(|_| ())
            .map_err(|error| StorageError::R2(error.to_string()))
    }

    async fn append(
        &self,
        key: &str,
        expected_offset: u64,
        body: Bytes,
    ) -> Result<u64, StorageError> {
        let mut uploads = self.uploads.lock().await;
        let state = if let Some(state) = uploads.get_mut(key) {
            state
        } else {
            let output = self
                .client
                .create_multipart_upload()
                .bucket(&self.bucket)
                .key(key)
                .send()
                .await
                .map_err(|error| StorageError::R2(error.to_string()))?;
            let upload_id = output
                .upload_id()
                .ok_or_else(|| {
                    StorageError::R2("R2 did not return a multipart upload id".to_owned())
                })?
                .to_owned();
            uploads.entry(key.to_owned()).or_insert(MultipartState {
                upload_id,
                next_part_number: 1,
                offset: 0,
                tail: Vec::new(),
                parts: Vec::new(),
            })
        };
        if state.offset != expected_offset {
            return Err(StorageError::OffsetMismatch {
                expected: state.offset,
                actual: expected_offset,
            });
        }
        let old_len = body.len();
        let mut combined = Vec::with_capacity(state.tail.len() + body.len());
        combined.extend_from_slice(&state.tail);
        combined.extend_from_slice(&body);
        let mut cursor = 0;
        while combined.len().saturating_sub(cursor) >= PART_SIZE {
            let part = Bytes::copy_from_slice(&combined[cursor..cursor + PART_SIZE]);
            let output = self
                .client
                .upload_part()
                .bucket(&self.bucket)
                .key(key)
                .upload_id(&state.upload_id)
                .part_number(state.next_part_number)
                .body(ByteStream::from(part))
                .send()
                .await
                .map_err(|error| StorageError::R2(error.to_string()))?;
            let etag = output.e_tag().ok_or_else(|| {
                StorageError::R2("R2 did not return a multipart part ETag".to_owned())
            })?;
            state.parts.push(
                CompletedPart::builder()
                    .part_number(state.next_part_number)
                    .e_tag(etag)
                    .build(),
            );
            state.next_part_number += 1;
            cursor += PART_SIZE;
        }
        state.tail = combined[cursor..].to_vec();
        state.offset += old_len as u64;
        Ok(state.offset)
    }

    async fn finalize_append(&self, key: &str) -> Result<(), StorageError> {
        let Some(mut state) = self.uploads.lock().await.remove(key) else {
            return Ok(());
        };
        if state.parts.is_empty() && state.tail.is_empty() {
            self.client
                .abort_multipart_upload()
                .bucket(&self.bucket)
                .key(key)
                .upload_id(state.upload_id)
                .send()
                .await
                .map_err(|error| StorageError::R2(error.to_string()))?;
            return self.put(key, Bytes::new()).await.map(|_| ());
        }
        if !state.tail.is_empty() {
            let part = Bytes::from(std::mem::take(&mut state.tail));
            let output = self
                .client
                .upload_part()
                .bucket(&self.bucket)
                .key(key)
                .upload_id(&state.upload_id)
                .part_number(state.next_part_number)
                .body(ByteStream::from(part))
                .send()
                .await
                .map_err(|error| StorageError::R2(error.to_string()))?;
            let etag = output.e_tag().ok_or_else(|| {
                StorageError::R2("R2 did not return a final part ETag".to_owned())
            })?;
            state.parts.push(
                CompletedPart::builder()
                    .part_number(state.next_part_number)
                    .e_tag(etag)
                    .build(),
            );
        }
        let multipart = CompletedMultipartUpload::builder()
            .set_parts(Some(state.parts))
            .build();
        self.client
            .complete_multipart_upload()
            .bucket(&self.bucket)
            .key(key)
            .upload_id(state.upload_id)
            .multipart_upload(multipart)
            .send()
            .await
            .map_err(|error| StorageError::R2(error.to_string()))?;
        Ok(())
    }

    async fn health(&self) -> Result<(), StorageError> {
        self.client
            .head_bucket()
            .bucket(&self.bucket)
            .send()
            .await
            .map(|_| ())
            .map_err(|error| StorageError::R2(error.to_string()))
    }
}

fn quoted_sha256(body: &[u8]) -> String {
    format!("\"{}\"", hex::encode(Sha256::digest(body)))
}
