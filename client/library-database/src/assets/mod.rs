//! Transfer workflow: planning, snapshot drivers, operation status.


// ---- planning ----
// Typed asset-planning reads and conditional settlements.
//
// Transfer orchestration receives durable snapshots from here; it never
// checks out SQLite or chooses transaction boundaries itself.

use crate::{Database, DatabaseError};
use crate::{AssetCandidate, };
use library_model::DownloadState;
use library_replica::{BlobKind, TransferOrigin};
use rusqlite::OptionalExtension as _;
use sync_common::ContentHash;

fn unix_secs() -> Result<i64, DatabaseError> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .map_err(DatabaseError::operation)
}

#[derive(Debug)]
pub struct AssetPlanningPage {
    pub rows: Vec<AssetCandidate>,
    pub through: i64,
}

#[derive(Clone, Debug)]
pub struct BookUploadSnapshot {
    pub local_versions: Vec<(String, String, String)>,
    pub intent: Option<BookUploadIntent>,
}

#[derive(Clone, Debug)]
pub struct BookUploadCompletion {
    pub content_hash: ContentHash,
    pub local_versions: Vec<(String, String, String)>,
    pub intent: BookUploadIntent,
}

/// A rejected asset transfer whose persistence must be settled atomically.
pub enum RejectedAsset<'a> {
    BookDownload,
    BookUpload { intent: Option<&'a BookUploadIntent> },
    ThumbnailUpload,
}

/// One located asset_work row awaiting planning, plus the per-hash
/// allowance/state flags assembled for it. Shared by the paged drain and
/// targeted payload reads.
struct CandidateFound {
    work_id: i64,
    hash: String,
    row_id: Option<i64>,
    deleted_at: Option<i64>,
}
struct CandidateFlags {
    thumbnail_upload_allowed: bool,
    thumbnail_remote: bool,
    thumbnail_pending: bool,
    requested: bool,
    rejected: bool,
    book_work_pending: bool,
    file_work_pending: bool,
}

impl Database {
    /// Stored placement paths for one book, for transfer snapshots.
    pub fn live_placement_paths(&self, hash: &ContentHash) -> Result<Vec<String>, DatabaseError> {
        let mut paths = Vec::new();
        self.connection.transfer_live_placements(hash.as_str(), |row| {
            paths.push(row.get::<_, String>(2)?);
            Ok(())
        })?;
        Ok(paths)
    }

    /// Clears one outstanding book download request.
    pub fn complete_download_request(&self, hash: &ContentHash) -> Result<(), DatabaseError> {
        self.connection.transfer_complete_download(hash.as_str())?;
        Ok(())
    }

    /// Records why a transfer job was rejected, or clears the outstanding
    /// download request. A stale upload intent (superseded since the job was
    /// planned) is left alone rather than recording a rejection for it.
    pub fn settle_rejected_asset(&self, hash: &ContentHash, asset: RejectedAsset<'_>, reason: &str) -> Result<(), DatabaseError> {
        match asset {
            RejectedAsset::BookDownload => {
                self.connection.transfer_complete_download(hash.as_str())?;
            }
            RejectedAsset::BookUpload { intent } => {
                if intent.is_none() || self.upload_intent(hash)?.as_ref() == intent {
                    self.connection.transfer_record_book_rejection(hash.as_str(), reason)?;
                }
            }
            RejectedAsset::ThumbnailUpload => {
                self.connection.transfer_record_thumbnail_rejection(hash.as_str(), reason)?;
            }
        }
        Ok(())
    }

    pub fn ready_upload_hashes(&self, hashes: &[ContentHash]) -> Result<std::collections::HashSet<ContentHash>, DatabaseError> {
        let hashes = serde_json::to_string(&hashes.iter().map(ContentHash::as_str).collect::<Vec<_>>()).map_err(DatabaseError::operation)?;
        let mut ready = std::collections::HashSet::new();
        self.connection.transfer_ready_uploads(&hashes, |row| { ready.insert(ContentHash::new(&row.get::<_, String>(0)?)); Ok(()) })?;
        Ok(ready)
    }

    pub fn book_upload_intents(&self, hashes: &[ContentHash]) -> Result<Vec<BookUploadIntent>, DatabaseError> {
        hashes.iter().filter_map(|hash| self.upload_intent(hash).transpose()).collect()
    }

    /// Upload planning reads the intent queue directly instead of deriving
    /// upload-ness from the asset_work drain. Bounded like a plan page;
    /// upload completion wakes the next pass for the remainder.
    pub fn pending_book_uploads(&self, limit: i64) -> Result<Vec<BookUploadIntent>, DatabaseError> {
        let mut intents = Vec::new();
        self.transfer_pending_book_uploads(limit, |row| {
            intents.push(BookUploadIntent {
                id: row.get(0)?,
                content_hash: ContentHash::new(&row.get::<_, String>(1)?),
                checksum: ContentHash::new(&row.get::<_, String>(2)?),
                size_bytes: row.get::<_, i64>(3)? as u64,
            });
            Ok(())
        })?;
        Ok(intents)
    }

    pub fn book_upload_snapshot(&self, hash: &ContentHash) -> Result<BookUploadSnapshot, DatabaseError> {
        Ok(BookUploadSnapshot {
            local_versions: self.local_versions(hash)?, intent: self.upload_intent(hash)?,
        })
    }

    /// Completes a verified upload only if the local revision still matches
    /// its byte snapshot. Returns false if a newer revision won.
    pub fn complete_book_upload_if_current(
        &self,
        hash: &ContentHash,
        expected_versions: &[(String, String, String)],
        intent: Option<&BookUploadIntent>,
    ) -> Result<bool, DatabaseError> {
        let transaction = self.connection.unchecked_transaction()?;
        if Self::local_versions_of(&transaction, hash)? != expected_versions {
            transaction.transfer_forget_remote_book(hash.as_str())?;
            transaction.commit()?;
            return Ok(false);
        }
        transaction.transfer_record_remote_book(hash.as_str())?;
        if let Some(intent) = intent {
            transaction.transfer_complete_upload(intent.id, intent.content_hash.as_str(), intent.checksum.as_str())?;
        }
        transaction.commit()?;
        Ok(true)
    }

