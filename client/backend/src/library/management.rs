//! User-initiated library organization operations.
//!
//! These methods keep synchronized identities stable while changing their
//! filesystem projection. Local failures are rolled back before returning;
//! destructive files are staged until their SQLite transaction commits.

use super::{database as db, filenames, LibraryBackend, LibraryFolderDestination, LibraryTrashView, TrashedBook, TrashedFolder};
use crate::{BlobKind, ContentHash, DirId};
use rusqlite::{OptionalExtension, Transaction};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn timestamp() -> Result<i64, Box<dyn std::error::Error>> {
    Ok(i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())?)
}

fn unique_file_name(tx: &Transaction<'_>, destination: &DirId, content_hash: &ContentHash, requested: &str, directory: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let requested = filenames::validate_component(requested)?;
    let mut statement = tx.prepare("SELECT file_name FROM book_dir WHERE dir_id=?1 AND deleted_at IS NULL AND book_row_id!=(SELECT row_id FROM book WHERE content_hash=?2)")?;
    let occupied =
        statement.query_map([destination.to_string(), content_hash.to_string()], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?.into_iter().map(|name| library_model::portable_name_key(&name)).collect::<HashSet<_>>();
    let available = |candidate: &str| !occupied.contains(&library_model::portable_name_key(candidate)) && !directory.join(candidate).exists();
    if available(&requested) {
        return Ok(requested);
    }
    for number in 2.. {
        let candidate = library_model::numbered_file_name(&requested, number);
        if available(&candidate) {
            return Ok(candidate);
        }
    }
    unreachable!("numeric filename suffix space is unbounded")
}

#[derive(Debug)]
enum FsChange {
    None,
    Created(PathBuf),
    Moved { from: PathBuf, to: PathBuf },
}

impl FsChange {
    fn rollback(&self) {
        match self {
            Self::None => {}
            Self::Created(path) => {
                if path.is_dir() {
                    let _ = fs::remove_dir_all(path);
                } else {
                    let _ = fs::remove_file(path);
                }
            }
            Self::Moved { from, to } => {
                let _ = fs::rename(to, from);
            }
        }
    }
}

fn stage_path(path: &Path) -> Result<Option<FsChange>, Box<dyn std::error::Error>> {
    if !path.exists() {
        return Ok(None);
    }
    let parent = path.parent().ok_or("managed path has no parent")?;
    let staged = parent.join(format!(".bokheim-purge-{}", uuid::Uuid::new_v4()));
    fs::rename(path, &staged)?;
    Ok(Some(FsChange::Moved { from: path.to_owned(), to: staged }))
}

fn discard_staged(change: FsChange) {
    if let FsChange::Moved { to, .. } = change {
        if to.is_dir() {
            let _ = fs::remove_dir_all(to);
        } else {
            let _ = fs::remove_file(to);
        }
    }
}

fn require_managed_directory(path: &Path, dir_id: &DirId) -> Result<(), Box<dyn std::error::Error>> {
    let marker = fs::read_to_string(path.join(".biblos_uuid"))?;
    if marker.trim() != dir_id.to_string() {
        return Err(format!("directory marker at {} does not identify {dir_id}", path.display()).into());
    }
    Ok(())
}

fn active_directory_path(conn: &rusqlite::Connection, library_root: &Path, dir_id: &DirId) -> Result<Option<PathBuf>, rusqlite::Error> {
    if *dir_id == sync_common::ROOT_DIR_ID {
        return Ok(db::directory_exists(conn, &dir_id.to_string())?.then(|| library_root.to_owned()));
    }
    Ok(db::directory_relative_path(conn, dir_id)?.map(|relative| relative.resolve_under(library_root)))
}

fn unique_directory_name(tx: &Transaction<'_>, parent_id: &DirId, directory_id: &DirId, current_parent: &DirId, current_name: &str, requested: &str, parent_path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let mut statement = tx.prepare("SELECT name FROM dir WHERE parent_id=?1 AND id!=?2 AND deleted_at IS NULL")?;
    let occupied =
        statement.query_map([parent_id.to_string(), directory_id.to_string()], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?.into_iter().map(|name| library_model::portable_name_key(&name)).collect::<HashSet<_>>();
    let current_key = library_model::portable_name_key(current_name);
    let available = |candidate: &str| {
        let key = library_model::portable_name_key(candidate);
        let owns_existing_path = parent_id == current_parent && key == current_key;
        !occupied.contains(&key) && (owns_existing_path || !parent_path.join(candidate).exists())
    };
    if available(requested) {
        return Ok(requested.to_owned());
    }
    for number in 2.. {
        let candidate = library_model::numbered_folder_name(requested, number);
        if available(&candidate) {
            return Ok(candidate);
        }
    }
    unreachable!("numeric folder suffix space is unbounded")
}

impl LibraryBackend {
    pub(super) fn rename_directory_on_disk(&self, directory_id: &DirId, requested_name: &str) -> Result<String, Box<dyn std::error::Error>> {
        self.move_directory_on_disk(directory_id, None, Some(requested_name))
    }

    pub(super) fn move_directory_on_disk(&self, directory_id: &DirId, requested_parent: Option<&DirId>, requested_name: Option<&str>) -> Result<String, Box<dyn std::error::Error>> {
        if *directory_id == sync_common::ROOT_DIR_ID {
            return Err("the library root cannot be renamed or moved".into());
        }
        let requested_name = requested_name.map(filenames::validate_component).transpose()?;
        let mut conn = self.db.get()?;
        let tx = conn.transaction()?;
        let (current_parent, current_name): (String, String) =
            tx.query_row("SELECT parent_id,name FROM dir WHERE id=?1 AND deleted_at IS NULL", [directory_id.to_string()], |row| Ok((row.get(0)?, row.get(1)?))).optional()?.ok_or("folder does not exist or is in Trash")?;
        let current_parent = DirId::parse_str(&current_parent)?;
        let parent_id = requested_parent.copied().unwrap_or(current_parent);
        let parent_is_descendant: bool = tx.query_row(
            "WITH RECURSIVE subtree(id) AS (
                 SELECT id FROM dir WHERE id=?1
                 UNION ALL SELECT child.id FROM dir child JOIN subtree ON child.parent_id=subtree.id WHERE child.id!=child.parent_id
             ) SELECT EXISTS(SELECT 1 FROM subtree WHERE id=?2)",
            rusqlite::params![directory_id.to_string(), parent_id.to_string()],
            |row| row.get(0),
        )?;
        if parent_is_descendant {
            return Err("a folder cannot be moved into itself or one of its descendants".into());
        }
        let parent_path = active_directory_path(&tx, &self.library_root, &parent_id)?.ok_or("destination folder does not exist")?;
        let old_path = active_directory_path(&tx, &self.library_root, directory_id)?.ok_or("folder has no active filesystem path")?;
        require_managed_directory(&old_path, directory_id)?;
        let requested_name = requested_name.as_deref().unwrap_or(&current_name);
        let chosen_name = unique_directory_name(&tx, &parent_id, directory_id, &current_parent, &current_name, requested_name, &parent_path)?;
        if parent_id == current_parent && chosen_name == current_name {
            return Ok(chosen_name);
        }
        db::placements::upsert_dir(&tx, directory_id, &parent_id, &chosen_name)?;
        let new_path = parent_path.join(&chosen_name);
        let case_only = old_path.parent() == new_path.parent() && library_model::portable_name_key(&current_name) == library_model::portable_name_key(&chosen_name) && current_name != chosen_name;
        let mut intermediate: Option<PathBuf> = None;
        if case_only {
            let path = parent_path.join(format!(".bokheim-rename-{}", uuid::Uuid::new_v4()));
            fs::rename(&old_path, &path)?;
            intermediate = Some(path);
        }
        let source = intermediate.as_deref().unwrap_or(&old_path);
        if let Err(error) = fs::rename(source, &new_path) {
            if let Some(intermediate) = intermediate.as_deref() {
                let _ = fs::rename(intermediate, &old_path);
            }
            return Err(error.into());
        }
        if let Err(error) = tx.commit() {
            let _ = fs::rename(&new_path, &old_path);
            return Err(error.into());
        }
        Ok(chosen_name)
    }

    pub(super) fn copy_book_placement_on_disk(&self, content_hash: &ContentHash, source_id: &DirId, destination_id: &DirId) -> Result<String, Box<dyn std::error::Error>> {
        self.transfer_book_placement(content_hash, source_id, destination_id, false)
    }

    pub(super) fn move_book_placement_on_disk(&self, content_hash: &ContentHash, source_id: &DirId, destination_id: &DirId) -> Result<String, Box<dyn std::error::Error>> {
        self.transfer_book_placement(content_hash, source_id, destination_id, true)
    }

    fn transfer_book_placement(&self, content_hash: &ContentHash, source_id: &DirId, destination_id: &DirId, remove_source: bool) -> Result<String, Box<dyn std::error::Error>> {
        if source_id == destination_id {
            return Err("source and destination folders are the same".into());
        }
        let mut conn = self.db.get()?;
        let tx = conn.transaction()?;
        let (source_name, downloaded): (String, bool) = tx
            .query_row(
                "SELECT bd.file_name,bd.is_downloaded FROM book_dir bd JOIN book ON book.row_id=bd.book_row_id WHERE bd.dir_id=?1 AND book.content_hash=?2 AND bd.deleted_at IS NULL AND book.deleted_at IS NULL",
                [source_id.to_string(), content_hash.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or("book is not present in the source folder")?;
        let destination_directory = active_directory_path(&tx, &self.library_root, destination_id)?.ok_or("destination folder does not exist")?;
        let source_path = active_directory_path(&tx, &self.library_root, source_id)?.ok_or("source folder does not exist")?.join(&source_name);
        let existing_destination: Option<String> = tx
            .query_row("SELECT file_name FROM book_dir WHERE dir_id=?1 AND book_row_id=(SELECT row_id FROM book WHERE content_hash=?2) AND deleted_at IS NULL", [destination_id.to_string(), content_hash.to_string()], |row| row.get(0))
            .optional()?;
        if let Some(existing) = existing_destination {
            if !remove_source {
                return Ok(existing);
            }
            let changed = if source_path.is_file() {
                let staging = self.library_root.join(crate::APP_HIDDEN_DIR).join("staging").join(uuid::Uuid::new_v4().to_string());
                fs::create_dir_all(staging.parent().expect("staging path has a parent"))?;
                fs::rename(&source_path, &staging)?;
                FsChange::Moved { from: source_path, to: staging }
            } else {
                FsChange::None
            };
            tx.execute(
                "UPDATE book_dir SET deleted_at=?3,trash_origin_dir_id=NULL WHERE dir_id=?1 AND book_row_id=(SELECT row_id FROM book WHERE content_hash=?2)",
                rusqlite::params![source_id.to_string(), content_hash.as_str(), timestamp()?],
            )?;
            if let Err(error) = tx.commit() {
                changed.rollback();
                return Err(error.into());
            }
            if let FsChange::Moved { to, .. } = changed {
                let _ = fs::remove_file(to);
            }
            return Ok(existing);
        }
        let destination_name = unique_file_name(&tx, destination_id, content_hash, &source_name, &destination_directory)?;
        let destination_path = destination_directory.join(&destination_name);
        let changed = if downloaded && source_path.is_file() {
            if remove_source {
                fs::rename(&source_path, &destination_path)?;
                FsChange::Moved { from: source_path, to: destination_path.clone() }
            } else {
                fs::copy(&source_path, &destination_path)?;
                FsChange::Created(destination_path.clone())
            }
        } else {
            FsChange::None
        };
        let fingerprint = if destination_path.is_file() { db::local_file_fingerprint(&destination_path)?.to_string() } else { String::new() };
        tx.execute(
            "INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded,deleted_at,trash_origin_dir_id) VALUES(?1,(SELECT row_id FROM book WHERE content_hash=?2),?3,?4,?5,NULL,NULL) ON CONFLICT(dir_id,book_row_id) DO UPDATE SET file_name=excluded.file_name,local_hash=excluded.local_hash,is_downloaded=excluded.is_downloaded,deleted_at=NULL,trash_origin_dir_id=NULL",
            rusqlite::params![destination_id.to_string(), content_hash.as_str(), destination_name, fingerprint, i32::from(destination_path.is_file())],
        )?;
        if remove_source {
            tx.execute(
                "UPDATE book_dir SET deleted_at=?3,is_downloaded=0,trash_origin_dir_id=NULL WHERE dir_id=?1 AND book_row_id=(SELECT row_id FROM book WHERE content_hash=?2)",
                rusqlite::params![source_id.to_string(), content_hash.as_str(), timestamp()?],
            )?;
        }
        if let Err(error) = tx.commit() {
            changed.rollback();
            return Err(error.into());
        }
        Ok(destination_name)
    }

    pub(super) fn remove_book_placement_on_disk(&self, content_hash: &ContentHash, source_id: &DirId) -> Result<bool, Box<dyn std::error::Error>> {
        let mut conn = self.db.get()?;
        let tx = conn.transaction()?;
        let (file_name, downloaded): (String, bool) = tx
            .query_row(
                "SELECT bd.file_name,bd.is_downloaded FROM book_dir bd JOIN book ON book.row_id=bd.book_row_id WHERE bd.dir_id=?1 AND book.content_hash=?2 AND bd.deleted_at IS NULL AND book.deleted_at IS NULL",
                [source_id.to_string(), content_hash.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or("book is not present in this folder")?;
        let other_count: i64 =
            tx.query_row("SELECT COUNT(*) FROM book_dir WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1) AND dir_id!=?2 AND deleted_at IS NULL", [content_hash.to_string(), source_id.to_string()], |row| row.get(0))?;
        let source_path = active_directory_path(&tx, &self.library_root, source_id)?.ok_or("source folder does not exist")?.join(&file_name);
        let orphaned = other_count == 0;
        let trash_destination = if orphaned {
            let extension = Path::new(&file_name).extension().and_then(|value| value.to_str()).unwrap_or("book");
            Some(self.library_root.join(crate::APP_HIDDEN_DIR).join("trash/books").join(format!("{content_hash}.{extension}")))
        } else {
            None
        };
        // Even when Trash already has a retained copy, stage this placement
        // until SQLite commits. Deleting it immediately would make a database
        // failure impossible to roll back.
        let destination = trash_destination.as_ref().filter(|path| !path.exists()).cloned().unwrap_or_else(|| self.library_root.join(crate::APP_HIDDEN_DIR).join("staging").join(uuid::Uuid::new_v4().to_string()));
        let changed = if downloaded && source_path.is_file() {
            fs::create_dir_all(destination.parent().expect("managed destination has a parent"))?;
            if destination.exists() {
                fs::remove_file(&source_path)?;
                FsChange::None
            } else {
                fs::rename(&source_path, &destination)?;
                FsChange::Moved { from: source_path, to: destination.clone() }
            }
        } else {
            FsChange::None
        };
        let now = timestamp()?;
        tx.execute(
            "UPDATE book_dir SET deleted_at=?3,is_downloaded=0,trash_origin_dir_id=NULL WHERE dir_id=?1 AND book_row_id=(SELECT row_id FROM book WHERE content_hash=?2)",
            rusqlite::params![source_id.to_string(), content_hash.as_str(), now],
        )?;
        if orphaned {
            tx.execute("UPDATE book SET deleted_at=?2,purged_at=NULL,trash_origin_dir_id=NULL WHERE content_hash=?1", rusqlite::params![content_hash.as_str(), now])?;
            if let Some(trash_destination) = trash_destination.as_ref().filter(|path| path.is_file()) {
                db::upsert_blob_local_ref(&tx, BlobKind::Book, content_hash, trash_destination.to_str().ok_or("Trash path is not UTF-8")?, true)?;
            }
        }
        if let Err(error) = tx.commit() {
            changed.rollback();
            return Err(error.into());
        }
        if let FsChange::Moved { to, .. } = changed {
            if to.starts_with(self.library_root.join(crate::APP_HIDDEN_DIR).join("staging")) {
                let _ = fs::remove_file(to);
            }
        }
        Ok(orphaned)
    }

    pub(super) fn trash_book_on_disk(&self, content_hash: &ContentHash) -> Result<(), Box<dyn std::error::Error>> {
        let conn = self.db.get()?;
        let changed = conn.execute("UPDATE book SET deleted_at=?2,purged_at=NULL,trash_origin_dir_id=NULL WHERE content_hash=?1 AND deleted_at IS NULL", rusqlite::params![content_hash.as_str(), timestamp()?])?;
        if changed == 0 {
            return Err("book does not exist or is already in Trash".into());
        }
        Ok(())
    }

    pub(super) fn restore_book_from_trash(&self, content_hash: &ContentHash) -> Result<(), Box<dyn std::error::Error>> {
        let conn = self.db.get()?;
        let changed = conn.execute("UPDATE book SET deleted_at=NULL,purged_at=NULL,trash_origin_dir_id=NULL WHERE content_hash=?1 AND deleted_at IS NOT NULL AND purged_at IS NULL", [content_hash.as_str()])?;
        if changed == 0 {
            return Err("book is not in Trash".into());
        }
        Ok(())
    }

    pub(super) fn load_trash(&self) -> Result<LibraryTrashView, Box<dyn std::error::Error>> {
        let conn = self.db.get()?;
        let mut book_statement = conn.prepare(
            "SELECT content_hash,COALESCE(NULLIF(title,''),content_hash),format,deleted_at
             FROM book WHERE deleted_at IS NOT NULL AND purged_at IS NULL
             ORDER BY deleted_at DESC,portable_name_key(COALESCE(title,content_hash)),content_hash",
        )?;
        let books = book_statement.query_map([], |row| Ok(TrashedBook { content_hash: ContentHash::new(&row.get::<_, String>(0)?), title: row.get(1)?, format: row.get(2)?, deleted_at: row.get(3)? }))?.collect::<Result<Vec<_>, _>>()?;
        let mut folder_statement = conn.prepare(
            "SELECT id,name,deleted_at FROM dir
             WHERE deleted_at IS NOT NULL AND purged_at IS NULL AND intent_lifecycle=1
             ORDER BY deleted_at DESC,name_key,id",
        )?;
        let folders = folder_statement
            .query_map([], |row| {
                let raw_id: String = row.get(0)?;
                let id = DirId::parse_str(&raw_id).map_err(|error| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error)))?;
                Ok(TrashedFolder { id, name: row.get(1)?, deleted_at: row.get(2)? })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(LibraryTrashView { books, folders })
    }

    pub(super) fn load_folder_destinations(&self) -> Result<Vec<LibraryFolderDestination>, Box<dyn std::error::Error>> {
        let conn = self.db.get()?;
        let mut statement = conn.prepare(
            "WITH RECURSIVE tree(id,parent_id,label,path) AS (
                 SELECT id,NULL,'Library','' FROM dir WHERE id='00000000-0000-0000-0000-000000000000' AND deleted_at IS NULL
                 UNION ALL
                 SELECT child.id,child.parent_id,child.name,CASE WHEN tree.path='' THEN child.name ELSE tree.path||' / '||child.name END
                 FROM dir child JOIN tree ON child.parent_id=tree.id
                 WHERE child.id!=child.parent_id AND child.deleted_at IS NULL
             ) SELECT id,parent_id,label,path FROM tree ORDER BY portable_name_key(path),id",
        )?;
        let destinations = statement
            .query_map([], |row| {
                let raw_id: String = row.get(0)?;
                let id = DirId::parse_str(&raw_id).map_err(|error| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error)))?;
                let parent_id = row.get::<_, Option<String>>(1)?.map(|raw| DirId::parse_str(&raw).map_err(|error| rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(error)))).transpose()?;
                let label = row.get(2)?;
                let path: String = row.get(3)?;
                Ok(LibraryFolderDestination { id, parent_id, label, path: if path.is_empty() { "Library".to_owned() } else { path } })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(destinations)
    }

    pub(super) fn purge_book_from_trash(&self, content_hash: &ContentHash) -> Result<(), Box<dyn std::error::Error>> {
        let mut conn = self.db.get()?;
        let tx = conn.transaction()?;
        let (book_row_id, format): (i64, String) =
            tx.query_row("SELECT row_id,format FROM book WHERE content_hash=?1 AND deleted_at IS NOT NULL AND purged_at IS NULL", [content_hash.as_str()], |row| Ok((row.get(0)?, row.get(1)?))).optional()?.ok_or("book is not in Trash")?;
        let mut paths = HashSet::<PathBuf>::new();
        let mut path_statement = tx.prepare("SELECT path FROM blob_local_ref WHERE hash=?1 AND exists_flag=1")?;
        for path in path_statement.query_map([content_hash.as_str()], |row| row.get::<_, String>(0))? {
            paths.insert(PathBuf::from(path?));
        }
        drop(path_statement);
        if !format.is_empty() {
            paths.insert(self.library_root.join(crate::APP_HIDDEN_DIR).join("trash/books").join(format!("{content_hash}.{format}")));
        }
        paths.insert(self.context.thumbnails_dir()?.join(format!("{content_hash}.jpg")));
        paths.insert(crate::sync::transfer::thumbnail_path_from_content_hash(&self.library_root, content_hash));

        let mut changes = Vec::new();
        for path in paths {
            if let Some(change) = stage_path(&path)? {
                changes.push(change);
            }
        }
        let now = timestamp()?;
        let cleanup = (|| -> Result<(), Box<dyn std::error::Error>> {
            // Publish placement tombstones before removing their disposable
            // local rows. Other replicas use these to remove every projected
            // file before the following Purged lifecycle arrives.
            tx.execute("UPDATE book_dir SET deleted_at=?2,trash_origin_dir_id=NULL WHERE book_row_id=?1 AND deleted_at IS NULL", rusqlite::params![book_row_id, now])?;
            for table in [
                "book_publisher",
                "book_identifier",
                "book_language",
                "book_contributor",
                "book_subject_assignment_evidence",
                "book_subject_assignment",
                "book_subject",
                "book_external_entity",
                "external_metadata_attempt",
                "book_collection",
                "book_publication_date",
                "book_publication_value",
                "catalog_publication_book",
                "annotation",
                "bookmark",
            ] {
                tx.execute(&format!("DELETE FROM {table} WHERE book_row_id=?1"), [book_row_id])?;
            }
            tx.execute("DELETE FROM book_link WHERE source_book_row_id=?1 OR target_book_row_id=?1", [book_row_id])?;
            tx.execute("DELETE FROM book_dir WHERE book_row_id=?1", [book_row_id])?;
            tx.execute("DELETE FROM audiobook_metadata WHERE content_hash=?1", [content_hash.as_str()])?;
            tx.execute("DELETE FROM blob_local_ref WHERE hash=?1", [content_hash.as_str()])?;
            tx.execute("DELETE FROM download_requests WHERE target_key=?1", [content_hash.as_str()])?;
            tx.execute("DELETE FROM remote_asset WHERE hash=?1", [content_hash.as_str()])?;
            tx.execute(
                "UPDATE book SET title=NULL,description='',description_scanned=0,publication_metadata=X'',format='',hidden_at=NULL,read_pos=NULL,read_progress=0,added_at=NULL,deleted_at=?2,purged_at=?2,trash_origin_dir_id=NULL WHERE row_id=?1",
                rusqlite::params![book_row_id, now],
            )?;
            Ok(())
        })();
        if let Err(error) = cleanup.and_then(|()| tx.commit().map_err(Into::into)) {
            for change in changes.iter().rev() {
                change.rollback();
            }
            return Err(error);
        }
        for change in changes {
            discard_staged(change);
        }
        Ok(())
    }

    pub(super) fn purge_directory_from_trash(&self, directory_id: &DirId) -> Result<(), Box<dyn std::error::Error>> {
        if *directory_id == sync_common::ROOT_DIR_ID {
            return Err("the library root cannot be purged".into());
        }
        let mut conn = self.db.get()?;
        let tx = conn.transaction()?;
        let present: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM dir WHERE id=?1 AND deleted_at IS NOT NULL AND purged_at IS NULL AND intent_lifecycle=1)", [directory_id.to_string()], |row| row.get(0))?;
        if !present {
            return Err("folder is not in Trash".into());
        }
        let trash_path = self.library_root.join(crate::APP_HIDDEN_DIR).join("trash/folders").join(directory_id.to_string());
        let change = stage_path(&trash_path)?;
        let now = timestamp()?;
        let result = tx.execute(
            "WITH RECURSIVE subtree(id) AS (
                 SELECT id FROM dir WHERE id=?1
                 UNION ALL SELECT child.id FROM dir child JOIN subtree ON child.parent_id=subtree.id WHERE child.id!=child.parent_id
             )
             UPDATE dir SET deleted_at=?2,purged_at=?2,intent_lifecycle=2 WHERE id IN (SELECT id FROM subtree)",
            rusqlite::params![directory_id.to_string(), now],
        );
        if let Err(error) = result.and_then(|_| tx.commit()) {
            if let Some(change) = &change {
                change.rollback();
            }
            return Err(error.into());
        }
        if let Some(change) = change {
            discard_staged(change);
        }
        Ok(())
    }

    pub(super) fn empty_trash_on_disk(&self) -> Result<(), Box<dyn std::error::Error>> {
        let trash = self.load_trash()?;
        for folder in trash.folders {
            self.purge_directory_from_trash(&folder.id)?;
        }
        for book in trash.books {
            // A folder purge deliberately does not purge its books.
            self.purge_book_from_trash(&book.content_hash)?;
        }
        Ok(())
    }

    pub(super) fn trash_directory_on_disk(&self, directory_id: &DirId) -> Result<(), Box<dyn std::error::Error>> {
        if *directory_id == sync_common::ROOT_DIR_ID {
            return Err("the library root cannot be moved to Trash".into());
        }
        let mut conn = self.db.get()?;
        let tx = conn.transaction()?;
        let source = active_directory_path(&tx, &self.library_root, directory_id)?.ok_or("folder does not exist or is already in Trash")?;
        let trash_root = self.library_root.join(crate::APP_HIDDEN_DIR).join("trash/folders");
        let trashed = trash_root.join(directory_id.to_string());
        if trashed.exists() {
            return Err(format!("managed Trash destination already exists: {}", trashed.display()).into());
        }
        fs::create_dir_all(&trash_root)?;

        #[derive(Clone)]
        struct Placement {
            dir_id: String,
            content_hash: ContentHash,
            relative_file: PathBuf,
            downloaded: bool,
            book_present: bool,
        }
        let mut statement = tx.prepare(
            "WITH RECURSIVE subtree(id,relative_path) AS (
                 SELECT id,'' FROM dir WHERE id=?1
                 UNION ALL
                 SELECT child.id,CASE WHEN subtree.relative_path='' THEN child.name ELSE subtree.relative_path||'/'||child.name END
                 FROM dir child JOIN subtree ON child.parent_id=subtree.id WHERE child.id!=child.parent_id
             )
             SELECT bd.dir_id,book.content_hash,
                    CASE WHEN subtree.relative_path='' THEN bd.file_name ELSE subtree.relative_path||'/'||bd.file_name END,
                    bd.is_downloaded,book.deleted_at IS NULL
             FROM book_dir bd JOIN subtree ON subtree.id=bd.dir_id JOIN book ON book.row_id=bd.book_row_id
             WHERE bd.deleted_at IS NULL",
        )?;
        let placements = statement
            .query_map([directory_id.to_string()], |row| {
                Ok(Placement { dir_id: row.get(0)?, content_hash: ContentHash::new(&row.get::<_, String>(1)?), relative_file: PathBuf::from(row.get::<_, String>(2)?), downloaded: row.get(3)?, book_present: row.get(4)? })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        let directory_change = if source.exists() {
            require_managed_directory(&source, directory_id)?;
            fs::rename(&source, &trashed)?;
            FsChange::Moved { from: source, to: trashed.clone() }
        } else {
            // A missing projection is local availability state, not deletion
            // intent. The explicit Trash action must still be able to record
            // that intent, so create an empty managed placeholder that keeps
            // Restore reversible without pretending the missing files exist.
            fs::create_dir(&trashed)?;
            if let Err(error) = fs::write(trashed.join(".biblos_uuid"), directory_id.to_string()) {
                let _ = fs::remove_dir_all(&trashed);
                return Err(error.into());
            }
            FsChange::Created(trashed.clone())
        };
        let mut changes = vec![directory_change];
        let now = timestamp()?;
        tx.execute("UPDATE dir SET deleted_at=?2,purged_at=NULL,intent_lifecycle=1 WHERE id=?1", rusqlite::params![directory_id.to_string(), now])?;
        let mut orphaned = HashSet::new();
        for placement in &placements {
            tx.execute(
                "UPDATE book_dir SET deleted_at=?3,is_downloaded=0,trash_origin_dir_id=?4 WHERE dir_id=?1 AND book_row_id=(SELECT row_id FROM book WHERE content_hash=?2)",
                rusqlite::params![placement.dir_id, placement.content_hash.as_str(), now, directory_id.to_string()],
            )?;
            if placement.book_present {
                let outside: i64 =
                    tx.query_row("SELECT COUNT(*) FROM book_dir JOIN book ON book.row_id=book_dir.book_row_id WHERE book.content_hash=?1 AND book_dir.deleted_at IS NULL", [placement.content_hash.as_str()], |row| row.get(0))?;
                if outside == 0 {
                    orphaned.insert(placement.content_hash.clone());
                }
            }
        }
        for content_hash in &orphaned {
            tx.execute("UPDATE book SET deleted_at=?2,purged_at=NULL,trash_origin_dir_id=?3 WHERE content_hash=?1 AND deleted_at IS NULL", rusqlite::params![content_hash.as_str(), now, directory_id.to_string()])?;
            let candidates = placements.iter().filter(|placement| &placement.content_hash == content_hash && placement.downloaded).collect::<Vec<_>>();
            let extension = candidates.iter().find_map(|placement| placement.relative_file.extension().and_then(|value| value.to_str())).unwrap_or("book");
            let book_trash = self.library_root.join(crate::APP_HIDDEN_DIR).join("trash/books").join(format!("{content_hash}.{extension}"));
            fs::create_dir_all(book_trash.parent().expect("book Trash path has a parent"))?;
            let mut retained = book_trash.exists();
            for placement in candidates {
                let path = trashed.join(&placement.relative_file);
                if !path.is_file() {
                    continue;
                }
                if !retained {
                    fs::rename(&path, &book_trash)?;
                    changes.push(FsChange::Moved { from: path, to: book_trash.clone() });
                    retained = true;
                } else {
                    let staging = self.library_root.join(crate::APP_HIDDEN_DIR).join("staging").join(uuid::Uuid::new_v4().to_string());
                    fs::create_dir_all(staging.parent().expect("staging path has a parent"))?;
                    fs::rename(&path, &staging)?;
                    changes.push(FsChange::Moved { from: path, to: staging });
                }
            }
            if retained {
                db::upsert_blob_local_ref(&tx, BlobKind::Book, content_hash, book_trash.to_str().ok_or("Trash path is not UTF-8")?, true)?;
            }
        }
        if let Err(error) = tx.commit() {
            for change in changes.iter().rev() {
                change.rollback();
            }
            return Err(error.into());
        }
        for change in changes {
            if let FsChange::Moved { to, .. } = change {
                if to.starts_with(self.library_root.join(crate::APP_HIDDEN_DIR).join("staging")) {
                    let _ = fs::remove_file(to);
                }
            }
        }
        Ok(())
    }

    pub(super) fn restore_directory_from_trash(&self, directory_id: &DirId, requested_parent: Option<&DirId>) -> Result<String, Box<dyn std::error::Error>> {
        if *directory_id == sync_common::ROOT_DIR_ID {
            return Err("the library root is never in Trash".into());
        }
        let mut conn = self.db.get()?;
        let tx = conn.transaction()?;
        let (stored_parent, stored_name): (String, String) =
            tx.query_row("SELECT parent_id,name FROM dir WHERE id=?1 AND deleted_at IS NOT NULL AND purged_at IS NULL", [directory_id.to_string()], |row| Ok((row.get(0)?, row.get(1)?))).optional()?.ok_or("folder is not in Trash")?;
        let stored_parent = DirId::parse_str(&stored_parent)?;
        let parent_id = match requested_parent.copied().unwrap_or(stored_parent) {
            candidate if active_directory_path(&tx, &self.library_root, &candidate)?.is_some() => candidate,
            _ => sync_common::ROOT_DIR_ID,
        };
        let parent_path = active_directory_path(&tx, &self.library_root, &parent_id)?.ok_or("restore destination does not exist")?;
        let chosen_name = unique_directory_name(&tx, &parent_id, directory_id, &stored_parent, &stored_name, &stored_name, &parent_path)?;
        let trashed = self.library_root.join(crate::APP_HIDDEN_DIR).join("trash/folders").join(directory_id.to_string());
        require_managed_directory(&trashed, directory_id)?;
        let restored = parent_path.join(&chosen_name);
        fs::rename(&trashed, &restored)?;
        let mut changes = vec![FsChange::Moved { from: trashed, to: restored.clone() }];
        tx.execute("UPDATE dir SET parent_id=?2,name=?3,intent_parent_id=?2,intent_name=?3,deleted_at=NULL,purged_at=NULL,intent_lifecycle=0 WHERE id=?1", rusqlite::params![directory_id.to_string(), parent_id.to_string(), chosen_name])?;
        let subtree_ids = tx
            .prepare(
                "WITH RECURSIVE subtree(id) AS (
                     SELECT id FROM dir WHERE id=?1
                     UNION ALL SELECT child.id FROM dir child JOIN subtree ON child.parent_id=subtree.id WHERE child.id!=child.parent_id
                 )
                 SELECT id FROM subtree",
            )?
            .query_map([directory_id.to_string()], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        for subtree_id in subtree_ids {
            db::materialize_dir_with_markers(&tx, &self.library_root, &subtree_id)?.ok_or("restored folder has no projected path")?;
        }
        let mut statement = tx.prepare(
            "WITH RECURSIVE subtree(id,relative_path) AS (
                 SELECT id,'' FROM dir WHERE id=?1
                 UNION ALL SELECT child.id,CASE WHEN subtree.relative_path='' THEN child.name ELSE subtree.relative_path||'/'||child.name END
                 FROM dir child JOIN subtree ON child.parent_id=subtree.id WHERE child.id!=child.parent_id
             )
             SELECT bd.dir_id,book.content_hash,bd.file_name,subtree.relative_path,book.format,book.trash_origin_dir_id
             FROM book_dir bd JOIN subtree ON subtree.id=bd.dir_id JOIN book ON book.row_id=bd.book_row_id
             WHERE bd.deleted_at IS NOT NULL AND bd.trash_origin_dir_id=?2",
        )?;
        let rows = statement
            .query_map([directory_id.to_string(), directory_id.to_string()], |row| {
                Ok((row.get::<_, String>(0)?, ContentHash::new(&row.get::<_, String>(1)?), row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?, row.get::<_, Option<String>>(5)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        for (dir_id, content_hash, file_name, relative_dir, format, origin) in rows {
            tx.execute("UPDATE book_dir SET deleted_at=NULL,trash_origin_dir_id=NULL WHERE dir_id=?1 AND book_row_id=(SELECT row_id FROM book WHERE content_hash=?2)", rusqlite::params![dir_id, content_hash.as_str()])?;
            if origin.as_deref() == Some(directory_id.to_string().as_str()) {
                tx.execute("UPDATE book SET deleted_at=NULL,purged_at=NULL,trash_origin_dir_id=NULL WHERE content_hash=?1 AND purged_at IS NULL", [content_hash.as_str()])?;
                let destination = if relative_dir.is_empty() { restored.join(&file_name) } else { restored.join(&relative_dir).join(&file_name) };
                if !destination.exists() {
                    let extension = if format.is_empty() { Path::new(&file_name).extension().and_then(|value| value.to_str()).unwrap_or("book") } else { format.as_str() };
                    let source = self.library_root.join(crate::APP_HIDDEN_DIR).join("trash/books").join(format!("{content_hash}.{extension}"));
                    if source.is_file() {
                        fs::create_dir_all(destination.parent().expect("restored placement has a parent"))?;
                        fs::rename(&source, &destination)?;
                        changes.push(FsChange::Moved { from: source, to: destination });
                    }
                }
            }
        }
        if let Err(error) = tx.commit() {
            for change in changes.iter().rev() {
                change.rollback();
            }
            return Err(error.into());
        }
        Ok(chosen_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{AppBackend, AppDataLocation, BackendContext};
    use test_support::{fixture_content_hash, fixture_library_id};

    fn open_library(root: &Path, app_data: &Path) -> LibraryBackend {
        fs::create_dir_all(root).unwrap();
        fs::create_dir_all(app_data).unwrap();
        let app = AppBackend::initialize(AppDataLocation::PlatformLocal(app_data.to_owned())).unwrap();
        let context = BackendContext::initialize(AppDataLocation::PlatformLocal(app_data.to_owned())).unwrap();
        let (library, events) = LibraryBackend::open(&app, &context, root.to_str().unwrap(), fixture_library_id("management")).unwrap();
        while !matches!(events.recv_timeout(std::time::Duration::from_secs(5)).unwrap(), crate::platform::scanner::LibraryEvent::ScanComplete) {}
        library
    }

    #[test]
    fn management_operations_preserve_identity_and_finish_in_a_purged_tombstone() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("library");
        let library = open_library(&root, &temp.path().join("app"));
        let politics = library.create_directory(&sync_common::ROOT_DIR_ID, "Politics").unwrap();
        let second = library.create_directory(&sync_common::ROOT_DIR_ID, "Second").unwrap();
        assert_eq!(library.rename_directory(&second.id, "politics").unwrap(), "politics 2");

        let source = root.join("Politics/book.epub");
        fs::write(&source, b"managed book").unwrap();
        let content_hash = fixture_content_hash(41);
        {
            let conn = library.db.get().unwrap();
            conn.execute("INSERT INTO book(content_hash,title,format,added_at) VALUES(?1,'Managed','epub',1)", [content_hash.as_str()]).unwrap();
            conn.execute(
                "INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded) VALUES(?1,(SELECT row_id FROM book WHERE content_hash=?2),'book.epub',?3,1)",
                rusqlite::params![politics.id.to_string(), content_hash.as_str(), db::local_file_fingerprint(&source).unwrap().to_string()],
            )
            .unwrap();
            db::upsert_blob_local_ref(&conn, BlobKind::Book, &content_hash, source.to_str().unwrap(), true).unwrap();
        }

        let copied_name = library.copy_book_to_directory(&content_hash, &politics.id, &second.id).unwrap();
        assert_eq!(copied_name, "book.epub");
        assert!(root.join("politics 2/book.epub").is_file());
        assert!(!library.remove_book_from_directory(&content_hash, &second.id).unwrap());
        assert!(source.is_file());

        assert_eq!(library.move_book_to_directory(&content_hash, &politics.id, &second.id).unwrap(), "book.epub");
        assert!(!source.exists());
        assert!(root.join("politics 2/book.epub").is_file());
        assert_eq!(library.move_book_to_directory(&content_hash, &second.id, &politics.id).unwrap(), "book.epub");
        assert!(source.is_file());
        assert!(!root.join("politics 2/book.epub").exists());

        library.move_book_to_trash(&content_hash).unwrap();
        assert_eq!(library.trash().unwrap().books[0].content_hash, content_hash);
        library.restore_book(&content_hash).unwrap();
        assert!(library.trash().unwrap().books.is_empty());
        library.move_book_to_trash(&content_hash).unwrap();
        library.purge_book(&content_hash).unwrap();
        assert!(!source.exists());
        let conn = library.db.get().unwrap();
        let state: (bool, bool) = conn.query_row("SELECT deleted_at IS NOT NULL,purged_at IS NOT NULL FROM book WHERE content_hash=?1", [content_hash.as_str()], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
        assert_eq!(state, (true, true));
        assert!(library.trash().unwrap().books.is_empty());
    }

    #[test]
    fn folder_trash_restores_only_books_orphaned_by_that_folder() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("library");
        let library = open_library(&root, &temp.path().join("app"));
        let folder = library.create_directory(&sync_common::ROOT_DIR_ID, "Archive").unwrap();
        let source = root.join("Archive/orphan.epub");
        fs::write(&source, b"orphaned book").unwrap();
        let content_hash = fixture_content_hash(42);
        {
            let conn = library.db.get().unwrap();
            conn.execute("INSERT INTO book(content_hash,title,format,added_at) VALUES(?1,'Orphan','epub',1)", [content_hash.as_str()]).unwrap();
            conn.execute(
                "INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded) VALUES(?1,(SELECT row_id FROM book WHERE content_hash=?2),'orphan.epub',?3,1)",
                rusqlite::params![folder.id.to_string(), content_hash.as_str(), db::local_file_fingerprint(&source).unwrap().to_string()],
            )
            .unwrap();
        }

        library.move_directory_to_trash(&folder.id).unwrap();
        let trash = library.trash().unwrap();
        assert_eq!(trash.folders[0].id, folder.id);
        assert_eq!(trash.books[0].content_hash, content_hash);
        assert!(root.join(crate::APP_HIDDEN_DIR).join("trash/folders").join(folder.id.to_string()).is_dir());

        assert_eq!(library.restore_directory(&folder.id, None).unwrap(), "Archive");
        assert!(root.join("Archive/orphan.epub").is_file());
        let trash = library.trash().unwrap();
        assert!(trash.folders.is_empty());
        assert!(trash.books.is_empty());
    }

    #[test]
    fn missing_local_folder_can_still_be_moved_to_trash_and_restored() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("library");
        let library = open_library(&root, &temp.path().join("app"));
        let folder = library.create_directory(&sync_common::ROOT_DIR_ID, "Missing").unwrap();
        let child = library.create_directory(&folder.id, "Child").unwrap();
        fs::remove_dir_all(root.join("Missing")).unwrap();

        library.move_directory_to_trash(&folder.id).unwrap();
        let trash = library.trash().unwrap();
        assert_eq!(trash.folders[0].id, folder.id);
        let placeholder = root.join(crate::APP_HIDDEN_DIR).join("trash/folders").join(folder.id.to_string());
        assert_eq!(fs::read_to_string(placeholder.join(".biblos_uuid")).unwrap(), folder.id.to_string());

        assert_eq!(library.restore_directory(&folder.id, None).unwrap(), "Missing");
        assert_eq!(fs::read_to_string(root.join("Missing/.biblos_uuid")).unwrap(), folder.id.to_string());
        assert_eq!(fs::read_to_string(root.join("Missing/Child/.biblos_uuid")).unwrap(), child.id.to_string());
    }
}
