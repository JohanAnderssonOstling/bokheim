//! Native (on-device) filesystem state: work, directories, staged books.

use crate::placements::{FolderPaths, directory_relative_path, stored_book_path_column, stored_placement_path};
use crate::query_methods::{query_last, query_rows};
use crate::transactions::WriteOutcome::{Commit, Rollback};
use rusqlite::{Error as SqlError, Row, types::Type};
pub(crate) mod observations;
#[cfg(not(target_arch = "wasm32"))]
mod placement_edit;
#[cfg(not(target_arch = "wasm32"))]
pub use placement_edit::{NativeMoveSource, NativePlacementEdit, NativePlacementPlan, NativePlacementResult};

pub(crate) use crate::shared_sql::SharedSql;
pub(crate) use rusqlite::OptionalExtension;
// ---- native ----
// Native staged-book snapshots and conditional projection commits.

pub(crate) use crate::{Database, DatabaseError};
pub(crate) use include_sqlite_sql::include_sql;
pub(crate) use sync_common::{BookId, DirId};

#[derive(Clone, Debug)]
pub struct NativeBookSnapshot {
    pub content_hash: BookId,
    pub directory: DirId,
    pub name: String,
    pub relative_path: library_model::RelativeBookPath,
    pub occupied_names: Vec<String>,
    pub format: book_model::BookFormat,
    pub audiobook_track_titles: Vec<(String, String)>,
}

#[derive(Clone, Debug)]
pub struct NativeBookCompletion {
    pub snapshot: NativeBookSnapshot,
    pub publication: library_model::PublishedStagedBook,
    pub size_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeBookAcknowledgement {
    Applied,
    Obsolete,
}

impl Database {
    /// A replacement content folder has no verified local bytes. Keep canonical
    /// membership, reading state, and remote presence; discard old file work so
    /// recovery itself never materializes missing books.
    pub fn reset_local_storage(&self) -> Result<Vec<BookId>, DatabaseError> {
        self.write(|tx| {
            let affected = tx
                .prepare(
                    "SELECT content_hash FROM book WHERE row_id IN (
                    SELECT book_row_id FROM library_book_presence WHERE downloaded
                 ) OR content_hash IN (SELECT content_hash FROM book_transfer WHERE book_download IS NOT NULL)",
                )?
                .query_map([], |row| row.get(0))?
                .collect::<Result<Vec<BookId>, _>>()?;
            tx.execute_batch(
                "UPDATE book_dir SET is_downloaded=0,local_hash='',size_bytes=NULL;
                 DELETE FROM local_file_projection;
                 DELETE FROM local_directory_projection;
                 DELETE FROM local_file_work;
                 DELETE FROM local_directory_work;
                 DELETE FROM local_book_work;
                 UPDATE book_transfer SET book_download=NULL,book_download_at=NULL,
                     upload_id=NULL,upload_size=NULL,upload_prepared=NULL,
                     cover_state=NULL,cover_checked=0;",
            )?;
            Ok(affected)
        })
    }

    pub fn complete_downloaded_book_with_content(&self, completion: NativeBookCompletion, checksum: content_address::ContentHash, thumbnail_set_exists: bool) -> Result<NativeBookAcknowledgement, DatabaseError> {
        self.write(|tx| {
            let result = Self::complete_downloaded_book_in_transaction(tx, &completion, thumbnail_set_exists)?;
            if result == NativeBookAcknowledgement::Applied {
                crate::content_identity::freeze_content(tx, completion.snapshot.content_hash, checksum, Some(completion.size_bytes))?;
                crate::content_identity::observe_content(tx, completion.snapshot.content_hash, completion.snapshot.directory, checksum)?;
            }
            Ok(result)
        })
    }

