use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Duration, Utc};
use rand::RngCore;
use sha2::{Digest, Sha256};
use sqlx::PgPool;

const ADMIN_USERNAME: &str = "biblos-admin";
const TOKEN_BYTES: usize = 32;
const INVITE_BYTES: usize = 24;
const INVITE_DEFAULT_DAYS: i64 = 14;

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("password hashing error: {0}")]
    Hash(String),
    #[error("invalid credentials")]
    InvalidCredentials,
    #[error("username already taken")]
    UsernameTaken,
    #[error("user_id already adopted by another account")]
    UserIdTaken,
    #[error("invalid or expired invite")]
    InvalidInvite,
    #[error("forbidden")]
    Forbidden,
}

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

/// Generates a fresh bearer token and its sha256 hash. The token is what the
/// client stores; the hash is what we persist.
pub fn mint_token() -> (String, String) {
    let mut buf = [0u8; TOKEN_BYTES];
    OsRng.fill_bytes(&mut buf);
    let token = URL_SAFE_NO_PAD.encode(buf);
    let hash = sha256_hex(token.as_bytes());
    (token, hash)
}

pub fn mint_invite() -> (String, String) {
    let mut buf = [0u8; INVITE_BYTES];
    OsRng.fill_bytes(&mut buf);
    let token = URL_SAFE_NO_PAD.encode(buf);
    let hash = sha256_hex(token.as_bytes());
    (token, hash)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        let bytes = bytes.as_ref();
        let mut out = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            out.push_str(&format!("{:02x}", b));
        }
        out
    }
}

pub async fn bootstrap_admin(pool: &PgPool) -> Result<(), AuthError> {
    let Ok(password) = std::env::var("BIBLOS_ADMIN_PASSWORD") else {
        return Ok(());
    };
    let existing: Option<String> = sqlx::query_scalar("SELECT id FROM users WHERE username = $1")
        .bind(ADMIN_USERNAME)
        .fetch_optional(pool)
        .await?;
    if existing.is_some() {
        return Ok(());
    }
    let user_id = uuid::Uuid::new_v4().to_string();
    let hash = hash_password(&password)?;
    sqlx::query(
        "INSERT INTO users (id, username, password_hash, is_admin)
         VALUES ($1, $2, $3, TRUE)",
    )
    .bind(&user_id)
    .bind(ADMIN_USERNAME)
    .bind(&hash)
    .execute(pool)
    .await?;
    tracing::info!("bootstrapped admin user '{}'", ADMIN_USERNAME);
    Ok(())
}

#[derive(Debug)]
pub struct SessionRecord {
    pub user_id: String,
    pub username: String,
}

/// Reads the bearer token from the database, returning the resolved user.
/// Updates `last_used_at` as a side effect.
pub async fn resolve_token(pool: &PgPool, token: &str) -> Result<SessionRecord, AuthError> {
    let token_hash = sha256_hex(token.as_bytes());
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT users.id, users.username
         FROM auth_token
         JOIN users ON users.id = auth_token.user_id
         WHERE auth_token.token_hash = $1",
    )
    .bind(&token_hash)
    .fetch_optional(pool)
    .await?;
    let Some((user_id, username)) = row else {
        return Err(AuthError::InvalidCredentials);
    };
    let _ = sqlx::query("UPDATE auth_token SET last_used_at = now() WHERE token_hash = $1")
        .bind(&token_hash)
        .execute(pool)
        .await;
    Ok(SessionRecord { user_id, username })
}

pub async fn login(pool: &PgPool, username: &str, password: &str) -> Result<(String, SessionRecord), AuthError> {
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT id, password_hash FROM users WHERE username = $1 AND password_hash IS NOT NULL",
    )
    .bind(username)
    .fetch_optional(pool)
    .await?;
    let Some((user_id, password_hash)) = row else {
        return Err(AuthError::InvalidCredentials);
    };
    if !verify_password(password, &password_hash) {
        return Err(AuthError::InvalidCredentials);
    }
    let (token, token_hash) = mint_token();
    sqlx::query("INSERT INTO auth_token (token_hash, user_id) VALUES ($1, $2)")
        .bind(&token_hash)
        .bind(&user_id)
        .execute(pool)
        .await?;
    Ok((
        token,
        SessionRecord { user_id, username: username.to_string() },
    ))
}

