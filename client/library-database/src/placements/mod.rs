//! The stored collection: placements, discovery, physical paths.


// ---- placements ----
// Directory and physical book-placement persistence: moves, copies, and the
// Trash lifecycle (trash/restore/purge) for both books and folders.

use crate::{Database, DatabaseError};
use include_sqlite_sql::include_sql;
use library_model::{Dir, LibraryFolderDestination, LibraryTrashView, TrashedBook, TrashedFolder};
use library_replica::{BlobKind, BookPlacement, TransferOrigin};
use rusqlite::OptionalExtension;
use std::collections::HashSet;
use sync_common::{ContentHash, DirId, FileName};

fn unix_millis() -> Result<i64, DatabaseError> {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|duration| duration.as_millis() as i64).map_err(DatabaseError::operation)
}

fn parse_dir_id(raw: String) -> Result<DirId, DatabaseError> {
    DirId::parse_str(&raw).map_err(DatabaseError::operation)
}

fn book_is_trashed(connection: &rusqlite::Connection, content_hash: &ContentHash) -> Result<bool, DatabaseError> {
    let mut trashed = false;
    connection.placements_book_is_trashed(content_hash.as_str(), |row| {
        trashed = row.get(0)?;
        Ok(())
    })?;
    Ok(trashed)
}

fn recoverable_placements(connection: &rusqlite::Connection, content_hash: &ContentHash) -> Result<Vec<(String, String)>, DatabaseError> {
    let mut placements = Vec::new();
    connection.restore_book_record_with_parent_select_2(content_hash.as_str(), |row| {
        placements.push((row.get::<_, String>(0)?, row.get::<_, String>(1)?));
        Ok(())
    })?;
    let mut origin: Option<Option<String>> = None;
    connection.restore_book_record_with_parent_select_3(content_hash.as_str(), |row| {
        origin = Some(row.get(0)?);
        Ok(())
    })?;
    let origin: Option<String> = origin.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
    if let Some(origin) = origin {
        if placements.iter().any(|(dir, _)| *dir == origin) {
            placements.retain(|(dir, _)| *dir == origin);
        }
    }
    Ok(placements)
}

fn directory_is_live(connection: &rusqlite::Connection, dir_id: &DirId) -> Result<bool, DatabaseError> {
    let mut directory: Option<(String, bool)> = None;
    connection.restore_book_record_with_parent_select_4(&dir_id.to_string(), |row| {
        directory = Some((row.get(0)?, row.get(1)?));
        Ok(())
    })?;
    Ok(directory.is_some_and(|(_, live)| live))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirPathComponent {
    pub id: DirId,
    pub name: FileName,
}

/// One step of a book restore: a live flag and relative path snapshot for
/// every directory the walk would visit, starting at the original.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreAncestor {
    pub id: DirId,
    pub path: String,
    pub live: bool,
}

/// Snapshot in for a book restore. The caller walks each ancestry with
/// its own filesystem knowledge and commits the chosen targets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestorePlacement {
    pub original: DirId,
    pub file_name: String,
    pub ancestry: Vec<RestoreAncestor>,
}

fn dir_path_components(connection: &rusqlite::Connection, dir_id: &str) -> Result<Vec<DirPathComponent>, DatabaseError> {
    let mut raw = Vec::new();
    connection.placements_dir_path_components(dir_id, |row| {
        raw.push((row.get::<_, String>(0)?, row.get::<_, String>(1)?));
        Ok(())
    })?;
    let components = raw.into_iter().map(|(id, name)| Ok(DirPathComponent { id: parse_dir_id(id)?, name })).collect::<Result<Vec<_>, DatabaseError>>()?;
    Ok(components)
}

fn directory_exists_in(connection: &rusqlite::Connection, directory_id: &str) -> Result<bool, DatabaseError> {
    let mut exists = false;
    connection.placements_live_directory_exists(directory_id, |row| {
        exists = row.get(0)?;
        Ok(())
    })?;
    Ok(exists)
}

/// Live database names reserved in a destination. Physical collisions are
/// checked separately by the file store.
fn occupied_file_names_in(connection: &rusqlite::Connection, destination: &str) -> Result<Vec<String>, DatabaseError> {
    let mut names = Vec::new();
    connection.placements_occupied_file_names(destination, |row| {
        names.push(row.get::<_, String>(0)?);
        Ok(())
    })?;
    Ok(names)
}

/// The filename of a live placement, or `None` when it is absent.
fn live_file_name(connection: &rusqlite::Connection, directory: &DirId, hash: &ContentHash) -> Result<Option<String>, DatabaseError> {
    let mut name = None;
    connection.placements_live_file_name(&directory.to_string(), hash.as_str(), |row| {
        name = Some(row.get(0)?);
        Ok(())
    })?;
    Ok(name)
}

fn choose_move_folder_name(connection: &rusqlite::Connection, parent: &str, excluded: &str, requested: &str) -> Result<String, DatabaseError> {
    for number in 1..=8 {
        let candidate = if number == 1 { requested.to_owned() } else { library_replica::numbered_folder_name(requested, number) };
        let mut occupied = false;
        connection.placements_folder_name_occupied(parent, &library_replica::portable_name_key(&candidate), excluded, |row| {
            occupied = row.get(0)?;
            Ok(())
        })?;
        if !occupied {
            return Ok(candidate);
        }
    }
    let mut names = Vec::new();
    connection.placements_sibling_folder_names(parent, excluded, |row| {
        names.push(row.get::<_, String>(0)?);
        Ok(())
    })?;
    Ok(library_replica::unique_folder_name(requested, names.iter().map(String::as_str)))
}

fn choose_move_file_name(connection: &rusqlite::Connection, destination: &str, requested: &str) -> Result<String, DatabaseError> {
    for number in 1..=8 {
        let candidate = if number == 1 { requested.to_owned() } else { library_replica::numbered_file_name(requested, number) };
        let mut occupied = false;
        connection.placements_file_name_occupied(destination, &library_replica::portable_name_key(&candidate), |row| {
            occupied = row.get(0)?;
            Ok(())
        })?;
        if !occupied {
            return Ok(candidate);
        }
    }
    let names = occupied_file_names_in(connection, destination)?;
    Ok(library_replica::unique_file_name(requested, names.iter().map(String::as_str)))
}

/// Records this directory into `dir`, validating it is neither the root nor
/// its own ancestor.
fn upsert_dir(transaction: &rusqlite::Transaction<'_>, dir_id: &DirId, parent_id: &DirId, name: &str) -> Result<(), DatabaseError> {
    if *dir_id == sync_common::ROOT_DIR_ID {
        return Err(DatabaseError::operation("the root directory is reserved"));
    }
    if dir_id == parent_id {
        return Err(DatabaseError::operation("a directory cannot be its own parent"));
    }
    let mut closes_cycle = false;
    transaction.placements_directory_move_closes_cycle(&parent_id.to_string(), &dir_id.to_string(), |row| {
        closes_cycle = row.get(0)?;
        Ok(())
    })?;
    if closes_cycle {
        return Err(DatabaseError::operation("directory move would create a cycle"));
    }
    transaction.placements_upsert_dir(&dir_id.to_string(), &parent_id.to_string(), name)?;
    Ok(())
}

