use crate::books::audiobook::{chapters_from_navigation, track_title_hints};
use crate::placements::{FolderPaths, directory_exists_in, parse_dir_id, stored_book_path_column};
use crate::query_methods::query_rows;
use crate::query_methods::{blob_column, optional_blob_column, try_for_each_row};
use book_model::{BookFormat, BookTocEntry};
// ---- io ----
// Transfer snapshot in, direct writes out.
//
// One drain reads everything it needs up front, then works on the network
// without touching the database. Each result is written as it arrives, or
// once per batch where one request answers for many items; every write
// re-checks what it depends on.
use super::BookUploadSnapshot;
use super::TransfersSql;
use crate::shared_sql::SharedSql;
use crate::{Database, DatabaseError, NativeBookSnapshot};
use library_model::{BlobKind, BookUploadIntent, RelativeBookPath};
use std::collections::{HashMap, HashSet};
use sync_common::{BookId, DirId, ROOT_DIR_ID};

/// One live placement's transfer inputs, keyed by its stored path.
#[derive(Clone, Debug)]
pub struct PlacementTransferEntry {
    pub book_id: BookId,
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

struct TransferBookMetadata {
    format: BookFormat,
    track_titles: Vec<(String, String)>,
}

impl TransferSnapshot {
    pub fn asset_for(&self, hash: &BookId) -> Option<&AssetTransferState> {
        self.assets.get(hash)
    }

    pub fn placement_for(&self, hash: &BookId, rel_path: &RelativeBookPath) -> Option<&PlacementTransferEntry> {
        self.placements.iter().find(|entry| entry.book_id == *hash && entry.rel_path == *rel_path)
    }
}

impl Database {
    /// Reads every transfer input for the given hashes: upload intents and
    /// snapshots, book paths, and per-placement download/book state.
    pub fn transfer_snapshot(&self, hashes: &[BookId]) -> Result<TransferSnapshot, DatabaseError> {
        if hashes.is_empty() {
            return Ok(TransferSnapshot::default());
        }
        let snapshot = {
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
                    asset.upload.intent = Some(BookUploadIntent { byte_checksum: row.get(3)?, book_id: hash, id: row.get(1)?, size_bytes: row.get::<_, i64>(2)? as u64 });
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
            let mut folders = FolderPaths::for_directories(&transaction, assets.values().flat_map(|asset| asset.upload.local_versions.iter().map(|(id, _, _)| id.as_str())))?;
            let mut placement_paths: HashMap<BookId, Vec<(DirId, String, RelativeBookPath)>> = HashMap::new();
            for (hash, asset) in &mut assets {
                for (dir_id, file_name, _) in &asset.upload.local_versions {
                    let Some(path) = folders.placement(dir_id, file_name)? else { continue };
                    asset.upload_paths.push(path.clone());
                    placement_paths.entry(*hash).or_default().push((parse_dir_id(dir_id)?, file_name.clone(), path));
                }
            }
            let mut books = HashMap::new();
            let mut occupied: HashMap<DirId, Vec<String>> = HashMap::new();
            if !placement_paths.is_empty() && directory_exists_in(&transaction, &ROOT_DIR_ID.to_string())? {
                let live_hashes = serde_json::to_string(&placement_paths.keys().collect::<Vec<_>>()).map_err(DatabaseError::operation)?;
                try_for_each_row(
                    |read| transaction.transfer_snapshot_books(&live_hashes, read),
                    |row| {
                        let hash = row.get::<_, BookId>(0)?;
                        let format = row.get::<_, Option<i64>>(1)?.and_then(BookFormat::from_code).ok_or_else(|| DatabaseError::message("book has no known format"))?;
                        let mut track_titles = Vec::new();
                        if format == BookFormat::Mp3Folder
                            && let Some(duration) = row.get::<_, Option<i64>>(2)?.filter(|duration| *duration > 0)
                        {
                            let entries: Vec<BookTocEntry> = crate::stored_document::decode(blob_column(row, 3)?)?;
                            let tracks = optional_blob_column(row, 4)?.map(crate::stored_document::decode).transpose()?.unwrap_or_default();
                            let chapters = chapters_from_navigation(&entries, duration as u64).collect::<Vec<_>>();
                            track_titles = track_title_hints(hash, &chapters, tracks);
                        }
                        books.insert(hash, TransferBookMetadata { format, track_titles });
                        Ok(())
                    },
                )?;
                let directories = placement_paths.values().flatten().map(|(directory, _, _)| *directory).collect::<HashSet<_>>();
                let directories = serde_json::to_string(&directories).map_err(DatabaseError::operation)?;
                try_for_each_row(
                    |read| transaction.transfer_snapshot_occupied_names(&directories, read),
                    |row| {
                        let directory = parse_dir_id(&row.get::<_, String>(0)?)?;
                        occupied.entry(directory).or_default().push(row.get(1)?);
                        Ok(())
                    },
                )?;
            }
            let mut snapshot = TransferSnapshot { assets, placements: Vec::new() };
            for hash in hashes {
                for (directory, name, rel_path) in placement_paths.get(hash).into_iter().flatten() {
                    // All inputs belong to this read snapshot. Publication still
                    // rechecks placement identity after filesystem/network work.
                    let book = books.get(hash).map(|metadata| NativeBookSnapshot {
                        book_id: *hash,
                        format: metadata.format,
                        audiobook_track_titles: metadata.track_titles.clone(),
                        directory: *directory,
                        name: name.clone(),
                        relative_path: rel_path.clone(),
                        occupied_names: occupied.get(directory).cloned().unwrap_or_default(),
                    });
                    snapshot.placements.push(PlacementTransferEntry { book_id: *hash, rel_path: rel_path.clone(), book });
                }
            }
            transaction.commit()?;
            snapshot
        };
        Ok(snapshot)
    }

