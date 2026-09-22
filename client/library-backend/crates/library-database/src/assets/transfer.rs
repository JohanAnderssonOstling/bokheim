// ---- io ----
// Transfer snapshot in, outcome commit out.
//
// One drain reads everything it needs up front and records nothing until
// the orchestrator (the session, which owns this database) commits the
// returned outcome. Sync works only on these values: no connection,
// transaction, or callback crosses the boundary in either direction.
//
// Commit replays the same guarded methods the inline flow used, so a
// placement that changed mid-drain is still rejected at commit time and
// every replayed write is idempotent under credential-refresh retries.
use super::{BookUploadCompletion, BookUploadSnapshot};
use super::{RejectedAsset, TransfersSql};
use crate::shared_sql::SharedSql;
use crate::{BookPlacement, BookUploadIntent, Database, DatabaseError, NativeBookCompletion, NativeBookSnapshot, RelativeBookPath};
use std::collections::HashMap;
use sync_common::{ContentHash, DirId};

/// One live placement's transfer inputs, keyed by its stored path.
#[derive(Clone, Debug)]
pub struct PlacementTransferEntry {
    pub content_hash: ContentHash,
    pub rel_path: String,
    pub download_directory: Option<DirId>,
    pub placement_current: bool,
    pub book: Option<NativeBookSnapshot>,
}

/// Point-in-time inputs for one transfer drain.
#[derive(Clone, Debug, Default)]
pub struct TransferSnapshot {
    pub assets: HashMap<ContentHash, AssetTransferState>,
    pub placements: Vec<PlacementTransferEntry>,
}

/// Point-in-time state for one content hash during a transfer drain.
#[derive(Clone, Debug)]
pub struct AssetTransferState {
    pub upload: BookUploadSnapshot,
    pub upload_paths: Vec<String>,
    pub thumbnail_allowed: bool,
}

struct DatabaseTransferSnapshot {
    assets: HashMap<ContentHash, AssetTransferState>,
    placement_paths: HashMap<ContentHash, Vec<String>>,
}

impl TransferSnapshot {
    pub fn asset_for(&self, hash: &ContentHash) -> Option<&AssetTransferState> {
        self.assets.get(hash)
    }

    pub fn placement_for(&self, hash: &ContentHash, rel_path: &str) -> Option<&PlacementTransferEntry> {
        self.placements.iter().find(|entry| entry.content_hash == *hash && entry.rel_path == rel_path)
    }
}

/// One durable transfer effect, recorded after the drain.
#[derive(Clone, Debug)]
pub enum TransferWrite {
    SettleManifest { intents: Vec<BookUploadIntent>, present_remotely: Vec<ContentHash>, rejected: Vec<(ContentHash, String)> },
    CompleteUploadedBook { hash: ContentHash, local_versions: Vec<(String, String, String)>, intent: Option<BookUploadIntent> },
    CompleteUploadedBatch { completions: Vec<BookUploadCompletion> },
    CompleteDownload { hash: ContentHash },
    CancelDownload { hash: ContentHash },
    CompleteRetainedDownload { hash: ContentHash, rel_path: String },
    SettleRejectedUpload { hash: ContentHash, intent: Option<BookUploadIntent>, reason: String },
    SettleRejectedThumbnail { hash: ContentHash, reason: String },
    SettleAvailability { available: Vec<ContentHash>, missing: Vec<ContentHash> },
    RecordUploadedThumbnail { hash: ContentHash },
    CompleteDownloadedThumbnail { hash: ContentHash },
    ForgetRemoteThumbnail { hash: ContentHash },
    ConfirmThumbnailAvailable { hash: ContentHash },
    RequestMissingThumbnail { hash: ContentHash },
    CompleteDownloadedBook { completion: NativeBookCompletion, thumbnail_set_exists: bool },
    CompleteLocalBookRequest { hash: ContentHash },
}

/// Accumulated durable effects of one drain, in execution order.
#[derive(Clone, Debug, Default)]
pub struct TransferOutcome {
    pub writes: Vec<TransferWrite>,
}

impl TransferOutcome {
    pub fn record(&mut self, write: TransferWrite) {
        self.writes.push(write);
    }
}

impl Database {
    fn read_transfer_database_snapshot(&self, hashes: &[ContentHash]) -> Result<DatabaseTransferSnapshot, DatabaseError> {
        let transaction = self.connection.unchecked_transaction()?;
        let requested = serde_json::to_string(&hashes.iter().map(ContentHash::as_str).collect::<Vec<_>>()).map_err(DatabaseError::operation)?;
        let mut assets: HashMap<ContentHash, AssetTransferState> =
            hashes.iter().map(|hash| (hash.clone(), AssetTransferState { upload: BookUploadSnapshot { local_versions: Vec::new(), intent: None }, upload_paths: Vec::new(), thumbnail_allowed: false })).collect();

        transaction.transfer_snapshot_versions(&requested, |row| {
            let hash = ContentHash::new(&row.get::<_, String>(0)?);
            if let Some(asset) = assets.get_mut(&hash) {
                asset.upload.local_versions.push((row.get(1)?, row.get(2)?, row.get(3)?));
            }
            Ok(())
        })?;
        transaction.transfer_snapshot_intents(&requested, |row| {
            let hash = ContentHash::new(&row.get::<_, String>(0)?);
            if let Some(asset) = assets.get_mut(&hash) {
                asset.upload.intent = Some(BookUploadIntent { content_hash: hash, id: row.get(1)?, checksum: ContentHash::new(&row.get::<_, String>(2)?), size_bytes: row.get::<_, i64>(3)? as u64 });
            }
            Ok(())
        })?;
        transaction.transfer_snapshot_paths(&requested, |row| {
            let hash = ContentHash::new(&row.get::<_, String>(0)?);
            if let Some(asset) = assets.get_mut(&hash) {
                asset.upload_paths.push(row.get(1)?);
            }
            Ok(())
        })?;
        transaction.transfer_snapshot_thumbnail_allowed(&requested, |row| {
            let hash = ContentHash::new(&row.get::<_, String>(0)?);
            if let Some(asset) = assets.get_mut(&hash) {
                asset.thumbnail_allowed = row.get(1)?;
            }
            Ok(())
        })?;

        let mut placement_paths: HashMap<ContentHash, Vec<String>> = HashMap::new();
        transaction.transfer_snapshot_placements(&requested, |row| {
            let hash = ContentHash::new(&row.get::<_, String>(0)?);
            placement_paths.entry(hash).or_default().push(row.get(1)?);
            Ok(())
        })?;
        transaction.commit()?;
        Ok(DatabaseTransferSnapshot { assets, placement_paths })
    }