/// The relative path a live directory would materialize at, or `None` when
/// it is not reachable from the library root.
fn directory_relative_path(connection: &rusqlite::Connection, dir_id: &DirId) -> Result<Option<String>, DatabaseError> {
    let components = dir_path_components(connection, &dir_id.to_string())?;
    let Some(first) = components.first() else { return Ok(None) };
    let mut rooted = false;
    connection.placements_directory_is_rooted(&first.id.to_string(), &sync_common::ROOT_DIR_ID.to_string(), |row| {
        rooted = row.get(0)?;
        Ok(())
    })?;
    if !rooted {
        return Ok(None);
    }
    Ok(Some(components.iter().map(|component| component.name.as_str()).collect::<Vec<_>>().join("/")))
}

/// Publishes a directory's queued physical projection to `local_directory_work`.
fn queue_directory_work(transaction: &rusqlite::Transaction<'_>, dir: &DirId) -> Result<(), DatabaseError> {
    #[cfg(target_arch = "wasm32")]
    let _ = (transaction, dir);
    #[cfg(not(target_arch = "wasm32"))]
    transaction.placements_queue_directory_work(&dir.to_string())?;
    Ok(())
}

/// Publishes a book's queued local file operation. A no-op on wasm, which has
/// no separate physical file store to reconcile.
fn queue_file_work(transaction: &rusqlite::Transaction<'_>, operation: &str, hash: &ContentHash, path: &str) -> Result<(), DatabaseError> {
    #[cfg(target_arch = "wasm32")]
    let _ = (transaction, operation, hash, path);
    #[cfg(not(target_arch = "wasm32"))]
    transaction.placements_queue_file_work(operation, hash.as_str(), path)?;
    Ok(())
}

fn placement_path(connection: &rusqlite::Connection, dir: &DirId, name: &str) -> Result<String, DatabaseError> {
    let components = dir_path_components(connection, &dir.to_string())?;
    let mut parts = components.into_iter().map(|component| component.name.to_string()).filter(|name| !name.is_empty()).collect::<Vec<_>>();
    parts.push(name.to_owned());
    Ok(format!("/{}", parts.join("/")))
}

/// Preserves upload provenance across a metadata-only copy/move. No upload work.
fn copy_book_baseline(transaction: &rusqlite::Transaction<'_>, source: &DirId, destination: &DirId, hash: &ContentHash) -> Result<(), DatabaseError> {
    transaction.placements_copy_book_baseline(&source.to_string(), &destination.to_string(), hash.as_str())?;
    Ok(())
}

/// Records a user-initiated download request for a restored book.
fn add_download_request(transaction: &rusqlite::Transaction<'_>, hash: &ContentHash) -> Result<(), DatabaseError> {
    transaction.placements_add_download_request(BlobKind::Book.storage(), hash.as_str(), TransferOrigin::UserInitiated.storage(), unix_millis()?)?;
    Ok(())
}

impl Database {
    pub fn directory_exists(&self, directory_id: &DirId) -> Result<bool, DatabaseError> {
        directory_exists_in(&self.connection, &directory_id.to_string())
    }

    pub fn dir_path_components(&self, directory_id: &DirId) -> Result<Vec<DirPathComponent>, DatabaseError> {
        dir_path_components(&self.connection, &directory_id.to_string())
    }

    /// A destination is usable only when its complete ancestry is live and rooted.
    pub fn is_live_destination(&self, directory: &DirId) -> Result<bool, DatabaseError> {
        let mut live = false;
        self.connection.placements_is_live_destination(&directory.to_string(), |row| {
            live = row.get(0)?;
            Ok(())
        })?;
        Ok(live)
    }

    pub fn set_book_hash_downloaded(&self, content_hash: &ContentHash, downloaded: bool) -> Result<(), DatabaseError> {
        self.connection.set_book_hash_downloaded(content_hash.as_str(), i32::from(downloaded))?;
        Ok(())
    }

    /// Relative path of one directory, if it is still rooted.
    pub fn directory_relative_path_string(&self, dir_id: &DirId) -> Result<Option<String>, DatabaseError> {
        directory_relative_path(&self.connection, dir_id)
    }

    /// Whether a placement still addresses these bytes through live
    /// directories. A move, rename, or deletion that landed mid-transfer
    /// makes the finished bytes belong to a stale path.
    pub fn book_placement_expects_hash(&self, placement: &BookPlacement, content_hash: &ContentHash) -> Result<bool, DatabaseError> {
        let Some((stored_hash, dir_id, file_name)) = self.book_dir_entry_by_path(placement.rel_path.as_str())? else {
            return Ok(false);
        };
        if stored_hash != placement.content_hash || stored_hash != *content_hash {
            return Ok(false);
        }
        let mut exists = false;
        self.connection
            .placement_expects_hash(&dir_id, content_hash.as_str(), &file_name, |row| {
                exists = row.get(0)?;
                Ok(())
            })
            .map_err(Into::into)?;
        Ok(exists)
    }

    fn book_dir_entry_by_path(&self, rel_path: &str) -> Result<Option<(ContentHash, String, String)>, DatabaseError> {
        let mut found = None;
        self.connection
            .dir_entry_by_path(rel_path, |row| {
                found = Some((
                    ContentHash::new(&row.get::<_, String>(0)?),
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ));
                Ok(())
            })
            .map_err(Into::into)?;
        Ok(found)
    }

    /// Seeds one book placement with explicit physical state.
    pub fn add_book_placement(&self, dir: &DirId, hash: &ContentHash, file_name: &str, local_hash: &str, downloaded: bool) {
        self.connection
            .add_book_placement(&dir.to_string(), hash.as_str(), file_name, local_hash, downloaded)
            .expect("seed book placement");
    }

    /// Seeds one book row with explicit identity columns.
    pub fn seed_book(&self, hash: &ContentHash, title: Option<&str>, added_at: i64, format: &str) {
        self.connection
            .seed_book(hash.as_str(), title, added_at, format)
            .expect("seed book");
    }

    /// Seeds one directory row with an explicit parent and name.
    pub fn seed_dir(&self, dir_id: &DirId, parent_id: &DirId, name: &str) {
        self.connection
            .seed_dir(&dir_id.to_string(), &parent_id.to_string(), name)
            .expect("seed directory");
    }

