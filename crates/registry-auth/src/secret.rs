use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand_core::{OsRng, RngCore};
use sha2::{Digest as _, Sha256};
use subtle::ConstantTimeEq;

#[derive(Clone)]
pub(crate) struct SecretVerifier {
    salt: [u8; 16],
    digest: [u8; 32],
}

impl SecretVerifier {
    pub(crate) fn new(secret: &str) -> Self {
        let mut salt = [0_u8; 16];
        OsRng.fill_bytes(&mut salt);
        Self {
            salt,
            digest: salted_digest(&salt, secret),
        }
    }

    pub(crate) fn verify(&self, secret: &str) -> bool {
        self.digest.ct_eq(&salted_digest(&self.salt, secret)).into()
    }
}

pub(crate) fn issue_secret(prefix: &str) -> (String, String, SecretVerifier) {
    let mut random = [0_u8; 32];
    OsRng.fill_bytes(&mut random);
    let secret = format!("{prefix}{}", URL_SAFE_NO_PAD.encode(random));
    let token_prefix = secret.chars().take(16).collect();
    let verifier = SecretVerifier::new(&secret);
    (secret, token_prefix, verifier)
}

pub(crate) fn raw_digest(secret: &str) -> [u8; 32] {
    Sha256::digest(secret.as_bytes()).into()
}

fn salted_digest(salt: &[u8; 16], secret: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(salt);
    hasher.update(secret.as_bytes());
    hasher.finalize().into()
}
