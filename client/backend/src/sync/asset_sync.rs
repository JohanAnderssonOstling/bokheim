use super::error::{SyncError, SyncResult};
use super::transfer::{cleanup_uploaded_pdf_base, open_hashed_file, TransferJob, TransferOrigin, TransferWorker};
use super::DbPool;
use crate::library::database as db;
use crate::{BlobKind, BookPlacement, ContentHash, RelativeBookPath};
use std::collections::HashSet;
use std::path::Path;
use std::sync::{Mutex, RwLock};
use std::time::Duration;
use sync_common::api::assets::{BlobManifestEntry, BlobManifestResponse, MAX_BLOB_MANIFEST_ENTRIES};
use sync_common::LibraryId;

pub(super) struct AssetSyncEngine<'a> {
    db_pool: &'a DbPool,
    http_client: &'a reqwest::Client,
    library_id: LibraryId,
    library_root: &'a Path,
    session: &'a RwLock<Option<crate::auth::Session>>,
    pending_thumbnail_jobs: &'a Mutex<HashSet<ContentHash>>,
    transfer_worker: TransferWorker,
}

pub(super) struct AssetSyncConfig<'a> {
    pub(super) db_pool: &'a DbPool,
    pub(super) http_client: &'a reqwest::Client,
    pub(super) library_id: LibraryId,
    pub(super) library_root: &'a Path,
    pub(super) session: &'a RwLock<Option<crate::auth::Session>>,
    pub(super) pending_thumbnail_jobs: &'a Mutex<HashSet<ContentHash>>,
    pub(super) transfer_worker: TransferWorker,
}

impl<'a> AssetSyncEngine<'a> {
    pub(super) fn new(config: AssetSyncConfig<'a>) -> Self {
        Self {
            db_pool: config.db_pool,
            http_client: config.http_client,
            library_id: config.library_id,
            library_root: config.library_root,
            session: config.session,
            pending_thumbnail_jobs: config.pending_thumbnail_jobs,
            transfer_worker: config.transfer_worker,
        }
    }