    /// Commits successful batch members as one account-conditional operation.
    /// Each returned flag corresponds to the supplied member and says whether
    /// its local revision still matched at commit time.
    pub fn complete_book_upload_batch(
        &self,
        completed: &[BookUploadCompletion],
    ) -> Result<Vec<bool>, DatabaseError> {
        let transaction = self.connection.unchecked_transaction()?;
        let mut current = Vec::with_capacity(completed.len());
        for completion in completed {
            let matches = Self::local_versions_of(&transaction, &completion.content_hash)? == completion.local_versions;
            if matches {
                transaction.transfer_record_remote_book(completion.content_hash.as_str())?;
                transaction.transfer_complete_upload(completion.intent.id, completion.intent.content_hash.as_str(), completion.intent.checksum.as_str())?;
            } else {
                transaction.transfer_forget_remote_book(completion.content_hash.as_str())?;
            }
            current.push(matches);
        }
        transaction.commit()?;
        Ok(current)
    }

    pub fn asset_planning_page(&self, after: i64, through: Option<i64>) -> Result<AssetPlanningPage, DatabaseError> {
        let transaction = self.connection.unchecked_transaction()?;
        let mut watermark = 0;
        transaction.transfer_work_watermark(|row| { watermark = row.get(0)?; Ok(()) })?;
        let through = through.unwrap_or(watermark);
        let rows = self.candidate_page_of(after, through)?;
        transaction.commit()?;
        Ok(AssetPlanningPage { rows, through })
    }

    pub fn settle_asset_candidates(
        &self,
        settled: &[(i64, bool)],
    ) -> Result<(), DatabaseError> {
        let transaction = self.connection.unchecked_transaction()?;
        if !settled.is_empty() { transaction.transfer_settle_candidates(&serde_json::to_string(settled).map_err(DatabaseError::operation)?)?; }
        transaction.commit().map_err(Into::into)
    }

    pub fn record_remote_thumbnail(&self, hash: &ContentHash) -> Result<(), DatabaseError> {
        self.connection.transfer_record_remote_thumbnail(hash.as_str()).map(|_| ()).map_err(Into::into)
    }

    pub fn is_book_downloaded(&self, hash: &ContentHash) -> Result<bool, DatabaseError> {
        let mut downloaded = false;
        self.connection.transfer_is_book_downloaded(hash.as_str(), |row| {
            downloaded = row.get(0)?;
            Ok(())
        })?;
        Ok(downloaded)
    }

    /// Clears a locally evicted book's download/restore state in one
    /// transaction. Returns whether it had been downloaded, so the caller
    /// knows whether to also remove its bytes from storage.
    pub fn evict_local_book(&self, hash: &ContentHash) -> Result<bool, DatabaseError> {
        if !self.is_book_downloaded(hash)? {
            return Ok(false);
        }
        let transaction = self.connection.unchecked_transaction()?;
        transaction.transfer_complete_download(hash.as_str())?;
        transaction.transfer_set_book_hash_downloaded(hash.as_str(), 0)?;
        #[cfg(not(target_arch = "wasm32"))]
        transaction.transfer_cancel_book_restore(hash.as_str())?;
        transaction.commit()?;
        Ok(true)
    }

