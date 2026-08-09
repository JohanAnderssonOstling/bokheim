use super::{crypto, email_taken, internal, issue_token_pair, issued_session, normalized_email, PostgresAccountService, TOKEN_LEN};
use crate::core::{AuthError, AuthenticatedUser, IssuedSession};
use bokheim_account_contract::{PASSWORD_MAX_BYTES, PASSWORD_MIN_CHARACTERS};
use chrono::{Duration, Utc};
use sqlx::{Postgres, Transaction};

const REGISTRATION_TTL_HOURS: i64 = 24;
const PASSWORD_RESET_TTL_MINUTES: i64 = 60;
const RESEND_COOLDOWN_SECONDS: i64 = 60;

async fn queue_email(transaction: &mut Transaction<'_, Postgres>, recipient: &str, kind: &str, token: &str) -> Result<(), AuthError> {
    sqlx::query("INSERT INTO account_email_outbox(id, recipient, message_kind, token) VALUES ($1, $2, $3, $4)")
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(recipient)
        .bind(kind)
        .bind(token)
        .execute(&mut **transaction)
        .await
        .map_err(internal)?;
    Ok(())
}

fn validate_password(password: &str) -> Result<(), AuthError> {
    if password.chars().count() < PASSWORD_MIN_CHARACTERS || password.is_empty() || password.len() > PASSWORD_MAX_BYTES {
        return Err(AuthError::InvalidPassword { min_characters: PASSWORD_MIN_CHARACTERS, max_bytes: PASSWORD_MAX_BYTES });
    }
    Ok(())
}