    pub(super) async fn synchronize(self, origin: TransferOrigin) -> SyncResult<()> {
        cleanup_stale_transfer_temps(self.library_root, std::time::SystemTime::now() - Duration::from_secs(24 * 60 * 60));
        let conn = self.db_pool.get()?;
        let assets = db::get_book_assets(&conn)?;
        let mut jobs = Vec::new();
        let mut known_hashes = HashSet::new();

        for asset in &assets {
            let local_path = self.resolve_book_path(&asset.rel_path);
            let thumbnail_path = self.thumbnail_path_for_content_hash(&asset.content_hash);
            db::upsert_blob_local_ref(&conn, BlobKind::Book, &asset.content_hash, &local_path, Path::new(&local_path).exists())?;
            db::upsert_blob_local_ref(&conn, BlobKind::Thumbnail, &asset.content_hash, &thumbnail_path, Path::new(&thumbnail_path).exists())?;
            known_hashes.insert(asset.content_hash);
        }

        let upload_blob_hashes = db::get_locally_present_blob_hashes(&conn, BlobKind::Book)?;
        let remote_book_hashes = db::get_remote_asset_hashes(&conn, BlobKind::Book)?;
        let mut upload_blobs = Vec::with_capacity(upload_blob_hashes.len());
        for content_hash in upload_blob_hashes {
            if remote_book_hashes.contains(&content_hash) {
                for local_path in db::get_blob_local_paths(&conn, BlobKind::Book, &content_hash)? {
                    cleanup_uploaded_pdf_base(&conn, self.library_root, &content_hash, &local_path)?;
                }
                continue;
            }
            let mut upload = None;
            for local_path in db::get_blob_local_paths(&conn, BlobKind::Book, &content_hash)? {
                match std::fs::metadata(&local_path) {
                    Ok(metadata) if metadata.is_file() => {
                        let Ok((file, _, actual_hash)) = open_hashed_file(Path::new(&local_path)).await else { continue };
                        drop(file);
                        if actual_hash == content_hash.as_str() {
                            upload = Some((content_hash, local_path, metadata.len()));
                            break;
                        }
                    }
                    _ => db::upsert_blob_local_ref(&conn, BlobKind::Book, &content_hash, &local_path, false)?,
                }
            }
            if let Some(upload) = upload {
                upload_blobs.push(upload);
            } else {
                log::warn!("no pristine local blob matches PDF/book base hash {content_hash}; upload deferred");
            }
        }

        let upload_thumbnail_hashes = db::get_locally_present_blob_hashes(&conn, BlobKind::Thumbnail)?;
        let remote_thumbnail_hashes = db::get_remote_asset_hashes(&conn, BlobKind::Thumbnail)?;
        let pending_thumbnail_jobs = self.pending_thumbnail_jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        let pending_requests = db::get_asset_requests(&conn, BlobKind::Book)?;
        drop(conn);

        let manifest = upload_blobs.iter().map(|(content_hash, _, size_bytes)| BlobManifestEntry { content_hash: *content_hash, size_bytes: *size_bytes }).collect();
        let negotiated = self.negotiate_upload_blobs(manifest).await?;
        let remotely_present: HashSet<_> = negotiated.owned.into_iter().chain(negotiated.claimed).collect();
        let should_upload: HashSet<_> = negotiated.upload.into_iter().collect();
        for rejected in negotiated.rejected {
            log::warn!("Server rejected blob {} during batch admission: {:?}", rejected.content_hash, rejected.reason);
        }
        for (content_hash, local_path, _) in upload_blobs {
            if remotely_present.contains(&content_hash) {
                let conn = self.db_pool.get()?;
                db::record_remote_asset(&conn, BlobKind::Book, &content_hash)?;
                cleanup_uploaded_pdf_base(&conn, self.library_root, &content_hash, &local_path)?;
                continue;
            }
            if should_upload.contains(&content_hash) {
                jobs.push(TransferJob::UploadBlob { content_hash, local_path, placement: None, origin });
            }
        }
        for content_hash in upload_thumbnail_hashes {
            if remote_thumbnail_hashes.contains(&content_hash) {
                continue;
            }
            let conn = self.db_pool.get()?;
            let Some(thumbnail_path) = db::get_blob_local_path(&conn, BlobKind::Thumbnail, &content_hash)? else { continue };
            jobs.push(TransferJob::UploadThumbnail { thumbnail_hash: content_hash, thumbnail_path, book_content_hash: None, origin });
        }

        for (content_hash, request_origin) in pending_requests {
            if request_origin != TransferOrigin::UserInitiated {
                continue;
            }
            let conn = self.db_pool.get()?;
            if db::is_blob_local(&conn, BlobKind::Book, &content_hash)? {
                db::complete_asset_request(&conn, BlobKind::Book, &content_hash)?;
                continue;
            }
            match self.download_job(&conn, &content_hash, request_origin) {
                Ok(Some(job)) => jobs.push(job),
                Ok(None) => {}
                Err(error) => log::warn!("Skipping invalid requested book download {content_hash}: {error}"),
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
            jobs.push(TransferJob::DownloadThumbnail { thumbnail_hash: content_hash, book_content_hash: book_path.content_hash, origin });
        }

        self.transfer_worker.run(jobs, 1).await?;
        Ok(())
    }

    pub(super) fn download_job(&self, conn: &rusqlite::Connection, content_hash: &ContentHash, origin: TransferOrigin) -> Result<Option<TransferJob>, rusqlite::Error> {
        let book_paths = db::get_book_paths_by_content_hash(conn, content_hash)?;
        let Some(chosen) = book_paths.iter().find(|path| !Path::new(&self.resolve_book_path(&path.rel_path)).exists()).or_else(|| book_paths.first()) else {
            return Ok(None);
        };
        let materialized_path = db::materialize_download_path(conn, self.library_root, &BookPlacement::new(chosen.content_hash, chosen.rel_path.clone()))?;
        let local_path = self.resolve_book_path(&materialized_path);
        db::upsert_blob_local_ref(conn, BlobKind::Book, content_hash, &local_path, Path::new(&local_path).exists())?;
        Ok(Some(TransferJob::DownloadBlob { content_hash: *content_hash, placement: BookPlacement::new(chosen.content_hash, materialized_path), origin }))
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

    fn resolve_book_path(&self, rel_path: &RelativeBookPath) -> String {
        rel_path.resolve_under(self.library_root).to_string_lossy().into_owned()
    }

    fn thumbnail_path_for_content_hash(&self, content_hash: &ContentHash) -> String {
        super::transfer::thumbnail_path_from_content_hash(self.library_root, content_hash).to_string_lossy().into_owned()
    }
}

pub(super) fn should_download_thumbnail(book_is_local: bool, thumbnail_is_local: bool, thumbnail_is_known_remote: bool, thumbnail_generation_pending: bool) -> bool {
    !thumbnail_is_local && !thumbnail_generation_pending && (!book_is_local || thumbnail_is_known_remote)
}

fn cleanup_stale_transfer_temps(root: &Path, cutoff: std::time::SystemTime) {
    for entry in walkdir::WalkDir::new(root).follow_links(false).into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        let Some(name) = entry.file_name().to_str() else { continue };
        let Some(raw_uuid) = name.rsplit_once(".tmp-").map(|(_, suffix)| suffix) else { continue };
        if uuid::Uuid::parse_str(raw_uuid).is_err() {
            continue;
        }
        let old_enough = entry.metadata().ok().and_then(|metadata| metadata.modified().ok()).is_some_and(|modified| modified <= cutoff);
        if old_enough {
            if let Err(error) = std::fs::remove_file(entry.path()) {
                log::warn!("Failed to remove stale transfer temp {}: {error}", entry.path().display());
            }
        }
    }
}