    /// Reads every transfer input for the given hashes: upload intents and
    /// snapshots, book paths, rejection reasons, thumbnail allowance,
    /// and per-placement download/book state.
    pub fn transfer_snapshot(&self, hashes: &[ContentHash]) -> Result<TransferSnapshot, DatabaseError> {
        let database = self.read_transfer_database_snapshot(hashes)?;
        let mut snapshot = TransferSnapshot { assets: database.assets, placements: Vec::new() };
        for hash in hashes {
            for rel_path in database.placement_paths.get(hash).into_iter().flatten() {
                let placement = BookPlacement::new(hash.clone(), RelativeBookPath::parse(&rel_path).map_err(|error| DatabaseError::message(format!("stored placement path is not readable: {error:?}")))?);
                snapshot.placements.push(PlacementTransferEntry {
                    content_hash: hash.clone(),
                    rel_path: rel_path.clone(),
                    download_directory: self.native_download_directory(&placement, hash)?,
                    placement_current: self.book_placement_is_current(&placement, hash)?,
                    book: self.native_book_snapshot(&placement, hash)?,
                });
            }
        }
        Ok(snapshot)
    }

    /// Replays one drain's durable effects in order through the same guarded
    /// methods the inline flow used. Every replayed write is idempotent, so
    /// credential-refresh retries that re-executed jobs commit safely.
    ///
    /// Returns whether every completed upload's local version was still
    /// current at commit time. A queued batch retries a stale member on its
    /// next planning pass regardless; a caller committing one explicit job
    /// synchronously (e.g. [`crate::assets::AssetStore`]'s single-book
    /// upload) has no later pass and must surface staleness as an error.
    pub fn commit_transfer_outcome(&self, outcome: &TransferOutcome) -> Result<bool, DatabaseError> {
        let mut all_current = true;
        for write in &outcome.writes {
            match write {
                TransferWrite::SettleManifest { intents, present_remotely, rejected } => {
                    self.settle_book_manifest(intents, present_remotely, rejected)?;
                }
                TransferWrite::CompleteUploadedBook { hash, local_versions, intent } => {
                    if !self.complete_book_upload_if_current(hash, local_versions, intent.as_ref())? {
                        all_current = false;
                    }
                }
                TransferWrite::CompleteUploadedBatch { completions } => {
                    if self.complete_book_upload_batch(completions)?.iter().any(|current| !current) {
                        all_current = false;
                    }
                }
                TransferWrite::CompleteDownload { hash } => {
                    self.connection.shared_transfer_complete_download(hash.as_str())?;
                }
                TransferWrite::CancelDownload { hash } => {
                    self.cancel_book_download(hash)?;
                }
                TransferWrite::CompleteRetainedDownload { hash, rel_path } => {
                    let placement = BookPlacement::new(hash.clone(), RelativeBookPath::parse(rel_path).map_err(|error| DatabaseError::message(format!("stored placement path is not readable: {error:?}")))?);
                    self.complete_retained_download(hash, &placement)?;
                }
                TransferWrite::SettleRejectedUpload { hash, intent, reason } => {
                    self.settle_rejected_asset(hash, RejectedAsset::BookUpload { intent: intent.as_ref() }, reason)?;
                }
                TransferWrite::SettleRejectedThumbnail { hash, reason } => {
                    self.settle_rejected_asset(hash, RejectedAsset::ThumbnailUpload, reason)?;
                }
                TransferWrite::SettleAvailability { available, missing } => {
                    self.settle_thumbnail_availability(available, missing)?;
                }
                TransferWrite::RecordUploadedThumbnail { hash } => {
                    self.record_uploaded_thumbnail(hash)?;
                }
                TransferWrite::CompleteDownloadedThumbnail { hash } => {
                    self.complete_downloaded_thumbnail(hash)?;
                }
                TransferWrite::ForgetRemoteThumbnail { hash } => {
                    self.forget_remote_thumbnail(hash)?;
                }
                TransferWrite::ConfirmThumbnailAvailable { hash } => {
                    self.confirm_thumbnail_available(hash)?;
                }
                TransferWrite::RequestMissingThumbnail { hash } => {
                    self.request_missing_thumbnail(hash)?;
                }
                TransferWrite::CompleteDownloadedBook { completion, thumbnail_set_exists } => {
                    self.complete_downloaded_book(completion.clone(), *thumbnail_set_exists)?;
                }
                TransferWrite::CompleteLocalBookRequest { hash } => {
                    self.complete_local_book_request(hash.clone())?;
                }
            }
        }
        Ok(all_current)
    }
}