    /// Whether local file work is outstanding. Errors surface through the
    /// file-work queue itself, not through stored metadata.
    pub fn local_file_work_status(&self) -> Result<(bool, Option<String>), DatabaseError> {
        #[cfg(target_arch = "wasm32")]
        return Ok((false, None));
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut pending = false;
            self.connection.transfer_has_pending_file_work(|row| {
                pending = row.get(0)?;
                Ok(())
            })?;
            Ok((pending, None))
        }
    }

    /// Whether any durable asset request awaits planning or upload.
    pub fn has_pending_asset_work(&self) -> Result<bool, DatabaseError> {
        let mut pending = false;
        self.connection.transfer_has_pending_asset_work(|row| {
            pending = row.get(0)?;
            Ok(())
        })?;
        Ok(pending)
    }

    /// Applies definitive book-manifest outcomes. Returns only intents whose
    /// durable revision is unchanged and still requires transfer.
    pub fn settle_book_manifest(
        &self,
        intents: &[BookUploadIntent],
        present_remotely: &[ContentHash],
        rejected: &[(ContentHash, String)],
    ) -> Result<Vec<BookUploadIntent>, DatabaseError> {
        let transaction = self.connection.unchecked_transaction()?;
        let mut remaining = Vec::new();
        for intent in intents {
            if Self::upload_intent_of(&transaction, &intent.content_hash)?.as_ref() != Some(intent) {
                continue;
            }
            if present_remotely.contains(&intent.content_hash) {
                transaction.transfer_record_remote_book(intent.content_hash.as_str())?;
                transaction.transfer_complete_upload(intent.id, intent.content_hash.as_str(), intent.checksum.as_str())?;
            } else if let Some((_, reason)) = rejected.iter().find(|(hash, _)| *hash == intent.content_hash) {
                transaction.transfer_record_book_rejection(intent.content_hash.as_str(), reason)?;
            } else {
                remaining.push(intent.clone());
            }
        }
        transaction.commit()?;
        Ok(remaining)
    }

    /// Resolves a user-requested book transfer after filesystem ownership has
    /// been inspected. Invalid requests are retired atomically; a valid
    /// placement is returned without exposing the connection.
    pub fn requested_book_download(&self, hash: ContentHash) -> Result<Option<BookPlacement>, DatabaseError> {
        match self.book_path(&hash)? {
            Some(path) => Ok(Some(BookPlacement::new(hash, path))),
            None => {
                self.connection.transfer_complete_download(hash.as_str())?;
                Ok(None)
            }
        }
    }

    /// Download planning reads the request queue directly instead of
    /// deriving from the asset_work drain. FIFO by insertion.
    pub fn pending_download_requests(&self) -> Result<Vec<ContentHash>, DatabaseError> {
        let mut hashes = Vec::new();
        self.transfer_pending_download_requests(|row| {
            hashes.push(ContentHash::new(&row.get::<_, String>(0)?));
            Ok(())
        })?;
        Ok(hashes)
    }

    /// Marks a requested book already present in local storage as downloaded,
    /// queues every live placement for physical reconciliation, and retires
    /// the request in one transaction.
    pub fn complete_local_book_request(&self, hash: ContentHash) -> Result<(), DatabaseError> {
        let transaction = self.connection.unchecked_transaction()?;
        transaction.transfer_set_book_hash_downloaded(hash.as_str(), 1)?;
        let mut placements = Vec::new();
        transaction.transfer_live_placements(hash.as_str(), |row| { placements.push(row.get::<_, String>(2)?); Ok(()) })?;
        for path in placements {
            transaction.transfer_queue_restore(hash.as_str(), &path)?;
        }
        transaction.transfer_complete_download(hash.as_str())?;
        transaction.commit().map_err(Into::into)
    }

    /// A hash-addressed store (browser OPFS) can satisfy a placement from
    /// bytes it already retains under another placement's name. Marks that
    /// placement downloaded and clears its outstanding request; returns
    /// `false` when the retained bytes do not match this exact placement.
    pub fn complete_retained_download(&self, hash: &ContentHash, placement: &BookPlacement) -> Result<bool, DatabaseError> {
        let mut matches = false;
        self.connection.transfer_placement_matches(hash.as_str(), placement.rel_path.as_str(), |row| {
            matches = row.get(0)?;
            Ok(())
        })?;
        if !matches {
            return Ok(false);
        }
        let transaction = self.connection.unchecked_transaction()?;
        transaction.transfer_set_book_hash_downloaded(hash.as_str(), 1)?;
        transaction.transfer_complete_download(hash.as_str())?;
        transaction.commit()?;
        Ok(true)
    }

    pub fn cancel_book_download(&self, hash: &ContentHash) -> Result<(), DatabaseError> {
        let transaction = self.connection.unchecked_transaction()?;
        transaction.transfer_forget_remote_book(hash.as_str())?;
        transaction.transfer_complete_download(hash.as_str())?;
        transaction.commit().map_err(Into::into)
    }

    pub fn book_placement_is_current(
        &self,
        placement: &BookPlacement,
        hash: &ContentHash,
    ) -> Result<bool, DatabaseError> {
        if placement.content_hash != *hash { return Ok(false); }
        let mut matches = false;
        self.connection.transfer_placement_matches(hash.as_str(), placement.rel_path.as_str(), |row| { matches = row.get(0)?; Ok(()) })?;
        Ok(matches)
    }

    /// Returns the live parent directory for a requested download. The caller
    /// may materialize that directory physically, then revalidate before
    /// publishing staged bytes.
    pub fn native_download_directory(
        &self,
        placement: &BookPlacement,
        hash: &ContentHash,
    ) -> Result<Option<sync_common::DirId>, DatabaseError> {
        if placement.content_hash != *hash { return Ok(None); }
        let mut raw_directory = None;
        self.connection.transfer_placement_directory(hash.as_str(), placement.rel_path.as_str(), |row| { raw_directory = Some(row.get::<_, String>(0)?); Ok(()) })?;
        raw_directory.map(|id| sync_common::DirId::parse_str(&id).map_err(DatabaseError::operation)).transpose()
    }

    fn local_versions(&self, hash: &ContentHash) -> Result<Vec<(String, String, String)>, DatabaseError> { Self::local_versions_of(&self.connection, hash) }
    fn local_versions_of(connection: &rusqlite::Connection, hash: &ContentHash) -> Result<Vec<(String, String, String)>, DatabaseError> { let mut rows = Vec::new(); connection.transfer_local_versions(hash.as_str(), |row| { rows.push((row.get(0)?, row.get(1)?, row.get(2)?)); Ok(()) })?; Ok(rows) }
    fn upload_intent(&self, hash: &ContentHash) -> Result<Option<BookUploadIntent>, DatabaseError> { Self::upload_intent_of(&self.connection, hash) }
    fn upload_intent_of(connection: &rusqlite::Connection, hash: &ContentHash) -> Result<Option<BookUploadIntent>, DatabaseError> { let mut intent = None; connection.transfer_upload_intent(hash.as_str(), |row| { intent = Some(BookUploadIntent { id: row.get(0)?, content_hash: hash.clone(), checksum: ContentHash::new(&row.get::<_, String>(1)?), size_bytes: row.get::<_, i64>(2)? as u64 }); Ok(()) })?; Ok(intent) }
    /// Walks one directory chain to the root, collecting ancestor ids and the
    /// display names from the starting directory upward (root name omitted).
    /// Returns whether the chain reaches a live root, plus the names
    /// bottom-up. Missing and deleted rows break the chain; corrupt cycles
    /// terminate. The ancestor set keeps no deleted filter, mirroring the old
    /// `ancestors` CTE, while only a live chain yields display names.
    fn walk_chain(
        &self,
        start_dir_id: &str,
        ancestors: &mut std::collections::HashSet<String>,
    ) -> Result<(bool, Vec<String>), DatabaseError> {
        let mut current = start_dir_id.to_owned();
        let mut names = Vec::new();
        // Corrupt cycles must terminate; real trees are shallow.
        for _ in 0..1024 {
            ancestors.insert(current.clone());
            let mut found: Option<(String, String, Option<i64>)> = None;
            self.transfer_dir_entry(current.as_str(), |row| {
                found = Some((row.get(0)?, row.get(1)?, row.get(2)?));
                Ok(())
            })?;
            let Some((parent_id, name, deleted_at)) = found else {
                return Ok((false, names));
            };
            if deleted_at.is_some() {
                return Ok((false, names));
            }
            if parent_id == current {
                return Ok((true, names));
            }
            names.push(name);
            current = parent_id;
        }
        Ok((false, names))
    }

    /// Display paths and ancestor sets for one book, replacing the old
    /// recursive `paths`/`ancestors` CTEs.
    fn placement_paths(&self, book_row_id: Option<i64>, content_hash: &str) -> Result<(Option<String>, std::collections::HashSet<String>), DatabaseError> {
        let mut ancestors = std::collections::HashSet::new();
        let mut display: Option<String> = None;
        if let Some(book_row_id) = book_row_id {
            let mut placements: Vec<(String, String, Option<i64>)> = Vec::new();
            self.transfer_book_dirs(book_row_id, |row| {
                placements.push((row.get(0)?, row.get(1)?, row.get(2)?));
                Ok(())
            })?;
            for (dir_id, file_name, dir_deleted) in &placements {
                let (rooted, names) = self.walk_chain(dir_id, &mut ancestors)?;
                if rooted && dir_deleted.is_none() {
                    let path = names.into_iter().rev().chain([file_name.clone()]).collect::<Vec<_>>().join("/");
                    if display.as_deref().is_none_or(|current| path.as_str() < current) {
                        display = Some(path);
                    }
                }
            }
        }
        let mut projection_dirs: Vec<String> = Vec::new();
        self.transfer_projection_dirs(content_hash, |row| {
            projection_dirs.push(row.get(0)?);
            Ok(())
        })?;
        for dir_id in &projection_dirs {
            self.walk_chain(dir_id, &mut ancestors)?;
        }
        Ok((display.map(|path| format!("/{path}")), ancestors))
    }

    fn candidate_page_of(&self, after: i64, through: i64) -> Result<Vec<AssetCandidate>, DatabaseError> {
        let mut found = Vec::new();
        self.transfer_candidate_page(after, through, |row| {
            found.push(CandidateFound { work_id: row.get(0)?, hash: row.get(1)?, row_id: row.get(2)?, deleted_at: row.get(3)? });
            Ok(())
        })?;
        if found.is_empty() {
            return Ok(Vec::new());
        }
        self.assemble_candidates(found)
    }

    /// Targeted drain entry for payload wakes: plan one hash directly
    /// instead of paging from the cursor. Returns None when the row already
    /// settled (or never existed); the bounded drain remains the backstop.
    pub fn candidate_row_of(&self, hash: &ContentHash) -> Result<Option<AssetCandidate>, DatabaseError> {
        let mut found = Vec::new();
        self.transfer_candidate_row(hash.as_str(), |row| {
            found.push(CandidateFound { work_id: row.get(0)?, hash: row.get(1)?, row_id: row.get(2)?, deleted_at: row.get(3)? });
            Ok(())
        })?;
        let mut rows = self.assemble_candidates(found)?;
        Ok(rows.pop())
    }

    fn assemble_candidates(&self, found: Vec<CandidateFound>) -> Result<Vec<AssetCandidate>, DatabaseError> {
        let hashes = serde_json::to_string(&found.iter().map(|row| row.hash.as_str()).collect::<Vec<_>>()).map_err(DatabaseError::operation)?;
        let mut flags = std::collections::HashMap::new();
        self.transfer_candidate_flags(&hashes, |row| {
            flags.insert(
                row.get::<_, String>(0)?,
                CandidateFlags {
                    thumbnail_upload_allowed: row.get(1)?,
                    thumbnail_remote: row.get(2)?,
                    thumbnail_pending: row.get(3)?,
                    requested: row.get(4)?,
                    rejected: row.get(5)?,
                    book_work_pending: row.get(6)?,
                    file_work_pending: row.get(7)?,
                },
            );
            Ok(())
        })?;
        let mut directory_work = std::collections::HashSet::new();
        self.transfer_directory_work(|row| {
            directory_work.insert(row.get::<_, String>(0)?);
            Ok(())
        })?;
        found
            .into_iter()
            .map(|row| {
                let live = row.row_id.is_some() && row.deleted_at.is_none();
                let (display, ancestors) = self.placement_paths(row.row_id, &row.hash)?;
                let flags = flags.remove(&row.hash).unwrap_or(CandidateFlags {
                    thumbnail_upload_allowed: false,
                    thumbnail_remote: false,
                    thumbnail_pending: false,
                    requested: false,
                    rejected: false,
                    book_work_pending: false,
                    file_work_pending: false,
                });
                Ok(AssetCandidate {
                    work_id: row.work_id,
                    content_hash: ContentHash::new(&row.hash),
                    live,
                    path: if live {
                        display.map(|raw| crate::RelativeBookPath::parse(&raw).map_err(DatabaseError::operation)).transpose()?
                    } else {
                        None
                    },
                    thumbnail_upload_allowed: flags.thumbnail_upload_allowed,
                    thumbnail_remote: flags.thumbnail_remote,
                    thumbnail_pending: flags.thumbnail_pending,
                    requested: flags.requested,
                    rejected: flags.rejected,
                    file_work_pending: flags.book_work_pending || flags.file_work_pending || ancestors.intersection(&directory_work).next().is_some(),
                })
            })
            .collect()
    }
    fn book_path(&self, hash: &ContentHash) -> Result<Option<crate::RelativeBookPath>, DatabaseError> {
        let mut row_id = None;
        self.book_row_id_select(hash.as_str(), |row| {
            row_id = Some(row.get::<_, i64>(0)?);
            Ok(())
        })?;
        let Some(book_row_id) = row_id else { return Ok(None) };
        let (display, _) = self.placement_paths(Some(book_row_id), hash.as_str())?;
        display.map(|raw| crate::RelativeBookPath::parse(&raw).map_err(DatabaseError::operation)).transpose()
    }

    /// Requests downloads for books with live placements in one transaction.
    /// A hash without a live placement fails the whole batch, matching the
    /// previous all-or-nothing request semantics.
    pub fn sync_request_book_downloads(
        &self,
        hashes: &[ContentHash],
        origin: library_replica::TransferOrigin,
    ) -> Result<(), DatabaseError> {
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(DatabaseError::operation)?
            .as_secs() as i64;
        let transaction = self.connection.unchecked_transaction()?;
        for hash in hashes {
            let mut placed = false;
            transaction.transfer_download_placement(hash.as_str(), |row| {
                placed = row.get(0)?;
                Ok(())
            })?;
            if !placed {
                return Err(DatabaseError::operation(format!("book {hash} has no live placement")));
            }
            transaction.transfer_add_download_request(hash.as_str(), origin.storage(), created_at)?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Reads the durable half of a book's download state. The caller combines
    /// this with live storage presence, which only it can observe.
    /// Durable download state as the shared model enum. The caller supplies
    /// live storage presence, which only it can observe; a stale downloaded
    /// mark without bytes and without a request reads as not downloaded.
    pub fn transfer_download_state(&self, hash: &ContentHash, bytes_present: bool) -> Result<DownloadState, DatabaseError> {
        let mut state = (false, false);
        self.connection.transfer_download_state(hash.as_str(), |row| {
            state = (row.get::<_, bool>(0)?, row.get::<_, bool>(1)?);
            Ok(())
        })?;
        Ok(match (state.0, state.1, bytes_present) {
            (true, _, true) => DownloadState::Downloaded,
            (_, true, _) => DownloadState::Queued,
            _ => DownloadState::NotDownloaded,
        })
    }

    /// Whether pending book uploads have a recent server quota rejection.
    pub fn book_uploads_waiting_for_storage(&self) -> Result<bool, DatabaseError> {
        self.connection.transfer_uploads_waiting_for_storage(|row| row.get(0)).map_err(Into::into)
    }

    /// Current durable rejection for a locally authored book upload, if any.
    /// The caller decides presentation; storage only reports the record.
    pub fn book_upload_rejection_reason(&self, hash: &ContentHash) -> Result<Option<String>, DatabaseError> {
        let mut reason: Option<String> = None;
        self.connection.transfer_book_upload_rejection_reason(hash.as_str(), |row| {
            reason = row.get(0)?;
            Ok(())
        })?;
        Ok(reason)
    }

    /// First file name per requested hash, for transfer presentation. Hashes
    /// without a live placement are absent from the result.
    pub fn transfer_first_file_names(&self, hashes: &[ContentHash]) -> Result<Vec<(ContentHash, String)>, DatabaseError> {
        if hashes.is_empty() {
            return Ok(Vec::new());
        }
        let payload =
            serde_json::to_string(&hashes.iter().map(|hash| hash.to_string()).collect::<Vec<_>>()).map_err(DatabaseError::operation)?;
        let mut names: Vec<(ContentHash, String)> = Vec::new();
        self.connection.transfer_first_file_names(&payload, |row| {
            let hash = ContentHash::new(&row.get::<_, String>(0)?);
            if let Some(name) = row.get::<_, Option<String>>(1)? {
                names.push((hash, name));
            }
            Ok(())
        })?;
        Ok(names)
    }

    /// Records that the server already holds these bytes.
    pub fn record_remote_asset(&self, kind: BlobKind, hash: &ContentHash) -> Result<(), DatabaseError> {
        self.connection.record_remote_asset(kind.storage(), hash.as_str())?;
        Ok(())
    }

    /// Forgets server-side presence, e.g. after a local edit.
    pub fn forget_remote_asset(&self, kind: BlobKind, hash: &ContentHash) -> Result<(), DatabaseError> {
        self.connection.forget_remote_asset(kind.storage(), hash.as_str())?;
        Ok(())
    }

    /// Hashes the server is known to hold for one asset kind.
    pub fn remote_asset_hashes(&self, kind: BlobKind) -> Result<std::collections::HashSet<ContentHash>, DatabaseError> {
        let mut hashes = std::collections::HashSet::new();
        self.connection.remote_asset_hashes(kind.storage(), |row| {
            hashes.insert(ContentHash::new(&row.get::<_, String>(0)?));
            Ok(())
        })?;
        Ok(hashes)
    }

    /// Queues a durable transfer request, preserving a user-initiated origin.
    pub fn add_asset_request(&self, kind: BlobKind, hash: &ContentHash, origin: TransferOrigin) -> Result<(), DatabaseError> {
        self.connection.add_asset_request(kind.storage(), hash.as_str(), origin.storage(), unix_secs()?)?;
        Ok(())
    }

    /// Durable transfer requests for one asset kind, oldest first.
    pub fn asset_requests(&self, kind: BlobKind) -> Result<Vec<(ContentHash, TransferOrigin)>, DatabaseError> {
        let mut rows_out = Vec::new();
        self.connection.asset_requests(kind.storage(), |row| {
            rows_out.push((row.get::<_, String>(0)?, row.get::<_, String>(1)?));
            Ok(())
        })?;
        let mut requests = Vec::new();
        for (key, origin) in rows_out {
            let origin = TransferOrigin::parse_storage(&origin).ok_or_else(|| DatabaseError::operation("invalid transfer origin"))?;
            requests.push((ContentHash::new(&key), origin));
        }
        Ok(requests)
    }

    /// Hashes with a live upload rejection. Expired rejections are deleted
    /// first, mirroring the five-minute backoff in the asset predicates.
    pub fn rejected_asset_upload_hashes(&self, kind: BlobKind) -> Result<std::collections::HashSet<ContentHash>, DatabaseError> {
        const REJECTION_BACKOFF_SECONDS: i64 = 5 * 60;
        let retry_cutoff = unix_secs()?.saturating_sub(REJECTION_BACKOFF_SECONDS);
        self.connection.rejected_uploads_prune(retry_cutoff)?;
        let mut hashes = std::collections::HashSet::new();
        self.connection.rejected_upload_hashes(kind.storage(), retry_cutoff, |row| {
            hashes.insert(ContentHash::new(&row.get::<_, String>(0)?));
            Ok(())
        })?;
        Ok(hashes)
    }

    /// Whether a live placement of this book has a queued download request.
    pub fn is_book_download_requested(&self, hash: &ContentHash) -> Result<bool, DatabaseError> {
        let mut requested = false;
        self.connection
            .download_requested(hash.as_str(), |row| {
                requested = row.get(0)?;
                Ok(())
            })
            .map_err(Into::into)?;
        Ok(requested)
    }

    /// Records a verified local version with its upload intent. Known
    /// versions do not create fresh work when seen again.
    pub fn observe_local_book(&self, hash: &ContentHash, checksum: &ContentHash, size_bytes: u64) -> Result<(), DatabaseError> {
        let transaction = self.connection.unchecked_transaction()?;
        let size = i64::try_from(size_bytes).map_err(DatabaseError::operation)?;
        let inserted = transaction.observe_local_book_insert(hash.as_str(), checksum.as_str(), size, "local")?;
        if inserted == 0 {
            transaction.commit()?;
            return Ok(());
        }
        Self::enqueue_version(&transaction, hash, checksum, size)?;
        transaction.commit()?;
        Ok(())
    }

    /// Records a verified local version for one placement, tracking the
    /// placement baseline so a new placement of known bytes stays quiet.
    pub fn observe_local_placement_book(
        &self,
        dir: &sync_common::DirId,
        hash: &ContentHash,
        checksum: &ContentHash,
        size_bytes: u64,
    ) -> Result<(), DatabaseError> {
        let transaction = self.connection.unchecked_transaction()?;
        let mut previous = None;
        transaction
            .current_checksum_select(&dir.to_string(), hash.as_str(), |row| {
                previous = row.get::<_, String>(0)?;
                Ok(())
            })?;
        if previous.as_deref() == Some(checksum.as_str()) {
            return Ok(());
        }
        let size = i64::try_from(size_bytes).map_err(DatabaseError::operation)?;
        let unseen = transaction.observe_local_book_insert(hash.as_str(), checksum.as_str(), size, "local")? != 0;
        let changed = previous.is_some();
        if unseen || changed {
            Self::enqueue_version(&transaction, hash, checksum, size)?;
        }
        let origin: String = if changed {
            "local".to_owned()
        } else {
            let mut stored_origin = None;
            transaction.version_origin_select(hash.as_str(), checksum.as_str(), |row| {
                stored_origin = Some(row.get::<_, String>(0)?);
                Ok(())
            })?;
            stored_origin.ok_or(rusqlite::Error::QueryReturnedNoRows)?
        };
        transaction.current_replace(&dir.to_string(), hash.as_str(), checksum.as_str(), &origin)?;
        transaction.commit()?;
        Ok(())
    }

    fn enqueue_version(connection: &rusqlite::Connection, hash: &ContentHash, checksum: &ContentHash, size: i64) -> Result<(), DatabaseError> {
        connection.enqueue_upload_insert(hash.as_str(), checksum.as_str(), size)?;
        connection.rejected_book_cleanup(hash.as_str())?;
        connection.asset_work_touch(hash.as_str())?;
        Ok(())
    }

    /// Current upload intent for one book, if any.
    pub fn book_upload_intent(&self, hash: &ContentHash) -> Result<Option<BookUploadIntent>, DatabaseError> {
        self.upload_intent(hash)
    }

    /// Seeds or replaces the upload intent queue entry for one book.
    pub fn queue_book_upload(&self, hash: &ContentHash, checksum: &ContentHash, size_bytes: u64) -> Result<(), DatabaseError> {
        let size = i64::try_from(size_bytes).map_err(DatabaseError::operation)?;
        self.connection.enqueue_upload_insert(hash.as_str(), checksum.as_str(), size)?;
        Ok(())
    }

    /// Durable asset-work entries in one id window, oldest first.
    pub fn asset_work_page(&self, after: i64, through: i64) -> Vec<(i64, ContentHash)> {
        let mut page = Vec::new();
        self.connection
            .asset_work_page(after, through, |row| {
                page.push((row.get::<_, i64>(0)?, ContentHash::new(&row.get::<_, String>(1)?)));
                Ok(())
            })
            .expect("asset_work_page query");
        page
    }

    /// Admits one enrichment pass inline: snapshots every live book with its
    /// current upload token, then records the snapshot as the preparation
    /// set, exactly like `upload_pipeline::finish` over its snapshot.
    pub fn finish_upload_pipeline(&self) -> Result<(), DatabaseError> {
        let mut members: Vec<(String, Option<i64>)> = Vec::new();
        self.connection.pipeline_members(|row| {
            members.push((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?));
            Ok(())
        })?;
        let json = serde_json::to_string(&members).map_err(DatabaseError::operation)?;
        self.connection.pipeline_prepare(&json, metadata_contract::audible::POLICY_VERSION)?;
        Ok(())
    }
}

include_sql!("src/assets/transfers.sql");

#[cfg(test)]
mod planning_tests {
    use crate::*;

    struct TestDatabase {
        database: Database,
        path: std::path::PathBuf,
    }

    impl Drop for TestDatabase {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    fn open_test_database() -> TestDatabase {
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("library-database-asset-planning-{}-{id}", std::process::id()));
        let database = Database::open(&path, |_| Ok(())).expect("open test database");
        database.initialize_library().expect("initialize test library");
        TestDatabase { database, path }
    }

    #[test]
    fn rejection_reason_needs_a_queued_upload() {
        let holder = open_test_database();
        let database = &holder.database;
        let hash = ContentHash::new(&"0".repeat(64));
        assert_eq!(database.book_upload_rejection_reason(&hash).expect("read reason"), None);
        database
            .settle_rejected_asset(&hash, RejectedAsset::BookUpload { intent: None }, "quota")
            .expect("settle rejection");
        // The rejection is recorded, but without a live upload row there is
        // nothing to report it against — matching the ported query.
        assert_eq!(database.book_upload_rejection_reason(&hash).expect("reread reason"), None);
    }
}

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
use crate::{BookPlacement, BookUploadCompletion, BookUploadIntent, BookUploadSnapshot, , NativeBookCompletion, NativeBookSnapshot, RejectedAsset, RelativeBookPath};

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
    pub intents: Vec<BookUploadIntent>,
    pub upload_states: Vec<(ContentHash, BookUploadSnapshot)>,
    pub upload_paths: Vec<(ContentHash, Vec<String>)>,
    pub rejections: Vec<(ContentHash, String)>,
    pub thumbnail_allowed: Vec<(ContentHash, bool)>,
    pub placements: Vec<PlacementTransferEntry>,
}

