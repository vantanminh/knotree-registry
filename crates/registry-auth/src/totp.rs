use hmac::{Hmac, Mac};
use rand_core::{OsRng, RngCore};
use sha1::Sha1;
use subtle::ConstantTimeEq;

type HmacSha1 = Hmac<Sha1>;

const SECRET_BYTES: usize = 20;
const STEP_SECONDS: u64 = 30;
const CODE_DIGITS: u32 = 1_000_000;
const BASE32_ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

pub(crate) fn generate_secret() -> String {
    let mut bytes = [0_u8; SECRET_BYTES];
    OsRng.fill_bytes(&mut bytes);
    encode_base32(&bytes)
}

pub(crate) fn verify(secret: &str, code: &str, now_seconds: u64) -> bool {
    let normalized = code
        .chars()
        .filter(|character| !character.is_ascii_whitespace())
        .collect::<String>();
    if normalized.len() != 6 || !normalized.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    let Some(secret) = decode_base32(secret) else {
        return false;
    };
    let counter = now_seconds / STEP_SECONDS;
    for offset in [
        counter.checked_sub(1),
        Some(counter),
        counter.checked_add(1),
    ]
    .into_iter()
    .flatten()
    {
        let expected = code_at(&secret, offset);
        if normalized.as_bytes().ct_eq(expected.as_bytes()).into() {
            return true;
        }
    }
    false
}

pub(crate) fn otpauth_uri(issuer: &str, account: &str, secret: &str) -> String {
    let issuer_label = encode_uri_component(issuer);
    let account_label = encode_uri_component(account);
    let issuer_query = encode_uri_component(issuer);
    format!(
        "otpauth://totp/{issuer_label}:{account_label}?secret={secret}&issuer={issuer_query}&algorithm=SHA1&digits=6&period={STEP_SECONDS}"
    )
}

fn code_at(secret: &[u8], counter: u64) -> String {
    let mut mac = HmacSha1::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(&counter.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = (digest[19] & 0x0f) as usize;
    let binary = (u32::from(digest[offset]) & 0x7f) << 24
        | u32::from(digest[offset + 1]) << 16
        | u32::from(digest[offset + 2]) << 8
        | u32::from(digest[offset + 3]);
    format!("{:06}", binary % CODE_DIGITS)
}

fn encode_base32(bytes: &[u8]) -> String {
    let mut output = String::with_capacity((bytes.len() * 8).div_ceil(5));
    let mut buffer = 0_u16;
    let mut bits = 0_u8;
    for byte in bytes {
        buffer = (buffer << 8) | u16::from(*byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            output.push(BASE32_ALPHABET[((buffer >> bits) & 0x1f) as usize] as char);
        }
    }
    if bits > 0 {
        output.push(BASE32_ALPHABET[((buffer << (5 - bits)) & 0x1f) as usize] as char);
    }
    output
}

fn decode_base32(value: &str) -> Option<Vec<u8>> {
    let mut output = Vec::with_capacity(value.len() * 5 / 8);
    let mut buffer = 0_u32;
    let mut bits = 0_u8;
    for byte in value.bytes().filter(|byte| *byte != b'=') {
        let upper = byte.to_ascii_uppercase();
        let digit = match upper {
            b'A'..=b'Z' => upper - b'A',
            b'2'..=b'7' => upper - b'2' + 26,
            _ => return None,
        };
        buffer = (buffer << 5) | u32::from(digit);
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            output.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    (output.len() >= 16).then_some(output)
}

fn encode_uri_component(value: &str) -> String {
    let mut output = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            output.push(byte as char);
        } else {
            output.push('%');
            output.push(char::from(b"0123456789ABCDEF"[(byte >> 4) as usize]));
            output.push(char::from(b"0123456789ABCDEF"[(byte & 0x0f) as usize]));
        }
    }
    output
}

#[cfg(test)]
pub(crate) fn code_for_test(secret: &str, counter: u64) -> String {
    code_at(&decode_base32(secret).expect("test secret"), counter)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_rfc6238_sha1_vector() {
        let secret = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";
        assert_eq!(code_for_test(secret, 59 / STEP_SECONDS), "287082");
        assert!(verify(secret, "287082", 59));
    }

    #[test]
    fn generates_uri_without_padding() {
        let secret = generate_secret();
        assert_eq!(secret.len(), 32);
        assert!(!secret.contains('='));
        let uri = otpauth_uri("Knotree Registry", "admin", &secret);
        assert!(uri.starts_with("otpauth://totp/Knotree%20Registry:admin?"));
        assert!(uri.contains("algorithm=SHA1"));
    }
}
