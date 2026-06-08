//! Pure auth primitives: password hashing, token/invite generation, hashing,
//! and HTTP Basic decoding. No database, no async — unit-testable in isolation.

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;

use super::AuthError;

const TOKEN_BYTES: usize = 32;
const INVITE_BYTES: usize = 24;

pub fn hash_password(password: &str) -> Result<String, AuthError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AuthError::Hash(e.to_string()))
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok()
}

/// Runs an argon2 verification against a throwaway hash and discards the result.
/// Used to equalize login response time when the username does not exist, so the
/// fast "no such user" path can't be distinguished from the slow "wrong password"
/// path (which would otherwise leak which usernames are registered).
pub fn verify_password_dummy(password: &str) {
    // A real argon2id PHC string over an arbitrary secret; the value is
    // irrelevant, only the work performed matters.
    static DUMMY_HASH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    let hash = DUMMY_HASH.get_or_init(|| hash_password("dummy-password-for-timing").unwrap_or_default());
    if !hash.is_empty() {
        let _ = verify_password(password, hash);
    }
}

/// Generates a fresh bearer token and its sha256 hash. The token is what the
/// client stores; the hash is what we persist.
pub fn mint_token() -> (String, String) {
    mint(TOKEN_BYTES)
}

pub fn mint_invite() -> (String, String) {
    mint(INVITE_BYTES)
}

fn mint(bytes: usize) -> (String, String) {
    let mut buf = vec![0u8; bytes];
    OsRng.fill_bytes(&mut buf);
    let token = URL_SAFE_NO_PAD.encode(&buf);
    let hash = sha256_hex(token.as_bytes());
    (token, hash)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Decodes an HTTP `Authorization: Basic <base64(user:pass)>` header.
pub fn decode_basic(header: &str) -> Option<(String, String)> {
    let encoded = header.strip_prefix("Basic ")?;
    let decoded = base64::engine::general_purpose::STANDARD.decode(encoded.trim()).ok()?;
    let s = String::from_utf8(decoded).ok()?;
    let (u, p) = s.split_once(':')?;
    Some((u.to_string(), p.to_string()))
}
