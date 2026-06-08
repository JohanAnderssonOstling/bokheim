//! Authentication: error type, session records, and the database-backed
//! operations (login, signup, logout, token resolution, invites, admin
//! bootstrap). Pure primitives live in [`crypto`] and are re-exported.

mod crypto;

pub use crypto::{hash_password, mint_invite, mint_token, sha256_hex, verify_password};

use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;

const ADMIN_USERNAME: &str = "biblos-admin";
const INVITE_DEFAULT_DAYS: i64 = 14;
pub const MIN_PASSWORD_LEN: usize = 10;
const TOKEN_TTL_DAYS: i64 = 90;

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
    #[error("invalid or expired invite")]
    InvalidInvite,
    #[error("forbidden")]
    Forbidden,
    #[error("password must be at least {0} characters")]
    WeakPassword(usize),
    #[error("username must not be empty")]
    EmptyUsername,
}

#[derive(Debug)]
pub struct SessionRecord {
    pub user_id: String,
    pub username: String,
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
    let hash = crypto::hash_password(&password)?;
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

/// Inserts a freshly minted token with a rolling expiry `TOKEN_TTL_DAYS` out.
async fn persist_token(pool: &PgPool, token_hash: &str, user_id: &str) -> Result<(), AuthError> {
    let expires_at = Utc::now() + Duration::days(TOKEN_TTL_DAYS);
    sqlx::query("INSERT INTO auth_token (token_hash, user_id, expires_at) VALUES ($1, $2, $3)")
        .bind(token_hash)
        .bind(user_id)
        .bind(expires_at)
        .execute(pool)
        .await?;
    Ok(())
}

/// Reads the bearer token from the database, returning the resolved user.
/// Rejects expired tokens; on success extends the rolling expiry and bumps
/// `last_used_at`.
pub async fn resolve_token(pool: &PgPool, token: &str) -> Result<SessionRecord, AuthError> {
    let token_hash = crypto::sha256_hex(token.as_bytes());
    let row: Option<(String, String, Option<DateTime<Utc>>)> = sqlx::query_as(
        "SELECT users.id, users.username, auth_token.expires_at
         FROM auth_token
         JOIN users ON users.id = auth_token.user_id
         WHERE auth_token.token_hash = $1",
    )
    .bind(&token_hash)
    .fetch_optional(pool)
    .await?;
    let Some((user_id, username, expires_at)) = row else {
        return Err(AuthError::InvalidCredentials);
    };
    if let Some(exp) = expires_at {
        if exp < Utc::now() {
            // Expired — drop it and reject.
            let _ = sqlx::query("DELETE FROM auth_token WHERE token_hash = $1").bind(&token_hash).execute(pool).await;
            return Err(AuthError::InvalidCredentials);
        }
    }
    let next_expiry = Utc::now() + Duration::days(TOKEN_TTL_DAYS);
    let _ = sqlx::query("UPDATE auth_token SET last_used_at = now(), expires_at = $2 WHERE token_hash = $1")
        .bind(&token_hash)
        .bind(next_expiry)
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
        // Spend the same argon2 work as the happy path so response time doesn't
        // reveal whether the username exists.
        crypto::verify_password_dummy(password);
        return Err(AuthError::InvalidCredentials);
    };
    if !crypto::verify_password(password, &password_hash) {
        return Err(AuthError::InvalidCredentials);
    }
    let (token, token_hash) = crypto::mint_token();
    persist_token(pool, &token_hash, &user_id).await?;
    Ok((token, SessionRecord { user_id, username: username.to_string() }))
}

pub async fn logout(pool: &PgPool, token: &str) -> Result<(), AuthError> {
    let token_hash = crypto::sha256_hex(token.as_bytes());
    sqlx::query("DELETE FROM auth_token WHERE token_hash = $1")
        .bind(&token_hash)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn signup(pool: &PgPool, invite: &str, username: &str, password: &str) -> Result<(String, SessionRecord), AuthError> {
    if username.trim().is_empty() {
        return Err(AuthError::EmptyUsername);
    }
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err(AuthError::WeakPassword(MIN_PASSWORD_LEN));
    }

    // Check + consume the invite atomically.
    let mut tx = pool.begin().await?;
    let invite_hash = crypto::sha256_hex(invite.as_bytes());
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

    // The server is the sole authority for user identity; the client never
    // supplies it. Local data is not user-scoped (events carry no user_id), so
    // the device simply pushes its existing local library under this new id.
    let user_id = uuid::Uuid::new_v4().to_string();

    let hash = crypto::hash_password(password)?;
    sqlx::query("INSERT INTO users (id, username, password_hash) VALUES ($1, $2, $3)")
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

    let (token, token_hash) = crypto::mint_token();
    let token_expiry = Utc::now() + Duration::days(TOKEN_TTL_DAYS);
    sqlx::query("INSERT INTO auth_token (token_hash, user_id, expires_at) VALUES ($1, $2, $3)")
        .bind(&token_hash)
        .bind(&user_id)
        .bind(token_expiry)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok((token, SessionRecord { user_id, username: username.to_string() }))
}

pub async fn create_invite(pool: &PgPool, note: Option<&str>, expires_in_days: Option<i64>) -> Result<(String, DateTime<Utc>), AuthError> {
    let (token, token_hash) = crypto::mint_invite();
    let days = expires_in_days.unwrap_or(INVITE_DEFAULT_DAYS).max(1);
    let expires_at = Utc::now() + Duration::days(days);
    sqlx::query("INSERT INTO invite (token_hash, note, expires_at) VALUES ($1, $2, $3)")
        .bind(&token_hash)
        .bind(note)
        .bind(expires_at)
        .execute(pool)
        .await?;
    Ok((token, expires_at))
}

/// Verify HTTP Basic admin credentials (`biblos-admin:<password>`).
pub async fn verify_admin_basic(pool: &PgPool, basic: &str) -> Result<(), AuthError> {
    let (username, password) = crypto::decode_basic(basic).ok_or(AuthError::Forbidden)?;
    if username != ADMIN_USERNAME {
        return Err(AuthError::Forbidden);
    }
    let stored: Option<String> = sqlx::query_scalar("SELECT password_hash FROM users WHERE username = $1 AND is_admin = TRUE")
        .bind(ADMIN_USERNAME)
        .fetch_optional(pool)
        .await?;
    let Some(stored) = stored else {
        return Err(AuthError::Forbidden);
    };
    if !crypto::verify_password(&password, &stored) {
        return Err(AuthError::Forbidden);
    }
    Ok(())
}
