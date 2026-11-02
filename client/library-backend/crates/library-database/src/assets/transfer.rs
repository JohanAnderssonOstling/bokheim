use crate::placements::{book_at_path, stored_book_path_column};
// ---- io ----
// Transfer snapshot in, direct writes out.
//
// One drain reads everything it needs up front, then works on the network
// without touching the database. Each result is written as it arrives, or
// once per batch where one request answers for many items; every write
// re-checks what it depends on.
use super::BookUploadSnapshot;
use super::TransfersSql;
use crate::enrichment::EnrichmentSql;
use crate::shared_sql::SharedSql;
use crate::{Database, DatabaseError, NativeBookSnapshot};
use library_model::{BlobKind, BookUploadIntent, RelativeBookPath};
use std::collections::HashMap;
use sync_common::BookId;

/// One live placement's transfer inputs, keyed by its stored path.
#[derive(Clone, Debug)]
pub struct PlacementTransferEntry {
    pub content_hash: BookId,
    pub rel_path: RelativeBookPath,
    pub book: Option<NativeBookSnapshot>,
}

/// Point-in-time inputs for one transfer drain.
#[derive(Clone, Debug, Default)]
pub struct TransferSnapshot {
    pub assets: HashMap<BookId, AssetTransferState>,
    pub placements: Vec<PlacementTransferEntry>,
}

/// Point-in-time state for one content hash during a transfer drain.
#[derive(Clone, Debug)]
pub struct AssetTransferState {
    pub upload: BookUploadSnapshot,
    pub upload_paths: Vec<RelativeBookPath>,
}

struct DatabaseTransferSnapshot {
    assets: HashMap<BookId, AssetTransferState>,
    placement_paths: HashMap<BookId, Vec<RelativeBookPath>>,
}

impl TransferSnapshot {
    pub fn asset_for(&self, hash: &BookId) -> Option<&AssetTransferState> {
        self.assets.get(hash)
    }

    pub fn placement_for(&self, hash: &BookId, rel_path: &RelativeBookPath) -> Option<&PlacementTransferEntry> {
        self.placements.iter().find(|entry| entry.content_hash == *hash && entry.rel_path == *rel_path)
    }
}

impl Database {
    /// Reads every transfer input for the given hashes: upload intents and
    /// snapshots, book paths, and per-placement download/book state.
    pub fn transfer_snapshot(&self, hashes: &[BookId]) -> Result<TransferSnapshot, DatabaseError> {
        let database = {
            let transaction = self.connection.unchecked_transaction()?;
            let requested = serde_json::to_string(hashes).map_err(DatabaseError::operation)?;
            let mut assets: HashMap<BookId, AssetTransferState> = hashes.iter().map(|hash| (*hash, AssetTransferState { upload: BookUploadSnapshot { local_versions: Vec::new(), intent: None }, upload_paths: Vec::new() })).collect();

            // Each live placement, with the local fingerprint an upload records.
            transaction.transfer_snapshot_versions(&requested, |row| {
                let hash = row.get::<_, BookId>(0)?;
                if let Some(asset) = assets.get_mut(&hash) {
                    asset.upload.local_versions.push((row.get(1)?, row.get(2)?, row.get(3)?));
                }
                Ok(())
            })?;
            transaction.transfer_snapshot_intents(&requested, |row| {
                let hash = row.get::<_, BookId>(0)?;
                if let Some(asset) = assets.get_mut(&hash) {
                    asset.upload.intent = Some(BookUploadIntent { byte_checksum: row.get(3)?, content_hash: hash, id: row.get(1)?, size_bytes: row.get::<_, i64>(2)? as u64 });
                }
                Ok(())
            })?;
            // Materialized paths first, then the live placements read below.
            transaction.transfer_snapshot_projection_paths(&requested, |row| {
                let hash = row.get::<_, BookId>(0)?;
                if let Some(asset) = assets.get_mut(&hash) {
                    asset.upload_paths.push(stored_book_path_column(row, 1)?);
                }
                Ok(())
            })?;
            let mut folders = crate::placements::FolderPaths::new(&transaction);
            let mut placement_paths: HashMap<BookId, Vec<RelativeBookPath>> = HashMap::new();
            for (hash, asset) in &mut assets {
                for (dir_id, file_name, _) in &asset.upload.local_versions {
                    let Some(path) = folders.placement(dir_id, file_name)? else { continue };
                    asset.upload_paths.push(path.clone());
                    placement_paths.entry(*hash).or_default().push(path);
                }
            }
            transaction.commit()?;
            DatabaseTransferSnapshot { assets, placement_paths }
        };
        let mut snapshot = TransferSnapshot { assets: database.assets, placements: Vec::new() };
        for hash in hashes {
            for rel_path in database.placement_paths.get(hash).into_iter().flatten() {
                // The path still holds this book, or the placement is stale.
                let book = match book_at_path(&self.connection, rel_path.as_str())?.filter(|(current, _, _)| current == hash) {
                    Some((_, directory, name)) => {
                        let format = self.book_format(hash)?.ok_or_else(|| DatabaseError::message("book has no format"))?;
                        Some(NativeBookSnapshot {
                            content_hash: *hash,
                            format,
                            audiobook_track_titles: self.audiobook_download_track_titles(hash, format)?,
                            directory,
                            name,
                            relative_path: rel_path.clone(),
                            occupied_names: self.shared_occupied_file_names(&directory)?,
                        })
                    }
                    None => None,
                };
                snapshot.placements.push(PlacementTransferEntry { content_hash: *hash, rel_path: rel_path.clone(), book });
            }
        }
        Ok(snapshot)
    }