    pub(crate) fn complete_downloaded_book_in_transaction(connection: &rusqlite::Connection, completion: &NativeBookCompletion, thumbnail_set_exists: bool) -> Result<NativeBookAcknowledgement, DatabaseError> {
        let current = crate::placements::book_at_path(connection, completion.snapshot.relative_path.as_str())?;
        let directory = completion.snapshot.directory;
        let placement_current = current.is_some_and(|(hash, dir, name)| hash == completion.snapshot.content_hash && dir == directory && name == completion.snapshot.name);
        let mut conflict = false;
        if placement_current {
            connection.shared_book_name_conflict(&directory.to_string(), &library_replica::portable_name_key(&completion.publication.name), &completion.snapshot.content_hash, |row| {
                conflict = row.get(0)?;
                Ok(())
            })?;
        }
        if !placement_current || conflict {
            if completion.publication.published {
                // Purge may have removed its original job before this download published.
                // Retry deletion of the actual published path through durable file work.
                connection.shared_placements_queue_file_work("remove", &completion.snapshot.content_hash, &completion.publication.relative_path)?;
            }
            return Ok(NativeBookAcknowledgement::Obsolete);
        }
        connection.native_rename_book(&directory.to_string(), &completion.snapshot.content_hash, &completion.publication.name)?;
        if completion.publication.published && completion.publication.fingerprint.is_none() {
            return Err(DatabaseError::message("published native file has no fingerprint"));
        }
        observations::record_file(
            connection,
            &directory.to_string(),
            &completion.snapshot.content_hash,
            &completion.publication.relative_path,
            completion.publication.fingerprint.as_deref(),
            observations::Revision::Remote { size: i64::try_from(completion.size_bytes).map_err(DatabaseError::operation)? },
        )?;

        connection.native_record_remote_book(&completion.snapshot.content_hash)?;

        connection.shared_transfer_complete_download(&completion.snapshot.content_hash)?;
        connection.native_queue_book_work(&completion.snapshot.content_hash)?;
        if !thumbnail_set_exists {
            connection.native_request_thumbnail(&completion.snapshot.content_hash)?;
        }
        Ok(NativeBookAcknowledgement::Applied)
    }
}

include_sql!("src/native/sql/schema/tables.sql");
include_sql!("src/native/sql/schema/index.sql");
include_sql!("src/native/sql/native_file_work.sql");

// ---- work ----
// Native file-work queue: listing, snapshots, acknowledgements, purge.

#[derive(Clone, Debug)]
pub struct NativeFileWork {
    pub id: i64,
    pub operation: String,
    pub content_hash: BookId,
    pub original_path: library_model::RelativeBookPath,
}

#[derive(Clone, Debug)]
pub struct NativeFileWorkSnapshot {
    pub work: NativeFileWork,
    pub resolved_path: library_model::RelativeBookPath,
    pub placement: Option<(String, String)>,
    pub book_paths: Vec<library_model::RelativeBookPath>,
    pub live_book_paths: Vec<library_model::RelativeBookPath>,
}

#[derive(Clone, Debug)]
pub struct NativeFileWorkCompletion {
    pub snapshot: NativeFileWorkSnapshot,
    pub final_path: String,
    pub renamed: Option<(String, String)>,
    pub published: bool,
    pub thumbnail_set_exists: bool,
}

#[derive(Clone, Debug)]
pub struct PurgeBookSnapshot {
    pub(crate) work_id: i64,
    pub content_hash: BookId,
    pub remove_bytes: bool,
}

impl Database {
    /// Native placement work waiting for physical replay.
    pub fn has_pending_file_work(&self) -> Result<bool, DatabaseError> {
        #[cfg(target_arch = "wasm32")]
        return Ok(false);
        #[cfg(not(target_arch = "wasm32"))]
        self.connection.native_has_pending_file_work(|row| row.get(0)).map_err(Into::into)
    }

    pub fn native_file_work(&self) -> Result<Vec<NativeFileWork>, DatabaseError> {
        Ok(query_rows(|read| self.connection.native_file_work_rows(read), |row| Ok(NativeFileWork { id: row.get(0)?, operation: row.get(1)?, content_hash: row.get(2)?, original_path: stored_book_path_column(row, 3)? }))?)
    }

