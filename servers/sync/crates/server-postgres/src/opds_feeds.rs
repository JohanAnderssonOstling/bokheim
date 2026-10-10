//! The account's OPDS feeds: a name and an address each, listed by every
//! device. The server never fetches them, and never sees their credentials.

use crate::sync::{internal, PostgresSyncRepository, SyncError};
use sync_common::api::opds_feeds::OpdsFeed;

impl PostgresSyncRepository {
    pub async fn list_opds_feeds(&self, user_id: &str) -> Result<Vec<OpdsFeed>, SyncError> {
        let rows = sqlx::query_as::<_, (String, String, String)>("SELECT id, name, url FROM opds_feeds WHERE user_id = $1 AND deleted_at IS NULL ORDER BY lower(name), id").bind(user_id).fetch_all(&self.pool).await.map_err(internal)?;
        rows.into_iter().map(|(id, name, url)| Ok(OpdsFeed { id: id.parse().map_err(internal)?, name, url })).collect()
    }

    /// Adds a feed, or renames or moves one already added. A removed feed
    /// stays removed.
    pub async fn put_opds_feed(&self, user_id: &str, feed: &OpdsFeed) -> Result<OpdsFeed, SyncError> {
        let name = feed.name.trim();
        let url = feed.url.trim();
        let valid_url = url.len() <= 2048 && (url.starts_with("https://") || url.starts_with("http://")) && !url.chars().any(char::is_control);
        if name.is_empty() || name.len() > 255 || name.chars().any(char::is_control) || !valid_url {
            return Err(SyncError::InvalidOpdsFeed);
        }
        let stored = sqlx::query_scalar::<_, String>(
            "INSERT INTO opds_feeds(id, user_id, name, url) VALUES ($1, $2, $3, $4)
             ON CONFLICT (id) DO UPDATE SET name = excluded.name, url = excluded.url
             WHERE opds_feeds.user_id = excluded.user_id AND opds_feeds.deleted_at IS NULL
             RETURNING id",
        )
        .bind(feed.id.to_string())
        .bind(user_id)
        .bind(name)
        .bind(url)
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;
        stored.map(|_| OpdsFeed { id: feed.id, name: name.to_owned(), url: url.to_owned() }).ok_or(SyncError::OpdsFeedConflict)
    }

    /// Removes a feed, keeping its id so it cannot be added back. Removing
    /// one already removed, or never added, is not an error.
    pub async fn delete_opds_feed(&self, user_id: &str, id: &sync_common::api::opds_feeds::OpdsFeedId) -> Result<(), SyncError> {
        sqlx::query("UPDATE opds_feeds SET deleted_at = now() WHERE id = $1 AND user_id = $2 AND deleted_at IS NULL").bind(id.to_string()).bind(user_id).execute(&self.pool).await.map_err(internal)?;
        Ok(())
    }
}
