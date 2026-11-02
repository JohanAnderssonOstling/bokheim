//! Durable local evidence changes for an explicit, verified content-root move.
use super::observations::{self, Revision};
use crate::placements::{FolderPaths, stored_book_path};
use crate::shared_sql::SharedSql;
use crate::{Database, DatabaseError};
use content_address::ContentHash;
use library_model::RelativeBookPath;
use rusqlite::OptionalExtension;
use std::collections::{HashMap, HashSet};
use sync_common::{BookId, DirId, LibraryId};

impl Database {
    /// Record before committing the registry root. Opening either root after a
    /// crash then knows whether its verified relocation should be applied.
    pub fn stage_native_root_move(
        &self, source: &str, destination: &str, library: LibraryId, source_identity: &str, files: &[(String, String)], directories: &[(DirId, String)], adopted: &[(BookId, String, ContentHash)],
    ) -> Result<(), DatabaseError> {
        let files = serde_json::to_string(files).map_err(DatabaseError::operation)?;
        let directories = serde_json::to_string(directories).map_err(DatabaseError::operation)?;
        let adopted = serde_json::to_string(adopted).map_err(DatabaseError::operation)?;
        self.write(|tx| {
            tx.execute(
                "INSERT INTO local_storage_move(singleton,source_root,destination_root,files_json,directories_json,source_library,source_identity,adopted_json,applied) VALUES(1,?1,?2,?3,?4,?5,?6,?7,0)
                ON CONFLICT(singleton) DO UPDATE SET source_root=excluded.source_root,destination_root=excluded.destination_root,files_json=excluded.files_json,directories_json=excluded.directories_json,source_library=excluded.source_library,source_identity=excluded.source_identity,adopted_json=excluded.adopted_json,applied=0",
                (source, destination, &files, &directories, library.to_string(), source_identity, &adopted),
            )?;
            Ok(())
        })
    }

    /// Preserve identity jobs, transfer requests and remote state. Only verified
    /// relocated files retain local availability; no old path is trusted at the
    /// new root merely because a different file exists there.
    pub fn apply_native_root_move(&self, root: &str) -> Result<(), DatabaseError> {
        self.write(|tx| {
            let pending = tx
                .query_row("SELECT source_root,files_json,directories_json,source_identity,source_library,adopted_json FROM local_storage_move WHERE singleton=1 AND destination_root=?1 AND applied=0", [root], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<String>>(3)?, row.get::<_, Option<String>>(4)?, row.get::<_, String>(5)?))
                })
                .optional()?;
            let Some((source, files, directories, source_identity, source_library, adopted)) = pending else { return Ok(()) };
            let files: Vec<(String, String)> = serde_json::from_str(&files).map_err(DatabaseError::operation)?;
            let mut paths = HashMap::<RelativeBookPath, RelativeBookPath>::new();
            for (old, new) in files {
                paths.insert(stored_book_path(&old)?, stored_book_path(&new)?);
            }
            let directories: Vec<(DirId, String)> = serde_json::from_str(&directories).map_err(DatabaseError::operation)?;
            let placements = tx
                .prepare("SELECT b.content_hash,bd.dir_id,bd.file_name,p.relative_path FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id LEFT JOIN local_file_projection p ON p.content_hash=b.content_hash AND p.dir_id=bd.dir_id")?
                .query_map([], |row| Ok((row.get::<_, BookId>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<String>>(3)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            let adopted: Vec<(BookId, String, ContentHash)> = serde_json::from_str(&adopted).map_err(DatabaseError::operation)?;
            let mut folders = FolderPaths::for_directories(tx, placements.iter().map(|(_, id, _, _)| id.as_str()))?;
            let mut relocated = Vec::new();
            let mut live_places = HashMap::<BookId, Vec<(String, RelativeBookPath)>>::new();
            for (book, directory, name, observed) in &placements {
                let desired = folders.placement(directory, name)?;
                if let Some(desired) = &desired {
                    live_places.entry(*book).or_default().push((directory.clone(), desired.clone()));
                }
                let observed = observed.as_ref().map(|path| stored_book_path(path)).transpose()?;
                if let Some(new) = observed.as_ref().and_then(|path| paths.get(path)).or_else(|| desired.as_ref().and_then(|path| paths.get(path))) {
                    relocated.push((*book, directory.clone(), new.clone()));
                }
            }
            // Retained observations can outlive their logical placement.
            let retained =
                tx.prepare("SELECT content_hash,dir_id,relative_path FROM local_file_projection")?.query_map([], |row| Ok((row.get::<_, BookId>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)))?.collect::<Result<Vec<_>, _>>()?;
            tx.execute_batch(
                "UPDATE book_dir SET is_downloaded=0,local_hash='';
                              DELETE FROM local_file_projection;
                              DELETE FROM local_directory_projection;
                              UPDATE book_transfer SET cover_state='pending' WHERE cover_state='ready';",
            )?;
            // Cleanup describes old physical bytes, independently of the root
            // used by new materialization/download work.
            tx.execute(
                "DELETE FROM local_file_work AS current WHERE current.operation='remove' AND current.source_root IS NULL AND EXISTS(
                SELECT 1 FROM local_file_work older WHERE older.content_hash=current.content_hash AND older.relative_path=current.relative_path AND older.source_root=?1 AND older.source_identity IS ?2 AND older.source_library IS ?3)",
                (&source, &source_identity, &source_library),
            )?;
            tx.execute("UPDATE local_file_work SET source_root=?1,source_identity=?2,source_library=?3 WHERE operation='remove' AND source_root IS NULL", (&source, &source_identity, &source_library))?;
            for (book, directory, old) in retained {
                if let Some(new) = paths.get(&stored_book_path(&old)?) {
                    tx.execute("INSERT OR IGNORE INTO local_file_projection(content_hash,dir_id,relative_path) VALUES(?1,?2,?3)", (book, directory, new.as_str()))?;
                }
            }
            for (directory, path) in directories {
                tx.execute("INSERT INTO local_directory_projection(dir_id,relative_path) VALUES(?1,?2)", (directory.to_string(), &path))?;
            }
            let mut observed_books = HashSet::new();
            for (book, directory, path) in relocated {
                observations::record_file(tx, &directory, &book, path.as_str(), None, Revision::Retained)?;
                observed_books.insert(book);
                tx.shared_transfer_complete_download(&book)?;
            }
            for (book, path, checksum) in adopted {
                if observed_books.contains(&book) {
                    continue;
                }
                let path = stored_book_path(&path)?;
                let Some(placements) = live_places.get(&book) else { continue };
                let Some((directory, _)) = placements.iter().find(|(_, desired)| desired == &path).or_else(|| placements.first()) else { continue };
                let directory_id = DirId::parse_str(directory).map_err(DatabaseError::operation)?;
                crate::content_identity::observe_content(tx, book, directory_id, checksum)?;
                observations::record_file(tx, directory, &book, path.as_str(), None, Revision::Retained)?;
                tx.shared_transfer_complete_download(&book)?;
                tx.execute("INSERT INTO book_transfer(content_hash,cover_state) VALUES(?1,'pending') ON CONFLICT(content_hash) DO UPDATE SET cover_state='pending' WHERE cover_state IS NULL OR cover_state='ready'", [book])?;
                observed_books.insert(book);
            }
            tx.execute("UPDATE book_dir SET size_bytes=NULL WHERE is_downloaded=0", [])?;
            tx.execute("UPDATE local_storage_move SET applied=1 WHERE singleton=1", [])?;
            Ok(())
        })
    }
}
