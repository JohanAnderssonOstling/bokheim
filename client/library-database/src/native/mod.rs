//! Native (on-device) filesystem state: work, directories, staged books.


// ---- native ----
// Native staged-book snapshots and conditional projection commits.

use crate::{Database, DatabaseError};
use sync_common::{ContentHash, DirId};
use crate::{BookPlacement};

#[derive(Clone, Debug)]
pub struct NativeBookSnapshot {
    pub content_hash: ContentHash,
    pub directory: DirId,
    pub name: String,
    pub relative_path: String,
    pub occupied_names: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct NativeBookCompletion {
    pub snapshot: NativeBookSnapshot,
    pub name: String,
    pub relative_path: String,
    pub published: bool,
    pub fingerprint: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeBookAcknowledgement {
    Applied,
    Obsolete,
}

impl Database {
    pub fn native_book_snapshot(
        &self,
        placement: &BookPlacement,
        hash: &ContentHash,
    ) -> Result<Option<NativeBookSnapshot>, DatabaseError> {
        let connection = &self.connection;
        let Some((current_hash, directory, name)) = Self::native_book_entry(connection, placement.rel_path.as_str())? else {
            return Ok(None);
        };
        if &current_hash != hash {
            return Ok(None);
        }
        let relative_path = placement.rel_path.as_str().to_owned();
        if relative_path != placement.rel_path.as_str() {
            return Ok(None);
        }
        let occupied_names = Self::native_occupied_names(connection, &directory)?;
        Ok(Some(NativeBookSnapshot { content_hash: *hash, directory, name, relative_path, occupied_names }))
    }

    pub fn acknowledge_native_book(
        &self,
        completion: NativeBookCompletion,
    ) -> Result<NativeBookAcknowledgement, DatabaseError> {
        let transaction = self.connection.unchecked_transaction()?;
        let current = Self::native_book_entry(&transaction, &completion.snapshot.relative_path)?;
        let Some((hash, directory, name)) = current else { return Ok(NativeBookAcknowledgement::Obsolete) };
        if hash != completion.snapshot.content_hash || directory != completion.snapshot.directory || name != completion.snapshot.name {
            return Ok(NativeBookAcknowledgement::Obsolete);
        }
        let current_relative = completion.snapshot.relative_path.clone();
        if current_relative != completion.snapshot.relative_path {
            return Ok(NativeBookAcknowledgement::Obsolete);
        }
        let mut conflict = false;
        transaction.native_book_name_conflict(&directory.to_string(), &library_replica::portable_name_key(&completion.name), completion.snapshot.content_hash.as_str(), |row| { conflict = row.get(0)?; Ok(()) })?;
        if conflict {
            return Ok(NativeBookAcknowledgement::Obsolete);
        }
        transaction.native_rename_book(&directory.to_string(), completion.snapshot.content_hash.as_str(), &completion.name)?;
        transaction.native_publish_file_projection(completion.snapshot.content_hash.as_str(), &directory.to_string(), &completion.relative_path)?;
        if completion.published {
            let fingerprint = completion.fingerprint.ok_or_else(|| DatabaseError::operation("published native file has no fingerprint"))?;
            transaction.native_set_book_fingerprint(&directory.to_string(), completion.snapshot.content_hash.as_str(), &fingerprint)?;
        }
        transaction.commit().map_err(DatabaseError::operation)?;
        Ok(NativeBookAcknowledgement::Applied)
    }

    /// Finalize a downloaded book after the caller has published the
    /// verified bytes. This keeps all durable transfer state in one guarded
    /// transaction and never exposes SQLite to the transfer executor.
    pub fn complete_downloaded_book(
        &self,
        completion: NativeBookCompletion,
        thumbnail_set_exists: bool,
    ) -> Result<NativeBookAcknowledgement, DatabaseError> {
        let transaction = self.connection.unchecked_transaction()?;
        let current = Self::native_book_entry(&transaction, &completion.snapshot.relative_path)?;
        let Some((hash, directory, name)) = current else { return Ok(NativeBookAcknowledgement::Obsolete) };
        if hash != completion.snapshot.content_hash || directory != completion.snapshot.directory || name != completion.snapshot.name { return Ok(NativeBookAcknowledgement::Obsolete); }
        let mut conflict = false;
        transaction.native_book_name_conflict(&directory.to_string(), &library_replica::portable_name_key(&completion.name), completion.snapshot.content_hash.as_str(), |row| { conflict = row.get(0)?; Ok(()) })?;
        if conflict { return Ok(NativeBookAcknowledgement::Obsolete); }
        transaction.native_rename_book(&directory.to_string(), completion.snapshot.content_hash.as_str(), &completion.name)?;
        transaction.native_publish_file_projection(completion.snapshot.content_hash.as_str(), &directory.to_string(), &completion.relative_path)?;
        if completion.published {
            transaction.native_set_book_fingerprint(&directory.to_string(), completion.snapshot.content_hash.as_str(), &completion.fingerprint.ok_or_else(|| DatabaseError::operation("published native file has no fingerprint"))?)?;
        }
        transaction.native_record_remote_book(completion.snapshot.content_hash.as_str())?;
        transaction.native_mark_hash_downloaded(completion.snapshot.content_hash.as_str())?;
        transaction.native_complete_download(completion.snapshot.content_hash.as_str())?;
        transaction.native_queue_book_work(completion.snapshot.content_hash.as_str())?;
        if !thumbnail_set_exists { transaction.native_request_thumbnail(completion.snapshot.content_hash.as_str())?; }
        transaction.commit()?;
        Ok(NativeBookAcknowledgement::Applied)
    }

    fn native_book_entry(connection: &rusqlite::Connection, relative_path: &str) -> Result<Option<(ContentHash, DirId, String)>, DatabaseError> {
        let mut entry = None;
        connection.native_book_entry(relative_path, |row| { entry = Some((ContentHash::new(&row.get::<_, String>(0)?), row.get::<_, String>(1)?, row.get::<_, String>(2)?)); Ok(()) })?;
        entry.map(|(hash, directory, name)| DirId::parse_str(&directory).map(|directory| (hash, directory, name)).map_err(DatabaseError::operation)).transpose()
    }
    fn native_occupied_names(connection: &rusqlite::Connection, directory: &DirId) -> Result<Vec<String>, DatabaseError> {
        let mut names = Vec::new(); connection.native_occupied_file_names(&directory.to_string(), |row| { names.push(row.get(0)?); Ok(()) })?; Ok(names)
    }
}

include_sql!("src/native/native_file_work.sql");

// ---- work ----
// Native file-work queue: listing, snapshots, acknowledgements, purge.

#[derive(Clone, Debug)]
pub struct NativeFileWork {
    pub id: i64,
    pub operation: String,
    pub content_hash: ContentHash,
    pub original_path: String,
}

#[derive(Clone, Debug)]
pub struct NativeFileWorkSnapshot {
    pub work: NativeFileWork,
    pub resolved_path: String,
    pub live: bool,
    pub placement: Option<(String, String)>,
    pub occupied_names: Vec<String>,
    pub expected_checksum: Option<String>,
    pub book_paths: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct NativeFileWorkCompletion {
    pub work: NativeFileWork,
    pub final_path: String,
    pub renamed: Option<(String, String)>,
    pub published: bool,
    pub thumbnail_set_exists: bool,
}

#[derive(Clone, Debug)]
pub struct PurgeBookSnapshot {
    pub content_hash: ContentHash,
    pub remove_bytes: bool,
}

impl Database {
    pub fn native_file_work(&self) -> Result<Vec<NativeFileWork>, DatabaseError> {
        let mut rows = Vec::new();
        self.connection.native_file_work_rows(|row| {
            rows.push(NativeFileWork {
                id: row.get(0)?,
                operation: row.get(1)?,
                content_hash: ContentHash::new(&row.get::<_, String>(2)?),
                original_path: row.get(3)?,
            });
            Ok(())
        })?;
        Ok(rows)
    }

    pub fn native_file_work_snapshot(&self, work: NativeFileWork) -> Result<Option<NativeFileWorkSnapshot>, DatabaseError> {
        if !self.connection.native_file_is_current(work.id, work.operation.as_str(), work.content_hash.as_str(), work.original_path.as_str(), |row| row.get(0))? {
            return Ok(None);
        }
        let mut live_places = Vec::new();
        self.connection.native_live_placement_paths(work.content_hash.as_str(), |row| {
            live_places.push((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?));
            Ok(())
        })?;
        let live = !live_places.is_empty();
        let mut projections = Vec::new();
        self.connection.native_file_projection_paths(work.content_hash.as_str(), |row| {
            projections.push((row.get::<_, String>(0)?, row.get::<_, String>(1)?));
            Ok(())
        })?;
        let resolved_path = live_places
            .first()
            .map(|(_, _, display)| display.clone())
            .or_else(|| projections.first().map(|(_, relative)| relative.clone()))
            .unwrap_or_else(|| work.original_path.clone());
        let placement = live_places.into_iter().next().map(|(dir_id, file_name, _)| (dir_id, file_name));
        let mut occupied_names = Vec::new();
        if let Some((dir_id, _)) = placement.as_ref() {
            self.connection.native_occupied_file_names(dir_id.as_str(), |row| {
                occupied_names.push(row.get::<_, String>(0)?);
                Ok(())
            })?;
        }
        let mut expected_checksum = None;
        self.connection.native_current_checksum(work.content_hash.as_str(), resolved_path.as_str(), |row| {
            expected_checksum = row.get::<_, Option<String>>(0)?;
            Ok(())
        })?;
        Ok(Some(NativeFileWorkSnapshot {
            work,
            resolved_path,
            live,
            placement,
            occupied_names,
            expected_checksum,
            book_paths: projections.into_iter().map(|(_, relative)| relative).collect(),
        }))
    }

    pub fn acknowledge_native_file_work(&self, completion: NativeFileWorkCompletion) -> Result<(), DatabaseError> {
        let hash = completion.work.content_hash.as_str();
        if completion.published {
            if completion.work.operation == "trash" {
                self.connection.native_delete_file_projection(hash, completion.final_path.as_str())?;
            } else if completion.work.operation == "restore" {
                let mut live_dir = None;
                if completion.renamed.is_none() {
                    self.connection.native_live_placement_paths(hash, |row| {
                        live_dir = Some(row.get::<_, String>(0)?);
                        Ok(())
                    })?;
                }
                let dir_id = completion
                    .renamed
                    .as_ref()
                    .map(|(dir_id, _)| dir_id.clone())
                    .or(live_dir);
                if let Some((dir_id, name)) = completion.renamed.as_ref() {
                    self.connection.native_rename_book(dir_id.as_str(), hash, name.as_str())?;
                }
                if let Some(dir_id) = dir_id.as_ref() {
                    self.connection.native_publish_file_projection(hash, dir_id.as_str(), completion.final_path.as_str())?;
                }
                self.connection.native_mark_hash_downloaded(hash)?;
                self.connection.native_complete_download(hash)?;
                self.connection.native_request_thumbnail(hash)?;
            }
        }
        self.connection.native_delete_file_work(completion.work.id)?;
        Ok(())
    }

    pub fn requested_purge_books(&self, after: &str) -> Result<Vec<ContentHash>, DatabaseError> {
        let mut hashes = Vec::new();
        self.connection.requested_purge_books(after, |row| {
            hashes.push(ContentHash::new(&row.get::<_, String>(0)?));
            Ok(())
        })?;
        Ok(hashes)
    }

    pub fn purge_book_snapshot(&self, hash: &ContentHash) -> Result<Option<PurgeBookSnapshot>, DatabaseError> {
        if !self.connection.native_purge_requested(hash.as_str(), |row| row.get(0))? {
            return Ok(None);
        }
        let retired = self.connection.purge_book_is_retired(hash.as_str(), |row| row.get(0))?;
        Ok(Some(PurgeBookSnapshot { content_hash: *hash, remove_bytes: !retired }))
    }

    pub fn acknowledge_purge_book(&self, snapshot: PurgeBookSnapshot) -> Result<(), DatabaseError> {
        self.connection.acknowledge_purge_book(snapshot.content_hash.as_str())?;
        Ok(())
    }

    pub fn has_pending_purge_work(&self) -> Result<bool, DatabaseError> {
        self.connection.native_has_pending_purge_work(|row| row.get(0))
    }

    pub fn native_directory_work_ids(&self) -> Result<Vec<DirId>, DatabaseError> {
        let mut ids = Vec::new();
        self.connection.native_directory_work_ids(|row| {
            ids.push(DirId::parse_str(&row.get::<_, String>(0)?).map_err(|error| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error)))?);
            Ok(())
        })?;
        Ok(ids)
    }

    pub fn native_book_work_directory_ids(&self) -> Result<Vec<DirId>, DatabaseError> {
        let mut ids = Vec::new();
        self.connection.native_book_work_directory_ids(|row| {
            ids.push(DirId::parse_str(&row.get::<_, String>(0)?).map_err(|error| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error)))?);
            Ok(())
        })?;
        Ok(ids)
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
        let mut row = None;
        self.connection.native_directory_snapshot(id.to_string().as_str(), |row_out| {
            row = Some((
                row_out.get::<_, String>(0)?,
                row_out.get::<_, Option<String>>(1)?,
                row_out.get::<_, String>(2)?,
                row_out.get::<_, String>(3)?,
            ));
            Ok(())
        })?;
        let Some((relative_path, projected_path, parent_id, name)) = row else { return Ok(None) };
        let mut sibling_names = Vec::new();
        self.connection.native_directory_sibling_names(parent_id.as_str(), |sibling| {
            sibling_names.push(sibling.get::<_, String>(0)?);
            Ok(())
        })?;
        Ok(Some(NativeDirectorySnapshot { id: *id, relative_path, projected_path, name, sibling_names }))
    }

    pub fn acknowledge_native_directory(&self, completion: NativeDirectoryCompletion) -> Result<NativeDirectoryAcknowledgement, DatabaseError> {
        let snapshot = &completion.snapshot;
        let mut current = None;
        self.connection.native_directory_snapshot(snapshot.id.to_string().as_str(), |row| {
            current = Some((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ));
            Ok(())
        })?;
        let Some((relative_path, projected_path, _, name)) = current else {
            return Ok(NativeDirectoryAcknowledgement::Obsolete);
        };
        if relative_path != snapshot.relative_path || projected_path != snapshot.projected_path || name != snapshot.name {
            return Ok(NativeDirectoryAcknowledgement::Retry);
        }
        let final_name = completion.renamed_to.as_deref().unwrap_or(snapshot.name.as_str());
        if let Some(renamed_to) = completion.renamed_to.as_deref() {
            if self.connection.native_directory_rename_conflict(snapshot.id.to_string().as_str(), renamed_to, |row| row.get(0))? {
                return Ok(NativeDirectoryAcknowledgement::Retry);
            }
            self.connection.native_directory_rename(snapshot.id.to_string().as_str(), renamed_to)?;
            self.connection.native_directory_queue_work(snapshot.id.to_string().as_str())?;
            let upper = format!("{}\u{10FFFF}", snapshot.relative_path);
            self.connection.native_move_projection_paths(snapshot.relative_path.as_str(), final_relative(snapshot.relative_path.as_str(), final_name).as_str(), upper.as_str())?;
        }
        let final_path = final_relative(snapshot.relative_path.as_str(), final_name);
        self.connection.native_set_directory_projection(snapshot.id.to_string().as_str(), final_path.as_str())?;
        Ok(NativeDirectoryAcknowledgement::Applied)
    }
}

fn final_relative(relative_path: &str, name: &str) -> String {
    match relative_path.rsplit_once('/') {
        Some((parent, _)) if !parent.is_empty() => format!("{parent}/{name}"),
        _ => format!("/{name}"),
    }
}