    /// Seeds one shelf directory with its synchronized register intent.
    pub fn seed_shelf_dir(&self, dir_id: &DirId) {
        self.connection
            .seed_shelf_dir(&dir_id.to_string())
            .expect("seed shelf directory");
    }

    /// Marks every placement deleted.
    pub fn mark_all_placements_deleted(&self) {
        self.connection.mark_all_placements_deleted().expect("mark placements deleted");
    }

    /// Marks every book deleted and purged.
    pub fn mark_all_books_deleted_purged(&self) {
        self.connection.mark_all_books_deleted_purged().expect("mark books deleted and purged");
    }

    /// Renames one directory.
    pub fn rename_dir(&self, id: &DirId, name: &str) {
        self.connection.rename_dir(name, &id.to_string()).expect("rename directory");
    }

    /// Retouches every placement fingerprint, as a scan during upload would.
    pub fn touch_all_placement_hashes(&self) {
        self.connection.touch_all_placement_hashes().expect("touch placement hashes");
    }

    /// Stales every placement: deleted and no longer downloaded.
    pub fn stale_all_placements(&self) {
        self.connection.stale_all_placements().expect("stale placements");
    }

    /// Materialized path of one directory from the path view.
    pub fn dir_path_string(&self, id: &DirId) -> String {
        {
            let mut path = None;
            self.connection.dir_path_string(&id.to_string(), |row| {
                path = Some(row.get::<_, String>(0)?);
                Ok(())
            }).expect("read directory path");
            path.expect("read directory path")
        }
    }

    /// Display title of one book.
    pub fn book_title(&self, hash: &ContentHash) -> String {
        {
            let mut title = None;
            self.connection.book_title(hash.as_str(), |row| {
                title = Some(row.get::<_, String>(0)?);
                Ok(())
            }).expect("read book title");
            title.expect("read book title")
        }
    }

    /// Number of book rows with one content hash.
    pub fn book_count(&self, hash: &ContentHash) -> i64 {
        {
            let mut count = None;
            self.connection.book_count(hash.as_str(), |row| {
                count = Some(row.get::<_, i64>(0)?);
                Ok(())
            }).expect("count books");
            count.expect("count books")
        }
    }

    /// Number of book rows with one title.
    pub fn book_count_with_title(&self, title: &str) -> i64 {
        {
            let mut count = None;
            self.connection.book_count_with_title(title, |row| {
                count = Some(row.get::<_, i64>(0)?);
                Ok(())
            }).expect("count books by title");
            count.expect("count books by title")
        }
    }

    /// Number of book rows matching any of these hashes.
    pub fn count_books(&self, hashes: &[ContentHash]) -> i64 {
        if hashes.is_empty() {
            return 0;
        }
        let placeholders = hashes.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!("SELECT COUNT(*) FROM book WHERE content_hash IN ({placeholders})");
        let params: Vec<&str> = hashes.iter().map(ContentHash::as_str).collect();
        self.connection.query_row(&sql, rusqlite::params_from_iter(params), |row| row.get(0)).expect("count books by hashes")
    }

    /// Total downloaded placement flags; empty libraries read as zero.
    pub fn downloaded_placement_total(&self) -> i64 {
{
            let mut total = None;
            self.connection
                .downloaded_placement_total(|row| {
                    total = row.get::<_, Option<i64>>(0)?;
                    Ok(())
                })
                .expect("sum downloaded placements");
            total.expect("sum downloaded placements").unwrap_or(0)
        }
    }

    /// Name of one directory.
    pub fn dir_name(&self, id: &DirId) -> String {
        {
            let mut name = None;
            self.connection.dir_name(&id.to_string(), |row| {
                name = Some(row.get::<_, String>(0)?);
                Ok(())
            }).expect("read directory name");
            name.expect("read directory name")
        }
    }

    /// Creates and validates a synchronized directory with a fresh id in one
    /// transaction.
    pub fn create_directory(&self, parent_id: &DirId, name: &FileName) -> Result<Dir, DatabaseError> {
        self.create_directory_with_id(&DirId::new_v4(), parent_id, name)
    }

    /// Creates and validates a synchronized directory in one transaction.
    /// An import job may retry the same directory id after losing its worker.
    pub fn create_directory_with_id(&self, dir_id: &DirId, parent_id: &DirId, name: &FileName) -> Result<Dir, DatabaseError> {
        let transaction = self.connection.unchecked_transaction()?;
        let mut existing: Option<(String, String, Option<i64>)> = None;
        transaction.create_directory_with_id_select(&dir_id.to_string(), |row| {
            existing = Some((row.get(0)?, row.get(1)?, row.get(2)?));
            Ok(())
        })?;
        if let Some((parent, existing_name, deleted)) = existing {
            if deleted.is_some() || parent != parent_id.to_string() || existing_name != *name.as_str() {
                return Err(DatabaseError::operation("import directory was changed or deleted"));
            }
        } else {
            if !directory_exists_in(&transaction, &parent_id.to_string())? {
                return Err(DatabaseError::operation(format!("parent directory does not exist: {parent_id}")));
            }
            upsert_dir(&transaction, dir_id, parent_id, name.as_str())?;
            queue_directory_work(&transaction, dir_id)?;
        }
        directory_relative_path(&transaction, dir_id)?.ok_or_else(|| DatabaseError::operation("created directory has no path"))?;
        transaction.commit()?;
        Ok(Dir::new(*dir_id, name.clone(), 0, 0))
    }

    pub fn folder_destinations(&self) -> Result<Vec<LibraryFolderDestination>, DatabaseError> {
        let mut raw = Vec::new();
        self.connection.placements_folder_destinations(|row| {
            raw.push((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?));
            Ok(())
        })?;
        raw.into_iter()
            .map(|(id, parent_id, label, path)| {
                Ok(LibraryFolderDestination {
                    id: parse_dir_id(id)?,
                    parent_id: parent_id.map(parse_dir_id).transpose()?,
                    label,
                    path: if path.is_empty() { "Library".to_owned() } else { path },
                })
            })
            .collect()
    }

    pub fn move_directory(&self, directory_id: &DirId, requested_parent: Option<&DirId>, requested_name: Option<&str>) -> Result<String, DatabaseError> {
        let transaction = self.connection.unchecked_transaction()?;
        if *directory_id == sync_common::ROOT_DIR_ID {
            return Err(DatabaseError::operation("the library root cannot be renamed or moved"));
        }
        let mut existing: Option<(String, String)> = None;
        transaction.move_directory_record_select(&directory_id.to_string(), |row| {
            existing = Some((row.get(0)?, row.get(1)?));
            Ok(())
        })?;
        let (current_parent, current_name) = existing.ok_or_else(|| DatabaseError::operation("folder does not exist or is in Trash"))?;
        let current_parent = parse_dir_id(current_parent)?;
        let parent_id = requested_parent.copied().unwrap_or(current_parent);
        if !directory_exists_in(&transaction, &parent_id.to_string())? {
            return Err(DatabaseError::operation("destination folder does not exist"));
        }
        let requested = requested_name.unwrap_or(&current_name);
        let chosen = choose_move_folder_name(&transaction, &parent_id.to_string(), &directory_id.to_string(), requested)?;
        if parent_id != current_parent || chosen != current_name {
            upsert_dir(&transaction, directory_id, &parent_id, &chosen)?;
            queue_directory_work(&transaction, directory_id)?;
        }
        transaction.commit()?;
        Ok(chosen)
    }

