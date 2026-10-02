use crate::placements::book_at_path;
// ---- io ----
// Transfer snapshot in, outcome commit out.
//
// One drain reads everything it needs up front and records nothing until
// the orchestrator (the session, which owns this database) commits the
// returned outcome. Sync works only on these values: no connection,
// transaction, or callback crosses the boundary in either direction.
//
// Commit replays the guarded writes in one transaction, so a changed
// placement is still rejected and a later error rolls back the whole drain.
use super::BookUploadSnapshot;
use super::{RejectedAsset, TransfersSql};
use crate::enrichment::EnrichmentSql;
use crate::shared_sql::SharedSql;
use crate::transactions::WriteOutcome;
use crate::{Database, DatabaseError, NativeBookCompletion, NativeBookSnapshot};
use library_model::{BookPlacement, BookUploadIntent, RelativeBookPath};
use std::collections::HashMap;
use sync_common::{ContentHash, DirId};

/// One live placement's transfer inputs, keyed by its stored path.
#[derive(Clone, Debug)]
pub struct PlacementTransferEntry {
    pub content_hash: ContentHash,
    pub rel_path: RelativeBookPath,
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
    pub upload_paths: Vec<RelativeBookPath>,
    pub thumbnail_allowed: bool,
}

struct DatabaseTransferSnapshot {
    assets: HashMap<ContentHash, AssetTransferState>,
    placement_paths: HashMap<ContentHash, Vec<RelativeBookPath>>,
}

impl TransferSnapshot {
    pub fn asset_for(&self, hash: &ContentHash) -> Option<&AssetTransferState> {
        self.assets.get(hash)
    }

    pub fn placement_for(&self, hash: &ContentHash, rel_path: &RelativeBookPath) -> Option<&PlacementTransferEntry> {
        self.placements.iter().find(|entry| entry.content_hash == *hash && entry.rel_path == *rel_path)
    }
}

/// One durable transfer effect, recorded after the drain.
#[derive(Clone, Debug)]
pub enum TransferWrite {
    SettleManifest { intents: Vec<BookUploadIntent>, present_remotely: Vec<ContentHash>, rejected: Vec<(ContentHash, String)> },
    CompleteUploadedBook { hash: ContentHash, local_versions: Vec<(String, String, String)>, intent: Option<BookUploadIntent> },
    CompleteDownload { hash: ContentHash },
    CancelDownload { hash: ContentHash },
    CompleteRetainedDownload { hash: ContentHash, rel_path: RelativeBookPath },
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
    /// Reads every transfer input for the given hashes: upload intents and
    /// snapshots, book paths, rejection reasons, thumbnail allowance,
    /// and per-placement download/book state.
    pub fn transfer_snapshot(&self, hashes: &[ContentHash]) -> Result<TransferSnapshot, DatabaseError> {
        let database = {
            let transaction = self.connection.unchecked_transaction()?;
            let requested = serde_json::to_string(&hashes.iter().map(ContentHash::as_str).collect::<Vec<_>>()).map_err(DatabaseError::operation)?;
            let mut assets: HashMap<ContentHash, AssetTransferState> =
                hashes.iter().map(|hash| (hash.clone(), AssetTransferState { upload: BookUploadSnapshot { local_versions: Vec::new(), intent: None }, upload_paths: Vec::new(), thumbnail_allowed: false })).collect();

            // Each live placement, with the local fingerprint an upload records.
            let mut placements = Vec::new();
            transaction.transfer_snapshot_versions(&requested, |row| {
                let hash = ContentHash::new(&row.get::<_, String>(0)?);
                let dir_id: String = row.get(1)?;
                let file_name: String = row.get(2)?;
                if let Some(asset) = assets.get_mut(&hash) {
                    asset.upload.local_versions.push((dir_id.clone(), file_name.clone(), row.get(3)?));
                }
                placements.push((hash, dir_id, file_name));
                Ok(())
            })?;
            transaction.transfer_snapshot_intents(&requested, |row| {
                let hash = ContentHash::new(&row.get::<_, String>(0)?);
                if let Some(asset) = assets.get_mut(&hash) {
                    asset.upload.intent = Some(BookUploadIntent { content_hash: hash, id: row.get(1)?, checksum: ContentHash::new(&row.get::<_, String>(2)?), size_bytes: row.get::<_, i64>(3)? as u64 });
                }
                Ok(())
            })?;
            // Materialized paths first, then the live placements read below.
            let mut projected = Vec::new();
            transaction.transfer_snapshot_projection_paths(&requested, |row| {
                projected.push((ContentHash::new(&row.get::<_, String>(0)?), row.get::<_, String>(1)?));
                Ok(())
            })?;
            for (hash, path) in projected {
                if let Some(asset) = assets.get_mut(&hash) {
                    asset.upload_paths.push(crate::placements::stored_book_path(&path)?);
                }
            }
            transaction.transfer_snapshot_thumbnail_allowed(&requested, super::rejection_cutoff()?, |row| {
                let hash = ContentHash::new(&row.get::<_, String>(0)?);
                if let Some(asset) = assets.get_mut(&hash) {
                    asset.thumbnail_allowed = row.get(1)?;
                }
                Ok(())
            })?;

            let mut folders = crate::placements::FolderPaths::new(&transaction);
            let mut placement_paths: HashMap<ContentHash, Vec<RelativeBookPath>> = HashMap::new();
            for (hash, dir_id, file_name) in placements {
                let Some(path) = folders.placement(&dir_id, &file_name)? else { continue };
                if let Some(asset) = assets.get_mut(&hash) {
                    asset.upload_paths.push(path.clone());
                }
                placement_paths.entry(hash).or_default().push(path);
            }
            transaction.commit()?;
            DatabaseTransferSnapshot { assets, placement_paths }
        };
        let mut snapshot = TransferSnapshot { assets: database.assets, placements: Vec::new() };
        for hash in hashes {
            for rel_path in database.placement_paths.get(hash).into_iter().flatten() {
                // The path still holds this book, or the placement is stale.
                let entry = book_at_path(&self.connection, rel_path.as_str())?.filter(|(current, _, _)| current == hash);
                let book = match &entry {
                    Some((_, directory, name)) => Some(NativeBookSnapshot {
                        content_hash: *hash,
                        format: self.book_format(hash)?.ok_or_else(|| DatabaseError::message(format!("book {hash} has no format")))?,
                        directory: *directory,
                        name: name.clone(),
                        relative_path: rel_path.clone(),
                        occupied_names: ({
                            use crate::native::*;
                            let connection: &rusqlite::Connection = &self.connection;

                            let mut names = Vec::new();
                            connection.shared_occupied_file_names(&directory.to_string(), |row| {
                                names.push(row.get(0)?);
                                Ok(())
                            })?;
                            names
                        }),
                    }),
                    None => None,
                };
                snapshot.placements.push(PlacementTransferEntry { content_hash: hash.clone(), rel_path: rel_path.clone(), download_directory: entry.as_ref().map(|(_, directory, _)| *directory), placement_current: entry.is_some(), book });
            }
        }
        Ok(snapshot)
    }

