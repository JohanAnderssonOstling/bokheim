use std::{collections::HashSet, path::{Path, PathBuf}};
// Pending commands protect both their old physical paths and intended paths.
// Queue sequence numbers survive consumption, so work queued and completed
// during a scan still invalidates that scan's observations.
pub(super) fn file_work_revision(conn: &rusqlite::Connection) -> rusqlite::Result<i64> {
    conn.query_row(include_str!("sql/scanner_pending/file_work_revision_select.sql"), [], |row| row.get(0))
}

#[derive(Default)]
pub(super) struct PendingScanWork {
    pub(super) revision: i64,
    folders: HashSet<PathBuf>,
    files: HashSet<PathBuf>,
    directories: HashSet<String>,
    books: HashSet<String>,
}

impl PendingScanWork {
    pub(super) fn read(conn: &rusqlite::Connection) -> rusqlite::Result<Self> {
        let mut work = Self { revision: file_work_revision(conn)?, ..Self::default() };
        let mut directories = conn.prepare(
            include_str!("sql/scanner_pending/read_with.sql"),
        )?;
        for directory in directories.query_map([crate::ROOT_DIR_ID.to_string()], |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, Option<String>>(2)?)))? {
            let (id, live, remembered) = directory?;
            work.directories.insert(id);
            for path in [live, remembered].into_iter().flatten() {
                work.folders.insert(PathBuf::from(path.trim_matches('/')));
            }
        }
        work.books = conn.prepare(include_str!("sql/scanner_pending/read_select.sql"))?.query_map([], |row| row.get(0))?.collect::<Result<_, _>>()?;
        let mut paths = conn.prepare(
            include_str!("sql/scanner_pending/read_with_2.sql"),
        )?;
        for path in paths.query_map([], |row| row.get::<_, String>(0))? {
            work.files.insert(PathBuf::from(path?.trim_matches('/')));
        }
        Ok(work)
    }

    pub(super) fn excludes(&self, path: &Path) -> bool {
        self.files.contains(path) || path.ancestors().any(|ancestor| self.folders.contains(ancestor))
    }

    pub(super) fn protect_placements(&self, tx: &rusqlite::Transaction, scan_id: i64) -> rusqlite::Result<()> {
        for dir in &self.directories {
            tx.execute(include_str!("sql/scanner_pending/protect_placements_update.sql"), rusqlite::params![dir, scan_id])?;
        }
        for hash in &self.books {
            tx.execute(include_str!("sql/scanner_pending/protect_placements_update_2.sql"), rusqlite::params![hash, scan_id])?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod pending_path_tests {
    use super::*;
    use crate::DirId;

    #[test]
    fn folder_exclusions_match_ancestors_without_matching_name_prefixes() {
        let mut pending = PendingScanWork::default();
        pending.folders.insert(PathBuf::from("Shelf"));
        pending.files.insert(PathBuf::from("single.epub"));
        for path in ["Shelf", "Shelf/book.epub", "Shelf/Nested/book.epub", "single.epub"] {
            assert!(pending.excludes(Path::new(path)), "expected exclusion for {path}");
        }
        for path in ["Shelves/book.epub", "Shelf Two/book.epub", "Other/Shelf/book.epub", "single.epub/child", ""] {
            assert!(!pending.excludes(Path::new(path)), "unexpected exclusion for {path}");
        }
        pending.folders.insert(PathBuf::new());
        assert!(pending.excludes(Path::new("Other/book.epub")));
        assert!(pending.excludes(Path::new("")));
    }

    #[test]
    fn pending_folders_protect_live_and_remembered_subtrees_including_deleted_children() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        client_library::schema::init_db(&conn).unwrap();
        let parent = DirId::new_v4();
        let child = DirId::new_v4();
        conn.execute(include_str!("sql/scanner_pending/pending_book_paths_preserve_sources_without_hiding_unrelated_shortened_paths_insert.sql"), rusqlite::params![parent.to_string(), crate::ROOT_DIR_ID.to_string()]).unwrap();
        conn.execute(include_str!("sql/scanner_pending/pending_folders_protect_live_and_remembered_subtrees_including_deleted_children_insert_2.sql"), rusqlite::params![child.to_string(), parent.to_string()]).unwrap();
        conn.execute(include_str!("sql/scanner_pending/pending_folders_protect_live_and_remembered_subtrees_including_deleted_children_insert_3.sql"), [child.to_string()]).unwrap();
        client_library::file_work::queue_directory_work(&conn, &parent).unwrap();
        let pending = PendingScanWork::read(&conn).unwrap();
        assert!(pending.directories.contains(&parent.to_string()));
        assert!(pending.directories.contains(&child.to_string()));
        assert!(pending.excludes(Path::new("Parent/book.epub")));
        assert!(pending.excludes(Path::new("OldChild/book.epub")));
        assert!(!pending.excludes(Path::new("Unrelated/book.epub")));
        conn.execute(include_str!("sql/scanner/discovered_batch_does_not_resurrect_a_concurrently_deleted_directory_update.sql"), [parent.to_string()]).unwrap();
        let pending = PendingScanWork::read(&conn).unwrap();
        assert!(!pending.excludes(Path::new("Parent/book.epub")));
        assert!(pending.excludes(Path::new("OldChild/book.epub")));
        client_library::file_work::queue_directory_work(&conn, &crate::ROOT_DIR_ID).unwrap();
        assert!(PendingScanWork::read(&conn).unwrap().excludes(Path::new("Unrelated/book.epub")), "pending root work protects the whole library");
    }

    #[test]
    fn pending_book_paths_preserve_sources_without_hiding_unrelated_shortened_paths() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        client_library::schema::init_db(&conn).unwrap();
        let parent = DirId::new_v4().to_string();
        let child = DirId::new_v4().to_string();
        let hash = test_support::fixture_content_hash(910);
        conn.execute(include_str!("sql/scanner_pending/pending_book_paths_preserve_sources_without_hiding_unrelated_shortened_paths_insert.sql"), rusqlite::params![parent, crate::ROOT_DIR_ID.to_string()]).unwrap();
        conn.execute(include_str!("sql/scanner_pending/pending_book_paths_preserve_sources_without_hiding_unrelated_shortened_paths_insert_2.sql"), rusqlite::params![child, parent]).unwrap();
        conn.execute(include_str!("sql/scanner/scanner_with_missing_placement_insert.sql"), [hash.as_str()]).unwrap();
        conn.execute(include_str!("sql/scanner_pending/pending_book_paths_preserve_sources_without_hiding_unrelated_shortened_paths_insert_4.sql"), rusqlite::params![hash.as_str(), child]).unwrap();
        conn.execute(include_str!("sql/scanner_pending/pending_book_paths_preserve_sources_without_hiding_unrelated_shortened_paths_insert_5.sql"), rusqlite::params![hash.as_str(), child]).unwrap();
        client_library::file_work::queue_book_work(&conn, &hash).unwrap();
        client_library::file_work::queue_file_work(&conn, "restore", &hash, "/Queued/book.epub").unwrap();
        let pending = PendingScanWork::read(&conn).unwrap();
        for path in ["Old/book.epub", "Queued/book.epub", "Parent/Child/book.epub"] {
            assert!(pending.excludes(Path::new(path)), "pending work must protect {path}");
        }
        conn.execute(include_str!("sql/scanner/discovered_batch_does_not_resurrect_a_concurrently_deleted_directory_update.sql"), [parent]).unwrap();
        let pending = PendingScanWork::read(&conn).unwrap();
        assert!(pending.excludes(Path::new("Old/book.epub")));
        assert!(pending.excludes(Path::new("Queued/book.epub")));
        assert!(!pending.excludes(Path::new("Child/book.epub")), "disconnected metadata must not hide an unrelated scan discovery");
        assert!(pending.books.contains(hash.as_str()));
    }
}
