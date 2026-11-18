//! Bounded expiry collection, independent of login and refresh traffic.
use super::PostgresAccountService;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{interval, MissedTickBehavior};

impl PostgresAccountService {
    pub fn spawn_cleanup_worker(self: &Arc<Self>) {
        let service = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut ticks = interval(Duration::from_secs(60));
            ticks.set_missed_tick_behavior(MissedTickBehavior::Skip);
            loop {
                ticks.tick().await;
                let Some(service) = service.upgrade() else { break };
                if let Err(error) = service.collect_expired_credentials().await {
                    tracing::error!(%error, "authentication cleanup failed; will retry");
                }
            }
        });
    }

    async fn collect_expired_credentials(&self) -> Result<(), sqlx::Error> {
        // Each statement commits separately and bounds work to 256 parent rows.
        // SKIP LOCKED avoids waiting on active authentication transactions.
        // Security events are retained for 90 days after detection, independent
        // of whether the corresponding session has already been revoked.
        for query in [
            "DELETE FROM auth_consumed_refresh_token WHERE token_hash IN (SELECT token_hash FROM auth_consumed_refresh_token WHERE expires_at<=now() ORDER BY expires_at LIMIT 256 FOR UPDATE SKIP LOCKED)",
            "DELETE FROM auth_token WHERE token_hash IN (SELECT token_hash FROM auth_token WHERE expires_at<=now() ORDER BY expires_at LIMIT 256 FOR UPDATE SKIP LOCKED)",
            "DELETE FROM auth_session WHERE id IN (SELECT id FROM auth_session WHERE refresh_expires_at<=now() ORDER BY refresh_expires_at LIMIT 256 FOR UPDATE SKIP LOCKED)",
            "DELETE FROM password_reset WHERE user_id IN (SELECT user_id FROM password_reset WHERE expires_at<=now() ORDER BY expires_at LIMIT 256 FOR UPDATE SKIP LOCKED)",
            "DELETE FROM auth_security_event WHERE id IN (SELECT id FROM auth_security_event WHERE detected_at<=now()-INTERVAL '90 days' ORDER BY detected_at LIMIT 256 FOR UPDATE SKIP LOCKED)",
        ] {
            sqlx::query(query).execute(&self.pool).await?;
        }
        Ok(())
    }
}
