mod crypto;

use crate::PostgresDatabase;
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use server_core::{AuthError, AuthService, IssuedSession, SessionRecord};
use sqlx::PgPool;

const ADMIN_USERNAME: &str = "bokheim-admin";
const INVITE_DEFAULT_DAYS: i64 = 14;
const MIN_PASSWORD_LEN: usize = 10;
const MAX_PASSWORD_BYTES: usize = 1024;
const MIN_USERNAME_LEN: usize = 3;
const MAX_USERNAME_BYTES: usize = 64;
const TOKEN_TTL_DAYS: i64 = 90;
const TOKEN_LEN: usize = 43;
const INVITE_LEN: usize = 32;

fn internal(error: impl std::fmt::Display) -> AuthError {
    AuthError::Internal(error.to_string())
}

pub struct PostgresAuthService {
    pool: PgPool,
}

impl PostgresAuthService {
    pub fn new(database: PostgresDatabase) -> Self {
        Self { pool: database.pool().clone() }
    }

    async fn persist_token(&self, token_hash: &str, user_id: &str) -> Result<DateTime<Utc>, AuthError> {
        let expires_at = Utc::now() + Duration::days(TOKEN_TTL_DAYS);
        sqlx::query("INSERT INTO auth_token (token_hash, user_id, expires_at) VALUES ($1, $2, $3)").bind(token_hash).bind(user_id).bind(expires_at).execute(&self.pool).await.map_err(internal)?;
        Ok(expires_at)
    }
}

fn normalized_username(username: &str) -> Result<&str, AuthError> {
    let username = username.trim();
    if username.is_empty() {
        return Err(AuthError::EmptyUsername);
    }
    if username.len() < MIN_USERNAME_LEN
        || username.len() > MAX_USERNAME_BYTES
        || !username.chars().all(|character| character.is_alphanumeric() || matches!(character, '.' | '_' | '-'))
    {
        return Err(AuthError::InvalidUsername);
    }
    Ok(username)
}

fn valid_password_size(password: &str) -> bool {
    !password.is_empty() && password.len() <= MAX_PASSWORD_BYTES
}

