//! Directory and physical book-placement persistence.

use super::assets::get_dir_path_components;
use crate::assets::add_asset_request;
use crate::assets::get_book_dir_entry_by_path;
use crate::books::queries::BooksSql;
use crate::file_work::placement_path;
use crate::file_work::queue_directory_work;
use crate::file_work::queue_file_work;
use crate::local_paths::RelativeBookPath;
use crate::local_paths::RelativeDirPath;
use crate::placements::queries::PlacementsSql;
use crate::storage_values::invalid_text_column;
use crate::storage_values::parse_uuid_column;
use crate::storage_values::unix_millis;
#[cfg(test)]
use library_model::BookFormat;
use library_model::Dir;
use library_model::LibraryFolderDestination;
use library_model::LibraryTrashView;
use library_model::TrashedBook;
use library_model::TrashedFolder;
use library_replica::BlobKind;
use library_replica::TransferOrigin;
use rusqlite::Connection;
use rusqlite::OptionalExtension;
use rusqlite::Transaction;
use std::collections::HashSet;
use sync_common::ContentHash;
use sync_common::DirId;
use sync_common::FileName;

#[cfg(test)]
mod move_name_tests;

fn choose_move_folder_name(conn: &Connection, parent: &str, excluded: &str, requested: &str) -> Result<String, rusqlite::Error> {
    let mut statement = conn.prepare_cached(include_str!("sql/placements/choose_move_folder_name_select.sql"))?;
    // Bound probes so a heavily duplicated base name still uses one bulk scan.
    for number in 1..=8 {
        let candidate = if number == 1 { requested.to_owned() } else { library_model::numbered_folder_name(requested, number) };
        let occupied: bool = statement.query_row(rusqlite::params![parent, library_model::portable_name_key(&candidate), excluded], |row| row.get(0))?;
        if !occupied {
            return Ok(candidate);
        }
    }
    let names = conn.prepare(include_str!("sql/placements/choose_move_folder_name_select_2.sql"))?.query_map(rusqlite::params![parent, excluded], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
    Ok(library_model::unique_folder_name(requested, names.iter().map(String::as_str)))
}

/// Live database names reserved in a destination. Physical collisions are checked by the file store.
pub fn occupied_file_names(conn: &Connection, destination: &str) -> rusqlite::Result<Vec<String>> {
    conn.prepare(include_str!("sql/placements/choose_move_file_name_select_2.sql"))?.query_map([destination], |row| row.get::<_, String>(0))?.collect()
}

/// The filename of a live placement, or `QueryReturnedNoRows` when it is absent.
pub fn live_file_name(conn: &Connection, directory: &DirId, hash: &ContentHash) -> rusqlite::Result<String> {
    conn.query_row(include_str!("sql/placements/transfer_book_placement_record_select_2.sql"), rusqlite::params![directory.to_string(), hash.as_str()], |row| row.get(0))
}

/// A destination is usable only when its complete ancestry is live and rooted.
pub fn is_live_destination(conn: &Connection, directory: &DirId) -> rusqlite::Result<bool> {
    conn.query_row(include_str!("sql/live_destination.sql"), [directory.to_string()], |row| row.get(0))
}

fn choose_move_file_name(conn: &Connection, destination: &str, requested: &str) -> Result<String, rusqlite::Error> {
    let mut statement = conn.prepare_cached(include_str!("sql/placements/choose_move_file_name_select.sql"))?;
    for number in 1..=8 {
        let candidate = if number == 1 { requested.to_owned() } else { library_model::numbered_file_name(requested, number) };
        let occupied: bool = statement.query_row(rusqlite::params![destination, library_model::portable_name_key(&candidate)], |row| row.get(0))?;
        if !occupied {
            return Ok(candidate);
        }
    }
    let names = occupied_file_names(conn, destination)?;
    Ok(library_model::unique_file_name(requested, names.iter().map(String::as_str)))
}

pub fn directory_relative_path(conn: &Connection, dir_id: &DirId) -> Result<Option<RelativeDirPath>, rusqlite::Error> {
    let components = get_dir_path_components(conn, &dir_id.to_string())?;
    let Some(first) = components.first() else { return Ok(None) };
    let rooted: bool = conn.query_row(include_str!("sql/placements/directory_relative_path_select.sql"), rusqlite::params![first.id.to_string(), sync_common::ROOT_DIR_ID.to_string()], |row| row.get(0))?;
    if !rooted {
        return Ok(None);
    }
    RelativeDirPath::from_components(components.iter().map(|component| component.name.as_str())).map(Some).map_err(|error| invalid_text_column(1, error.to_string()))
}

pub fn set_book_downloaded(conn: &Connection, rel_path: &RelativeBookPath, content_hash: ContentHash, downloaded: bool) -> Result<(), rusqlite::Error> {
    let Some((found_content_hash, dir_id, _file_name)) = get_book_dir_entry_by_path(conn, rel_path)? else {
        return Ok(());
    };
    if found_content_hash != content_hash {
        return Err(invalid_text_column(0, "book placement identity mismatch"));
    }
    conn.set_book_downloaded(&dir_id.to_string(), content_hash.as_str(), i32::from(downloaded))?;
    Ok(())
}

/// Updates every live placement backed by the same content-addressed asset.
pub fn set_book_hash_downloaded(conn: &Connection, content_hash: ContentHash, downloaded: bool) -> Result<(), rusqlite::Error> {
    conn.set_book_hash_downloaded(content_hash.as_str(), i32::from(downloaded))?;
    Ok(())
}

pub fn upsert_dir(transaction: &Transaction, dir_uuid: &DirId, parent_uuid: &DirId, name: &FileName) -> Result<(), rusqlite::Error> {
    if *dir_uuid == sync_common::ROOT_DIR_ID {
        return Err(invalid_text_column(0, "the root directory is reserved"));
    }
    if dir_uuid == parent_uuid {
        return Err(invalid_text_column(0, "a directory cannot be its own parent"));
    }
    let mut closes_cycle = false;
    transaction.directory_move_closes_cycle(&parent_uuid.to_string(), &dir_uuid.to_string(), |row| {
        closes_cycle = row.get(0)?;
        Ok(())
    })?;
    if closes_cycle {
        return Err(invalid_text_column(0, "directory move would create a cycle"));
    }
    transaction.upsert_dir(&dir_uuid.to_string(), &parent_uuid.to_string(), name.as_str())?;
    Ok(())
}

/// Creates and validates the synchronized directory in one transaction.
pub fn create_directory(conn: &mut Connection, parent_id: &DirId, name: &FileName) -> Result<Dir, Box<dyn std::error::Error>> {
    create_directory_with_id(conn, &DirId::new_v4(), parent_id, name)
}

/// An import job retries the same directory ID after losing its worker.
pub fn create_directory_with_id(conn: &mut Connection, dir_id: &DirId, parent_id: &DirId, name: &FileName) -> Result<Dir, Box<dyn std::error::Error>> {
    let tx = crate::schema::library_write_transaction(conn)?;
    let existing: Option<(String, String, Option<i64>)> = tx.query_row(include_str!("sql/placements/create_directory_with_id_select.sql"), [dir_id.to_string()], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).optional()?;
    if let Some((parent, existing_name, deleted)) = existing {
        if deleted.is_some() || parent != parent_id.to_string() || existing_name != *name {
            return Err("Import directory was changed or deleted".into());
        }
    } else {
        if !directory_exists(&tx, &parent_id.to_string())? {
            return Err(invalid_text_column(0, format!("parent directory does not exist: {parent_id}")).into());
        }
        upsert_dir(&tx, &dir_id, parent_id, name)?;
        queue_directory_work(&tx, &dir_id)?;
    }
    directory_relative_path(&tx, &dir_id)?.ok_or("created directory has no path")?;
    tx.commit()?;
    Ok(Dir::new(*dir_id, name.clone(), 0, 0))
}

pub fn folder_destinations(conn: &Connection) -> Result<Vec<LibraryFolderDestination>, rusqlite::Error> {
    let mut statement = conn.prepare(include_str!("sql/placements/folder_destinations_with.sql"))?;
    statement
        .query_map([], |row| {
            let id = parse_uuid_column(row.get::<_, String>(0)?, 0)?;
            let parent_id = row.get::<_, Option<String>>(1)?.map(|raw| parse_uuid_column(raw, 1)).transpose()?;
            let label = row.get(2)?;
            let path: String = row.get(3)?;
            Ok(LibraryFolderDestination { id, parent_id, label, path: if path.is_empty() { "Library".to_owned() } else { path } })
        })?
        .collect()
}

pub fn move_directory_record(transaction: &Transaction, directory_id: &DirId, requested_parent: Option<&DirId>, requested_name: Option<&str>) -> Result<String, rusqlite::Error> {
    if *directory_id == sync_common::ROOT_DIR_ID {
        return Err(invalid_text_column(0, "the library root cannot be renamed or moved"));
    }
    let (current_parent, current_name): (String, String) = transaction
        .query_row(include_str!("sql/placements/move_directory_record_select.sql"), [directory_id.to_string()], |row| Ok((row.get(0)?, row.get(1)?)))
        .optional()?
        .ok_or_else(|| invalid_text_column(0, "folder does not exist or is in Trash"))?;
    let current_parent = parse_uuid_column(current_parent, 0)?;
    let parent_id = requested_parent.copied().unwrap_or(current_parent);
    if !directory_exists(transaction, &parent_id.to_string())? {
        return Err(invalid_text_column(0, "destination folder does not exist"));
    }
    let requested = requested_name.unwrap_or(&current_name);
    let chosen = choose_move_folder_name(transaction, &parent_id.to_string(), &directory_id.to_string(), requested)?;
    if parent_id != current_parent || chosen != current_name {
        upsert_dir(transaction, directory_id, &parent_id, &chosen)?;
        queue_directory_work(transaction, directory_id)?;
    }
    Ok(chosen)
}

/// Duplicates a folder and everything under it beneath `destination_parent_id`,
/// returning the copy's own directory ID — which is what an undo needs to find
/// what this call created.
///
/// Books are content-addressed, so a copied book is a second placement of the
/// same asset rather than a second copy of its bytes: this mirrors what
/// [`transfer_book_placement_record`] does for a single book, applied to a whole
/// subtree.
pub fn copy_directory_record(transaction: &Transaction, directory_id: &DirId, destination_parent_id: &DirId) -> Result<DirId, rusqlite::Error> {
    if *directory_id == sync_common::ROOT_DIR_ID {
        return Err(invalid_text_column(0, "the library root cannot be copied"));
    }
    let name: String =
        transaction.query_row(include_str!("sql/placements/copy_directory_record_select.sql"), [directory_id.to_string()], |row| row.get(0)).optional()?.ok_or_else(|| invalid_text_column(0, "folder does not exist or is in Trash"))?;
    if !directory_exists(transaction, &destination_parent_id.to_string())? {
        return Err(invalid_text_column(0, "destination folder does not exist"));
    }
    // The subtree is read as the copy is written, so a destination inside it
    // would be copying a folder into itself. The same query that rejects a
    // cycle-forming move answers this.
    let mut inside_source = false;
    transaction.directory_move_closes_cycle(&destination_parent_id.to_string(), &directory_id.to_string(), |row| {
        inside_source = row.get(0)?;
        Ok(())
    })?;
    if inside_source {
        return Err(invalid_text_column(0, "a folder cannot be copied into itself"));
    }
    let occupied = transaction.prepare(include_str!("sql/placements/copy_directory_record_select_2.sql"))?.query_map([destination_parent_id.to_string()], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
    let chosen = library_model::unique_folder_name(&name, occupied.iter().map(String::as_str));

    let copy_id = DirId::new_v4();
    let mut pending = vec![(*directory_id, copy_id, chosen, *destination_parent_id)];
    let mut visited = HashSet::new();
    while let Some((source_id, copy_id, copy_name, copy_parent)) = pending.pop() {
        if !visited.insert(source_id) {
            continue;
        }
        upsert_dir(transaction, &copy_id, &copy_parent, &copy_name)?;
        queue_directory_work(transaction, &copy_id)?;
        let placements = transaction
            .prepare(include_str!("sql/placements/copy_directory_record_select_3.sql"))?
            .query_map([source_id.to_string()], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, bool>(2)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        for (hash, file_name, downloaded) in placements {
            // Names are already unique within the source folder and the copy is
            // empty, so each placement keeps the name it had.
            transaction.execute(include_str!("sql/placements/copy_directory_record_insert.sql"), rusqlite::params![copy_id.to_string(), hash.as_str(), file_name, i32::from(downloaded)])?;
            super::assets::copy_publication_baseline(transaction, &source_id, &copy_id, &ContentHash::new(&hash))?;
            #[cfg(not(target_arch = "wasm32"))]
            queue_file_work(transaction, "restore", &ContentHash::new(&hash), &placement_path(transaction, &copy_id, &file_name)?)?;
        }
        let children =
            transaction.prepare(include_str!("sql/placements/copy_directory_record_select_4.sql"))?.query_map([source_id.to_string()], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?.collect::<Result<Vec<_>, _>>()?;
        for (child_id, child_name) in children {
            pending.push((parse_uuid_column(child_id, 0)?, DirId::new_v4(), child_name, copy_id));
        }
    }
    Ok(copy_id)
}

/// Puts a book back in a folder it was removed from, reviving the book itself
/// if that removal was what sent it to Trash.
///
/// The tombstoned placement is reused where it still exists, so the book
/// returns under the file name it had rather than a fresh unique one.
pub fn restore_book_placement_record(transaction: &Transaction, content_hash: &ContentHash, directory_id: &DirId) -> Result<(), rusqlite::Error> {
    if !directory_exists(transaction, &directory_id.to_string())? {
        return Err(invalid_text_column(0, "folder does not exist or is in Trash"));
    }
    let previous: Option<String> = transaction.query_row(include_str!("sql/placements/restore_book_placement_record_select.sql"), rusqlite::params![directory_id.to_string(), content_hash.as_str()], |row| row.get(0)).optional()?;
    let title: Option<String> = transaction.query_row(include_str!("sql/placements/restore_book_placement_record_select_2.sql"), [content_hash.as_str()], |row| row.get(0)).optional()?;
    let Some(requested) = previous.or(title) else {
        return Err(invalid_text_column(0, "book does not exist"));
    };
    let occupied = transaction
        .prepare(include_str!("sql/placements/restore_book_placement_record_select_3.sql"))?
        .query_map(rusqlite::params![directory_id.to_string(), content_hash.as_str()], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let chosen = library_model::unique_file_name(&requested, occupied.iter().map(String::as_str));
    transaction.execute(include_str!("sql/placements/restore_book_placement_record_update.sql"), [content_hash.as_str()])?;
    transaction.execute(include_str!("sql/placements/restore_book_placement_record_insert.sql"), rusqlite::params![directory_id.to_string(), content_hash.as_str(), chosen])?;
    #[cfg(not(target_arch = "wasm32"))]
    queue_file_work(transaction, "restore", content_hash, &placement_path(transaction, directory_id, &chosen)?)?;
    add_asset_request(transaction, BlobKind::Book, content_hash, TransferOrigin::UserInitiated)?;
    Ok(())
}

pub fn transfer_book_placement_record(transaction: &Transaction, content_hash: &ContentHash, source_id: &DirId, destination_id: &DirId, remove_source: bool) -> Result<String, rusqlite::Error> {
    if source_id == destination_id {
        return Err(invalid_text_column(0, "source and destination folders are the same"));
    }
    let (source_name, downloaded): (String, bool) = transaction
        .query_row(include_str!("sql/placements/transfer_book_placement_record_select.sql"), rusqlite::params![source_id.to_string(), content_hash.as_str()], |row| Ok((row.get(0)?, row.get(1)?)))
        .optional()?
        .ok_or_else(|| invalid_text_column(0, "book is not present in the source folder"))?;
    if !directory_exists(transaction, &destination_id.to_string())? {
        return Err(invalid_text_column(0, "destination folder does not exist"));
    }
    let existing = live_file_name(transaction, destination_id, content_hash).optional()?;
    let destination_name = if let Some(existing) = existing {
        existing
    } else {
        let chosen = choose_move_file_name(transaction, &destination_id.to_string(), &source_name)?;
        transaction.execute(include_str!("sql/placements/transfer_book_placement_record_insert.sql"), rusqlite::params![destination_id.to_string(), content_hash.as_str(), chosen, i32::from(downloaded)])?;
        super::assets::copy_publication_baseline(transaction, source_id, destination_id, content_hash)?;
        chosen
    };
    if remove_source {
        transaction.execute(include_str!("sql/placements/remove_book_placement_record_update.sql"), rusqlite::params![source_id.to_string(), content_hash.as_str(), unix_millis()?])?;
    }
    Ok(destination_name)
}

pub fn remove_book_placement_record(transaction: &Transaction, content_hash: &ContentHash, source_id: &DirId) -> Result<bool, rusqlite::Error> {
    let exists: bool = transaction.query_row(include_str!("sql/placements/remove_book_placement_record_select.sql"), rusqlite::params![source_id.to_string(), content_hash.as_str()], |row| row.get(0))?;
    if !exists {
        return Err(invalid_text_column(0, "book is not present in this folder"));
    }
    let orphaned: bool = transaction.query_row(include_str!("sql/placements/remove_book_placement_record_select_2.sql"), rusqlite::params![content_hash.as_str(), source_id.to_string()], |row| row.get(0))?;
    let now = unix_millis()?;
    transaction.execute(include_str!("sql/placements/remove_book_placement_record_update.sql"), rusqlite::params![source_id.to_string(), content_hash.as_str(), now])?;
    if orphaned {
        transaction.execute(include_str!("sql/placements/trash_directory_record_update.sql"), rusqlite::params![content_hash.as_str(), now, source_id.to_string()])?;
    }
    Ok(orphaned)
}

pub fn library_trash(conn: &Connection) -> Result<LibraryTrashView, rusqlite::Error> {
    let mut book_statement = conn.prepare(include_str!("sql/placements/library_trash_select.sql"))?;
    let books = book_statement.query_map([], |row| Ok(TrashedBook { content_hash: ContentHash::new(&row.get::<_, String>(0)?), title: row.get(1)?, format: row.get(2)?, deleted_at: row.get(3)? }))?.collect::<Result<Vec<_>, _>>()?;
    let mut folder_statement = conn.prepare(include_str!("sql/placements/library_trash_select_2.sql"))?;
    let folders = folder_statement.query_map([], |row| Ok(TrashedFolder { id: parse_uuid_column(row.get::<_, String>(0)?, 0)?, name: row.get(1)?, deleted_at: row.get(2)? }))?.collect::<Result<Vec<_>, _>>()?;
    Ok(LibraryTrashView { books, folders })
}

pub fn trash_book_record(tx: &Transaction, content_hash: &ContentHash) -> Result<(), rusqlite::Error> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let placements = tx.prepare(include_str!("sql/placements/trash_book_record_select.sql"))?.query_map([content_hash.as_str()], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?.collect::<Result<Vec<_>, _>>()?;
        for (dir, name) in &placements {
            queue_file_work(tx, "trash", content_hash, &placement_path(tx, &parse_uuid_column(dir.clone(), 0)?, name)?)?;
        }
    }
    let now = unix_millis()?;
    let changed = tx.execute(include_str!("sql/placements/trash_book_record_update.sql"), rusqlite::params![content_hash.as_str(), now])?;
    if changed == 0 {
        return Err(invalid_text_column(0, "book does not exist or is already in Trash"));
    }
    tx.execute(include_str!("sql/placements/trash_book_record_update_2.sql"), rusqlite::params![content_hash.as_str(), now])?;
    Ok(())
}

/// Restores the most recently removed placements, walking each original ancestry.
/// The host may reject a directory that exists in metadata but not on disk.
pub fn restore_book_record_with_parent(tx: &Transaction, content_hash: &ContentHash, mut available: impl FnMut(&str) -> bool) -> Result<(), rusqlite::Error> {
    let trashed: bool = tx.query_row(include_str!("sql/placements/restore_book_record_with_parent_select.sql"), [content_hash.as_str()], |row| row.get(0))?;
    if !trashed {
        return Err(invalid_text_column(0, "book is not in Trash"));
    }
    let mut placements =
        tx.prepare(include_str!("sql/placements/restore_book_record_with_parent_select_2.sql"))?.query_map([content_hash.as_str()], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?.collect::<Result<Vec<_>, _>>()?;
    let origin: Option<String> = tx.query_row(include_str!("sql/placements/restore_book_record_with_parent_select_3.sql"), [content_hash.as_str()], |row| row.get(0))?;
    if let Some(origin) = origin {
        if placements.iter().any(|(dir, _)| *dir == origin) {
            placements.retain(|(dir, _)| *dir == origin);
        }
    }
    if placements.is_empty() {
        return Err(invalid_text_column(0, "book has no recoverable placement"));
    }
    tx.execute(include_str!("sql/placements/restore_book_record_with_parent_update.sql"), [content_hash.as_str()])?;
    for (original, name) in placements {
        let mut target = parse_uuid_column(original, 0)?;
        let mut visited = HashSet::new();
        while target != sync_common::ROOT_DIR_ID {
            if !visited.insert(target) {
                target = sync_common::ROOT_DIR_ID;
                break;
            }
            let directory: Option<(String, bool)> = tx.query_row(include_str!("sql/placements/restore_book_record_with_parent_select_4.sql"), [target.to_string()], |row| Ok((row.get(0)?, row.get(1)?))).optional()?;
            let Some((parent, live)) = directory else {
                target = sync_common::ROOT_DIR_ID;
                break;
            };
            let path = directory_relative_path(tx, &target)?.map(|p| p.as_str().to_owned()).unwrap_or_default();
            if live && available(&path) {
                break;
            }
            target = parse_uuid_column(parent, 0)?;
        }
        let existing = live_file_name(tx, &target, content_hash).optional()?;
        let chosen = if let Some(existing) = existing {
            existing
        } else {
            let occupied = occupied_file_names(&tx, &target.to_string())?;
            library_model::unique_file_name(&name, occupied.iter().map(String::as_str))
        };
        tx.execute(include_str!("sql/placements/restore_book_record_with_parent_insert.sql"), rusqlite::params![target.to_string(), content_hash.as_str(), chosen])?;
        queue_file_work(tx, "restore", content_hash, &placement_path(tx, &target, &chosen)?)?;
    }
    add_asset_request(tx, BlobKind::Book, content_hash, TransferOrigin::UserInitiated)?;
    Ok(())
}

/// Permanently removes the disposable state for a book that is in Trash while
/// retaining its synchronized identity and Purged lifecycle tombstone.
///
/// Filesystem-backed callers remove projected files separately; browser callers
/// similarly remove their OPFS publication after this transaction commits.
pub fn purge_book_record(tx: &Transaction, content_hash: &ContentHash) -> Result<(), rusqlite::Error> {
    tx.execute(include_str!("sql/placements/purge_book_record_insert.sql"), [content_hash.as_str()])?;
    let book_row_id = tx.query_row(include_str!("sql/placements/purge_book_record_select.sql"), [content_hash.as_str()], |row| row.get::<_, i64>(0)).optional()?.ok_or_else(|| invalid_text_column(0, "book is not in Trash"))?;
    let now = unix_millis()?;

    // Publish placement tombstones before removing their disposable local
    // rows. Other replicas use these before the Purged lifecycle arrives.
    tx.execute(include_str!("sql/placements/purge_book_record_update.sql"), rusqlite::params![book_row_id, now])?;
    for sql in [
        include_str!("sql/purge_book_publisher.sql"),
        include_str!("sql/purge_book_identifier.sql"),
        include_str!("sql/purge_book_language.sql"),
        include_str!("sql/purge_book_contributor.sql"),
        include_str!("sql/purge_book_subject_assignment_evidence.sql"),
        include_str!("sql/purge_book_subject_assignment.sql"),
        include_str!("sql/purge_book_subject.sql"),
        include_str!("sql/purge_book_external_entity.sql"),
        include_str!("sql/purge_external_metadata_attempt.sql"),
        include_str!("sql/purge_book_collection.sql"),
        include_str!("sql/purge_book_publication_date.sql"),
        include_str!("sql/purge_book_publication_value.sql"),
        include_str!("sql/purge_catalog_publication_book.sql"),
        include_str!("sql/purge_annotation.sql"),
    ] {
        tx.execute(sql, [book_row_id])?;
    }
    crate::file_work::cancel_book_restore(tx, content_hash)?;
    tx.execute(include_str!("sql/placements/purge_book_record_delete_2.sql"), [content_hash.as_str()])?;
    tx.execute(include_str!("sql/placements/purge_book_record_delete_3.sql"), [book_row_id])?;
    tx.execute(include_str!("sql/placements/purge_book_record_delete_4.sql"), [content_hash.as_str()])?;
    tx.execute(include_str!("sql/placements/purge_book_record_delete_5.sql"), [content_hash.as_str()])?;
    tx.execute(include_str!("sql/placements/purge_book_record_delete_6.sql"), [content_hash.as_str()])?;
    tx.execute(include_str!("sql/placements/purge_book_record_update_2.sql"), rusqlite::params![book_row_id, now])?;
    tx.execute(include_str!("sql/placements/purge_book_record_delete_7.sql"), [book_row_id])?;
    Ok(())
}

pub fn trash_directory_record(tx: &Transaction, directory_id: &DirId) -> Result<(), rusqlite::Error> {
    if *directory_id == sync_common::ROOT_DIR_ID {
        return Err(invalid_text_column(0, "the library root cannot be moved to Trash"));
    }
    if !directory_exists(tx, &directory_id.to_string())? {
        return Err(invalid_text_column(0, "folder does not exist or is already in Trash"));
    }
    let now = unix_millis()?;
    #[cfg(not(target_arch = "wasm32"))]
    {
        let files = tx
            .prepare(include_str!("sql/placements/trash_directory_record_with.sql"))?
            .query_map([directory_id.to_string()], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        for (hash, dir, name) in files {
            queue_file_work(tx, "trash", &ContentHash::new(&hash), &placement_path(tx, &parse_uuid_column(dir, 0)?, &name)?)?;
        }
    }
    let hashes = tx.prepare(include_str!("sql/placements/trash_directory_record_with_2.sql"))?.query_map([directory_id.to_string()], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
    tx.execute(include_str!("sql/placements/trash_directory_record_with_3.sql"), rusqlite::params![directory_id.to_string(), now])?;
    for hash in hashes {
        tx.execute(include_str!("sql/placements/trash_directory_record_update.sql"), rusqlite::params![hash, now, directory_id.to_string()])?;
    }
    tx.execute(include_str!("sql/placements/trash_directory_record_update_2.sql"), rusqlite::params![directory_id.to_string(), now])?;
    Ok(())
}

pub fn restore_directory_record(tx: &Transaction, directory_id: &DirId, requested_parent: Option<&DirId>) -> Result<String, rusqlite::Error> {
    let (stored_parent, name): (String, String) =
        tx.query_row(include_str!("sql/placements/restore_directory_record_select.sql"), [directory_id.to_string()], |row| Ok((row.get(0)?, row.get(1)?))).optional()?.ok_or_else(|| invalid_text_column(0, "folder is not in Trash"))?;
    let stored_parent = parse_uuid_column(stored_parent, 0)?;
    let requested_parent = requested_parent.copied().unwrap_or(stored_parent);
    let parent = if directory_exists(tx, &requested_parent.to_string())? { requested_parent } else { sync_common::ROOT_DIR_ID };
    let occupied =
        tx.prepare(include_str!("sql/placements/choose_move_folder_name_select_2.sql"))?.query_map(rusqlite::params![parent.to_string(), directory_id.to_string()], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
    let chosen = library_model::unique_folder_name(&name, occupied.iter().map(String::as_str));
    upsert_dir(tx, directory_id, &parent, &chosen)?;
    queue_directory_work(tx, directory_id)?;
    let restored_files = tx
        .prepare(include_str!("sql/placements/restore_directory_record_select_3.sql"))?
        .query_map([directory_id.to_string()], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    tx.execute(include_str!("sql/placements/restore_directory_record_update.sql"), [directory_id.to_string()])?;
    tx.execute(include_str!("sql/placements/restore_directory_record_update_2.sql"), [directory_id.to_string()])?;
    for (hash, dir, name) in restored_files {
        let hash = ContentHash::new(&hash);
        queue_file_work(tx, "restore", &hash, &placement_path(tx, &parse_uuid_column(dir, 0)?, &name)?)?;
        add_asset_request(tx, BlobKind::Book, &hash, TransferOrigin::UserInitiated)?;
    }
    Ok(chosen)
}

pub fn purge_directory_record(tx: &Transaction, directory_id: &DirId) -> Result<(), rusqlite::Error> {
    let changed = tx.execute(include_str!("sql/placements/purge_directory_record_with.sql"), rusqlite::params![directory_id.to_string(), unix_millis()?])?;
    if changed == 0 {
        return Err(invalid_text_column(0, "folder is not in Trash"));
    }
    Ok(())
}

/// Permanently purges every current Trash entry in one transaction. Folder
/// purging deliberately does not imply book purging, so both collections are
/// processed explicitly.
pub fn empty_trash_records(tx: &Transaction) -> Result<(), rusqlite::Error> {
    tx.execute(include_str!("sql/placements/empty_trash_records_with.sql"), [unix_millis()?])?;
    let books = tx.prepare(include_str!("sql/placements/empty_trash_records_select.sql"))?.query_map([], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
    for hash in books {
        purge_book_record(tx, &ContentHash::new(&hash))?;
    }
    Ok(())
}

#[cfg(any(test, feature = "scanner"))]
pub use super::scanner::{BookDir, book_has_live_placement, get_book_dirs_by_local_fingerprint, get_tombstoned_book_file_keys, insert_book, mark_known_book_dirs_seen};
pub use super::scanner::{insert_imported_book_dir, insert_local_book_dir};

#[cfg(test)]
mod tests {
    use super::*;
    use test_support::fixture_content_hash;

    #[test]
    fn empty_trash_purges_nested_entries_and_preserves_live_siblings() {
        let mut connection = Connection::open_in_memory().unwrap();
        crate::schema::configure_connection(&connection).unwrap();
        crate::schema::init_db(&connection).unwrap();
        let transaction = connection.transaction().unwrap();
        let parent = DirId::new_v4();
        let child = DirId::new_v4();
        let sibling = DirId::new_v4();
        for (id, parent_id, name) in [(parent, sync_common::ROOT_DIR_ID, "Parent"), (child, parent, "Child"), (sibling, sync_common::ROOT_DIR_ID, "Sibling")] {
            transaction.execute(include_str!("sql/placements/empty_trash_purges_nested_entries_and_preserves_live_siblings_insert.sql"), rusqlite::params![id.to_string(), parent_id.to_string(), name]).unwrap();
        }
        trash_directory_record(&transaction, &child).unwrap();
        trash_directory_record(&transaction, &parent).unwrap();
        empty_trash_records(&transaction).unwrap();
        for id in [parent, child] {
            assert!(transaction.query_row(include_str!("sql/placements/empty_trash_purges_nested_entries_and_preserves_live_siblings_select.sql"), [id.to_string()], |row| row.get::<_, bool>(0)).unwrap());
        }
        assert!(directory_exists(&transaction, &sibling.to_string()).unwrap());
        assert!(library_trash(&transaction).unwrap().folders.is_empty());
        empty_trash_records(&transaction).unwrap();
    }

    #[test]
    fn restoring_a_directory_rejects_descendants_without_reviving_it() {
        let mut connection = Connection::open_in_memory().unwrap();
        crate::schema::configure_connection(&connection).unwrap();
        crate::schema::init_db(&connection).unwrap();
        let transaction = connection.transaction().unwrap();
        let parent = DirId::new_v4();
        let child = DirId::new_v4();
        transaction.execute(include_str!("sql/placements/restoring_a_directory_rejects_descendants_without_reviving_it_insert.sql"), rusqlite::params![parent.to_string(), sync_common::ROOT_DIR_ID.to_string()]).unwrap();
        transaction.execute(include_str!("sql/placements/restoring_a_directory_rejects_descendants_without_reviving_it_insert_2.sql"), rusqlite::params![child.to_string(), parent.to_string()]).unwrap();
        assert!(restore_directory_record(&transaction, &parent, Some(&child)).unwrap_err().to_string().contains("cycle"));
        assert!(!directory_exists(&transaction, &parent.to_string()).unwrap());
        assert_eq!(restore_directory_record(&transaction, &parent, None).unwrap(), "Parent");
        assert!(directory_exists(&transaction, &parent.to_string()).unwrap());
    }

    #[test]
    fn directory_moves_reject_cycles_through_live_and_deleted_ancestors() {
        let mut connection = Connection::open_in_memory().unwrap();
        crate::schema::configure_connection(&connection).unwrap();
        crate::schema::init_db(&connection).unwrap();
        let transaction = connection.transaction().unwrap();
        let parent = DirId::new_v4();
        let middle = DirId::new_v4();
        let child = DirId::new_v4();
        for (id, parent_id, name) in [(parent, sync_common::ROOT_DIR_ID, "Parent"), (middle, parent, "Middle"), (child, middle, "Child")] {
            transaction.execute(include_str!("sql/placements/empty_trash_purges_nested_entries_and_preserves_live_siblings_insert.sql"), rusqlite::params![id.to_string(), parent_id.to_string(), name]).unwrap();
        }
        assert!(move_directory_record(&transaction, &parent, Some(&parent), None).is_err());
        for deleted in [false, true] {
            if deleted {
                transaction.execute(include_str!("sql/placements/directory_moves_reject_cycles_through_live_and_deleted_ancestors_update.sql"), [middle.to_string()]).unwrap();
            }
            let error = move_directory_record(&transaction, &parent, Some(&child), None).unwrap_err();
            assert!(error.to_string().contains("cycle"), "{error}");
            assert_eq!(
                transaction.query_row(include_str!("sql/placements/directory_moves_reject_cycles_through_live_and_deleted_ancestors_select.sql"), [parent.to_string()], |row| row.get::<_, String>(0)).unwrap(),
                sync_common::ROOT_DIR_ID.to_string()
            );
        }
        assert_eq!(move_directory_record(&transaction, &parent, None, Some("Renamed")).unwrap(), "Renamed");
    }

    #[test]
    fn copying_a_folder_duplicates_its_subtree_without_duplicating_books() {
        let mut connection = Connection::open_in_memory().unwrap();
        crate::schema::configure_connection(&connection).unwrap();
        crate::schema::init_db(&connection).unwrap();
        let source = DirId::new_v4();
        let child = DirId::new_v4();
        let trashed = DirId::new_v4();
        let destination = DirId::new_v4();
        let hash = fixture_content_hash(1);
        {
            let transaction = connection.transaction().unwrap();
            for (id, parent, name) in [(source, sync_common::ROOT_DIR_ID, "Fiction"), (child, source, "Novels"), (trashed, source, "Old"), (destination, sync_common::ROOT_DIR_ID, "Archive")] {
                upsert_dir(&transaction, &id, &parent, &name.to_owned()).unwrap();
            }
            transaction.execute(include_str!("sql/placements/copying_a_folder_duplicates_its_subtree_without_duplicating_books_update.sql"), [trashed.to_string()]).unwrap();
            insert_book(&transaction, hash.clone(), BookFormat::Epub).unwrap();
            insert_local_book_dir(&transaction, &child, hash, &"novel.epub".to_owned(), 7, 1).unwrap();
            transaction.commit().unwrap();
        }

        let transaction = connection.transaction().unwrap();
        let copy = copy_directory_record(&transaction, &source, &destination).unwrap();
        assert_eq!(transaction.query_row(include_str!("sql/placements/copying_a_folder_duplicates_its_subtree_without_duplicating_books_select.sql"), [copy.to_string()], |row| row.get::<_, String>(0)).unwrap(), "Fiction");
        // A folder cannot be copied into itself, in either direction.
        assert!(copy_directory_record(&transaction, &source, &child).is_err());
        assert!(copy_directory_record(&transaction, &sync_common::ROOT_DIR_ID, &destination).is_err());
        // A second copy lands beside the first under a free name.
        let second = copy_directory_record(&transaction, &source, &destination).unwrap();
        assert_eq!(transaction.query_row(include_str!("sql/placements/copying_a_folder_duplicates_its_subtree_without_duplicating_books_select.sql"), [second.to_string()], |row| row.get::<_, String>(0)).unwrap(), "Fiction 2");

        let copy_id: String = copy.to_string();
        let copied_child: String = transaction.query_row(include_str!("sql/placements/copying_a_folder_duplicates_its_subtree_without_duplicating_books_select_3.sql"), [copy_id.clone()], |row| row.get(0)).unwrap();
        assert_ne!(copied_child, child.to_string());
        assert_eq!(transaction.query_row(include_str!("sql/placements/choose_move_file_name_select_2.sql"), [copied_child], |row| row.get::<_, String>(0)).unwrap(), "novel.epub");
        // The trashed subfolder is left behind, and the book is one book with
        // several placements rather than several books.
        assert_eq!(transaction.query_row(include_str!("sql/placements/copying_a_folder_duplicates_its_subtree_without_duplicating_books_select_5.sql"), [], |row| row.get::<_, i64>(0)).unwrap(), 0);
        assert_eq!(transaction.query_row(include_str!("sql/placements/copying_a_folder_duplicates_its_subtree_without_duplicating_books_select_6.sql"), [], |row| row.get::<_, i64>(0)).unwrap(), 1);
        assert_eq!(transaction.query_row(include_str!("sql/placements/copying_a_folder_duplicates_its_subtree_without_duplicating_books_select_7.sql"), [], |row| row.get::<_, i64>(0)).unwrap(), 3);
        // Ownership counts follow the copies, so the destination reports the book.
        assert_eq!(transaction.query_row(include_str!("sql/placements/copying_a_folder_duplicates_its_subtree_without_duplicating_books_select_8.sql"), [destination.to_string()], |row| row.get::<_, i64>(0)).unwrap(), 2);
    }

    #[test]
    fn observed_placements_are_marked_in_bulk_and_tombstones_are_preloaded() {
        let mut connection = Connection::open_in_memory().unwrap();
        crate::schema::configure_connection(&connection).unwrap();
        crate::schema::init_db(&connection).unwrap();
        let root = sync_common::ROOT_DIR_ID;
        let first = fixture_content_hash(1);
        let second = fixture_content_hash(2);
        let removed = fixture_content_hash(3);
        {
            let transaction = connection.transaction().unwrap();
            insert_book(&transaction, first.clone(), BookFormat::Epub).unwrap();
            insert_book(&transaction, second.clone(), BookFormat::Epub).unwrap();
            insert_book(&transaction, removed.clone(), BookFormat::Epub).unwrap();
            insert_local_book_dir(&transaction, &root, first, &"first.epub".to_owned(), 11, 1).unwrap();
            insert_local_book_dir(&transaction, &root, second, &"second.epub".to_owned(), 22, 1).unwrap();
            insert_local_book_dir(&transaction, &root, removed, &"removed.epub".to_owned(), 33, 1).unwrap();
            transaction.execute(include_str!("sql/placements/observed_placements_are_marked_in_bulk_and_tombstones_are_preloaded_update.sql"), []).unwrap();
            transaction.commit().unwrap();
        }

        let transaction = connection.transaction().unwrap();
        mark_known_book_dirs_seen(&transaction, 9, &[(root, "first.epub".to_owned(), 11), (root, "second.epub".to_owned(), 22)]).unwrap();
        transaction.commit().unwrap();

        let scans = connection
            .prepare(include_str!("sql/placements/observed_placements_are_marked_in_bulk_and_tombstones_are_preloaded_select.sql"))
            .unwrap()
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(scans, vec![("first.epub".to_owned(), 9), ("removed.epub".to_owned(), 1), ("second.epub".to_owned(), 9)]);
        assert_eq!(get_tombstoned_book_file_keys(&connection).unwrap(), HashSet::from([(root, "removed.epub".to_owned())]));
    }
}

pub(crate) mod queries;
pub use queries::*;