impl PostgresAccountService {
    pub(super) async fn request_public_registration_impl(&self, email: &str, password: &str) -> Result<(), AuthError> {
        let email = normalized_email(email, &self.blocked_email_domains)?;
        validate_password(password)?;

        // Hash before conflict checks so accepted responses do not expose
        // email existence through the dominant work factor.
        let password_hash = self.hash_password(password.to_owned()).await?;
        let (token, token_hash) = crypto::mint_token();
        let mut transaction = self.pool.begin().await.map_err(internal)?;
        sqlx::query("DELETE FROM pending_registration WHERE expires_at <= now()").execute(&mut *transaction).await.map_err(internal)?;
        let conflict: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM users WHERE email=$1)
                OR EXISTS(SELECT 1 FROM pending_registration WHERE email=$1)",
        )
        .bind(&email)
        .fetch_one(&mut *transaction)
        .await
        .map_err(internal)?;
        if conflict {
            transaction.commit().await.map_err(internal)?;
            return Ok(());
        }

        let inserted = sqlx::query(
            "INSERT INTO pending_registration(id, email, password_hash, verification_hash, expires_at)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(&email)
        .bind(password_hash)
        .bind(token_hash)
        .bind(Utc::now() + Duration::hours(REGISTRATION_TTL_HOURS))
        .execute(&mut *transaction)
        .await;
        if let Err(error) = inserted {
            return if email_taken(&error) { Ok(()) } else { Err(internal(error)) };
        }
        queue_email(&mut transaction, &email, "verify_email", &token).await?;
        transaction.commit().await.map_err(internal)
    }

    pub(super) async fn resend_verification_impl(&self, email: &str) -> Result<(), AuthError> {
        let email = normalized_email(email, &self.blocked_email_domains)?;
        let mut transaction = self.pool.begin().await.map_err(internal)?;
        let pending: Option<(String, chrono::DateTime<Utc>)> =
            sqlx::query_as("SELECT id, last_sent_at FROM pending_registration WHERE email=$1 AND expires_at > now() FOR UPDATE").bind(&email).fetch_optional(&mut *transaction).await.map_err(internal)?;
        let Some((id, last_sent_at)) = pending else {
            transaction.commit().await.map_err(internal)?;
            return Ok(());
        };
        if last_sent_at > Utc::now() - Duration::seconds(RESEND_COOLDOWN_SECONDS) {
            transaction.commit().await.map_err(internal)?;
            return Ok(());
        }
        let (token, token_hash) = crypto::mint_token();
        sqlx::query(
            "UPDATE pending_registration
             SET verification_hash=$2, expires_at=$3, last_sent_at=now()
             WHERE id=$1",
        )
        .bind(id)
        .bind(token_hash)
        .bind(Utc::now() + Duration::hours(REGISTRATION_TTL_HOURS))
        .execute(&mut *transaction)
        .await
        .map_err(internal)?;
        sqlx::query("DELETE FROM account_email_outbox WHERE recipient=$1 AND message_kind='verify_email' AND claimed_by IS NULL").bind(&email).execute(&mut *transaction).await.map_err(internal)?;
        queue_email(&mut transaction, &email, "verify_email", &token).await?;
        transaction.commit().await.map_err(internal)
    }

    pub(super) async fn verify_email_impl(&self, token: &str) -> Result<IssuedSession, AuthError> {
        if !super::valid_urlsafe_secret(token, TOKEN_LEN) {
            return Err(AuthError::InvalidOneTimeToken);
        }
        let token_hash = crypto::sha256_hex(token.as_bytes());
        let mut transaction = self.pool.begin().await.map_err(internal)?;
        let pending: Option<(String, String, String)> = sqlx::query_as(
            "SELECT id, email, password_hash
             FROM pending_registration
             WHERE verification_hash=$1 AND expires_at > now()
             FOR UPDATE",
        )
        .bind(token_hash)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(internal)?;
        let Some((pending_id, email, password_hash)) = pending else {
            return Err(AuthError::InvalidOneTimeToken);
        };
        let user_id = uuid::Uuid::new_v4().to_string();
        let inserted = sqlx::query("INSERT INTO users(id, password_hash, email) VALUES ($1, $2, $3)").bind(&user_id).bind(password_hash).bind(&email).execute(&mut *transaction).await;
        if let Err(error) = inserted {
            return if email_taken(&error) { Err(AuthError::InvalidOneTimeToken) } else { Err(internal(error)) };
        }
        sqlx::query("DELETE FROM pending_registration WHERE id=$1").bind(pending_id).execute(&mut *transaction).await.map_err(internal)?;
        sqlx::query("DELETE FROM account_email_outbox WHERE recipient=$1 AND message_kind='verify_email'").bind(&email).execute(&mut *transaction).await.map_err(internal)?;
        let tokens = issue_token_pair(&mut transaction, &user_id).await?;
        transaction.commit().await.map_err(internal)?;
        Ok(issued_session(tokens, AuthenticatedUser { user_id, email, is_admin: false }))
    }

    pub(super) async fn request_password_reset_impl(&self, email: &str) -> Result<(), AuthError> {
        let email = normalized_email(email, &self.blocked_email_domains)?;
        let mut transaction = self.pool.begin().await.map_err(internal)?;
        let user_id: Option<String> = sqlx::query_scalar("SELECT id FROM users WHERE email=$1").bind(&email).fetch_optional(&mut *transaction).await.map_err(internal)?;
        let Some(user_id) = user_id else {
            transaction.commit().await.map_err(internal)?;
            return Ok(());
        };
        let requested_at: Option<chrono::DateTime<Utc>> = sqlx::query_scalar("SELECT requested_at FROM password_reset WHERE user_id=$1 FOR UPDATE").bind(&user_id).fetch_optional(&mut *transaction).await.map_err(internal)?;
        if requested_at.is_some_and(|requested| requested > Utc::now() - Duration::seconds(RESEND_COOLDOWN_SECONDS)) {
            transaction.commit().await.map_err(internal)?;
            return Ok(());
        }
        let (token, token_hash) = crypto::mint_token();
        sqlx::query(
            "INSERT INTO password_reset(user_id, token_hash, requested_at, expires_at)
             VALUES ($1, $2, now(), $3)
             ON CONFLICT (user_id) DO UPDATE SET token_hash=EXCLUDED.token_hash, requested_at=now(), expires_at=EXCLUDED.expires_at",
        )
        .bind(&user_id)
        .bind(token_hash)
        .bind(Utc::now() + Duration::minutes(PASSWORD_RESET_TTL_MINUTES))
        .execute(&mut *transaction)
        .await
        .map_err(internal)?;
        sqlx::query("DELETE FROM account_email_outbox WHERE recipient=$1 AND message_kind='reset_password' AND claimed_by IS NULL").bind(&email).execute(&mut *transaction).await.map_err(internal)?;
        queue_email(&mut transaction, &email, "reset_password", &token).await?;
        transaction.commit().await.map_err(internal)
    }

    pub(super) async fn reset_password_impl(&self, token: &str, new_password: &str) -> Result<(), AuthError> {
        if !super::valid_urlsafe_secret(token, TOKEN_LEN) {
            return Err(AuthError::InvalidOneTimeToken);
        }
        validate_password(new_password)?;
        let password_hash = self.hash_password(new_password.to_owned()).await?;
        let token_hash = crypto::sha256_hex(token.as_bytes());
        let mut transaction = self.pool.begin().await.map_err(internal)?;
        let reset: Option<(String, String)> = sqlx::query_as(
            "SELECT password_reset.user_id, users.email
             FROM password_reset
             JOIN users ON users.id=password_reset.user_id
             WHERE password_reset.token_hash=$1 AND password_reset.expires_at > now()
             FOR UPDATE OF password_reset",
        )
        .bind(token_hash)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(internal)?;
        let Some((user_id, email)) = reset else {
            return Err(AuthError::InvalidOneTimeToken);
        };
        sqlx::query("UPDATE users SET password_hash=$2 WHERE id=$1").bind(&user_id).bind(password_hash).execute(&mut *transaction).await.map_err(internal)?;
        sqlx::query("DELETE FROM auth_session WHERE user_id=$1").bind(&user_id).execute(&mut *transaction).await.map_err(internal)?;
        sqlx::query("DELETE FROM auth_token WHERE user_id=$1").bind(&user_id).execute(&mut *transaction).await.map_err(internal)?;
        sqlx::query("DELETE FROM password_reset WHERE user_id=$1").bind(&user_id).execute(&mut *transaction).await.map_err(internal)?;
        sqlx::query("DELETE FROM account_email_outbox WHERE recipient=$1 AND message_kind='reset_password'").bind(email).execute(&mut *transaction).await.map_err(internal)?;
        transaction.commit().await.map_err(internal)?;
        self.identity_cache.invalidate_user(&user_id);
        Ok(())
    }
}