pub async fn logout(pool: &PgPool, token: &str) -> Result<(), AuthError> {
    let token_hash = sha256_hex(token.as_bytes());
    sqlx::query("DELETE FROM auth_token WHERE token_hash = $1")
        .bind(&token_hash)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn signup(
    pool: &PgPool,
    invite: &str,
    username: &str,
    password: &str,
    adopt_user_id: Option<&str>,
) -> Result<(String, SessionRecord), AuthError> {
    // Check + consume the invite atomically.
    let mut tx = pool.begin().await?;
    let invite_hash = sha256_hex(invite.as_bytes());
    let invite_row: Option<(Option<String>, DateTime<Utc>)> = sqlx::query_as(
        "SELECT consumed_by_user_id, expires_at FROM invite WHERE token_hash = $1 FOR UPDATE",
    )
    .bind(&invite_hash)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((consumed, expires_at)) = invite_row else {
        return Err(AuthError::InvalidInvite);
    };
    if consumed.is_some() || expires_at < Utc::now() {
        return Err(AuthError::InvalidInvite);
    }

    // Username taken?
    let username_taken: Option<String> = sqlx::query_scalar("SELECT id FROM users WHERE username = $1")
        .bind(username)
        .fetch_optional(&mut *tx)
        .await?;
    if username_taken.is_some() {
        return Err(AuthError::UsernameTaken);
    }

    // user_id adoption: if requested, ensure the row is either missing entirely
    // or exists with no username (legacy pre-auth user). Otherwise mint a fresh one.
    let user_id = if let Some(requested) = adopt_user_id {
        let existing: Option<Option<String>> = sqlx::query_scalar("SELECT username FROM users WHERE id = $1")
            .bind(requested)
            .fetch_optional(&mut *tx)
            .await?;
        match existing {
            Some(None) => requested.to_string(),       // row exists, no username -> safe to adopt
            None => requested.to_string(),              // doesn't exist yet -> we'll INSERT below
            Some(Some(_)) => return Err(AuthError::UserIdTaken),
        }
    } else {
        uuid::Uuid::new_v4().to_string()
    };

    let hash = hash_password(password)?;
    sqlx::query(
        "INSERT INTO users (id, username, password_hash)
         VALUES ($1, $2, $3)
         ON CONFLICT (id) DO UPDATE SET username = EXCLUDED.username, password_hash = EXCLUDED.password_hash",
    )
    .bind(&user_id)
    .bind(username)
    .bind(&hash)
    .execute(&mut *tx)
    .await?;

    sqlx::query("UPDATE invite SET consumed_by_user_id = $1 WHERE token_hash = $2")
        .bind(&user_id)
        .bind(&invite_hash)
        .execute(&mut *tx)
        .await?;

    let (token, token_hash) = mint_token();
    sqlx::query("INSERT INTO auth_token (token_hash, user_id) VALUES ($1, $2)")
        .bind(&token_hash)
        .bind(&user_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok((token, SessionRecord { user_id, username: username.to_string() }))
}

pub async fn create_invite(
    pool: &PgPool,
    note: Option<&str>,
    expires_in_days: Option<i64>,
) -> Result<(String, DateTime<Utc>), AuthError> {
    let (token, token_hash) = mint_invite();
    let days = expires_in_days.unwrap_or(INVITE_DEFAULT_DAYS).max(1);
    let expires_at = Utc::now() + Duration::days(days);
    sqlx::query(
        "INSERT INTO invite (token_hash, note, expires_at) VALUES ($1, $2, $3)",
    )
    .bind(&token_hash)
    .bind(note)
    .bind(expires_at)
    .execute(pool)
    .await?;
    Ok((token, expires_at))
}

/// Verify HTTP Basic admin credentials (`biblos-admin:<password>`).
pub async fn verify_admin_basic(pool: &PgPool, basic: &str) -> Result<(), AuthError> {
    let (username, password) = decode_basic(basic).ok_or(AuthError::Forbidden)?;
    if username != ADMIN_USERNAME {
        return Err(AuthError::Forbidden);
    }
    let stored: Option<String> = sqlx::query_scalar(
        "SELECT password_hash FROM users WHERE username = $1 AND is_admin = TRUE",
    )
    .bind(ADMIN_USERNAME)
    .fetch_optional(pool)
    .await?;
    let Some(stored) = stored else {
        return Err(AuthError::Forbidden);
    };
    if !verify_password(&password, &stored) {
        return Err(AuthError::Forbidden);
    }
    Ok(())
}

fn decode_basic(header: &str) -> Option<(String, String)> {
    let encoded = header.strip_prefix("Basic ")?;
    let decoded = base64::engine::general_purpose::STANDARD.decode(encoded.trim()).ok()?;
    let s = String::from_utf8(decoded).ok()?;
    let (u, p) = s.split_once(':')?;
    Some((u.to_string(), p.to_string()))
}
