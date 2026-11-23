//! Unreferenced asset cleanup. Publication and collection share a database
//! advisory lock, so a successful upload cannot lose its file to a concurrent
//! sweep. Network uploads are staged before taking the publication lock.

use crate::AssetApplication;
use futures_util::TryStreamExt;
use server_asset_store::{AssetError, FsAssetStore};
use sqlx::{Postgres, Transaction};
use std::collections::HashSet;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use sync_common::BookId;
use tokio::time::{interval, MissedTickBehavior};

const ASSET_PUBLICATION_LOCK: i64 = 0x426f6b6865696d;
// Bound synchronous filesystem work while publishers wait for collection.
const THUMBNAIL_COLLECTION_BATCH: usize = 16;

fn storage(error: impl std::fmt::Display) -> AssetError {
    AssetError::Storage(error.to_string())
}

pub(super) async fn lock_publication(tx: &mut Transaction<'_, Postgres>) -> Result<(), AssetError> {
    sqlx::query("SELECT pg_advisory_xact_lock_shared($1)").bind(ASSET_PUBLICATION_LOCK).execute(&mut **tx).await.map_err(storage)?;
    Ok(())
}

/// Unlinking a private copy must exclude publishers that may be opening or
/// hard-linking it. Catalog pin installation itself only needs the shared lock.
async fn lock_collection(tx: &mut Transaction<'_, Postgres>) -> Result<(), AssetError> {
    sqlx::query("SELECT pg_advisory_xact_lock($1)").bind(ASSET_PUBLICATION_LOCK).execute(&mut **tx).await.map_err(storage)?;
    Ok(())
}

/// Repair leftover private aliases outside the publication transaction. The
/// common case takes no exclusive lock: only aliases with no DB binding and
/// an existing private file need a recheck and unlink under collection's lock.
pub(super) async fn repair_unbound_aliases(pool: &sqlx::PgPool, store: &FsAssetStore, books: &[BookId]) -> Result<(), AssetError> {
    if books.is_empty() {
        return Ok(());
    }
    let hashes = books.iter().map(|book| book.as_bytes().to_vec()).collect::<Vec<_>>();
    let bound: HashSet<Vec<u8>> = sqlx::query_scalar("SELECT content_hash FROM blob_object WHERE content_hash=ANY($1)").bind(&hashes).fetch_all(pool).await.map_err(storage)?.into_iter().collect();
    let mut candidates = Vec::new();
    for book in books {
        if !bound.contains(book.as_bytes()) && store.private_book_exists(book).await? {
            candidates.push(*book);
        }
    }
    if candidates.is_empty() {
        return Ok(());
    }
    let mut tx = pool.begin().await.map_err(storage)?;
    lock_collection(&mut tx).await?;
    let hashes = candidates.iter().map(|book| book.as_bytes().to_vec()).collect::<Vec<_>>();
    let bound: HashSet<Vec<u8>> = sqlx::query_scalar("SELECT content_hash FROM blob_object WHERE content_hash=ANY($1)").bind(&hashes).fetch_all(&mut *tx).await.map_err(storage)?.into_iter().collect();
    for book in candidates {
        if !bound.contains(book.as_bytes()) {
            store.discard_unbound_book_alias(&book)?;
        }
    }
    tx.commit().await.map_err(storage)
}

#[derive(Default, Debug, PartialEq, Eq)]
pub struct CleanupResult {
    pub books: usize,
    pub thumbnails: usize,
    pub deduplicated_books: usize,
}

impl AssetApplication {
    /// Deletions and catalog adoption enqueue durable, targeted work. Retry it
    /// every minute; an hourly audit also recovers files orphaned by rollbacks
    /// or process crashes, which cannot leave a committed queue entry.
    pub fn spawn_cleanup_worker(self: &Arc<Self>) {
        let assets = Arc::downgrade(self);
        let signal = self.cleanup.clone();
        tokio::spawn(async move {
            let mut retry = interval(Duration::from_secs(60));
            let mut audit = interval(Duration::from_secs(60 * 60));
            retry.set_missed_tick_behavior(MissedTickBehavior::Skip);
            audit.set_missed_tick_behavior(MissedTickBehavior::Skip);
            loop {
                let full_audit = tokio::select! {
                    _ = signal.notified() => false,
                    _ = retry.tick() => false,
                    _ = audit.tick() => true,
                };
                let Some(assets) = assets.upgrade() else { break };
                let result = if full_audit {
                    assets.collect_unused_assets().await
                } else {
                    assets.collect_queued_assets().await.map(|(result, more)| {
                        if more {
                            signal.notify_one();
                        }
                        result
                    })
                };
                match result {
                    Ok(result) if result.books + result.thumbnails + result.deduplicated_books > 0 => {
                        tracing::info!(books = result.books, thumbnails = result.thumbnails, deduplicated_books = result.deduplicated_books, full_audit, "collected and consolidated assets")
                    }
                    Ok(_) => {}
                    Err(error) => {
                        if full_audit {
                            audit.reset_after(Duration::from_secs(60));
                        }
                        tracing::error!(%error, full_audit, "asset cleanup failed; will retry");
                    }
                }
            }
        });
    }