fn valid_urlsafe_secret(value: &str, expected_len: usize) -> bool {
    value.len() == expected_len && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn username_taken(error: &sqlx::Error) -> bool {
    error.as_database_error().and_then(|error| error.code()).as_deref() == Some("23505")
}

#[async_trait]
impl AuthService for PostgresAuthService {
    async fn bootstrap_admin(&self, password: Option<&str>) -> Result<(), AuthError> {
        let Some(password) = password else {
            return Ok(());
        };
        if password.chars().count() < MIN_PASSWORD_LEN || !valid_password_size(password) {
            return Err(AuthError::InvalidPassword { min_characters: MIN_PASSWORD_LEN, max_bytes: MAX_PASSWORD_BYTES });
        }
        let existing: Option<String> = sqlx::query_scalar("SELECT id FROM users WHERE username = $1").bind(ADMIN_USERNAME).fetch_optional(&self.pool).await.map_err(internal)?;
        if existing.is_some() {
            return Ok(());
        }
        let user_id = uuid::Uuid::new_v4().to_string();
        let hash = crypto::hash_password(password)?;
        sqlx::query("INSERT INTO users (id, username, password_hash, is_admin) VALUES ($1, $2, $3, TRUE)").bind(&user_id).bind(ADMIN_USERNAME).bind(&hash).execute(&self.pool).await.map_err(internal)?;
        tracing::info!("bootstrapped admin user '{}'", ADMIN_USERNAME);
        Ok(())
    }

    async fn resolve_token(&self, token: &str) -> Result<SessionRecord, AuthError> {
        if !valid_urlsafe_secret(token, TOKEN_LEN) {
            return Err(AuthError::InvalidCredentials);
        }
        let token_hash = crypto::sha256_hex(token.as_bytes());
        let row: Option<(String, String)> = sqlx::query_as(
            "UPDATE auth_token
             SET last_used_at = now()
             FROM users
             WHERE auth_token.token_hash = $1
               AND auth_token.expires_at > now()
               AND users.id = auth_token.user_id
             RETURNING users.id, users.username",
        )
        .bind(&token_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;
        let Some((user_id, username)) = row else {
            return Err(AuthError::InvalidCredentials);
        };
        Ok(SessionRecord { user_id, username })
    }

    async fn login(&self, username: &str, password: &str) -> Result<IssuedSession, AuthError> {
        let Ok(username) = normalized_username(username) else {
            crypto::verify_password_dummy(if valid_password_size(password) { password } else { "invalid-password-size" });
            return Err(AuthError::InvalidCredentials);
        };
        if !valid_password_size(password) {
            crypto::verify_password_dummy("invalid-password-size");
            return Err(AuthError::InvalidCredentials);
        }
        let row: Option<(String, String, String)> = sqlx::query_as("SELECT id, username, password_hash FROM users WHERE username = $1 AND password_hash IS NOT NULL").bind(username).fetch_optional(&self.pool).await.map_err(internal)?;
        let Some((user_id, stored_username, password_hash)) = row else {
            crypto::verify_password_dummy(password);
            return Err(AuthError::InvalidCredentials);
        };
        if !crypto::verify_password(password, &password_hash) {
            return Err(AuthError::InvalidCredentials);
        }
        let (token, token_hash) = crypto::mint_token();
        let expires_at = self.persist_token(&token_hash, &user_id).await?;
        Ok(IssuedSession { token, expires_at, user: SessionRecord { user_id, username: stored_username } })
    }

    async fn logout(&self, token: &str) -> Result<(), AuthError> {
        if !valid_urlsafe_secret(token, TOKEN_LEN) {
            return Err(AuthError::InvalidCredentials);
        }
        let token_hash = crypto::sha256_hex(token.as_bytes());
        sqlx::query("DELETE FROM auth_token WHERE token_hash = $1").bind(&token_hash).execute(&self.pool).await.map_err(internal)?;
        Ok(())
    }

    async fn signup(&self, invite: &str, username: &str, password: &str) -> Result<IssuedSession, AuthError> {
        let username = normalized_username(username)?;
        if password.chars().count() < MIN_PASSWORD_LEN || !valid_password_size(password) {
            return Err(AuthError::InvalidPassword { min_characters: MIN_PASSWORD_LEN, max_bytes: MAX_PASSWORD_BYTES });
        }
        if !valid_urlsafe_secret(invite, INVITE_LEN) {
            return Err(AuthError::InvalidInvite);
        }
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let invite_hash = crypto::sha256_hex(invite.as_bytes());
        let invite_row: Option<(Option<String>, DateTime<Utc>)> = sqlx::query_as("SELECT consumed_by_user_id, expires_at FROM invite WHERE token_hash = $1 FOR UPDATE").bind(&invite_hash).fetch_optional(&mut *tx).await.map_err(internal)?;
        let Some((consumed, expires_at)) = invite_row else {
            return Err(AuthError::InvalidInvite);
        };
        if consumed.is_some() || expires_at < Utc::now() {
            return Err(AuthError::InvalidInvite);
        }
        let existing_user: Option<String> = sqlx::query_scalar("SELECT id FROM users WHERE username = $1").bind(username).fetch_optional(&mut *tx).await.map_err(internal)?;
        if existing_user.is_some() {
            return Err(AuthError::UsernameTaken);
        }
        let user_id = uuid::Uuid::new_v4().to_string();
        let hash = crypto::hash_password(password)?;
        if let Err(error) = sqlx::query("INSERT INTO users (id, username, password_hash) VALUES ($1, $2, $3)").bind(&user_id).bind(username).bind(&hash).execute(&mut *tx).await {
            return if username_taken(&error) { Err(AuthError::UsernameTaken) } else { Err(internal(error)) };
        }
        sqlx::query("UPDATE invite SET consumed_by_user_id = $1 WHERE token_hash = $2").bind(&user_id).bind(&invite_hash).execute(&mut *tx).await.map_err(internal)?;
        let (token, token_hash) = crypto::mint_token();
        let token_expiry = Utc::now() + Duration::days(TOKEN_TTL_DAYS);
        sqlx::query("INSERT INTO auth_token (token_hash, user_id, expires_at) VALUES ($1, $2, $3)").bind(&token_hash).bind(&user_id).bind(token_expiry).execute(&mut *tx).await.map_err(internal)?;
        tx.commit().await.map_err(internal)?;
        Ok(IssuedSession { token, expires_at: token_expiry, user: SessionRecord { user_id, username: username.to_string() } })
    }

    async fn create_invite(&self, note: Option<&str>, expires_in_days: Option<i64>) -> Result<(String, DateTime<Utc>), AuthError> {
        let (token, token_hash) = crypto::mint_invite();
        let days = expires_in_days.unwrap_or(INVITE_DEFAULT_DAYS).max(1);
        let expires_at = Utc::now() + Duration::days(days);
        sqlx::query("INSERT INTO invite (token_hash, note, expires_at) VALUES ($1, $2, $3)").bind(&token_hash).bind(note).bind(expires_at).execute(&self.pool).await.map_err(internal)?;
        Ok((token, expires_at))
    }

    async fn verify_admin(&self, username: &str, password: &str) -> Result<(), AuthError> {
        if username != ADMIN_USERNAME {
            return Err(AuthError::Forbidden);
        }
        let stored: Option<String> = sqlx::query_scalar("SELECT password_hash FROM users WHERE username = $1 AND is_admin = TRUE").bind(ADMIN_USERNAME).fetch_optional(&self.pool).await.map_err(internal)?;
        if !stored.as_deref().is_some_and(|stored| crypto::verify_password(password, stored)) {
            return Err(AuthError::Forbidden);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    #[test]
    fn authentication_inputs_are_bounded_before_expensive_work() {
        assert_eq!(normalized_username(" reader ").unwrap(), "reader");
        assert!(matches!(normalized_username("ab"), Err(AuthError::InvalidUsername)));
        assert!(matches!(normalized_username("reader name"), Err(AuthError::InvalidUsername)));
        assert!(matches!(normalized_username(&"a".repeat(MAX_USERNAME_BYTES + 1)), Err(AuthError::InvalidUsername)));
        assert!(!valid_password_size(&"x".repeat(MAX_PASSWORD_BYTES + 1)));
    }

    #[test]
    fn opaque_tokens_and_invites_have_one_canonical_wire_shape() {
        let (token, _) = crypto::mint_token();
        let (invite, _) = crypto::mint_invite();
        assert!(valid_urlsafe_secret(&token, TOKEN_LEN));
        assert!(valid_urlsafe_secret(&invite, INVITE_LEN));
        assert!(!valid_urlsafe_secret("", TOKEN_LEN));
        assert!(!valid_urlsafe_secret(&format!("{token} "), TOKEN_LEN));
        assert!(!valid_urlsafe_secret(&"a".repeat(TOKEN_LEN + 1), TOKEN_LEN));
    }

    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn resolving_a_session_never_extends_its_absolute_expiry() {
        let database_url = std::env::var("SYNC_E2E_DATABASE_URL").expect("SYNC_E2E_DATABASE_URL must be set");
        let pool = PgPoolOptions::new().max_connections(1).connect(&database_url).await.unwrap();
        let database = PostgresDatabase::from_pool(pool.clone());
        crate::migrate(&database).await.unwrap();
        let service = PostgresAuthService::new(database);
        let user_id = uuid::Uuid::new_v4().to_string();
        let username = format!("auth-test-{}", uuid::Uuid::new_v4().simple());
        sqlx::query("INSERT INTO users (id, username) VALUES ($1, $2)").bind(&user_id).bind(&username).execute(&pool).await.unwrap();
        let (token, token_hash) = crypto::mint_token();
        let expires_at = Utc::now() + Duration::hours(1);
        sqlx::query("INSERT INTO auth_token (token_hash, user_id, expires_at) VALUES ($1, $2, $3)").bind(&token_hash).bind(&user_id).bind(expires_at).execute(&pool).await.unwrap();
        let stored_before: DateTime<Utc> = sqlx::query_scalar("SELECT expires_at FROM auth_token WHERE token_hash = $1").bind(&token_hash).fetch_one(&pool).await.unwrap();

        assert_eq!(service.resolve_token(&token).await.unwrap().user_id, user_id);
        let stored_expiry: DateTime<Utc> = sqlx::query_scalar("SELECT expires_at FROM auth_token WHERE token_hash = $1").bind(&token_hash).fetch_one(&pool).await.unwrap();
        assert_eq!(stored_expiry, stored_before);

        sqlx::query("UPDATE auth_token SET expires_at = now() - INTERVAL '1 second' WHERE token_hash = $1").bind(&token_hash).execute(&pool).await.unwrap();
        assert!(matches!(service.resolve_token(&token).await, Err(AuthError::InvalidCredentials)));
        sqlx::query("DELETE FROM users WHERE id = $1").bind(&user_id).execute(&pool).await.unwrap();
    }
}