    /// Duplicates a folder and everything under it beneath `destination_parent_id`,
    /// returning the copy's own directory id — what an undo needs to find what
    /// this call created.
    ///
    /// Books are content-addressed, so a copied book is a second placement of
    /// the same asset rather than a second copy of its bytes.
    pub fn copy_directory(&self, directory_id: &DirId, destination_parent_id: &DirId) -> Result<DirId, DatabaseError> {
        let transaction = self.connection.unchecked_transaction()?;
        if *directory_id == sync_common::ROOT_DIR_ID {
            return Err(DatabaseError::operation("the library root cannot be copied"));
        }
        let mut name: Option<String> = None;
        transaction.copy_directory_record_select(&directory_id.to_string(), |row| {
            name = row.get(0)?;
            Ok(())
        })?;
        let name = name.ok_or_else(|| DatabaseError::operation("folder does not exist or is in Trash"))?;
        if !directory_exists_in(&transaction, &destination_parent_id.to_string())? {
            return Err(DatabaseError::operation("destination folder does not exist"));
        }
        // The subtree is read as the copy is written, so a destination inside
        // it would be copying a folder into itself; the same query that
        // rejects a cycle-forming move answers this.
        let mut inside_source = false;
        transaction.placements_directory_move_closes_cycle(&destination_parent_id.to_string(), &directory_id.to_string(), |row| {
            inside_source = row.get(0)?;
            Ok(())
        })?;
        if inside_source {
            return Err(DatabaseError::operation("a folder cannot be copied into itself"));
        }
        let mut occupied = Vec::new();
        transaction.copy_directory_record_select_2(&destination_parent_id.to_string(), |row| {
            occupied.push(row.get::<_, String>(0)?);
            Ok(())
        })?;
        let chosen = library_replica::unique_folder_name(&name, occupied.iter().map(String::as_str));

        let copy_id = DirId::new_v4();
        let mut pending = vec![(*directory_id, copy_id, chosen, *destination_parent_id)];
        let mut visited = HashSet::new();
        while let Some((source_id, copy_id, copy_name, copy_parent)) = pending.pop() {
            if !visited.insert(source_id) {
                continue;
            }
            upsert_dir(&transaction, &copy_id, &copy_parent, &copy_name)?;
            queue_directory_work(&transaction, &copy_id)?;
            let mut placements = Vec::new();
            transaction.copy_directory_record_select_3(&source_id.to_string(), |row| {
                placements.push((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, bool>(2)?));
                Ok(())
            })?;
            for (hash, file_name, downloaded) in placements {
                // Names are already unique within the source folder and the
                // copy is empty, so each placement keeps the name it had.
                transaction.copy_directory_record_insert(&copy_id.to_string(), hash.as_str(), &file_name, i32::from(downloaded))?;
                copy_book_baseline(&transaction, &source_id, &copy_id, &ContentHash::new(&hash))?;
                #[cfg(not(target_arch = "wasm32"))]
                queue_file_work(&transaction, "restore", &ContentHash::new(&hash), &placement_path(&transaction, &copy_id, &file_name)?)?;
            }
            let mut children = Vec::new();
            transaction.copy_directory_record_select_4(&source_id.to_string(), |row| {
                children.push((row.get::<_, String>(0)?, row.get::<_, String>(1)?));
                Ok(())
            })?;
            for (child_id, child_name) in children {
                pending.push((parse_dir_id(child_id)?, DirId::new_v4(), child_name, copy_id));
            }
        }
        transaction.commit()?;
        Ok(copy_id)
    }