    async fn collect_queued_assets(&self) -> Result<(CleanupResult, bool), AssetError> {
        let entries: Vec<(i64, String, String, Vec<u8>)> = sqlx::query_as("SELECT id, kind, owner, content_hash FROM asset_cleanup_queue ORDER BY id LIMIT 256").fetch_all(&self.pool).await.map_err(storage)?;
        if entries.is_empty() {
            return Ok((CleanupResult::default(), false));
        }
        let mut books = Vec::new();
        let mut catalog = Vec::new();
        let mut thumbnails = Vec::new();
        for (_, kind, owner, bytes) in &entries {
            let hash = BookId::try_from(bytes.as_slice()).map_err(storage)?;
            match kind.as_str() {
                "book" => books.push(hash),
                "catalog" => catalog.push(hash),
                "thumbnail" => thumbnails.push((FsAssetStore::user_scope(owner), hash)),
                _ => return Err(AssetError::Storage("invalid asset cleanup kind".into())),
            }
        }
        let mut result = self.collect_book_batch(books, false).await?;
        let consolidated = self.collect_book_batch(catalog, true).await?;
        result.books += consolidated.books;
        result.deduplicated_books += consolidated.deduplicated_books;
        result.thumbnails = self.collect_thumbnail_batch(thumbnails).await?;
        // Acknowledgement happens after filesystem and database commits. A
        // crash retries harmlessly, and later enqueues have independent IDs.
        let ids = entries.iter().map(|entry| entry.0).collect::<Vec<_>>();
        sqlx::query("DELETE FROM asset_cleanup_queue WHERE id=ANY($1)").bind(&ids).execute(&self.pool).await.map_err(storage)?;
        Ok((result, entries.len() == 256))
    }

    pub async fn collect_unused_assets(&self) -> Result<CleanupResult, AssetError> {
        self.store.remove_stale_catalog_preparations().await?;
        // Directory enumeration and ownership filtering use bounded batches.
        // No global list of objects, users, or live lifecycle pairs is retained.
        let totals = Mutex::new(CleanupResult::default());
        self.store
            .visit_book_batches(|batch| {
                let totals = &totals;
                async move {
                    let result = self.collect_book_batch(batch, true).await?;
                    let mut total = totals.lock().unwrap_or_else(PoisonError::into_inner);
                    total.books += result.books;
                    total.deduplicated_books += result.deduplicated_books;
                    Ok(())
                }
            })
            .await?;
        // Also collect metadata for objects whose files disappeared earlier.
        let mut after = Vec::<u8>::new();
        loop {
            let hashes: Vec<Vec<u8>> = sqlx::query_scalar(
                "SELECT content_hash FROM blob_object object WHERE content_hash>$1
                 AND NOT EXISTS(SELECT 1 FROM user_blob_charge WHERE content_hash=object.content_hash)
                 AND NOT EXISTS(SELECT 1 FROM library_blob_claim WHERE content_hash=object.content_hash)
                 AND NOT EXISTS(SELECT 1 FROM user_canonical_charge WHERE byte_checksum=object.content_hash)
                 ORDER BY content_hash LIMIT 256",
            )
            .bind(&after)
            .fetch_all(&self.pool)
            .await
            .map_err(storage)?;
            let Some(last) = hashes.last() else { break };
            after = last.clone();
            let batch = hashes.into_iter().map(|hash| BookId::try_from(hash.as_slice()).map_err(storage)).collect::<Result<Vec<_>, _>>()?;
            let result = self.collect_book_batch(batch, true).await?;
            let mut total = totals.lock().unwrap_or_else(PoisonError::into_inner);
            total.books += result.books;
            total.deduplicated_books += result.deduplicated_books;
        }
        self.store
            .visit_thumbnail_batches(|batch| {
                let totals = &totals;
                async move {
                    let removed = self.collect_thumbnail_batch(batch).await?;
                    totals.lock().unwrap_or_else(PoisonError::into_inner).thumbnails += removed;
                    Ok(())
                }
            })
            .await?;
        let result = totals.into_inner().unwrap_or_else(PoisonError::into_inner);
        if result.books > 0 {
            let mut tx = self.pool.begin().await.map_err(storage)?;
            lock_publication(&mut tx).await?;
            sqlx::query("DELETE FROM canonical_blob_object object WHERE NOT EXISTS(SELECT 1 FROM blob_object book WHERE book.byte_checksum=object.byte_checksum) AND NOT EXISTS(SELECT 1 FROM user_canonical_charge charge WHERE charge.byte_checksum=object.byte_checksum)").execute(&mut *tx).await.map_err(storage)?;
            tx.commit().await.map_err(storage)?;
        }
        Ok(result)
    }