impl TransferSnapshot {
    pub fn intent_for(&self, hash: &ContentHash) -> Option<&BookUploadIntent> {
        self.intents.iter().find(|intent| intent.content_hash == *hash)
    }

    pub fn upload_state_for(&self, hash: &ContentHash) -> Option<&BookUploadSnapshot> {
        self.upload_states.iter().find(|(entry_hash, _)| entry_hash == hash).map(|(_, state)| state)
    }

    pub fn paths_for(&self, hash: &ContentHash) -> &[String] {
        self.upload_paths.iter().find(|(entry_hash, _)| entry_hash == hash).map(|(_, paths)| paths.as_slice()).unwrap_or(&[])
    }

    pub fn rejection_for(&self, hash: &ContentHash) -> Option<&str> {
        self.rejections.iter().find(|(entry_hash, _)| entry_hash == hash).map(|(_, reason)| reason.as_str())
    }

    pub fn thumbnail_allowed_for(&self, hash: &ContentHash) -> bool {
        self.thumbnail_allowed.iter().find(|(entry_hash, _)| entry_hash == hash).map(|(_, allowed)| *allowed).unwrap_or(false)
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
    /// Reads every transfer input for the given hashes: upload intents and
    /// snapshots, book paths, rejection reasons, thumbnail allowance,
    /// and per-placement download/book state.
    pub fn transfer_snapshot(&self, hashes: &[ContentHash]) -> Result<TransferSnapshot, DatabaseError> {
        let mut snapshot = TransferSnapshot::default();
        snapshot.intents = self.book_upload_intents(hashes)?;
        for hash in hashes {
            snapshot.upload_states.push((hash.clone(), self.book_upload_snapshot(hash)?));
            snapshot.upload_paths.push((hash.clone(), self.book_paths(hash)?));
            if let Some(reason) = self.book_upload_rejection_reason(hash)? {
                snapshot.rejections.push((hash.clone(), reason));
            }
            snapshot.thumbnail_allowed.push((hash.clone(), self.thumbnail_upload_allowed(hash)?));
            for rel_path in self.live_placement_paths(hash)? {
                let placement = BookPlacement::new(hash.clone(), RelativeBookPath::parse(&rel_path).map_err(|error| DatabaseError::operation(format!("stored placement path is not readable: {error:?}")))?);
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
    pub fn commit_transfer_outcome(&self, outcome: &TransferOutcome) -> Result<(), DatabaseError> {
        for write in &outcome.writes {
            match write {
                TransferWrite::SettleManifest { intents, present_remotely, rejected } => {
                    self.settle_book_manifest(intents, present_remotely, rejected)?;
                }
                TransferWrite::CompleteUploadedBook { hash, local_versions, intent } => {
                    self.complete_book_upload_if_current(hash, local_versions, intent.as_ref())?;
                }
                TransferWrite::CompleteUploadedBatch { completions } => {
                    self.complete_book_upload_batch(completions)?;
                }
                TransferWrite::CompleteDownload { hash } => {
                    self.complete_download_request(hash)?;
                }
                TransferWrite::CancelDownload { hash } => {
                    self.cancel_book_download(hash)?;
                }
                TransferWrite::CompleteRetainedDownload { hash, rel_path } => {
                    let placement = BookPlacement::new(hash.clone(), RelativeBookPath::parse(rel_path).map_err(|error| DatabaseError::operation(format!("stored placement path is not readable: {error:?}")))?);
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
        Ok(())
    }
}

// ---- operations ----
// Durable import/sync workflow status, layered over one-snapshot progress
// accounting. Records are device-local; progress reflects durable work, not
// job lifetimes.

use library_replica::{LibraryOperation, LibraryOperationKind as Kind, LibraryOperationState as State};

#[derive(serde::Serialize, serde::Deserialize)]
struct Record {
    id: String,
    kind: Kind,
    scan_complete: bool,
    scan_failed: bool,
    discovered: u64,
    failures: u64,
    requires_sync: bool,
    completed: bool,
    #[serde(default)]
    sync_unfinished: bool,
}

impl Record {
    fn new(kind: Kind, requires_sync: bool) -> Self {
        Self { id: uuid::Uuid::new_v4().to_string(), kind, scan_complete: kind == Kind::Sync, scan_failed: false, discovered: 0, failures: 0, requires_sync, completed: false, sync_unfinished: false }
    }
}

fn flag(value: bool) -> i64 {
    i64::from(value)
}

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Import => "Import",
        Kind::Sync => "Sync",
    }
}

fn parse_kind(value: &str) -> rusqlite::Result<Kind> {
    match value {
        "Import" => Ok(Kind::Import),
        "Sync" => Ok(Kind::Sync),
        _ => Err(rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, format!("unknown library operation {value}").into())),
    }
}

fn read(connection: &rusqlite::Connection) -> Result<Option<Record>, DatabaseError> {
    let mut record = None;
    connection.read_operation(|row| {
        let id: Option<String> = row.get(0)?;
        let Some(id) = id else { return Ok(()) };
        let kind: Option<String> = row.get(1)?;
        let get_flag = |index: usize| row.get::<_, Option<i64>>(index).map(|value| value.unwrap_or(0) != 0);
        record = Some(Record {
            id,
            kind: parse_kind(kind.as_deref().unwrap_or(""))?,
            scan_complete: get_flag(2)?,
            scan_failed: get_flag(3)?,
            discovered: row.get::<_, Option<i64>>(4)?.unwrap_or(0).max(0) as u64,
            failures: row.get::<_, Option<i64>>(5)?.unwrap_or(0).max(0) as u64,
            requires_sync: get_flag(6)?,
            completed: get_flag(7)?,
            sync_unfinished: get_flag(8)?,
        });
        Ok(())
    })?;
    Ok(record)
}

fn write(connection: &rusqlite::Connection, record: &Record) -> Result<(), DatabaseError> {
    connection.write_operation(
        &record.id,
        kind_name(record.kind),
        flag(record.scan_complete),
        flag(record.scan_failed),
        record.discovered.min(i64::MAX as u64) as i64,
        record.failures.min(i64::MAX as u64) as i64,
        flag(record.requires_sync),
        flag(record.completed),
        flag(record.sync_unfinished),
    )?;
    Ok(())
}

#[derive(Default)]
pub struct OperationActivity<'a> {
    pub asset_uploads_disabled: bool,
    pub scanning: bool,
    pub discovered: u64,
    pub scan_failures: u64,
    pub local_busy: bool,
    pub local_waiting: bool,
    pub sync_busy: bool,
    pub signed_in: bool,
    pub file_error: Option<&'a str>,
    pub transfer_retrying: bool,
}

pub struct OperationStatus {
    pub operation: Option<LibraryOperation>,
    pub total_books: u64,
    pub uploaded_books: u64,
}

impl Database {
    /// Marks a sync workflow as needed, preserving an existing unfinished
    /// import as the owner of its metadata sync jobs.
    pub fn begin_sync_operation(&self) -> Result<(), DatabaseError> {
        let transaction = self.connection.unchecked_transaction()?;
        let current = read(&transaction)?;
        if current.as_ref().is_some_and(|record| !record.completed && record.requires_sync && record.sync_unfinished == (record.kind == Kind::Sync)) {
            transaction.commit()?;
            return Ok(());
        }
        let mut record = current.filter(|record| !record.completed).unwrap_or_else(|| Record::new(Kind::Sync, true));
        record.requires_sync = true;
        // Import already has durable scan and upload/outbox dependencies. Do
        // not add another pair of disk commits for every metadata batch it produces.
        record.sync_unfinished = record.kind == Kind::Sync;
        write(&transaction, &record)?;
        transaction.commit()?;
        Ok(())
    }

    /// Commits a finished sync/exchange pass in one transaction: optionally
    /// records the successful-sync timestamp and clears the workflow's
    /// unfinished flag. The local library is authoritative, so completion
    /// never depends on which account is signed in.
    pub fn finish_state_sync(&self, completed_at: Option<u64>) -> Result<(), DatabaseError> {
        let transaction = self.connection.unchecked_transaction()?;
        if let Some(completed_at) = completed_at {
            transaction.operations_set_last_successful_sync_at(&completed_at.to_string())?;
        }
        if let Some(mut record) = read(&transaction)?.filter(|record| !record.completed && record.sync_unfinished) {
            record.sync_unfinished = false;
            write(&transaction, &record)?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Reconciles a workflow against a consistent queue snapshot. Only
    /// workflow creation/completion writes a record; ordinary progress reads
    /// do not commit unless the record already needs one of those transitions.
    pub fn operation_status(&self, activity: OperationActivity<'_>) -> Result<OperationStatus, DatabaseError> {
        let transaction = self.connection.unchecked_transaction()?;
        let mut progress = None;
        transaction.operation_progress(|row| {
            progress = Some((
                row.get::<_, i64>(0)?.max(0) as u64,
                row.get::<_, i64>(1)?.max(0) as u64,
                row.get::<_, i64>(2)?.max(0) as u64,
                row.get::<_, i64>(3)?.max(0) as u64,
                row.get::<_, i64>(4)?.max(0) as u64,
                row.get::<_, i64>(5)?.max(0) as u64,
                row.get::<_, i64>(6)?.max(0) as u64,
                row.get(7)?,
            ));
            Ok(())
        })?;
        let (books, uploaded_books, pending_uploads, pending_changes, pending_thumbnails, pending_covers, rejections, checkpoint): (u64, u64, u64, u64, u64, u64, u64, bool) =
            progress.ok_or_else(|| DatabaseError::operation("operation progress query returned no row"))?;
        let (pending_uploads, pending_covers, rejections) = if activity.asset_uploads_disabled { (0, 0, 0) } else { (pending_uploads, pending_covers, rejections) };
        let mut stored = read(&transaction)?;
        let remote_pending = pending_uploads + pending_changes + pending_covers > 0;
        if stored.as_ref().is_none_or(|record| record.completed) {
            if activity.scanning {
                let record = Record::new(Kind::Import, activity.signed_in);
                write(&transaction, &record)?;
                stored = Some(record);
            } else if activity.signed_in && (remote_pending || activity.sync_busy) {
                let record = Record::new(Kind::Sync, true);
                write(&transaction, &record)?;
                stored = Some(record);
            } else {
                transaction.commit()?;
                return Ok(OperationStatus { operation: None, total_books: books, uploaded_books });
            }
        }
        let mut record = stored.expect("operation established above");
        if activity.signed_in && !record.requires_sync {
            record.requires_sync = true;
            write(&transaction, &record)?;
        }
        let local_pending = !record.scan_complete || activity.scanning || activity.local_busy || pending_thumbnails > 0;
        let sync_pending = record.requires_sync && (remote_pending || activity.sync_busy || record.sync_unfinished || !checkpoint);
        let waiting_reason = if record.scan_failed {
            Some("Scan interrupted; reopen the library to retry".to_owned())
        } else if let Some(error) = activity.file_error {
            Some(error.to_owned())
        } else if sync_pending && !activity.signed_in {
            Some("Sign in to finish syncing".to_owned())
        } else if record.requires_sync && (rejections > 0 || activity.transfer_retrying) {
            Some("Waiting to retry uploads".to_owned())
        } else if !activity.scanning && activity.local_waiting && pending_thumbnails > 0 {
            Some("Waiting to retry thumbnails".to_owned())
        } else if !activity.scanning && !activity.local_busy && !activity.sync_busy && (local_pending || sync_pending) {
            Some("Waiting for background work".to_owned())
        } else {
            None
        };
        let complete = !local_pending && !sync_pending && waiting_reason.is_none();
        if complete {
            record.completed = true;
            write(&transaction, &record)?;
        }
        let result = LibraryOperation {
            id: record.id,
            kind: record.kind,
            state: if complete {
                State::Completed
            } else if waiting_reason.is_some() {
                State::Waiting
            } else {
                State::Running
            },
            scanning: activity.scanning,
            // Counts describe library state and never reset for an individual batch.
            books: books.max(record.discovered).max(activity.discovered),
            uploaded_books,
            pending_uploads,
            pending_changes,
            pending_thumbnails,
            failures: record.failures.max(activity.scan_failures) + rejections,
            needs_attention: record.scan_failed
                || activity.file_error.is_some()
                || (sync_pending && !activity.signed_in)
                || (record.requires_sync && (rejections > 0 || activity.transfer_retrying))
                || (!activity.scanning && activity.local_waiting && pending_thumbnails > 0),
            waiting_reason,
        };
        transaction.commit()?;
        Ok(OperationStatus { operation: Some(result), total_books: books, uploaded_books })
    }
}

include_sql!("src/assets/operations.sql");