    /// Replays one drain's durable effects in order in a single transaction.
    /// Every replayed write remains guarded for credential-refresh retries.
    ///
    /// Returns whether every completed upload's local version was still
    /// current at commit time. A queued batch retries a stale member on its
    /// next planning pass regardless; a caller committing one explicit job
    /// synchronously (e.g. [`crate::assets::AssetStore`]'s single-book
    /// upload) has no later pass and must surface staleness as an error.
    pub fn commit_transfer_outcome(&self, outcome: &TransferOutcome) -> Result<bool, DatabaseError> {
        if outcome.writes.is_empty() {
            return Ok(true);
        }
        self.with_write_transaction(|transaction| {
            let mut all_current = true;
            for write in &outcome.writes {
                match write {
                    TransferWrite::SettleManifest { intents, present_remotely, rejected } => {
                        ({
                            use crate::assets::uploads::*;
                            let connection: &rusqlite::Connection = transaction;

                            let mut remaining = Vec::new();
                            for intent in intents {
                                if Database::upload_intent_of(connection, &intent.content_hash)?.as_ref() != Some(intent) {
                                    continue;
                                }
                                if present_remotely.contains(&intent.content_hash) {
                                    connection.record_remote_asset(BlobKind::Book.storage(), intent.content_hash.as_str())?;
                                    connection.transfer_complete_upload(intent.id, intent.content_hash.as_str(), intent.checksum.as_str())?;
                                } else if let Some((_, reason)) = rejected.iter().find(|(hash, _)| *hash == intent.content_hash) {
                                    connection.transfer_record_book_rejection(intent.content_hash.as_str(), reason)?;
                                } else {
                                    remaining.push(intent.clone());
                                }
                            }
                            remaining
                        });
                    }
                    TransferWrite::CompleteUploadedBook { hash, local_versions, intent } => {
                        if !Self::complete_book_upload_in_transaction(transaction, hash, local_versions, intent.as_ref())? {
                            all_current = false;
                        }
                    }
                    TransferWrite::CompleteDownload { hash } => {
                        transaction.shared_transfer_complete_download(hash.as_str())?;
                    }
                    TransferWrite::CancelDownload { hash } => {
                        ({
                            use crate::assets::downloads::*;
                            let connection: &rusqlite::Connection = transaction;

                            connection.forget_remote_asset(BlobKind::Book.storage(), hash.as_str())?;
                            connection.shared_transfer_complete_download(hash.as_str())?;
                            ()
                        });
                    }
                    TransferWrite::CompleteRetainedDownload { hash, rel_path } => {
                        let placement = BookPlacement::new(hash.clone(), rel_path.clone());
                        ({
                            use crate::assets::downloads::*;
                            let connection: &rusqlite::Connection = transaction;
                            let placement: &BookPlacement = &placement;
                            'commit_transfer_outcome: {
                                // The placement's path must still resolve to a live placement of `hash`.
                                if !book_at_path(connection, placement.rel_path.as_str())?.is_some_and(|(current, _, _)| current == *hash) {
                                    break 'commit_transfer_outcome false;
                                }
                                connection.shared_set_book_hash_downloaded(hash.as_str(), 1)?;
                                connection.shared_transfer_complete_download(hash.as_str())?;
                                true
                            }
                        });
                    }
                    TransferWrite::SettleRejectedUpload { hash, intent, reason } => {
                        Self::settle_rejected_asset_on(transaction, hash, RejectedAsset::BookUpload { intent: intent.as_ref() }, reason)?;
                    }
                    TransferWrite::SettleRejectedThumbnail { hash, reason } => {
                        Self::settle_rejected_asset_on(transaction, hash, RejectedAsset::ThumbnailUpload, reason)?;
                    }
                    TransferWrite::SettleAvailability { available, missing } => {
                        ({
                            use crate::enrichment::*;
                            let connection: &rusqlite::Connection = transaction;

                            for hash in available {
                                connection.record_remote_asset("thumbnail", hash.as_str())?;
                            }
                            let mut uploads = Vec::new();
                            for hash in missing {
                                use rusqlite::OptionalExtension as _;
                                let mut allowed = false;
                                connection
                                    .thumbnail_upload_allowed(hash.as_str(), crate::assets::rejection_cutoff()?, |row| {
                                        allowed = row.get(0)?;
                                        Ok(())
                                    })
                                    .optional()?;
                                if allowed {
                                    uploads.push(*hash);
                                }
                            }
                            uploads
                        });
                    }
                    TransferWrite::RecordUploadedThumbnail { hash } => {
                        transaction.record_remote_asset("thumbnail", hash.as_str())?;
                    }
                    TransferWrite::CompleteDownloadedThumbnail { hash } => {
                        Self::complete_downloaded_thumbnail_on(transaction, hash)?;
                    }
                    TransferWrite::ForgetRemoteThumbnail { hash } => {
                        transaction.forget_remote_asset("thumbnail", hash.as_str())?;
                    }
                    TransferWrite::ConfirmThumbnailAvailable { hash } => {
                        transaction.thumbnail_set_result(hash.as_str(), "ready")?;
                    }
                    TransferWrite::RequestMissingThumbnail { hash } => {
                        transaction.thumbnail_request_missing(hash.as_str())?;
                    }
                    TransferWrite::CompleteDownloadedBook { completion, thumbnail_set_exists } => {
                        Self::complete_downloaded_book_in_transaction(transaction, completion, *thumbnail_set_exists)?;
                    }
                    TransferWrite::CompleteLocalBookRequest { hash } => {
                        Self::complete_local_book_request_on(transaction, hash)?;
                    }
                }
            }
            Ok(WriteOutcome::Commit(all_current))
        })
    }
}

#[cfg(test)]
mod commit_tests {
    use super::*;

    #[test]
    fn later_invalid_write_rolls_back_earlier_transfer_effects() {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        let hash = ContentHash::new(&"a".repeat(64));
        let mut outcome = TransferOutcome::default();
        db.connection.execute("INSERT INTO book(content_hash,title,added_at,format) VALUES (?1,'Book',1,1)", [hash.as_str()]).unwrap();
        db.connection.execute_batch("CREATE TEMP TRIGGER reject_download_completion BEFORE UPDATE OF book_download ON book_transfer BEGIN SELECT RAISE(ABORT, 'rejected'); END").unwrap();
        outcome.record(TransferWrite::RecordUploadedThumbnail { hash });
        outcome.record(TransferWrite::CompleteDownload { hash });

        assert!(db.commit_transfer_outcome(&outcome).is_err());
        assert!(!{
            use crate::enrichment::EnrichmentSql;
            db.connection.thumbnail_has_remote(hash.as_str(), |row| row.get::<_, bool>(0))
        }
        .unwrap());
        assert!(db.connection.is_autocommit());
    }
}
