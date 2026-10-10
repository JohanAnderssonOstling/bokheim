use crate::placements::book_at_path;
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

impl Database {
    /// Reads every transfer input for the given hashes: upload intents and
    /// snapshots, book paths, and per-placement download/book state.
    pub fn transfer_snapshot(&self, hashes: &[ContentHash]) -> Result<TransferSnapshot, DatabaseError> {
        let database = {
            let transaction = self.connection.unchecked_transaction()?;
            let requested = serde_json::to_string(&hashes.iter().map(ContentHash::to_hex).collect::<Vec<_>>()).map_err(DatabaseError::operation)?;
            let mut assets: HashMap<ContentHash, AssetTransferState> = hashes.iter().map(|hash| (*hash, AssetTransferState { upload: BookUploadSnapshot { local_versions: Vec::new(), intent: None }, upload_paths: Vec::new() })).collect();

            // Each live placement, with the local fingerprint an upload records.
            let mut placements = Vec::new();
            transaction.transfer_snapshot_versions(&requested, |row| {
                let hash = row.get::<_, ContentHash>(0)?;
                let dir_id: String = row.get(1)?;
                let file_name: String = row.get(2)?;
                if let Some(asset) = assets.get_mut(&hash) {
                    asset.upload.local_versions.push((dir_id.clone(), file_name.clone(), row.get(3)?));
                }
                placements.push((hash, dir_id, file_name));
                Ok(())
            })?;
            transaction.transfer_snapshot_intents(&requested, |row| {
                let hash = row.get::<_, ContentHash>(0)?;
                if let Some(asset) = assets.get_mut(&hash) {
                    asset.upload.intent = Some(BookUploadIntent { content_hash: hash, id: row.get(1)?, size_bytes: row.get::<_, i64>(2)? as u64 });
                }
                Ok(())
            })?;
            // Materialized paths first, then the live placements read below.
            let mut projected = Vec::new();
            transaction.transfer_snapshot_projection_paths(&requested, |row| {
                projected.push((row.get::<_, ContentHash>(0)?, row.get::<_, String>(1)?));
                Ok(())
            })?;
            for (hash, path) in projected {
                if let Some(asset) = assets.get_mut(&hash) {
                    asset.upload_paths.push(crate::placements::stored_book_path(&path)?);
                }
            }
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
                        format: self.book_format(hash)?.ok_or_else(|| DatabaseError::message("book has no format"))?,
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
                snapshot.placements.push(PlacementTransferEntry { content_hash: *hash, rel_path: rel_path.clone(), download_directory: entry.as_ref().map(|(_, directory, _)| *directory), placement_current: entry.is_some(), book });
            }
        }
        Ok(snapshot)
    }

    /// Books the server already holds. Each upload intent still current is
    /// done; one that changed meanwhile is left for the next planning pass.
    pub fn settle_book_manifest(&self, already_on_server: &[BookUploadIntent]) -> Result<(), DatabaseError> {
        self.write(|transaction| {
            for intent in already_on_server {
                if Self::upload_intent_of(transaction, &intent.content_hash)?.as_ref() != Some(intent) {
                    continue;
                }
                transaction.record_remote_asset(BlobKind::Book.storage(), &intent.content_hash)?;
                transaction.transfer_complete_upload(intent.id, &intent.content_hash)?;
            }
            Ok(())
        })
    }

    /// Records an uploaded book. Returns false when its local files changed
    /// since `local_versions` was read; the upload then stays queued.
    pub fn complete_book_upload(&self, hash: &ContentHash, local_versions: &[(String, String, String)], intent: Option<&BookUploadIntent>) -> Result<bool, DatabaseError> {
        self.write(|transaction| Self::complete_book_upload_in_transaction(transaction, hash, local_versions, intent))
    }

    /// Ends a download request without its bytes.
    pub fn complete_download(&self, hash: &ContentHash) -> Result<(), DatabaseError> {
        self.connection.shared_transfer_complete_download(hash)?;
        Ok(())
    }

    /// The server has no copy to download: forget it and end the request.
    pub fn cancel_download(&self, hash: &ContentHash) -> Result<(), DatabaseError> {
        self.write(|transaction| {
            transaction.forget_remote_asset(BlobKind::Book.storage(), hash)?;
            transaction.shared_transfer_complete_download(hash)?;
            Ok(())
        })
    }

    /// Downloaded bytes are stored here.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn complete_local_book_request(&self, hash: &ContentHash) -> Result<(), DatabaseError> {
        self.write(|transaction| Self::complete_local_book_request_on(transaction, hash))
    }

    /// Covers the server holds.
    pub fn record_server_covers(&self, hashes: &[ContentHash]) -> Result<(), DatabaseError> {
        self.write(|transaction| {
            for hash in hashes {
                transaction.record_remote_asset(BlobKind::Thumbnail.storage(), hash)?;
            }
            Ok(())
        })
    }

    /// One validated cover presence response, written atomically.
    pub fn record_cover_presence(&self, requested: &[ContentHash], present: &[ContentHash]) -> Result<(), DatabaseError> {
        self.write(|transaction| {
            for hash in requested {
                if present.contains(hash) {
                    transaction.record_remote_asset(BlobKind::Thumbnail.storage(), hash)?;
                } else {
                    transaction.forget_remote_asset(BlobKind::Thumbnail.storage(), hash)?;
                }
            }
            Ok(())
        })
    }

    /// One cover download batch: covers now ready here, the ones among them
    /// that came from the server, and covers the server does not have.
    pub fn complete_cover_downloads(&self, ready: &[ContentHash], received: &[ContentHash], missing: &[ContentHash]) -> Result<(), DatabaseError> {
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