    pub fn native_file_work_snapshot(&self, work: NativeFileWork) -> Result<Option<NativeFileWorkSnapshot>, DatabaseError> {
        if !self.connection.native_file_is_current(work.id, work.operation.as_str(), &work.content_hash, work.original_path.as_str(), |row| row.get(0))? {
            return Ok(None);
        }
        let placements = query_rows(|read| self.connection.native_live_placement_paths(&work.content_hash, read), |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
        let mut folders = FolderPaths::new(&self.connection);
        let mut live_places = Vec::new();
        for (dir_id, file_name) in placements {
            if let Some(path) = folders.placement(&dir_id, &file_name)? {
                live_places.push((dir_id, file_name, path));
            }
        }
        live_places.sort_by(|left, right| left.2.cmp(&right.2));
        let projections = query_rows(|read| self.connection.native_file_projection_paths(&work.content_hash, read), |row| Ok((row.get::<_, String>(0)?, stored_book_path_column(row, 1)?)))?;
        // A hash can have many placements. Resolve this job's path, never an
        // arbitrary sibling. A remembered projection can identify a moved one.
        let remembered = projections.iter().find(|(_, path)| path == &work.original_path);
        let target = live_places.iter().find(|(_, _, path)| path == &work.original_path).or_else(|| remembered.and_then(|(id, _)| live_places.iter().find(|(dir, _, _)| dir == id)));
        let resolved_path = target.map(|(_, _, path)| path.clone()).unwrap_or_else(|| work.original_path.clone());
        let placement = target.map(|(dir, name, _)| (dir.clone(), name.clone()));
        let live_book_paths = live_places.into_iter().map(|(_, _, path)| path).collect();
        Ok(Some(NativeFileWorkSnapshot { work, resolved_path, placement, book_paths: projections.into_iter().map(|(_, relative)| relative).collect(), live_book_paths }))
    }

    pub fn acknowledge_native_file_work(&self, completion: NativeFileWorkCompletion) -> Result<(), DatabaseError> {
        self.with_write_transaction(|transaction| {
            let snapshot = completion.snapshot;
            let Some(current) = self.native_file_work_snapshot(snapshot.work)? else { return Ok(Rollback(())) };
            let work = &current.work;
            let hash = &work.content_hash;
            if current.resolved_path != snapshot.resolved_path || current.placement != snapshot.placement {
                return Err(DatabaseError::message("file placement changed during reconciliation; retry"));
            }
            if completion.published {
                if work.operation == "remove" {
                    if current.placement.is_some() {
                        return Err(DatabaseError::message("placement became live before removal; retry"));
                    }
                    // Do not forget a replacement file now recorded for the same membership.
                    if let Some(dir) = transaction.observations_file_directory(&hash, &completion.final_path, |row| row.get::<_, String>(0)).optional()? {
                        observations::forget_file(transaction, &dir, &work.content_hash)?;
                    }
                } else if work.operation == "materialize" {
                    let (dir_id, _) = snapshot.placement.as_ref().ok_or_else(|| DatabaseError::message("materialization placement no longer exists"))?;
                    if let Some((renamed_dir, name)) = completion.renamed.as_ref() {
                        if renamed_dir != dir_id {
                            return Err(DatabaseError::message("materialization changed its destination directory"));
                        }
                        let mut conflict = false;
                        transaction.shared_book_name_conflict(dir_id, &library_replica::portable_name_key(name), hash, |row| {
                            conflict = row.get(0)?;
                            Ok(())
                        })?;
                        if conflict {
                            return Err(DatabaseError::message("destination filename is occupied; retry"));
                        }
                        transaction.native_rename_book(dir_id, hash, name)?;
                    }
                    observations::record_file(transaction, dir_id, &work.content_hash, &completion.final_path, None, observations::Revision::Retained)?;
                    transaction.shared_transfer_complete_download(hash)?;
                    if !completion.thumbnail_set_exists {
                        transaction.native_request_thumbnail(hash)?;
                    }
                }
            }
            transaction.native_delete_file_work(work.id)?;
            Ok(Commit(()))
        })
    }

    pub fn requested_purge_books(&self, after: Option<&BookId>) -> Result<Vec<BookId>, DatabaseError> {
        Ok(query_rows(|read| self.connection.requested_purge_books(after, read), |row| row.get::<_, BookId>(0))?)
    }

    pub fn purge_book_snapshot(&self, hash: &BookId) -> Result<Option<PurgeBookSnapshot>, DatabaseError> {
        let work_id: Option<i64> = self.connection.observations_purge_work(&(&hash), |r| r.get(0)).optional()?;
        let Some(work_id) = work_id else { return Ok(None) };
        let retired = self.connection.purge_book_is_retired(hash, |row| row.get::<_, bool>(0)).map_err(DatabaseError::operation)?;
        // Finish placement removal before acknowledging the permanent purge.
        if retired && self.connection.observations_has_file_work(&(&hash), |r| r.get::<_, bool>(0))? {
            return Ok(None);
        }
        Ok(Some(PurgeBookSnapshot { work_id, content_hash: *hash, remove_bytes: retired }))
    }

    pub fn acknowledge_purge_book(&self, snapshot: PurgeBookSnapshot) -> Result<(), DatabaseError> {
        self.connection.acknowledge_purge_book(&snapshot.content_hash, snapshot.work_id)?;
        Ok(())
    }

    pub fn has_pending_purge_work(&self) -> Result<bool, DatabaseError> {
        Ok(self.connection.native_has_pending_purge_work(|row| row.get(0))?)
    }

    /// Queue IDs are generations: requeuing the same resource replaces its ID.
    pub fn native_directory_replay_work(&self) -> Result<Vec<(i64, DirId)>, DatabaseError> {
        Ok(query_rows(
            |read| self.connection.native_directory_replay_work(read),
            |row| {
                let dir = directory_id_column(row, 1)?;
                Ok((row.get(0)?, dir))
            },
        )?)
    }

    /// Removes replayed folders from the directory replay list.
    pub fn complete_directory_replay(&self, ids: &[i64]) -> Result<(), DatabaseError> {
        self.connection.native_complete_directory_replay(&serde_json::to_string(ids).map_err(DatabaseError::operation)?)?;
        Ok(())
    }

    /// Removes replayed books from the book replay list, keeping any that
    /// still have file work.
    pub fn complete_book_replay(&self, ids: &[i64]) -> Result<(), DatabaseError> {
        self.connection.native_complete_book_replay(&serde_json::to_string(ids).map_err(DatabaseError::operation)?)?;
        Ok(())
    }

    pub fn native_book_replay_work_ids(&self) -> Result<Vec<i64>, DatabaseError> {
        Ok(query_rows(|read| self.connection.native_book_replay_work_ids(read), |row| row.get(0))?)
    }

    /// Content hashes with queued follow-up file reconciliation.
    pub fn local_book_work_hashes(&self) -> Result<Vec<BookId>, DatabaseError> {
        Ok(query_rows(|read| self.connection.local_book_work_hashes(read), |row| row.get::<_, BookId>(0))?)
    }

    pub fn native_book_work_directory_ids(&self) -> Result<Vec<DirId>, DatabaseError> {
        Ok(query_rows(|read| self.connection.native_book_work_directory_ids(read), |row| directory_id_column(row, 0))?)
    }

    /// The library-relative path a book named `name` would occupy in `parent`,
    /// or `None` when `parent` is not a live directory. Callers combine this
    /// with their own library root to reach a filesystem path; this crate
    /// never resolves or touches the filesystem itself.
    pub fn native_book_path(&self, parent: DirId, name: &str) -> Result<Option<library_model::RelativeBookPath>, DatabaseError> {
        directory_relative_path(&self.connection, &parent)?.map(|folder| stored_placement_path(&folder, name)).transpose()
    }
}

// ---- directory ----
// Native directory snapshots and conditional materialization commits.

#[derive(Clone, Debug)]
pub struct NativeDirectorySnapshot {
    pub id: DirId,
    pub relative_path: String,
    pub projected_path: Option<String>,
    pub name: String,
    pub sibling_names: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct NativeDirectoryCompletion {
    pub snapshot: NativeDirectorySnapshot,
    pub renamed_to: Option<String>,
    pub materialized: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeDirectoryAcknowledgement {
    Applied,
    Obsolete,
    Retry,
}

impl Database {
    pub fn native_directory_snapshot(&self, id: &DirId) -> Result<Option<NativeDirectorySnapshot>, DatabaseError> {
        let snapshot = self.connection.is_autocommit().then(|| self.connection.unchecked_transaction()).transpose()?;
        let Some(record) = native_directory_record(&self.connection, id)? else { return Ok(None) };
        let sibling_names = query_rows(|read| self.connection.native_directory_sibling_names(record.parent_id.as_str(), read), |row| row.get::<_, String>(0))?;
        if let Some(snapshot) = snapshot {
            snapshot.commit()?;
        }
        Ok(Some(NativeDirectorySnapshot { id: *id, relative_path: record.relative_path, projected_path: record.projected_path, name: record.name, sibling_names }))
    }

    /// Physical locations remain recoverable after a directory is hidden or trashed.
    pub fn native_directory_projected_path(&self, id: &DirId) -> Result<Option<String>, DatabaseError> {
        use rusqlite::OptionalExtension;
        Ok(self.connection.observations_directory_path(&(id.to_string()), |row| row.get(0)).optional()?)
    }

    /// Remember an intermediate physical location independently of logical lifecycle.
    pub fn acknowledge_native_directory_staging(&self, id: &DirId, previous: &str, staged_path: &str) -> Result<(), DatabaseError> {
        self.with_write_transaction(|transaction| {
            if self.native_directory_projected_path(id)?.as_deref() != Some(previous) {
                return Err(DatabaseError::message("directory physical location changed during staging"));
            }
            let prefix = format!("{}/", previous.trim_end_matches('/'));
            let replacement = format!("{staged_path}/");
            let upper = format!("{prefix}\u{10FFFF}");
            transaction.native_move_projection_paths(&prefix, &replacement, &upper)?;
            transaction.native_move_directory_projection_paths(&prefix, &replacement, &upper)?;
            transaction.native_move_file_work_paths(&prefix, &replacement, &upper)?;
            transaction.native_set_directory_projection(&id.to_string(), staged_path)?;
            Ok(Commit(()))
        })
    }

    pub fn acknowledge_native_directory(&self, completion: NativeDirectoryCompletion) -> Result<NativeDirectoryAcknowledgement, DatabaseError> {
        self.with_write_transaction(|transaction| {
            let snapshot = &completion.snapshot;
            let Some(record) = native_directory_record(transaction, &snapshot.id)? else {
                return Ok(Rollback(NativeDirectoryAcknowledgement::Obsolete));
            };
            if record.relative_path != snapshot.relative_path || record.projected_path != snapshot.projected_path || record.name != snapshot.name {
                return Ok(Rollback(NativeDirectoryAcknowledgement::Retry));
            }
            let final_name = completion.renamed_to.as_deref().unwrap_or(snapshot.name.as_str());
            if let Some(renamed_to) = completion.renamed_to.as_deref() {
                if snapshot.id == sync_common::ROOT_DIR_ID {
                    return Err(DatabaseError::message("the root directory is reserved"));
                }
                if transaction.native_directory_rename_conflict(snapshot.id.to_string().as_str(), renamed_to, |row| row.get(0))? {
                    return Ok(Rollback(NativeDirectoryAcknowledgement::Retry));
                }
                crate::sync::apply::registers::record_local(transaction, library_replica::LibraryStateEntry::DirectoryName { dir_id: snapshot.id, name: renamed_to.to_owned() })?;
                crate::projection::project_dirty(transaction)?;
                transaction.native_directory_queue_work(snapshot.id.to_string().as_str())?;
            }
            let final_path = {
                let relative_path: &str = snapshot.relative_path.as_str();
                let name: &str = final_name;

                match relative_path.rsplit_once('/') {
                    Some((parent, _)) if !parent.is_empty() => format!("{parent}/{name}"),
                    _ => format!("/{name}"),
                }
            };
            if completion.materialized {
                let previous = snapshot.projected_path.as_deref().unwrap_or(&snapshot.relative_path);
                if previous != final_path {
                    // Include the separator so /Shelf cannot match /Shelfish.
                    let prefix = format!("{}/", previous.trim_end_matches('/'));
                    let replacement = format!("{final_path}/");
                    let upper = format!("{prefix}\u{10FFFF}");
                    transaction.native_move_projection_paths(&prefix, &replacement, &upper)?;
                    transaction.native_move_directory_projection_paths(&prefix, &replacement, &upper)?;
                    transaction.native_move_file_work_paths(&prefix, &replacement, &upper)?;
                }
                transaction.native_set_directory_projection(snapshot.id.to_string().as_str(), final_path.as_str())?;
            }
            Ok(Commit(NativeDirectoryAcknowledgement::Applied))
        })
    }
}

struct NativeDirectoryRecord {
    relative_path: String,
    projected_path: Option<String>,
    parent_id: String,
    name: String,
}

fn directory_id_column(row: &Row<'_>, index: usize) -> rusqlite::Result<DirId> {
    DirId::parse_str(&row.get::<_, String>(index)?).map_err(|error| SqlError::FromSqlConversionFailure(index, Type::Text, Box::new(error)))
}

fn native_directory_record(connection: &rusqlite::Connection, id: &DirId) -> Result<Option<NativeDirectoryRecord>, DatabaseError> {
    let Some(relative_path) = directory_relative_path(connection, id)? else { return Ok(None) };
    Ok(query_last(
        |read| connection.native_directory_snapshot(id.to_string().as_str(), read),
        |row| Ok(NativeDirectoryRecord { relative_path: format!("/{relative_path}"), projected_path: row.get(0)?, parent_id: row.get(1)?, name: row.get(2)? }),
    )?)
}

use crate::native::observations::ObservationsSql;
