use super::error::{SyncError, SyncResult};
use super::transfer::{TransferJob, TransferOrigin, TransferWorker};
use super::DbPool;
use crate::library::database as db;
use crate::{BlobKind, BookPlacement, ContentHash};
use std::collections::HashSet;
use std::sync::{Mutex, RwLock};
use sync_common::api::assets::{BlobManifestEntry, BlobManifestResponse, MAX_BLOB_MANIFEST_ENTRIES};
use sync_common::LibraryId;

pub(super) struct AssetSyncEngine<'a> {
    db_pool: &'a DbPool,
    http_client: &'a reqwest::Client,
    library_id: LibraryId,
    session: &'a RwLock<Option<sync_client::Session>>,
    pending_thumbnail_jobs: &'a Mutex<HashSet<ContentHash>>,
    transfer_worker: TransferWorker,
}

pub(super) struct AssetSyncConfig<'a> {
    pub(super) db_pool: &'a DbPool,
    pub(super) http_client: &'a reqwest::Client,
    pub(super) library_id: LibraryId,
    pub(super) session: &'a RwLock<Option<sync_client::Session>>,
    pub(super) pending_thumbnail_jobs: &'a Mutex<HashSet<ContentHash>>,
    pub(super) transfer_worker: TransferWorker,
}

impl<'a> AssetSyncEngine<'a> {
    pub(super) fn new(config: AssetSyncConfig<'a>) -> Self {
        Self { db_pool: config.db_pool, http_client: config.http_client, library_id: config.library_id, session: config.session, pending_thumbnail_jobs: config.pending_thumbnail_jobs, transfer_worker: config.transfer_worker }
    }

    pub(super) async fn synchronize(self, origin: TransferOrigin) -> SyncResult<()> {
        let conn = self.db_pool.get()?;
        let assets = db::get_book_assets(&conn)?;
        let mut jobs = Vec::new();
        let mut known_hashes = HashSet::new();

        for asset in &assets {
            known_hashes.insert(asset.content_hash);
        }

        let upload_blob_hashes = db::get_locally_present_blob_hashes(&conn, BlobKind::Book)?;
        let remote_book_hashes = db::get_remote_asset_hashes(&conn, BlobKind::Book)?;
        let mut upload_blobs = Vec::with_capacity(upload_blob_hashes.len());
        for content_hash in upload_blob_hashes {
            if remote_book_hashes.contains(&content_hash) {
                continue;
            }
            if let Some(bytes) = db::get_local_blob(&conn, BlobKind::Book, &content_hash)? {
                if blake3::hash(&bytes).to_hex().as_str() == content_hash.as_str() {
                    upload_blobs.push((content_hash, bytes.len() as u64));
                    continue;
                }
                db::remove_local_blob(&conn, BlobKind::Book, &content_hash)?;
                log::warn!("discarded local publication bytes that did not match {content_hash}");
            }
            log::warn!("no pristine local blob matches PDF/book base hash {content_hash}; upload deferred");
        }

        let upload_thumbnail_hashes = db::get_locally_present_blob_hashes(&conn, BlobKind::Thumbnail)?;
        let remote_thumbnail_hashes = db::get_remote_asset_hashes(&conn, BlobKind::Thumbnail)?;
        let pending_thumbnail_jobs = self.pending_thumbnail_jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        let pending_requests = db::get_asset_requests(&conn, BlobKind::Book)?;
        drop(conn);

        let manifest = upload_blobs.iter().map(|(content_hash, size_bytes)| BlobManifestEntry { content_hash: *content_hash, size_bytes: *size_bytes }).collect();
        let negotiated = self.negotiate_upload_blobs(manifest).await?;
        let remotely_present: HashSet<_> = negotiated.owned.into_iter().chain(negotiated.claimed).collect();
        let should_upload: HashSet<_> = negotiated.upload.into_iter().collect();
        for rejected in negotiated.rejected {
            log::warn!("Server rejected blob {} during batch admission: {:?}", rejected.content_hash, rejected.reason);
        }
        for (content_hash, _) in upload_blobs {
            if remotely_present.contains(&content_hash) {
                let conn = self.db_pool.get()?;
                db::record_remote_asset(&conn, BlobKind::Book, &content_hash)?;
                continue;
            }
            if should_upload.contains(&content_hash) {
                jobs.push(TransferJob::upload_blob(content_hash, None, origin));
            }
        }
        for content_hash in upload_thumbnail_hashes {
            if remote_thumbnail_hashes.contains(&content_hash) {
                continue;
            }
            let conn = self.db_pool.get()?;
            if !db::local_blob_exists(&conn, BlobKind::Thumbnail, &content_hash)? {
                continue;
            }
            jobs.push(TransferJob::upload_thumbnail(content_hash, None, origin));
        }

        for (content_hash, request_origin) in pending_requests {
            if request_origin != TransferOrigin::UserInitiated {
                continue;
            }
            let conn = self.db_pool.get()?;
            if db::is_blob_local(&conn, BlobKind::Book, &content_hash)? {
                if let Some(path) = db::get_book_paths_by_content_hash(&conn, &content_hash)?.first() {
                    db::set_book_downloaded(&conn, &path.rel_path, content_hash, true)?;
                }
                db::complete_asset_request(&conn, BlobKind::Book, &content_hash)?;
                continue;
            }
            match self.download_job(&conn, &content_hash, request_origin) {
                Ok(Some(job)) => jobs.push(job),
                Ok(None) => db::complete_asset_request(&conn, BlobKind::Book, &content_hash)?,
                Err(error) => {
                    db::complete_asset_request(&conn, BlobKind::Book, &content_hash)?;
                    log::warn!("Cancelled invalid requested book download {content_hash}: {error}");
                }
            }
        }

        for content_hash in known_hashes {
            let conn = self.db_pool.get()?;
            let thumbnail_is_local = db::is_blob_local(&conn, BlobKind::Thumbnail, &content_hash)?;
            let book_is_local = db::is_blob_local(&conn, BlobKind::Book, &content_hash)?;
            let thumbnail_is_known_remote = remote_thumbnail_hashes.contains(&content_hash);
            let thumbnail_generation_pending = pending_thumbnail_jobs.contains(&content_hash);
            if !should_download_thumbnail(book_is_local, thumbnail_is_local, thumbnail_is_known_remote, thumbnail_generation_pending) {
                continue;
            }
            let Some(book_path) = db::get_book_paths_by_content_hash(&conn, &content_hash)?.into_iter().next() else { continue };
            jobs.push(TransferJob::download_thumbnail(content_hash, book_path.content_hash, origin));
        }

        self.transfer_worker.run(jobs, 1).await?;
        Ok(())
    }

