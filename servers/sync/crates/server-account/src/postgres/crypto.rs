use crate::core::AuthError;
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::{Rng, RngCore};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;

const TOKEN_BYTES: usize = 32;

pub(super) fn hash_password(password: &str) -> Result<String, AuthError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default().hash_password(password.as_bytes(), &salt).map(|hash| hash.to_string()).map_err(|error| AuthError::Internal(error.to_string()))
}

pub(super) fn verify_password(password: &str, hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok()
}

/// Equalizes the missing-user and wrong-password work factors.
pub(super) fn verify_password_dummy(password: &str) {
    static DUMMY_HASH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    let hash = DUMMY_HASH.get_or_init(|| hash_password("dummy-password-for-timing").unwrap_or_default());
    if !hash.is_empty() {
        let _ = verify_password(password, hash);
    }
}

pub(super) fn mint_token() -> (String, String) {
    mint(TOKEN_BYTES)
}

pub(super) fn mint_verification_pin() -> (String, String) {
    let pin = format!("{:06}", OsRng.gen_range(0..1_000_000_u32));
    let hash = sha256_hex(pin.as_bytes());
    (pin, hash)
}

fn mint(bytes: usize) -> (String, String) {
    let mut buffer = vec![0_u8; bytes];
    OsRng.fill_bytes(&mut buffer);
    let token = URL_SAFE_NO_PAD.encode(&buffer);
    let hash = sha256_hex(token.as_bytes());
    (token, hash)
}

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().fold(String::new(), |mut output, byte| {
        let _ = write!(output, "{byte:02x}");
        output
    })
}

#[cfg(test)]
mod tests {
    use super::mint_verification_pin;

    #[test]
    fn verification_pin_contains_exactly_six_digits() {
        for _ in 0..100 {
            let (pin, hash) = mint_verification_pin();
            assert_eq!(pin.len(), 6);
            assert!(pin.bytes().all(|byte| byte.is_ascii_digit()));
            assert_eq!(hash.len(), 64);
        }
    }
}