    /// Books the server already holds. Each upload intent still current is
    /// done; one that changed meanwhile is left for the next planning pass.
    pub fn settle_book_manifest<'a>(&self, already_on_server: impl IntoIterator<Item = &'a BookUploadIntent>) -> Result<(), DatabaseError> {
        let intents = already_on_server.into_iter().collect::<Vec<_>>();
        if intents.is_empty() {
            return Ok(());
        }
        let requested = serde_json::to_string(&intents.iter().map(|intent| (intent.book_id, intent.id, intent.size_bytes, intent.byte_checksum)).collect::<Vec<_>>()).map_err(DatabaseError::operation)?;
        self.write(|transaction| {
            let matching = query_rows(|read| transaction.transfer_matching_manifest(&requested, read), |row| row.get::<_, i64>(0))?;
            let mut completed = Vec::with_capacity(matching.len());
            let mut contents = Vec::with_capacity(matching.len());
            for index in matching {
                let intent = intents[usize::try_from(index).map_err(DatabaseError::operation)?];
                if let Some(checksum) = intent.byte_checksum.or_else(|| intent.book_id.content_hash()) {
                    contents.push((intent.book_id, checksum, Some(intent.size_bytes)));
                }
                completed.push((intent.book_id, intent.id));
            }
            crate::content_identity::freeze_contents(transaction, &contents)?;
            crate::sync::apply::registers::record_uploaded_contents(transaction, &contents)?;
            if !completed.is_empty() {
                let completed = serde_json::to_string(&completed).map_err(DatabaseError::operation)?;
                transaction.transfer_record_manifest_books(&completed)?;
                transaction.transfer_complete_manifest(&completed)?;
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
        self.record_cover_presence(hashes, &[])
    }

    /// Present and missing hashes from one validated response, written atomically.
    pub fn record_cover_presence(&self, present: &[BookId], missing: &[BookId]) -> Result<(), DatabaseError> {
        let present = serde_json::to_string(present).map_err(DatabaseError::operation)?;
        let missing = serde_json::to_string(missing).map_err(DatabaseError::operation)?;
        self.write(|transaction| {
            // One statement per set keeps SQL preparation and Rust/SQLite calls
            // independent of response size. Unchanged facts produce no writes.
            transaction.record_cover_presence_present(present.as_str())?;
            transaction.record_cover_presence_missing(missing.as_str())?;
            Ok(())
        })
    }

    /// One cover download batch: covers now ready here, the ones among them
    /// that came from the server, and covers the server does not have.
    pub fn complete_cover_downloads(&self, ready: &[BookId], received: &[BookId], missing: &[BookId]) -> Result<(), DatabaseError> {
        let ready = serde_json::to_string(ready).map_err(DatabaseError::operation)?;
        let received = serde_json::to_string(received).map_err(DatabaseError::operation)?;
        let missing = serde_json::to_string(missing).map_err(DatabaseError::operation)?;
        self.write(|transaction| {
            transaction.record_cover_presence_present(received.as_str())?;
            transaction.complete_cover_downloads_ready(ready.as_str())?;
            transaction.record_cover_presence_missing(missing.as_str())?;
            Ok(())
        })
    }
}

// Async access owns its arguments until the synchronous database call completes.
database_methods! {
    fn settle_book_uploads(intents: Vec<BookUploadIntent>) -> ();
    arguments (Vec<BookUploadIntent>,) = (intents,);
    execute database, arguments => database.settle_book_manifest(arguments.0.iter());

    fn transfer_snapshot(hashes: &[BookId]) -> TransferSnapshot;
    arguments (Vec<BookId>,) = (hashes.to_vec(),);
    execute database, arguments => database.transfer_snapshot(&arguments.0);

    fn complete_download(hash: &BookId) -> ();
    arguments (BookId,) = (hash.clone(),);
    execute database, arguments => database.complete_download(&arguments.0);

    fn cancel_download(hash: &BookId) -> ();
    arguments (BookId,) = (hash.clone(),);
    execute database, arguments => database.cancel_download(&arguments.0);

    #[cfg(not(target_arch = "wasm32"))]
    fn complete_local_book_request(hash: &BookId) -> ();
    arguments (BookId,) = (hash.clone(),);
    execute database, arguments => database.complete_local_book_request(&arguments.0);

    fn record_server_covers(hashes: &[BookId]) -> ();
    arguments (Vec<BookId>,) = (hashes.to_vec(),);
    execute database, arguments => database.record_server_covers(&arguments.0);

    fn record_cover_presence(present: &[BookId], missing: &[BookId]) -> ();
    arguments (Vec<BookId>, Vec<BookId>,) = (present.to_vec(), missing.to_vec(),);
    execute database, arguments => database.record_cover_presence(&arguments.0, &arguments.1);

    fn complete_cover_downloads(ready: &[BookId], received: &[BookId], missing: &[BookId]) -> ();
    arguments (Vec<BookId>, Vec<BookId>, Vec<BookId>,) = (ready.to_vec(), received.to_vec(), missing.to_vec(),);
    execute database, arguments => database.complete_cover_downloads(&arguments.0, &arguments.1, &arguments.2);
}

impl crate::DatabaseClient {
    pub async fn settle_book_manifest<'a>(&self, intents: impl IntoIterator<Item = &'a BookUploadIntent>) -> Result<(), DatabaseError> {
        self.settle_book_uploads(intents.into_iter().cloned().collect()).await
    }
}
