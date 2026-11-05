use server_account::{AccountTokenKey, AuthAttempt, AuthError, PostgresAccountService};
use server_postgres::PostgresDatabase;
use sqlx::{postgres::PgPoolOptions, PgPool};
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires SYNC_E2E_DATABASE_URL"]
async fn aliases_share_persistent_subject_limits_and_success_clears_the_same_bucket() {
    let url = std::env::var("SYNC_E2E_DATABASE_URL").unwrap();
    let admin = PgPool::connect(&url).await.unwrap();
    let schema = format!("account_rate_limits_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
    let search_path = format!("SET search_path TO {schema}");
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .after_connect(move |connection, _| {
            let statement = search_path.clone();
            Box::pin(async move { sqlx::query(&statement).execute(connection).await.map(|_| ()) })
        })
        .connect(&url)
        .await
        .unwrap();
    let database = PostgresDatabase::from_pool(pool);
    server_postgres::migrate(&database).await.unwrap();
    let accounts = PostgresAccountService::new(database.clone(), AccountTokenKey::from_base64("WlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlo=").unwrap());

    for (attempt, allowance) in [
        (AuthAttempt::Login, 8),
        (AuthAttempt::DeleteAccount, 5),
        (AuthAttempt::PublicRegistration, 3),
        (AuthAttempt::ResendVerification, 3),
        (AuthAttempt::VerifyEmail, 5),
        (AuthAttempt::RequestPasswordReset, 3),
    ] {
        let canonical = format!("subject-{}@example.com", Uuid::new_v4().simple());
        for index in 0..allowance {
            let alias = format!(" {}{} ", canonical.to_uppercase(), ".".repeat(index));
            accounts.enforce_rate_limit(attempt, &format!("198.51.100.{}", index + 1), &alias).await.unwrap();
        }
        assert!(matches!(accounts.enforce_rate_limit(attempt, "203.0.113.1", &canonical).await, Err(AuthError::RateLimited)));
        // A fresh service still observes the same persistent bucket.
        let restarted = PostgresAccountService::new(database.clone(), AccountTokenKey::from_base64("WlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlo=").unwrap());
        assert!(matches!(restarted.enforce_rate_limit(attempt, "203.0.113.2", &format!("{canonical}..")).await, Err(AuthError::RateLimited)));
        restarted.clear_subject_rate_limit(attempt, &format!(" {}. ", canonical.to_uppercase())).await.unwrap();
        accounts.enforce_rate_limit(attempt, "203.0.113.3", &canonical).await.unwrap();
    }

    // Reset-token attempts have no email bucket. Rotating addresses within a
    // single IPv6 /64 must still consume the same sixty-attempt network limit.
    for index in 1..=60 {
        accounts.enforce_rate_limit(AuthAttempt::ResetPassword, &format!("2001:db8:1234:5678::{index:x}"), "password-reset").await.unwrap();
    }
    assert!(matches!(accounts.enforce_rate_limit(AuthAttempt::ResetPassword, "2001:db8:1234:5678::ffff", "password-reset").await, Err(AuthError::RateLimited)));
    accounts.enforce_rate_limit(AuthAttempt::ResetPassword, "2001:db8:1234:5679::1", "password-reset").await.unwrap();

    database.pool().close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE")).execute(&admin).await.unwrap();
    admin.close().await;
}
