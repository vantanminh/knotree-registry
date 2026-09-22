use serde_json::Value;
use thiserror::Error;

use crate::Digest;

pub const OCI_IMAGE_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";
pub const OCI_IMAGE_INDEX: &str = "application/vnd.oci.image.index.v1+json";
pub const DOCKER_MANIFEST: &str = "application/vnd.docker.distribution.manifest.v2+json";
pub const DOCKER_MANIFEST_LIST: &str = "application/vnd.docker.distribution.manifest.list.v2+json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Descriptor {
    pub media_type: String,
    pub digest: Digest,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestInfo {
    pub media_type: String,
    pub references: Vec<Descriptor>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ManifestError {
    #[error("manifest is not valid JSON")]
    InvalidJson,
    #[error("manifest must be a JSON object")]
    NotObject,
    #[error("manifest schemaVersion must be 2")]
    InvalidSchemaVersion,
    #[error("unsupported manifest media type '{0}'")]
    UnsupportedMediaType(String),
    #[error("manifest descriptor '{0}' is missing or invalid")]
    InvalidDescriptor(String),
    #[error("manifest descriptor '{0}' has a negative size")]
    NegativeSize(String),
    #[error("manifest descriptor '{0}' has an invalid digest")]
    InvalidDigest(String),
}

pub fn validate_manifest(
    bytes: &[u8],
    content_type: Option<&str>,
) -> Result<ManifestInfo, ManifestError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| ManifestError::InvalidJson)?;
    let object = value.as_object().ok_or(ManifestError::NotObject)?;
    if object.get("schemaVersion").and_then(Value::as_u64) != Some(2) {
        return Err(ManifestError::InvalidSchemaVersion);
    }
    let media_type = object
        .get("mediaType")
        .and_then(Value::as_str)
        .or_else(|| content_type.map(|value| value.split(';').next().unwrap_or(value).trim()))
        .ok_or_else(|| {
            if object.contains_key("manifests") {
                ManifestError::UnsupportedMediaType("missing index media type".to_owned())
            } else {
                ManifestError::UnsupportedMediaType("missing manifest media type".to_owned())
            }
        })?
        .to_owned();
    let is_index = matches!(media_type.as_str(), OCI_IMAGE_INDEX | DOCKER_MANIFEST_LIST);
    let is_manifest = matches!(media_type.as_str(), OCI_IMAGE_MANIFEST | DOCKER_MANIFEST);
    if !is_index && !is_manifest {
        return Err(ManifestError::UnsupportedMediaType(media_type));
    }
    let references = if is_index {
        descriptors(object.get("manifests"), "manifests")?
    } else {
        let mut references = Vec::new();
        references.push(descriptor(object.get("config"), "config")?);
        references.extend(descriptors(object.get("layers"), "layers")?);
        references
    };
    Ok(ManifestInfo {
        media_type,
        references,
    })
}

fn descriptors(value: Option<&Value>, field: &str) -> Result<Vec<Descriptor>, ManifestError> {
    let entries = value
        .and_then(Value::as_array)
        .ok_or_else(|| ManifestError::InvalidDescriptor(field.to_owned()))?;
    entries
        .iter()
        .enumerate()
        .map(|(index, value)| descriptor(Some(value), &format!("{field}[{index}]")))
        .collect()
}

fn descriptor(value: Option<&Value>, field: &str) -> Result<Descriptor, ManifestError> {
    let object = value
        .and_then(Value::as_object)
        .ok_or_else(|| ManifestError::InvalidDescriptor(field.to_owned()))?;
    let media_type = object
        .get("mediaType")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ManifestError::InvalidDescriptor(field.to_owned()))?;
    let digest_value = object
        .get("digest")
        .and_then(Value::as_str)
        .ok_or_else(|| ManifestError::InvalidDescriptor(field.to_owned()))?;
    let digest =
        Digest::parse(digest_value).map_err(|_| ManifestError::InvalidDigest(field.to_owned()))?;
    let size = object
        .get("size")
        .and_then(Value::as_i64)
        .ok_or_else(|| ManifestError::InvalidDescriptor(field.to_owned()))?;
    if size < 0 {
        return Err(ManifestError::NegativeSize(field.to_owned()));
    }
    Ok(Descriptor {
        media_type: media_type.to_owned(),
        digest,
        size: size as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn validates_an_oci_image_manifest_without_reserializing_it() {
        let raw = format!(
            r#"{{"schemaVersion":2,"mediaType":"{OCI_IMAGE_MANIFEST}","config":{{"mediaType":"application/vnd.oci.image.config.v1+json","digest":"{CONFIG}","size":12}},"layers":[]}}"#
        );
        let info = validate_manifest(raw.as_bytes(), None).expect("manifest");
        assert_eq!(info.media_type, OCI_IMAGE_MANIFEST);
        assert_eq!(info.references[0].digest.to_string(), CONFIG);
    }

    #[test]
    fn rejects_wrong_schema_and_negative_sizes() {
        assert_eq!(
            validate_manifest(br#"{"schemaVersion":1}"#, None),
            Err(ManifestError::InvalidSchemaVersion)
        );
        let raw = format!(
            r#"{{"schemaVersion":2,"mediaType":"{OCI_IMAGE_MANIFEST}","config":{{"mediaType":"x","digest":"{CONFIG}","size":-1}},"layers":[]}}"#
        );
        assert_eq!(
            validate_manifest(raw.as_bytes(), None),
            Err(ManifestError::NegativeSize("config".to_owned()))
        );
    }
}