    /// Books the server already holds. Each upload intent still current is
    /// done; one that changed meanwhile is left for the next planning pass.
    pub fn settle_book_manifest<'a>(&self, already_on_server: impl IntoIterator<Item = &'a BookUploadIntent>) -> Result<(), DatabaseError> {
        self.write(|transaction| {
            for intent in already_on_server {
                if Self::upload_intent_of(transaction, &intent.content_hash)?.as_ref() != Some(intent) {
                    continue;
                }
                if let Some(checksum) = intent.byte_checksum.or_else(|| intent.content_hash.content_hash()) {
                    crate::content_identity::freeze_content(transaction, intent.content_hash, checksum, Some(intent.size_bytes))?;
                    crate::sync::apply::registers::record_local(transaction, library_replica::LibraryStateEntry::BookContent { content_hash: intent.content_hash, checksum, size_bytes: Some(intent.size_bytes), pdf: None })?;
                }
                transaction.record_remote_asset(BlobKind::Book.storage(), &intent.content_hash)?;
                transaction.transfer_complete_upload(intent.id, &intent.content_hash)?;
            }
            Ok(())
        })
    }

    /// Ends a download request without its bytes.
    pub fn complete_download(&self, hash: &BookId) -> Result<(), DatabaseError> {
        self.connection.shared_transfer_complete_download(hash)?;
        Ok(())
    }

    /// The server has no copy to download: forget it and end the request.
    pub fn cancel_download(&self, hash: &BookId) -> Result<(), DatabaseError> {
        self.write(|transaction| {
            transaction.forget_remote_asset(BlobKind::Book.storage(), hash)?;
            transaction.shared_transfer_complete_download(hash)?;
            Ok(())
        })
    }

    /// Downloaded bytes are stored here.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn complete_local_book_request(&self, hash: &BookId) -> Result<(), DatabaseError> {
        self.write(|transaction| Self::complete_local_book_request_on(transaction, hash))
    }

    /// Covers the server holds.
    pub fn record_server_covers(&self, hashes: &[BookId]) -> Result<(), DatabaseError> {
        self.write(|transaction| {
            for hash in hashes {
                transaction.record_remote_asset(BlobKind::Thumbnail.storage(), hash)?;
            }
            Ok(())
        })
    }

    /// Present and missing hashes from one validated response, written atomically.
    pub fn record_cover_presence(&self, present: &[BookId], missing: &[BookId]) -> Result<(), DatabaseError> {
        self.write(|transaction| {
            for hash in present {
                transaction.record_remote_asset(BlobKind::Thumbnail.storage(), hash)?;
            }
            for hash in missing {
                transaction.forget_remote_asset(BlobKind::Thumbnail.storage(), hash)?;
            }
            Ok(())
        })
    }

    /// One cover download batch: covers now ready here, the ones among them
    /// that came from the server, and covers the server does not have.
    pub fn complete_cover_downloads(&self, ready: &[BookId], received: &[BookId], missing: &[BookId]) -> Result<(), DatabaseError> {
        self.write(|transaction| {
            for hash in received {
                transaction.record_remote_asset(BlobKind::Thumbnail.storage(), hash)?;
            }
            for hash in ready {
                transaction.thumbnail_set_result(hash, "ready")?;
            }
            for hash in missing {
                transaction.forget_remote_asset(BlobKind::Thumbnail.storage(), hash)?;
            }
            Ok(())
        })
    }
}
