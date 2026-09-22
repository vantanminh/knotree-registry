use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Digest(String);

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DigestError {
    #[error("digest must contain an algorithm and encoded value separated by ':'")]
    MissingSeparator,
    #[error("unsupported digest algorithm '{0}'")]
    UnsupportedAlgorithm(String),
    #[error("digest value must be lowercase hexadecimal")]
    InvalidEncoding,
    #[error("sha256 digests must contain exactly 64 hexadecimal characters")]
    InvalidSha256Length,
}

impl Digest {
    pub fn parse(value: &str) -> Result<Self, DigestError> {
        let (algorithm, encoded) = value.split_once(':').ok_or(DigestError::MissingSeparator)?;
        if algorithm != "sha256" {
            return Err(DigestError::UnsupportedAlgorithm(algorithm.to_owned()));
        }
        if encoded.len() != 64 {
            return Err(DigestError::InvalidSha256Length);
        }
        if encoded.is_empty()
            || encoded
                .bytes()
                .any(|byte| !(byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
        {
            return Err(DigestError::InvalidEncoding);
        }
        Ok(Self(value.to_owned()))
    }

    pub fn algorithm(&self) -> &'static str {
        "sha256"
    }

    pub fn encoded(&self) -> &str {
        self.0["sha256:".len()..].as_ref()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for Digest {
    type Err = DigestError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for Digest {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Digest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn parses_and_round_trips_sha256() {
        let digest = Digest::parse(VALID).expect("valid digest");
        assert_eq!(digest.algorithm(), "sha256");
        assert_eq!(digest.encoded().len(), 64);
        assert_eq!(digest.to_string(), VALID);
        assert_eq!(
            serde_json::to_string(&digest).expect("serialize"),
            format!("\"{VALID}\"")
        );
    }

    #[test]
    fn rejects_uppercase_and_wrong_lengths() {
        assert_eq!(
            Digest::parse(&VALID.to_uppercase()),
            Err(DigestError::UnsupportedAlgorithm("SHA256".to_owned()))
        );
        assert_eq!(
            Digest::parse("sha256:abcd"),
            Err(DigestError::InvalidSha256Length)
        );
    }
}
