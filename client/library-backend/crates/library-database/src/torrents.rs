//! Durable local torrent transfers and their associations with imported books.
use crate::placements::{book_at_path, resolve_placement_path};
use crate::{Database, DatabaseError};
use library_model::RelativeDirPath;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sync_common::{ContentHash, DirId};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TorrentRecord {
    pub info_hash: String,
    pub metadata: Vec<u8>,
    pub source: Option<String>,
    pub staging_path: String,
    pub output_path: String,
    pub imported: bool,
    pub publishing: bool,
    pub keep_seeding: bool,
    pub folder_id: Option<DirId>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BookTorrent {
    pub info_hash: String,
    pub file_index: usize,
    pub source: Option<String>,
    pub keep_seeding: bool,
}

impl Database {
    pub fn torrents(&self) -> Result<Vec<TorrentRecord>, DatabaseError> {
        let mut query = self.connection.prepare("SELECT info_hash,metadata,source,staging_path,output_path,imported,keep_seeding,folder_id,error,publishing FROM torrent ORDER BY info_hash")?;
        let rows = query.query_map([], |row| {
            Ok(TorrentRecord {
                info_hash: row.get(0)?,
                metadata: row.get(1)?,
                source: row.get(2)?,
                staging_path: row.get(3)?,
                output_path: row.get(4)?,
                imported: row.get(5)?,
                keep_seeding: row.get(6)?,
                folder_id: row.get::<_, Option<String>>(7)?.map(|id| id.parse()).transpose().map_err(|e| rusqlite::Error::FromSqlConversionFailure(7, rusqlite::types::Type::Text, Box::new(e)))?,
                error: row.get(8)?,
                publishing: row.get(9)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn save_torrent(&self, record: &TorrentRecord, files: &[(usize, String)]) -> Result<(), DatabaseError> {
        if record.info_hash.len() != 40 || !record.info_hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(DatabaseError::message("invalid torrent info hash"));
        }
        for path in [&record.staging_path, &record.output_path] {
            RelativeDirPath::parse(path).map_err(DatabaseError::operation)?;
        }
        for (_, name) in files {
            library_model::RelativeBookPath::parse(name).map_err(DatabaseError::operation)?;
        }
        let tx = self.connection.unchecked_transaction()?;
        tx.execute(
            "INSERT OR IGNORE INTO torrent(info_hash,metadata,source,staging_path,output_path,imported,keep_seeding) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![record.info_hash, record.metadata, record.source, record.staging_path, record.output_path, record.imported, record.keep_seeding],
        )?;
        for (index, name) in files {
            tx.execute("INSERT OR IGNORE INTO torrent_file VALUES(?1,?2,?3)", params![record.info_hash, *index as i64, name])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn torrent_files(&self, hash: &str) -> Result<Vec<(usize, String)>, DatabaseError> {
        let mut query = self.connection.prepare("SELECT file_index,relative_name FROM torrent_file WHERE info_hash=?1 ORDER BY file_index")?;
        Ok(query.query_map([hash], |row| Ok((row.get::<_, i64>(0)? as usize, row.get(1)?)))?.collect::<Result<_, _>>()?)
    }

    pub fn torrent_published(&self, hash: &str, path: &str) -> Result<(), DatabaseError> {
        RelativeDirPath::parse(path).map_err(DatabaseError::operation)?;
        self.connection.execute("UPDATE torrent SET output_path=?2,imported=1,publishing=0,error=NULL WHERE info_hash=?1", params![hash, path])?;
        Ok(())
    }

    pub fn torrent_publish_intent(&self, hash: &str, path: &str) -> Result<(), DatabaseError> {
        RelativeDirPath::parse(path).map_err(DatabaseError::operation)?;
        self.connection.execute("UPDATE torrent SET output_path=?2,publishing=1 WHERE info_hash=?1", params![hash, path])?;
        Ok(())
    }

    pub fn set_torrent_error(&self, hash: &str, error: Option<&str>) -> Result<(), DatabaseError> {
        self.connection.execute("UPDATE torrent SET error=?2 WHERE info_hash=?1", params![hash, error])?;
        Ok(())
    }

    pub fn keep_torrent_seeding(&self, hash: &str, keep: bool) -> Result<(), DatabaseError> {
        if self.connection.execute("UPDATE torrent SET keep_seeding=?2,error=NULL WHERE info_hash=?1", params![hash, keep])? == 0 {
            return Err(DatabaseError::message("torrent no longer exists"));
        }
        Ok(())
    }

    /// Forgetting a torrent never removes the library's payload.
    pub fn forget_torrent(&self, hash: &str) -> Result<(), DatabaseError> {
        self.connection.execute("DELETE FROM torrent WHERE info_hash=?1", [hash])?;
        Ok(())
    }

    pub fn book_torrents(&self, book: &ContentHash) -> Result<Vec<BookTorrent>, DatabaseError> {
        let mut query = self.connection.prepare("SELECT t.info_hash,bt.torrent_file_index,t.source,t.keep_seeding FROM book_torrent bt JOIN book b ON b.row_id=bt.book_row_id JOIN torrent t ON t.info_hash=bt.torrent_info_hash WHERE b.content_hash=?1 ORDER BY t.info_hash,bt.torrent_file_index")?;
        Ok(query.query_map([book], |row| Ok(BookTorrent { info_hash: row.get(0)?, file_index: row.get::<_, i64>(1)? as usize, source: row.get(2)?, keep_seeding: row.get(3)? }))?.collect::<Result<_, _>>()?)
    }

    /// Follow the physical folder projection, including moves of its ancestors.
    pub fn torrent_output_path(&self, torrent: &TorrentRecord) -> Result<Option<String>, DatabaseError> {
        if !torrent.imported {
            return Ok(Some(torrent.staging_path.clone()));
        }
        if let Some(id) = torrent.folder_id {
            return self.native_directory_projected_path(&id);
        }
        let book_path: Option<String> = self
            .connection
            .query_row(
                "SELECT p.relative_path FROM torrent t JOIN local_file_projection p JOIN book b ON b.content_hash=p.content_hash AND b.row_id=t.root_book_row_id WHERE t.info_hash=?1 ORDER BY p.relative_path=?2 DESC LIMIT 1",
                params![torrent.info_hash, format!("/{}", torrent.output_path.trim_matches('/'))],
                |row| row.get(0),
            )
            .optional()?;
        if book_path.is_some() {
            return Ok(book_path);
        }
        Ok(Some(torrent.output_path.clone()))
    }

    /// Called after a scan. Associate file indices with books, including all
    /// tracks of a folder audiobook, without changing an existing association.
    pub fn bind_torrent_books(&self) -> Result<(), DatabaseError> {
        for torrent in self.torrents()?.into_iter().filter(|t| t.imported) {
            if torrent.folder_id.is_none() {
                let folder = format!("/{}", torrent.output_path.trim_matches('/'));
                if let Some((hash, _, _)) = book_at_path(&self.connection, &folder)? {
                    self.connection.execute("UPDATE torrent SET root_book_row_id=(SELECT row_id FROM book WHERE content_hash=?2) WHERE info_hash=?1 AND root_book_row_id IS NULL", params![torrent.info_hash, hash])?;
                }
                if let Some((parent, name)) = resolve_placement_path(&self.connection, &folder)? {
                    let id: Option<String> = self.connection.query_row("SELECT id FROM dir WHERE parent_id=?1 AND name=?2 AND deleted_at IS NULL", params![parent, name], |row| row.get(0)).optional()?;
                    if let Some(id) = id {
                        self.connection.execute("UPDATE torrent SET folder_id=?2 WHERE info_hash=?1", params![torrent.info_hash, id])?;
                    }
                }
            }
            let Some(base) = self.torrent_output_path(&torrent)? else { continue };
            for (index, file) in self.torrent_files(&torrent.info_hash)? {
                let mut path = format!("/{}/{}", base.trim_matches('/'), file);
                loop {
                    if let Some((book, _, _)) = book_at_path(&self.connection, &path)? {
                        self.connection.execute("INSERT OR IGNORE INTO book_torrent SELECT row_id,?2,?3 FROM book WHERE content_hash=?1", params![book, torrent.info_hash, index as i64])?;
                        break;
                    }
                    let Some((parent, _)) = path.rsplit_once('/') else { break };
                    if parent.len() < base.trim_end_matches('/').len() {
                        break;
                    }
                    path = parent.to_owned();
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sync_common::ROOT_DIR_ID;

    fn database() -> Database {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        db
    }

    fn record(hash: &str) -> TorrentRecord {
        TorrentRecord {
            info_hash: hash.repeat(40),
            metadata: b"metadata".to_vec(),
            source: Some("magnet:?xt=urn:btih:test".into()),
            staging_path: ".bokheim-torrents/staged/Books".into(),
            output_path: "Books".into(),
            imported: true,
            publishing: false,
            keep_seeding: true,
            folder_id: None,
            error: None,
        }
    }

    #[test]
    fn links_multiple_torrents_and_audiobook_tracks_to_stable_book_ids() {
        let db = database();
        let folder = db.create_directory(&ROOT_DIR_ID, &"Books".into()).unwrap().id;
        db.connection.execute("INSERT INTO local_directory_projection VALUES(?1,'/Books')", [folder.to_string()]).unwrap();
        let novel = ContentHash::new(&"a".repeat(64));
        let audio = ContentHash::new(&"b".repeat(64));
        for (hash, name) in [(novel, "Novel.epub"), (audio, "Audio")] {
            db.connection.execute("INSERT INTO book(content_hash,title) VALUES(?1,?2)", params![hash, name]).unwrap();
            db.connection.execute("INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded) SELECT ?1,row_id,?2,'',1 FROM book WHERE content_hash=?3", params![folder.to_string(), name, hash]).unwrap();
            db.connection.execute("INSERT INTO local_file_projection VALUES(?1,?2,?3)", params![hash, folder.to_string(), format!("/Books/{name}")]).unwrap();
        }
        let files = vec![(0, "Novel.epub".into()), (1, "Audio/01.mp3".into()), (2, "Audio/02.mp3".into()), (3, "notes.txt".into())];
        for hash in ["c", "d"] {
            db.save_torrent(&record(hash), &files).unwrap();
        }
        db.bind_torrent_books().unwrap();
        assert_eq!(db.book_torrents(&novel).unwrap().len(), 2);
        assert_eq!(db.book_torrents(&audio).unwrap().len(), 4);
        let original = db.torrents().unwrap()[0].clone();
        assert_eq!(original.folder_id, Some(folder));
        db.move_directory(&folder, None, Some("Renamed")).unwrap();
        // The logical move alone does not reopen a future path. The physical
        // replay acknowledges the rename before the torrent follows it.
        assert_eq!(db.torrent_output_path(&original).unwrap().as_deref(), Some("/Books"));
        let snapshot = db.native_directory_snapshot(&folder).unwrap().unwrap();
        db.acknowledge_native_directory(crate::NativeDirectoryCompletion { snapshot, renamed_to: None, materialized: true }).unwrap();
        assert_eq!(db.torrent_output_path(&original).unwrap().as_deref(), Some("/Renamed"));
        assert_eq!(db.book_torrents(&audio).unwrap().len(), 4);
        db.forget_torrent(&original.info_hash).unwrap();
        assert_eq!(db.book_torrents(&audio).unwrap().len(), 2);
        assert_eq!(db.connection.query_row("SELECT COUNT(*) FROM book WHERE content_hash=?1", [audio], |row| row.get::<_, i64>(0)).unwrap(), 1);
    }

    #[test]
    fn root_audiobook_tracks_link_and_follow_the_book_folder() {
        let db = database();
        let audio = ContentHash::new(&"a".repeat(64));
        db.connection.execute("INSERT INTO book(content_hash,title) VALUES(?1,'Books')", [audio]).unwrap();
        db.connection.execute("INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded) SELECT ?1,row_id,'Books','',1 FROM book WHERE content_hash=?2", params![ROOT_DIR_ID.to_string(), audio]).unwrap();
        db.connection.execute("INSERT INTO local_file_projection VALUES(?1,?2,'/Books')", params![audio, ROOT_DIR_ID.to_string()]).unwrap();
        let record = record("b");
        db.save_torrent(&record, &[(0, "01.mp3".into()), (1, "02.mp3".into())]).unwrap();
        db.bind_torrent_books().unwrap();
        assert_eq!(db.book_torrents(&audio).unwrap().len(), 2);
        // A folder audiobook is projected as a book rather than a directory.
        db.connection.execute("UPDATE local_file_projection SET relative_path='/Renamed' WHERE content_hash=?1", [audio]).unwrap();
        assert_eq!(db.torrent_output_path(&record).unwrap().as_deref(), Some("/Renamed"));
        assert_eq!(db.book_torrents(&audio).unwrap().len(), 2);
    }

    #[test]
    fn version_36_migrates_without_losing_existing_books() {
        let db = database();
        let hash = ContentHash::new(&"e".repeat(64));
        db.connection.execute("INSERT INTO book(content_hash,title) VALUES(?1,'Existing')", [hash]).unwrap();
        crate::browse::revision::remove(&db.connection).unwrap();
        db.connection
            .execute_batch("DROP TABLE book_torrent; DROP TABLE torrent_file; DROP TABLE torrent; ALTER TABLE sync_metadata DROP COLUMN pull_generation; ALTER TABLE book DROP COLUMN popularity_rating_count; PRAGMA user_version=36;")
            .unwrap();
        db.initialize_library().unwrap();
        assert_eq!(db.connection.query_row("SELECT COUNT(*) FROM book WHERE content_hash=?1", [hash], |row| row.get::<_, i64>(0)).unwrap(), 1);
        assert!(db.torrents().unwrap().is_empty());
        assert_eq!(db.connection.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0)).unwrap(), crate::LIBRARY_SCHEMA_VERSION);
    }

    #[test]
    fn unsafe_payload_paths_are_rejected_before_any_state_is_saved() {
        let db = database();
        assert!(db.save_torrent(&record("f"), &[(0, "../outside.epub".into())]).is_err());
        assert!(db.torrents().unwrap().is_empty());
        let mut invalid = record("f");
        invalid.staging_path = "../outside".into();
        assert!(db.save_torrent(&invalid, &[]).is_err());
    }
}
