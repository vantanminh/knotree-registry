use std::time::{SystemTime, UNIX_EPOCH};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use p256::ecdsa::{
    Signature, SigningKey, VerifyingKey,
    signature::{Signer, Verifier},
};
use p256::elliptic_curve::rand_core::OsRng;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub(crate) enum JwtError {
    #[error("malformed bearer token")]
    Malformed,
    #[error("invalid bearer token encoding")]
    Encoding(#[from] base64::DecodeError),
    #[error("invalid bearer token JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid bearer token signature")]
    Signature,
    #[error("bearer token audience does not match")]
    Audience,
    #[error("bearer token is expired or not yet valid")]
    Time,
    #[error("bearer token algorithm or key id is invalid")]
    Header,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AccessEntry {
    pub typ: String,
    pub name: String,
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RegistryClaims {
    pub iss: String,
    pub sub: String,
    pub aud: String,
    pub exp: u64,
    pub iat: u64,
    pub nbf: u64,
    pub jti: String,
    pub access: Vec<AccessEntry>,
}

#[derive(Debug, Serialize)]
struct Header<'a> {
    alg: &'a str,
    typ: &'a str,
    kid: &'a str,
}

#[derive(Debug, Deserialize)]
struct DecodedHeader {
    alg: String,
    typ: String,
    kid: String,
}

#[derive(Clone)]
pub(crate) struct JwtIssuer {
    issuer: String,
    kid: String,
    signing_key: SigningKey,
    verifying_key: VerifyingKey,
}

impl JwtIssuer {
    pub(crate) fn new(issuer: impl Into<String>) -> Self {
        let signing_key = SigningKey::random(&mut OsRng);
        let verifying_key = *signing_key.verifying_key();
        Self {
            issuer: issuer.into(),
            kid: format!("k-{}", Uuid::new_v4()),
            signing_key,
            verifying_key,
        }
    }

    pub(crate) fn issue(&self, claims: &RegistryClaims) -> Result<String, JwtError> {
        let header = encode_json(&Header {
            alg: "ES256",
            typ: "JWT",
            kid: &self.kid,
        });
        let payload = encode_json(claims);
        let signing_input = format!("{header}.{payload}");
        let signature: Signature = self.signing_key.sign(signing_input.as_bytes());
        Ok(format!(
            "{signing_input}.{}",
            URL_SAFE_NO_PAD.encode(signature.to_bytes())
        ))
    }

    pub(crate) fn verify(&self, token: &str, audience: &str) -> Result<RegistryClaims, JwtError> {
        let mut pieces = token.split('.');
        let encoded_header = pieces.next().ok_or(JwtError::Malformed)?;
        let encoded_payload = pieces.next().ok_or(JwtError::Malformed)?;
        let encoded_signature = pieces.next().ok_or(JwtError::Malformed)?;
        if pieces.next().is_some() {
            return Err(JwtError::Malformed);
        }
        let header: DecodedHeader =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(encoded_header)?)?;
        if header.alg != "ES256" || header.typ != "JWT" || header.kid != self.kid {
            return Err(JwtError::Header);
        }
        let payload = URL_SAFE_NO_PAD.decode(encoded_payload)?;
        let signature_bytes = URL_SAFE_NO_PAD.decode(encoded_signature)?;
        let signature = Signature::from_slice(&signature_bytes).map_err(|_| JwtError::Signature)?;
        let signing_input = format!("{encoded_header}.{encoded_payload}");
        self.verifying_key
            .verify(signing_input.as_bytes(), &signature)
            .map_err(|_| JwtError::Signature)?;
        let claims: RegistryClaims = serde_json::from_slice(&payload)?;
        if claims.iss != self.issuer || claims.aud != audience {
            return Err(JwtError::Audience);
        }
        let now = now_seconds();
        if now > claims.exp.saturating_add(30) || claims.nbf > now.saturating_add(30) {
            return Err(JwtError::Time);
        }
        Ok(claims)
    }
}

pub(crate) fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn encode_json<T: Serialize>(value: &T) -> String {
    URL_SAFE_NO_PAD.encode(serde_json::to_vec(value).expect("JWT values are serializable"))
}