    pub(super) fn download_job(&self, conn: &rusqlite::Connection, content_hash: &ContentHash, origin: TransferOrigin) -> Result<Option<TransferJob>, rusqlite::Error> {
        let book_paths = db::get_book_paths_by_content_hash(conn, content_hash)?;
        let Some(chosen) = book_paths.first() else {
            return Ok(None);
        };
        Ok(Some(TransferJob::download_blob(*content_hash, BookPlacement::new(chosen.content_hash, chosen.rel_path.clone()), origin)))
    }

    async fn negotiate_upload_blobs(&self, blobs: Vec<BlobManifestEntry>) -> SyncResult<BlobManifestResponse> {
        if blobs.is_empty() {
            return Ok(BlobManifestResponse::default());
        }
        let session = self.session.read().ok().and_then(|session| session.clone()).ok_or(SyncError::AuthenticationRequired)?;
        let mut combined = BlobManifestResponse::default();
        for chunk in blobs.chunks(MAX_BLOB_MANIFEST_ENTRIES) {
            let mut response = sync_client::negotiate_blobs(self.http_client, &session, &self.library_id, chunk.to_vec()).await?;
            combined.owned.append(&mut response.owned);
            combined.claimed.append(&mut response.claimed);
            combined.upload.append(&mut response.upload);
            combined.rejected.append(&mut response.rejected);
        }
        Ok(combined)
    }
}

pub(super) fn should_download_thumbnail(book_is_local: bool, thumbnail_is_local: bool, thumbnail_is_known_remote: bool, thumbnail_generation_pending: bool) -> bool {
    !thumbnail_is_local && !thumbnail_generation_pending && (!book_is_local || thumbnail_is_known_remote)
}