    /// Puts a book back in a folder it was removed from, reviving the book
    /// itself if that removal was what sent it to Trash. The tombstoned
    /// placement is reused where it still exists, so the book returns under
    /// the file name it had rather than a fresh unique one.
    pub fn restore_book_placement(&self, content_hash: &ContentHash, directory_id: &DirId) -> Result<(), DatabaseError> {
        let transaction = self.connection.unchecked_transaction()?;
        if !directory_exists_in(&transaction, &directory_id.to_string())? {
            return Err(DatabaseError::operation("folder does not exist or is in Trash"));
        }
        let mut previous: Option<String> = None;
        transaction.restore_book_placement_record_select(&directory_id.to_string(), content_hash.as_str(), |row| {
            previous = row.get(0)?;
            Ok(())
        })?;
        let mut title: Option<String> = None;
        transaction.restore_book_placement_record_select_2(content_hash.as_str(), |row| {
            title = row.get(0)?;
            Ok(())
        })?;
        let requested = previous.or(title).ok_or_else(|| DatabaseError::operation("book does not exist"))?;
        let mut occupied = Vec::new();
        transaction.restore_book_placement_record_select_3(&directory_id.to_string(), content_hash.as_str(), |row| {
            occupied.push(row.get::<_, String>(0)?);
            Ok(())
        })?;
        let chosen = library_replica::unique_file_name(&requested, occupied.iter().map(String::as_str));
        transaction.restore_book_placement_record_update(content_hash.as_str())?;
        transaction.restore_book_placement_record_insert(&directory_id.to_string(), content_hash.as_str(), &chosen)?;
        #[cfg(not(target_arch = "wasm32"))]
        queue_file_work(&transaction, "restore", content_hash, &placement_path(&transaction, directory_id, &chosen)?)?;
        add_download_request(&transaction, content_hash)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn transfer_book_placement(&self, content_hash: &ContentHash, source_id: &DirId, destination_id: &DirId, remove_source: bool) -> Result<String, DatabaseError> {
        let transaction = self.connection.unchecked_transaction()?;
        if source_id == destination_id {
            return Err(DatabaseError::operation("source and destination folders are the same"));
        }
        // Native stores must materialize the requested filesystem placement;
        // a hash-addressed browser store has no separate physical placement.
        #[cfg(not(target_arch = "wasm32"))]
        let old_path = {
            let name = live_file_name(&transaction, source_id, content_hash)?.ok_or_else(|| DatabaseError::operation("book is not present in the source folder"))?;
            placement_path(&transaction, source_id, &name)?
        };
        let mut source: Option<(String, bool)> = None;
        transaction.transfer_book_placement_record_select(&source_id.to_string(), content_hash.as_str(), |row| {
            source = Some((row.get(0)?, row.get(1)?));
            Ok(())
        })?;
        let (source_name, downloaded): (String, bool) = source
            .ok_or_else(|| DatabaseError::operation("book is not present in the source folder"))?;
        if !directory_exists_in(&transaction, &destination_id.to_string())? {
            return Err(DatabaseError::operation("destination folder does not exist"));
        }
        let existing = live_file_name(&transaction, destination_id, content_hash)?;
        let destination_name = if let Some(existing) = existing {
            existing
        } else {
            let chosen = choose_move_file_name(&transaction, &destination_id.to_string(), &source_name)?;
            transaction.transfer_book_placement_record_insert(&destination_id.to_string(), content_hash.as_str(), &chosen, i32::from(downloaded))?;
            copy_book_baseline(&transaction, source_id, destination_id, content_hash)?;
            chosen
        };
        #[cfg(not(target_arch = "wasm32"))]
        {
            // Publish the destination before retiring the source. Only this
            // book's paths need work; no whole-library projection is needed.
            queue_file_work(&transaction, "restore", content_hash, &placement_path(&transaction, destination_id, &destination_name)?)?;
            if remove_source && source_id != destination_id {
                queue_file_work(&transaction, "trash", content_hash, &old_path)?;
            }
        }
        if remove_source {
            transaction.remove_book_placement_record_update(&source_id.to_string(), content_hash.as_str(), unix_millis()?)?;
        }
        transaction.commit()?;
        Ok(destination_name)
    }

    pub fn remove_book_placement(&self, content_hash: &ContentHash, source_id: &DirId) -> Result<bool, DatabaseError> {
        let transaction = self.connection.unchecked_transaction()?;
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(name) = live_file_name(&transaction, source_id, content_hash)? {
            queue_file_work(&transaction, "trash", content_hash, &placement_path(&transaction, source_id, &name)?)?;
        }
        let mut exists = false;
        transaction.remove_book_placement_record_select(&source_id.to_string(), content_hash.as_str(), |row| {
            exists = row.get(0)?;
            Ok(())
        })?;
        if !exists {
            return Err(DatabaseError::operation("book is not present in this folder"));
        }
        let mut orphaned = false;
        transaction.remove_book_placement_record_select_2(content_hash.as_str(), &source_id.to_string(), |row| {
            orphaned = row.get(0)?;
            Ok(())
        })?;
        let now = unix_millis()?;
        transaction.remove_book_placement_record_update(&source_id.to_string(), content_hash.as_str(), now)?;
        if orphaned {
            transaction.trash_directory_record_update(content_hash.as_str(), now, &source_id.to_string())?;
        }
        transaction.commit()?;
        Ok(orphaned)
    }

    pub fn library_trash(&self) -> Result<LibraryTrashView, DatabaseError> {
        let mut books = Vec::new();
        self.connection.library_trash_select(|row| {
            books.push(TrashedBook { content_hash: ContentHash::new(&row.get::<_, String>(0)?), title: row.get(1)?, format: row.get(2)?, deleted_at: row.get(3)? });
            Ok(())
        })?;
        let mut raw = Vec::new();
        self.connection.library_trash_select_2(|row| {
            raw.push((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?));
            Ok(())
        })?;
        let mut folders = Vec::new();
        for (id, name, deleted_at) in raw {
            folders.push(TrashedFolder { id: parse_dir_id(id)?, name, deleted_at });
        }
        Ok(LibraryTrashView { books, folders })
    }

    pub fn trash_book(&self, content_hash: &ContentHash) -> Result<(), DatabaseError> {
        let transaction = self.connection.unchecked_transaction()?;
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut placements = Vec::new();
            transaction.trash_book_record_select(content_hash.as_str(), |row| {
                placements.push((row.get::<_, String>(0)?, row.get::<_, String>(1)?));
                Ok(())
            })?;
            for (dir, name) in &placements {
                queue_file_work(&transaction, "trash", content_hash, &placement_path(&transaction, &parse_dir_id(dir.clone())?, name)?)?;
            }
        }
        let now = unix_millis()?;
        let changed = transaction.trash_book_record_update(content_hash.as_str(), now)?;
        if changed == 0 {
            return Err(DatabaseError::operation("book does not exist or is already in Trash"));
        }
        transaction.trash_book_record_update_2(content_hash.as_str(), now)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn restore_book_plan(&self, content_hash: &ContentHash) -> Result<Vec<RestorePlacement>, DatabaseError> {
        if !book_is_trashed(&self.connection, content_hash)? {
            return Err(DatabaseError::operation("book is not in Trash"));
        }
        let recoverable = recoverable_placements(&self.connection, content_hash)?;
        if recoverable.is_empty() {
            return Err(DatabaseError::operation("book has no recoverable placement"));
        }
        let mut plan = Vec::with_capacity(recoverable.len());
        for (original, name) in recoverable {
            let mut ancestry = Vec::new();
            let mut target = parse_dir_id(original.clone())?;
            let mut visited = HashSet::new();
            while target != sync_common::ROOT_DIR_ID {
                if !visited.insert(target) {
                    break;
                }
                let mut directory: Option<(String, bool)> = None;
                self.connection.restore_book_record_with_parent_select_4(&target.to_string(), |row| {
                    directory = Some((row.get(0)?, row.get(1)?));
                    Ok(())
                })?;
                let Some((parent, live)) = directory else {
                    break;
                };
                let path = directory_relative_path(&self.connection, &target)?.unwrap_or_default();
                ancestry.push(RestoreAncestor { id: target, path, live });
                target = parse_dir_id(parent)?;
            }
            plan.push(RestorePlacement { original: parse_dir_id(original)?, file_name: name, ancestry });
        }
        Ok(plan)
    }

    /// Commit out for a book restore. Every recoverable placement needs
    /// exactly one target; a target that stopped being live fails the whole
    /// commit so the caller can re-plan.
    pub fn restore_book_commit(&self, content_hash: &ContentHash, choices: &[(DirId, DirId)]) -> Result<(), DatabaseError> {
        let transaction = self.connection.unchecked_transaction()?;
        if !book_is_trashed(&transaction, content_hash)? {
            return Err(DatabaseError::operation("book is not in Trash"));
        }
        let recoverable: std::collections::HashMap<DirId, String> =
            recoverable_placements(&transaction, content_hash)?.into_iter().map(|(original, name)| parse_dir_id(original).map(|original| (original, name))).collect::<Result<_, _>>()?;
        if recoverable.is_empty() {
            return Err(DatabaseError::operation("book has no recoverable placement"));
        }
        if choices.len() != recoverable.len() || choices.iter().any(|(original, _)| !recoverable.contains_key(original)) {
            return Err(DatabaseError::operation("restore targets do not cover the recoverable placements"));
        }
        transaction.restore_book_record_with_parent_update(content_hash.as_str())?;
        for (original, target) in choices {
            if *target != sync_common::ROOT_DIR_ID && !directory_is_live(&transaction, target)? {
                return Err(DatabaseError::operation("restore target changed; retry the restore"));
            }
            let name = &recoverable[original];
            let existing = live_file_name(&transaction, target, content_hash)?;
            let chosen = if let Some(existing) = existing {
                existing
            } else {
                let occupied = occupied_file_names_in(&transaction, &target.to_string())?;
                library_replica::unique_file_name(name, occupied.iter().map(String::as_str))
            };
            transaction.restore_book_record_with_parent_insert(&target.to_string(), content_hash.as_str(), &chosen)?;
            queue_file_work(&transaction, "restore", content_hash, &placement_path(&transaction, target, &chosen)?)?;
        }
        add_download_request(&transaction, content_hash)?;
        transaction.commit()?;
        Ok(())
    }

    /// Permanently removes the disposable state for a book that is in Trash
    /// while retaining its synchronized identity and Purged lifecycle
    /// tombstone. Filesystem-backed callers remove projected files
    /// separately; browser callers similarly remove their OPFS book
    /// after this transaction commits.
    pub fn purge_book(&self, content_hash: &ContentHash) -> Result<(), DatabaseError> {
        let transaction = self.connection.unchecked_transaction()?;
        purge_book_in(&transaction, content_hash)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn trash_directory(&self, directory_id: &DirId) -> Result<(), DatabaseError> {
        let transaction = self.connection.unchecked_transaction()?;
        if *directory_id == sync_common::ROOT_DIR_ID {
            return Err(DatabaseError::operation("the library root cannot be moved to Trash"));
        }
        if !directory_exists_in(&transaction, &directory_id.to_string())? {
            return Err(DatabaseError::operation("folder does not exist or is already in Trash"));
        }
        let now = unix_millis()?;
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut files = Vec::new();
            transaction.trash_directory_record_with(&directory_id.to_string(), |row| {
                files.push((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?));
                Ok(())
            })?;
            for (hash, dir, name) in files {
                queue_file_work(&transaction, "trash", &ContentHash::new(&hash), &placement_path(&transaction, &parse_dir_id(dir)?, &name)?)?;
            }
        }
        let mut hashes = Vec::new();
        transaction.trash_directory_record_with_2(&directory_id.to_string(), |row| {
            hashes.push(row.get::<_, String>(0)?);
            Ok(())
        })?;
        transaction.trash_directory_record_with_3(&directory_id.to_string(), now)?;
        for hash in hashes {
            transaction.trash_directory_record_update(&hash, now, &directory_id.to_string())?;
        }
        transaction.trash_directory_record_update_2(&directory_id.to_string(), now)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn restore_directory(&self, directory_id: &DirId, requested_parent: Option<&DirId>) -> Result<String, DatabaseError> {
        let transaction = self.connection.unchecked_transaction()?;
        let mut existing: Option<(String, String)> = None;
        transaction.restore_directory_record_select(&directory_id.to_string(), |row| {
            existing = Some((row.get(0)?, row.get(1)?));
            Ok(())
        })?;
        let (stored_parent, name) = existing.ok_or_else(|| DatabaseError::operation("folder is not in Trash"))?;
        let stored_parent = parse_dir_id(stored_parent)?;
        let requested_parent = requested_parent.copied().unwrap_or(stored_parent);
        let parent = if directory_exists_in(&transaction, &requested_parent.to_string())? { requested_parent } else { sync_common::ROOT_DIR_ID };
        let mut occupied = Vec::new();
        transaction.placements_sibling_folder_names(&parent.to_string(), &directory_id.to_string(), |row| {
            occupied.push(row.get::<_, String>(0)?);
            Ok(())
        })?;
        let chosen = library_replica::unique_folder_name(&name, occupied.iter().map(String::as_str));
        upsert_dir(&transaction, directory_id, &parent, &chosen)?;
        queue_directory_work(&transaction, directory_id)?;
        let mut restored_files = Vec::new();
        transaction.restore_directory_record_select_3(&directory_id.to_string(), |row| {
            restored_files.push((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?));
            Ok(())
        })?;
        transaction.restore_directory_record_update(&directory_id.to_string())?;
        transaction.restore_directory_record_update_2(&directory_id.to_string())?;
        for (hash, dir, name) in restored_files {
            let hash = ContentHash::new(&hash);
            queue_file_work(&transaction, "restore", &hash, &placement_path(&transaction, &parse_dir_id(dir)?, &name)?)?;
            add_download_request(&transaction, &hash)?;
        }
        transaction.commit()?;
        Ok(chosen)
    }

    pub fn purge_directory(&self, directory_id: &DirId) -> Result<(), DatabaseError> {
        let changed = self.connection.purge_directory_record_with(&directory_id.to_string(), unix_millis()?)?;
        if changed == 0 {
            return Err(DatabaseError::operation("folder is not in Trash"));
        }
        Ok(())
    }

    /// Permanently purges every current Trash entry in one transaction.
    /// Folder purging deliberately does not imply book purging, so both
    /// collections are processed explicitly.
    pub fn empty_trash(&self) -> Result<(), DatabaseError> {
        let transaction = self.connection.unchecked_transaction()?;
        transaction.empty_trash_records_with(unix_millis()?)?;
        let mut books = Vec::new();
        transaction.empty_trash_records_select(|row| {
            books.push(row.get::<_, String>(0)?);
            Ok(())
        })?;
        for hash in books {
            purge_book_in(&transaction, &ContentHash::new(&hash))?;
        }
        transaction.commit()?;
        Ok(())
    }
}

fn purge_book_in(transaction: &rusqlite::Transaction<'_>, content_hash: &ContentHash) -> Result<(), DatabaseError> {
    transaction.purge_book_record_insert(content_hash.as_str())?;
    let mut book_row_id: Option<i64> = None;
    transaction.purge_book_record_select(content_hash.as_str(), |row| {
        book_row_id = row.get(0)?;
        Ok(())
    })?;
    let book_row_id = book_row_id.ok_or_else(|| DatabaseError::operation("book is not in Trash"))?;
    let now = unix_millis()?;

    // Publish placement tombstones before removing their disposable local
    // rows. Other replicas use these before the Purged lifecycle arrives.
    transaction.purge_book_record_update(book_row_id, now)?;
    transaction.purge_book_publisher(book_row_id)?;
    transaction.purge_book_identifier(book_row_id)?;
    transaction.purge_book_language(book_row_id)?;
    transaction.purge_book_contributor(book_row_id)?;
    transaction.purge_book_subject_assignment_evidence(book_row_id)?;
    transaction.purge_book_subject_assignment(book_row_id)?;
    transaction.purge_book_subject(book_row_id)?;
    transaction.purge_book_external_entity(book_row_id)?;
    transaction.purge_external_metadata_attempt(book_row_id)?;
    transaction.purge_book_collection(book_row_id)?;
    transaction.purge_book_date(book_row_id)?;
    transaction.purge_book_value(book_row_id)?;
    transaction.purge_annotation(book_row_id)?;
    transaction.cancel_book_restore(content_hash.as_str())?;
    transaction.purge_book_record_delete_2(content_hash.as_str())?;
    transaction.purge_book_record_delete_3(book_row_id)?;
    transaction.purge_book_record_delete_4(content_hash.as_str())?;
    transaction.purge_book_record_delete_5(content_hash.as_str())?;
    transaction.purge_book_record_delete_6(content_hash.as_str())?;
    transaction.purge_book_record_update_2(book_row_id, now)?;
    transaction.purge_book_record_delete_7(book_row_id)?;
    Ok(())
}

include_sql!("src/placements/placements.sql");

// ---- scanner ----
// Database-owned contracts for filesystem discovery.
//
// A scanner receives one immutable catalog snapshot and returns observations.
// It never owns a connection, emits application events, or decides when work
// is retried. The library session owns that orchestration.

use book_model::BookFormat;


#[derive(Clone, Debug)]
pub struct ScanCatalogSnapshot {
    pub work_revision: i64,
    pub scan_id: i64,
    pub excluded_paths: Vec<String>,
    pub directories: Vec<ScanKnownDirectory>,
    pub books: Vec<ScanKnownBook>,
    pub tombstoned_files: Vec<(DirId, String)>,
    pub audible_current: Vec<ContentHash>,
}

#[derive(Clone, Debug)]
pub struct ScanKnownDirectory {
    pub id: DirId,
    pub parent_id: DirId,
    pub name: String,
    pub path: Option<String>,
    pub deleted: bool,
}

#[derive(Clone, Debug)]
pub struct ScanKnownBook {
    pub content_hash: ContentHash,
    pub directory: DirId,
    pub name: String,
    pub fingerprint: u64,
    pub description_missing: bool,
}

#[derive(Clone, Debug, Default)]
pub struct FilesystemScan {
    pub directories: Vec<ScanDirectoryObservation>,
    pub files: Vec<ScanFileObservation>,
    pub readable: bool,
}

#[derive(Clone, Debug)]
pub struct ScanDirectoryObservation {
    pub id: DirId,
    pub parent_id: DirId,
    pub name: String,
    pub path: String,
}

#[derive(Clone, Debug)]
pub struct ScanFileObservation {
    pub directory: DirId,
    pub name: String,
    pub path: String,
    pub fingerprint: u64,
}

#[derive(Clone, Debug, Default)]
pub struct InspectedFilesystemScan {
    pub books: Vec<ScannedBook>,
    pub readable: bool,
}

#[derive(Clone, Debug)]
pub struct ScannedBook {
    pub directory: DirId,
    pub name: String,
    pub path: String,
    pub fingerprint: u64,
    pub content_hash: ContentHash,
    pub checksum: ContentHash,
    pub size_bytes: u64,
    pub extension: String,
}

impl Database {
    /// Takes the immutable catalog view required by filesystem discovery. The
    /// connection is released before the caller walks or hashes any path.
    #[cfg(feature = "scanner")]
    pub fn scan_catalog_snapshot(&self) -> Result<ScanCatalogSnapshot, DatabaseError> {
        let connection = &self.connection;
        let work_revision = connection.scanner_file_work_revision(|row| row.get(0))?;
        let mut sequence = None;
        connection.scanner_sequence(|row| { sequence = Some(row.get::<_, String>(0)?); Ok(()) })?;
        let current = sequence.map(|value| value.parse::<i64>().map_err(DatabaseError::operation)).transpose()?.unwrap_or(0);
        let scan_id = current.checked_add(1).ok_or_else(|| DatabaseError::operation("scan sequence overflow"))?;
        connection.scanner_set_sequence(&scan_id.to_string())?;
        let mut excluded_paths = Vec::new();
        let mut pending_directory_paths = Vec::new();
        connection.scanner_pending_directories(&sync_common::ROOT_DIR_ID.to_string(), |row| {
            pending_directory_paths.push((row.get::<_, Option<String>>(1)?, row.get::<_, Option<String>>(2)?));
            Ok(())
        })?;
        for (live, remembered) in pending_directory_paths {
            excluded_paths.extend(live.into_iter().chain(remembered).map(|path| path.trim_matches('/').to_owned()));
        }
        let mut pending_file_paths = Vec::new();
        connection.scanner_pending_files(|row| {
            pending_file_paths.push(row.get::<_, String>(0)?);
            Ok(())
        })?;
        excluded_paths.extend(
            pending_file_paths
                .into_iter()
                .map(|path| path.trim_matches('/').to_owned()),
        );
        let mut books = Vec::new();
        connection.scanner_known_books(|row| { let Ok(fingerprint) = row.get::<_, String>(0)?.parse::<u64>() else { return Ok(()) }; books.push((fingerprint, ContentHash::new(&row.get::<_, String>(1)?), row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get(4)?)); Ok(()) })?;
        let books = books.into_iter().map(|(fingerprint, content_hash, directory, name, description_missing)| Ok(ScanKnownBook { content_hash, directory: DirId::parse_str(&directory).map_err(DatabaseError::operation)?, name, fingerprint, description_missing })).collect::<Result<Vec<_>, DatabaseError>>()?;
        let mut directories = Vec::new();
        connection.scanner_known_directories(|row| { directories.push((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, bool>(3)?, row.get::<_, Option<String>>(4)?)); Ok(()) })?;
        let directories = directories.into_iter().map(|(id,parent_id,name,live,path)| Ok(ScanKnownDirectory { id: DirId::parse_str(&id).map_err(DatabaseError::operation)?, parent_id: DirId::parse_str(&parent_id).map_err(DatabaseError::operation)?, name, path: if live { path } else { None }, deleted: !live })).collect::<Result<Vec<_>, DatabaseError>>()?;
        let mut tombstoned_files = Vec::new();
        connection.scanner_tombstoned_files(|row| { tombstoned_files.push((row.get::<_, String>(0)?, row.get::<_, String>(1)?)); Ok(()) })?;
        let tombstoned_files = tombstoned_files.into_iter().map(|(directory,name)| DirId::parse_str(&directory).map(|directory| (directory,name)).map_err(DatabaseError::operation)).collect::<Result<Vec<_>, DatabaseError>>()?;
        let mut audible_current = Vec::new();
        connection.scanner_audible_current(metadata_contract::audible::POLICY_VERSION, |row| {
            audible_current.push(ContentHash::new(&row.get::<_, String>(0)?));
            Ok(())
        })?;
        Ok(ScanCatalogSnapshot { work_revision, scan_id, excluded_paths, directories, books, tombstoned_files, audible_current })
    }

    /// Applies one filesystem result only when the file-work revision that
    /// produced its snapshot is still current. Filesystem discovery never
    /// holds this transaction.
    #[cfg(feature = "scanner")]
    pub fn apply_filesystem_scan(
        &self,
        snapshot: &ScanCatalogSnapshot,
        discovery: &FilesystemScan,
        inspected: &InspectedFilesystemScan,
    ) -> Result<ScanApplyResult, DatabaseError> {
        let connection = &self.connection;
        let transaction = self.connection.unchecked_transaction()?;
        let current_revision: i64 = transaction.scanner_file_work_revision(|row| row.get(0))?;
        if current_revision != snapshot.work_revision {
            return Ok(ScanApplyResult::Stale);
        }
        for directory in &discovery.directories {
            transaction.scanner_upsert_directory(&directory.id.to_string(), &directory.parent_id.to_string(), &directory.name)?;
            transaction.scanner_set_directory_path(&directory.id.to_string(), &directory.path)?;
        }
        for book in &inspected.books {
            let format = BookFormat::from_extension(&book.extension)
                .ok_or_else(|| DatabaseError::operation(format!("unsupported scanned extension: {}", book.extension)))?;
            let created_at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(DatabaseError::operation)?.as_millis().try_into().map_err(DatabaseError::operation)?;
            transaction.scanner_insert_book(book.content_hash.as_str(), created_at, format.canonical_extension())?;
            transaction.scanner_insert_book_dir(&book.directory.to_string(), book.content_hash.as_str(), &book.name, &book.fingerprint.to_string(), snapshot.scan_id)?;
            transaction.scanner_clear_renamed_placement(&book.directory.to_string(), book.content_hash.as_str(), &book.name)?;
            let size = i64::try_from(book.size_bytes).map_err(DatabaseError::operation)?;
            transaction.scanner_observe_local_version(book.content_hash.as_str(), book.checksum.as_str(), size)?;
            transaction.scanner_enqueue_upload(book.content_hash.as_str(), book.checksum.as_str(), size)?;
            transaction.scanner_remember_placement(&book.directory.to_string(), book.content_hash.as_str(), book.checksum.as_str())?;
            transaction.scanner_enqueue_asset_work(book.content_hash.as_str())?;
            transaction.scanner_publish_projection(book.content_hash.as_str(), &book.directory.to_string(), &book.path)?;
        }
        let seen = discovery
            .files
            .iter()
            .map(|file| (file.directory, file.name.clone(), file.fingerprint))
            .collect::<Vec<_>>();
        for chunk in seen.chunks(250) { let values = serde_json::to_string(&chunk.iter().map(|(dir,name,fingerprint)| (dir.to_string(),name,fingerprint.to_string())).collect::<Vec<_>>()).map_err(DatabaseError::operation)?; transaction.scanner_mark_seen(snapshot.scan_id, &values)?; }
        transaction.commit()?;
        Ok(ScanApplyResult::Applied)
    }
}

include_sql!("src/placements/scanner.sql");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScanApplyResult {
    Applied,
    Stale,
}

// ---- paths ----
// Read-only book metadata used to resolve physical storage.
//
// The filesystem layer receives the resulting values; it never opens the
// library database to discover them.

use sync_common::ContentHash;

/// Database facts required to open a locally materialized book.
/// Filesystem code receives only `path`; it never consults SQLite itself.
pub struct LocalBookInfo {
    pub format: book_model::BookFormat,
    pub path: Option<String>,
}

impl Database {
    pub fn pdf_metadata(&self, hash: &ContentHash, checksum: &ContentHash) -> Result<Option<pdf_view_common::PdfReaderMetadata>, DatabaseError> {
        let connection = &self.connection;
        let mut bytes = None;
        connection.stored_pdf_metadata(hash.as_str(), checksum.as_str(), |row| {
            bytes = Some(row.get::<_, Vec<u8>>(0)?);
            Ok(())
        })?;
        Ok(bytes.and_then(|bytes| serde_json::from_slice::<pdf_view_common::PdfReaderMetadata>(&bytes).ok())
            .filter(|metadata| metadata.valid_for(checksum.as_str())))
    }

    pub fn local_book_info(&self, hash: &ContentHash) -> Result<LocalBookInfo, DatabaseError> {
        let connection = &self.connection;
        let mut info = None;
        connection.local_book_info(hash.as_str(), |row| {
            info = Some((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?));
            Ok(())
        })?;
        let (format, path) = info.ok_or_else(|| DatabaseError::operation("book format missing"))?;
        let format = book_model::BookFormat::from_extension(&format)
            .ok_or_else(|| DatabaseError::operation("book format missing"))?;
        Ok(LocalBookInfo { format, path })
    }

    /// Every remembered native path for a book, including a source
    /// retained while a move is pending.
    pub fn book_paths(&self, hash: &ContentHash) -> Result<Vec<String>, DatabaseError> {
        let connection = &self.connection;
        let mut paths = Vec::new();
        connection.book_paths(hash.as_str(), |row| {
            paths.push(row.get(0)?);
            Ok(())
        })?;
        Ok(paths)
    }

    /// Paths that contribute to local book storage usage.
    pub fn local_book_paths(&self) -> Result<Vec<String>, DatabaseError> {
        let connection = &self.connection;
        let mut paths = Vec::new();
        connection.local_book_paths(|row| {
            paths.push(row.get(0)?);
            Ok(())
        })?;
        Ok(paths)
    }

    /// PDF metadata only applies when the metadata fingerprint still matches
    /// the exact local placement selected for reading.
    pub fn local_pdf_metadata(
        &self,
        hash: &ContentHash,
        fingerprint: &str,
        relative_path: &str,
    ) -> Result<Option<pdf_view_common::PdfReaderMetadata>, DatabaseError> {
        let connection = &self.connection;
        let mut raw_checksum = None;
        use rusqlite::OptionalExtension as _;
        connection.local_pdf_checksum(hash.as_str(), fingerprint, relative_path, |row| {
            raw_checksum = Some(row.get::<_, String>(0)?);
            Ok(())
        }).optional()?;
        let Some(checksum) = raw_checksum.and_then(|value| value.parse::<ContentHash>().ok()) else {
            return Ok(None);
        };
        self.pdf_metadata(hash, &checksum)
    }

    /// Live file names already occupied in a destination folder. Physical
    /// name collisions are checked separately by the file store.
    pub fn occupied_file_names(&self, destination: &sync_common::DirId) -> Result<Vec<String>, DatabaseError> {
        let mut names = Vec::new();
        self.connection.occupied_file_names(&destination.to_string(), |row| {
            names.push(row.get::<_, String>(0)?);
            Ok(())
        })?;
        Ok(names)
    }
}

include_sql!("src/placements/book_paths.sql");