    async fn collect_book_batch(&self, books: Vec<BookId>, consolidate: bool) -> Result<CleanupResult, AssetError> {
        if books.is_empty() {
            return Ok(CleanupResult::default());
        }
        let hashes = books.iter().map(|hash| hash.as_bytes().to_vec()).collect::<Vec<_>>();
        let owned: HashSet<Vec<u8>> = sqlx::query_scalar(
            "SELECT content_hash FROM user_blob_charge WHERE content_hash=ANY($1)
             UNION SELECT content_hash FROM library_blob_claim WHERE content_hash=ANY($1)
             UNION SELECT byte_checksum FROM user_canonical_charge WHERE byte_checksum=ANY($1)",
        )
        .bind(&hashes)
        .fetch_all(&self.pool)
        .await
        .map_err(storage)?
        .into_iter()
        .collect();
        let mut result = CleanupResult::default();
        for hash in books.into_iter().collect::<HashSet<_>>() {
            if owned.contains(hash.as_bytes()) {
                if !consolidate {
                    continue;
                }
                if let Some(book) = self.store.prepare_catalog_duplicate(&hash).await? {
                    let mut tx = self.pool.begin().await.map_err(storage)?;
                    lock_collection(&mut tx).await?;
                    book.publish_and_consolidate()?;
                    tx.commit().await.map_err(storage)?;
                    result.deduplicated_books += 1;
                }
                continue;
            }
            let mut tx = self.pool.begin().await.map_err(storage)?;
            lock_collection(&mut tx).await?;
            let owned: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM user_blob_charge WHERE content_hash=$1)
                     OR EXISTS(SELECT 1 FROM library_blob_claim WHERE content_hash=$1)
                     OR EXISTS(SELECT 1 FROM user_canonical_charge WHERE byte_checksum=$1)",
            )
            .bind(hash.as_bytes())
            .fetch_one(&mut *tx)
            .await
            .map_err(storage)?;
            if !owned {
                self.store.remove_book(&hash)?;
                let checksum: Option<Vec<u8>> = sqlx::query_scalar("DELETE FROM blob_object WHERE content_hash=$1 RETURNING byte_checksum").bind(hash.as_bytes()).fetch_optional(&mut *tx).await.map_err(storage)?;
                let mut checksums = vec![hash.as_bytes().to_vec()];
                checksums.extend(checksum);
                sqlx::query(
                    "DELETE FROM canonical_blob_object object WHERE byte_checksum=ANY($1)
                    AND NOT EXISTS(SELECT 1 FROM blob_object book WHERE book.byte_checksum=object.byte_checksum)
                    AND NOT EXISTS(SELECT 1 FROM user_canonical_charge charge WHERE charge.byte_checksum=object.byte_checksum)",
                )
                .bind(&checksums)
                .execute(&mut *tx)
                .await
                .map_err(storage)?;
                result.books += 1;
            }
            tx.commit().await.map_err(storage)?;
        }
        Ok(result)
    }

    async fn live_thumbnail_candidates(&self, connection: &mut sqlx::PgConnection, candidates: &HashSet<(String, BookId)>) -> Result<HashSet<(String, BookId)>, AssetError> {
        let hashes = candidates.iter().map(|(_, hash)| hash.as_bytes().to_vec()).collect::<Vec<_>>();
        let mut rows = sqlx::query_as::<_, (String, Vec<u8>)>(
            "SELECT DISTINCT library.user_id, state.content_hash FROM sync_state state JOIN libraries library ON library.id=state.library_id
             WHERE library.deleted_at IS NULL AND state.kind='book_lifecycle' AND state.version_rank<>2 AND state.content_hash=ANY($1)",
        )
        .bind(&hashes)
        .fetch(connection);
        let mut live = HashSet::new();
        while let Some((user, hash)) = rows.try_next().await.map_err(storage)? {
            let candidate = (FsAssetStore::user_scope(&user), BookId::try_from(hash.as_slice()).map_err(storage)?);
            if candidates.contains(&candidate) {
                live.insert(candidate);
            }
        }
        Ok(live)
    }

    async fn collect_thumbnail_batch(&self, batch: Vec<(String, BookId)>) -> Result<usize, AssetError> {
        if batch.is_empty() {
            return Ok(0);
        }
        let mut candidates = batch.into_iter().collect::<HashSet<_>>();
        let live = {
            let mut connection = self.pool.acquire().await.map_err(storage)?;
            self.live_thumbnail_candidates(&mut connection, &candidates).await?
        };
        candidates.retain(|candidate| !live.contains(candidate));
        let mut removed = 0;
        // Only suspected orphans take the exclusive publication lock. Recheck
        // bounded groups there so concurrent cover publication cannot be unlinked,
        // without a transaction and liveness query for every individual cover.
        let mut candidates = candidates.into_iter();
        loop {
            let batch: HashSet<_> = candidates.by_ref().take(THUMBNAIL_COLLECTION_BATCH).collect();
            if batch.is_empty() { break; }
            let mut tx = self.pool.begin().await.map_err(storage)?;
            lock_collection(&mut tx).await?;
            let live = self.live_thumbnail_candidates(&mut tx, &batch).await?;
            for candidate in batch.difference(&live) {
                removed += usize::from(self.store.remove_scoped_thumbnail(&candidate.0, &candidate.1)?);
            }
            tx.commit().await.map_err(storage)?;
        }
        Ok(removed)
    }
}
