use super::*;
// Native asset-store projections. The shared model owns paths;
// these snapshots remember which physical paths this device has materialized.
pub(super) fn projection_error(error: impl std::fmt::Display) -> AssetStoreError {
    AssetStoreError(error.to_string())
}

// Include the last materialized paths so a pending rename can still read its
// source before the destination has been published.
pub(super) fn publication_paths(handle: &Handle, conn: &rusqlite::Connection, hash: &ContentHash) -> Result<Vec<PathBuf>, AssetStoreError> {
    let paths = conn
        .prepare(
            include_str!("../sql/native_projection/publication_paths_select.sql"),
        )?
        .query_map([hash.as_str()], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    paths.into_iter().map(|relative| placement_path(handle, &relative)).collect()
}

pub(in super::super) fn readable_publication_paths(handle: &Handle, hash: &ContentHash) -> Result<Vec<PathBuf>, AssetStoreError> {
    let Some(database) = handle.3.as_ref() else { return Ok(Vec::new()); };
    if !database.is_file() {
        return Ok(Vec::new());
    }
    // No schema setup or taxonomy initialization is needed to locate a file.
    let conn = rusqlite::Connection::open_with_flags(database, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut paths = publication_paths(handle, &conn, hash)?;
    let mut seen = HashSet::new();
    paths.retain(|path| seen.insert(path.clone()));
    Ok(paths)
}

fn stored_publication_path(handle: &Handle, hash: &ContentHash) -> Result<Option<PathBuf>, AssetStoreError> {
    Ok(readable_publication_paths(handle, hash)?.into_iter().find(|path| path.is_file()))
}

pub(super) fn readable_path(handle: &Handle, name: String) -> Result<Option<PathBuf>, AssetStoreError> {
    if !enabled(handle) { return Ok(None); }
    if let Some(hash) = name.strip_prefix("book/") {
        let hash = hash.parse::<ContentHash>().map_err(projection_error)?;
        return stored_publication_path(handle, &hash);
    }
    let path = path(handle, name);
    Ok(path.is_file().then_some(path))
}

pub(in super::super) fn evict_publication_files(handle: &Handle, conn: &rusqlite::Connection, hash: &ContentHash) -> Result<(), AssetStoreError> {
    for path in publication_paths(handle, conn, hash)? {
        if path.is_file() && content_matches(&path, hash)? {
            std::fs::remove_file(&path)?;
            sync_parent(&path)?;
        }
    }
    Ok(())
}

pub(in super::super) fn queue_book_placements(handle: &Handle, conn: &rusqlite::Connection, hash: &ContentHash) -> Result<(), AssetStoreError> {
    let files = conn
        .prepare(
            include_str!("../sql/native_projection/queue_book_placements_select.sql"),
        )?
        .query_map([hash.as_str()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    for (path, materialized) in files {
        if materialized.as_deref() != Some(path.as_str()) || !placement_path(handle, &path).is_ok_and(|path| path.is_file()) {
            client_library::file_work::queue_file_work(conn, "restore", hash, &path)?;
        }
    }
    Ok(())
}

fn directory_path(conn: &rusqlite::Connection, id: crate::DirId) -> Result<Option<String>, AssetStoreError> {
    Ok(client_library::placements::directory_relative_path(conn, &id)?.map(|path| path.as_str().to_owned()))
}

fn subtree_directories(conn: &rusqlite::Connection, root: crate::DirId) -> Result<Vec<crate::DirId>, AssetStoreError> {
    let ids = conn
        .prepare(
            include_str!("../sql/native_projection/subtree_directories_with.sql"),
        )?
        .query_map([root.to_string()], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    ids.into_iter().map(|id| crate::DirId::parse_str(&id).map_err(projection_error)).collect()
}

pub(in super::super) fn prepare_directory(handle: &Handle, conn: &mut rusqlite::Connection, id: crate::DirId) -> Result<(), AssetStoreError> {
    let directories = client_library::assets::get_dir_path_components(conn, &id.to_string())?.into_iter().map(|part| part.id).collect();
    prepare_directory_ids(handle, conn, directories)
}

// Inspect slow storage before acquiring SQLite's writer. Final publication only
// checks the inspected entries' metadata, then performs the required writes.
#[derive(PartialEq)]
struct DirectoryEntryStamp {
    directory: bool,
    symlink: bool,
    length: u64,
    modified: std::time::SystemTime,
}

fn directory_entry_stamp(path: &Path) -> Result<Option<DirectoryEntryStamp>, AssetStoreError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(DirectoryEntryStamp { directory: metadata.is_dir(), symlink: metadata.file_type().is_symlink(), length: metadata.len(), modified: metadata.modified()? })),
        Err(error) if matches!(error.kind(), std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

struct PreparedDirectory {
    id: crate::DirId,
    relative: String,
    old: Option<String>,
    chosen: Option<String>,
    destination: PathBuf,
    source: Option<PathBuf>,
    inspected: Vec<(PathBuf, Option<DirectoryEntryStamp>)>,
    exists: bool,
    missing_external: bool,
}

impl PreparedDirectory {
    fn inspect(handle: &Handle, conn: &rusqlite::Connection, id: crate::DirId) -> Result<Option<Self>, AssetStoreError> {
        use rusqlite::OptionalExtension as _;
        let Some(relative) = directory_path(conn, id)? else { return Ok(None) };
        let old = conn.query_row(include_str!("../sql/native_projection/inspect_select.sql"), [id.to_string()], |row| row.get::<_, String>(0)).optional()?;
        let mut destination = placement_path(handle, &relative)?;
        let mut inspected = Vec::new();
        let mut record = |path: &Path| -> Result<bool, AssetStoreError> {
            let stamp = directory_entry_stamp(path)?;
            let exists = stamp.is_some();
            inspected.push((path.to_owned(), stamp));
            Ok(exists)
        };
        let mut exists = record(&destination)?;
        let marker = destination.join(".biblos_uuid");
        let marker_exists = record(&marker)?;
        let owned = marker_exists && std::fs::read_to_string(&marker)?.trim() == id.to_string();
        let chosen = if exists && !owned {
            let (parent, name): (String, String) = conn.query_row(include_str!("../sql/native_projection/inspect_select_2.sql"), [id.to_string()], |r| Ok((r.get(0)?, r.get(1)?)))?;
            let mut occupied = std::fs::read_dir(destination.parent().unwrap())?.map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned())).collect::<Result<Vec<_>, _>>()?;
            occupied.extend(conn.prepare(include_str!("../sql/native_projection/inspect_select_3.sql"))?.query_map([parent], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?);
            let chosen = library_model::unique_folder_name(&name, occupied.iter().map(String::as_str));
            destination.set_file_name(&chosen);
            exists = record(&destination)?;
            if exists {
                return Err(AssetStoreError("folder destination changed during inspection".into()));
            }
            Some(chosen)
        } else {
            None
        };
        let missing_external = old.as_deref() == Some(relative.as_str()) && !exists && chosen.is_none();
        let source = if !exists && !missing_external {
            if let Some(old) = old.as_ref().filter(|old| *old != &relative || chosen.is_some()) {
                let source = placement_path(handle, old)?;
                let source_exists = record(&source)?;
                if source_exists && source.is_dir() {
                    let marker = source.join(".biblos_uuid");
                    record(&marker)?;
                    if std::fs::read_to_string(&marker)?.trim() != id.to_string() {
                        return Err(AssetStoreError("folder identity changed before materialization".into()));
                    }
                    Some(source)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };
        Ok(Some(Self { id, relative, old, chosen, destination, source, inspected, exists, missing_external }))
    }

    // False asks the caller to inspect again; no metadata or files were changed.
    fn publish(self, conn: &mut rusqlite::Connection) -> Result<bool, AssetStoreError> {
        use rusqlite::OptionalExtension as _;
        let tx = client_library::schema::library_write_transaction(conn)?;
        let old: Option<String> = tx.query_row(include_str!("../sql/native_projection/inspect_select.sql"), [self.id.to_string()], |row| row.get(0)).optional()?;
        if directory_path(&tx, self.id)?.as_deref() != Some(&self.relative) || old != self.old {
            return Ok(false);
        }
        // Metadata checks detect replaced markers and newly occupied paths
        // without rereading marker contents or listing directories under the writer.
        for (path, stamp) in &self.inspected {
            if directory_entry_stamp(path)? != *stamp {
                return Ok(false);
            }
        }
        if self.missing_external {
            return Ok(true);
        }
        let relative = if let Some(chosen) = self.chosen {
            let occupied: bool =
                tx.query_row(include_str!("../sql/native_projection/publish_select_2.sql"), rusqlite::params![self.id.to_string(), chosen], |row| row.get(0))?;
            if occupied {
                return Ok(false);
            }
            client_library::placements::move_directory_record(&tx, &self.id, None, Some(&chosen))?;
            directory_path(&tx, self.id)?.ok_or_else(|| AssetStoreError("folder disappeared during preparation".into()))?
        } else {
            self.relative
        };
        if let Some(source) = self.source {
            std::fs::rename(&source, &self.destination)?;
            sync_parent(&source)?;
            sync_parent(&self.destination)?;
            client_library::file_work::move_file_projection_paths(&tx, self.old.as_ref().unwrap(), &relative)?;
        } else if !self.exists {
            std::fs::create_dir(&self.destination)?;
            sync_parent(&self.destination)?;
            use std::io::Write as _;
            let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(self.destination.join(".biblos_uuid"))?;
            file.write_all(self.id.to_string().as_bytes())?;
            file.sync_all()?;
        }
        tx.execute(include_str!("../sql/native_projection/publish_insert.sql"), rusqlite::params![self.id.to_string(), relative])?;
        tx.commit()?;
        Ok(true)
    }
}

fn prepare_directory_ids(handle: &Handle, conn: &mut rusqlite::Connection, directories: Vec<crate::DirId>) -> Result<(), AssetStoreError> {
    if !root_available(handle) {
        return Err(AssetStoreError("library storage is unavailable".into()));
    }
    for id in directories {
        if id == crate::ROOT_DIR_ID {
            continue;
        }
        while let Some(prepared) = PreparedDirectory::inspect(handle, conn, id)? {
            if prepared.publish(conn)? {
                break;
            }
        }
    }
    Ok(())
}

enum ProjectionScope {
    Directory(crate::DirId),
    Book(ContentHash),
}

pub(in super::super) fn prepare_directory_work(handle: &Handle, conn: &mut rusqlite::Connection) -> Result<(), AssetStoreError> {
    // Parent jobs cover their live descendants, even when the child was queued
    // first. Retain the snapshot tokens so new requests cannot be acknowledged.
    let jobs = conn
        .prepare(
            include_str!("../sql/native_projection/prepare_directory_work_with.sql"),
        )?
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut failure = None;
    for (token, id) in jobs {
        if !conn.query_row(include_str!("../sql/native_projection/prepare_directory_work_select.sql"), [token], |row| row.get::<_, bool>(0))? {
            continue;
        }
        if let Err(error) = prepare_placements_in(handle, conn, token, ProjectionScope::Directory(crate::DirId::parse_str(&id).map_err(projection_error)?)) {
            failure.get_or_insert(error);
        }
    }
    let jobs = conn.prepare(include_str!("../sql/native_projection/prepare_directory_work_select_2.sql"))?.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?.collect::<Result<Vec<_>, _>>()?;
    for (token, hash) in jobs {
        if let Err(error) = prepare_placements_in(handle, conn, token, ProjectionScope::Book(ContentHash::new(&hash))) {
            failure.get_or_insert(error);
        }
    }
    // Each request validates its own ancestry. Keep failed requests durable,
    // and report failure so source-file retirement still waits for preparation.
    failure.map_or(Ok(()), Err)
}

fn prepare_placements_in(handle: &Handle, conn: &mut rusqlite::Connection, token: i64, scope: ProjectionScope) -> Result<(), AssetStoreError> {
    use rusqlite::OptionalExtension as _;
    let mut covered_tokens = vec![token];
    let (sql, key) = match &scope {
        ProjectionScope::Directory(root) => {
            covered_tokens.extend(
                conn.prepare(
                    include_str!("../sql/native_projection/prepare_placements_in_with.sql"),
                )?
                .query_map([root.to_string()], |row| row.get::<_, i64>(0))?
                .collect::<Result<Vec<_>, _>>()?,
            );
            prepare_directory(handle, conn, *root)?;
            let directories = subtree_directories(conn, *root)?.into_iter().filter(|id| id != root).collect();
            prepare_directory_ids(handle, conn, directories)?;
            (
                include_str!("../sql/native_projection/placements_directory.sql"),
                root.to_string(),
            )
        }
        ProjectionScope::Book(hash) => (include_str!("../sql/native_projection/placements_book.sql"), hash.to_string()),
    };
    let files = conn.prepare(sql)?
        .query_map([&key], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?)))?.collect::<Result<Vec<_>, _>>()?;
    if matches!(scope, ProjectionScope::Book(_)) {
        for (_, dir, _, _) in &files {
            prepare_directory(handle, conn, crate::DirId::parse_str(dir).map_err(projection_error)?)?;
        }
    }
    for (hash, dir, name, fingerprint) in files {
        let hash = ContentHash::new(&hash);
        let id = crate::DirId::parse_str(&dir).map_err(projection_error)?;
        let relative = client_library::file_work::placement_path(conn, &id, &name)?;
        let old: Option<String> = conn.query_row(include_str!("../sql/native_projection/prepare_placements_in_select.sql"), rusqlite::params![hash.as_str(), dir], |r| r.get(0)).optional()?;
        if old.as_deref() == Some(&relative) {
            continue;
        }
        // Filesystem checks never hold the writer. Validate this placement again
        // before committing its work, since a user may move it during preflight.
        let trash_old = old.as_ref().is_some_and(|old| placement_path(handle, old).is_ok_and(|path| path.is_file()));
        let restore = old.is_some() || (fingerprint.is_empty() && (placement_path(handle, &relative)?.is_file() || publication_paths(handle, conn, &hash)?.into_iter().any(|path| path.is_file())));
        let tx = client_library::schema::library_write_transaction(conn)?;
        let current: Option<(String, String, String)> = tx.query_row(
            include_str!("../sql/native_projection/prepare_placements_in_select_2.sql"),
            rusqlite::params![hash.as_str(), dir], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ).optional()?;
        let stored: Option<String> = tx.query_row(include_str!("../sql/native_projection/prepare_placements_in_select.sql"), rusqlite::params![hash.as_str(), dir], |r| r.get(0)).optional()?;
        if current != Some((name, fingerprint.clone(), relative.clone())) || stored != old {
            continue;
        }
        if restore {
            client_library::file_work::queue_file_work(&tx, "restore", &hash, &relative)?;
            if trash_old {
                client_library::file_work::queue_file_work(&tx, "trash", &hash, old.as_ref().unwrap())?;
            }
        } else if !fingerprint.is_empty() {
            tx.execute(include_str!("../sql/native_projection/prepare_placements_in_insert.sql"), rusqlite::params![hash.as_str(), dir, relative])?;
        }
        tx.commit()?;
    }
    let removed_sql = match &scope {
        ProjectionScope::Directory(_) => include_str!("../sql/native_projection/placements_removed_directory.sql"),
        ProjectionScope::Book(_) => include_str!("../sql/native_projection/placements_removed_book.sql"),
    };
    let removed = conn.prepare(removed_sql)?
        .query_map([&key], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?.collect::<Result<Vec<_>, _>>()?;
    for (hash, path) in removed {
        client_library::file_work::queue_file_work(conn, "trash", &ContentHash::new(&hash), &path).map_err(projection_error)?;
        // Retain the source path until its trash job completes, so an earlier
        // restore can copy a moved book before retiring the old placement.
    }
    // A replacement token belongs to a newer metadata intent and cannot be
    // acknowledged by this pass. Every placement above commits independently.
    let completed_sql = match scope {
        ProjectionScope::Directory(_) => include_str!("../sql/native_projection/complete_directory_work.sql"),
        ProjectionScope::Book(_) => include_str!("../sql/native_projection/complete_book_work.sql"),
    };
    let tx = client_library::schema::library_write_transaction(conn)?;
    for token in covered_tokens {
        tx.execute(completed_sql, [token])?;
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod prepared_directory_tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, Handle, rusqlite::Connection, crate::DirId) {
        let root = tempfile::tempdir().unwrap();
        let handle = open(root.path().to_str().unwrap()).unwrap();
        let mut conn = rusqlite::Connection::open(root.path().join("test.db")).unwrap();
        client_library::schema::init_db(&conn).unwrap();
        let folder = client_library::placements::create_directory(&mut conn, &crate::ROOT_DIR_ID, &"Source".into()).unwrap();
        prepare_directory(&handle, &mut conn, folder.id).unwrap();
        std::fs::write(root.path().join("Source/notes.txt"), b"preserved").unwrap();
        (root, handle, conn, folder.id)
    }

    fn rename(conn: &mut rusqlite::Connection, id: crate::DirId, name: &str) {
        let tx = client_library::schema::library_write_transaction(conn).unwrap();
        client_library::placements::move_directory_record(&tx, &id, None, Some(name)).unwrap();
        tx.commit().unwrap();
    }

    #[test]
    fn prepared_folder_rechecks_metadata_before_moving() {
        let (root, handle, mut conn, id) = fixture();
        rename(&mut conn, id, "Destination");
        let prepared = PreparedDirectory::inspect(&handle, &conn, id).unwrap().unwrap();
        rename(&mut conn, id, "Latest");
        assert!(!prepared.publish(&mut conn).unwrap());
        assert!(!root.path().join("Destination").exists());
        prepare_directory(&handle, &mut conn, id).unwrap();
        assert_eq!(std::fs::read(root.path().join("Latest/notes.txt")).unwrap(), b"preserved");
    }

    #[test]
    fn prepared_folder_preserves_a_new_destination_and_rejects_changed_identity() {
        let (root, handle, mut conn, id) = fixture();
        rename(&mut conn, id, "Destination");
        let prepared = PreparedDirectory::inspect(&handle, &conn, id).unwrap().unwrap();
        std::fs::create_dir(root.path().join("Destination")).unwrap();
        std::fs::write(root.path().join("Destination/foreign.txt"), b"unrelated").unwrap();
        assert!(!prepared.publish(&mut conn).unwrap());
        let prepared = PreparedDirectory::inspect(&handle, &conn, id).unwrap().unwrap();
        std::fs::write(root.path().join("Source/.biblos_uuid"), b"replaced identity").unwrap();
        assert!(!prepared.publish(&mut conn).unwrap());
        assert!(PreparedDirectory::inspect(&handle, &conn, id).is_err());
        std::fs::write(root.path().join("Source/.biblos_uuid"), id.to_string()).unwrap();
        prepare_directory(&handle, &mut conn, id).unwrap();
        assert_eq!(std::fs::read(root.path().join("Destination/foreign.txt")).unwrap(), b"unrelated");
        assert_eq!(std::fs::read(root.path().join("Destination 2/notes.txt")).unwrap(), b"preserved");
    }

    #[test]
    fn failed_parent_keeps_its_child_pending_but_does_not_block_a_sibling() {
        let (root, handle, mut conn, parent) = fixture();
        let child = client_library::placements::create_directory(&mut conn, &parent, &"Child".into()).unwrap();
        let sibling = client_library::placements::create_directory(&mut conn, &crate::ROOT_DIR_ID, &"Independent".into()).unwrap();
        rename(&mut conn, parent, "Destination");
        std::fs::write(root.path().join("Source/.biblos_uuid"), b"changed identity").unwrap();

        assert!(prepare_directory_work(&handle, &mut conn).unwrap_err().to_string().contains("folder identity"));
        assert!(root.path().join("Independent/.biblos_uuid").exists());
        assert!(!root.path().join("Destination").exists());
        assert_eq!(std::fs::read(root.path().join("Source/notes.txt")).unwrap(), b"preserved");
        for (id, pending) in [(parent, true), (child.id, true), (sibling.id, false)] {
            assert_eq!(conn.query_row(include_str!("../sql/native_projection/failed_parent_keeps_its_child_pending_but_does_not_block_a_sibling_select.sql"), [id.to_string()], |row| row.get::<_, bool>(0)).unwrap(), pending);
        }

        std::fs::write(root.path().join("Source/.biblos_uuid"), parent.to_string()).unwrap();
        prepare_directory_work(&handle, &mut conn).unwrap();
        assert!(root.path().join("Destination/Child/.biblos_uuid").exists());
        assert!(!client_library::file_work::has_pending_file_work(&conn).unwrap());
    }

    #[test]
    fn overlapping_jobs_visit_each_folder_once_even_when_parent_was_queued_last() {
        let (_root, handle, mut conn, parent) = fixture();
        let child = client_library::placements::create_directory(&mut conn, &parent, &"Child".into()).unwrap();
        client_library::placements::create_directory(&mut conn, &child.id, &"Grandchild".into()).unwrap();
        client_library::file_work::queue_directory_work(&conn, &parent).unwrap();
        conn.execute_batch(include_str!("../sql/native_projection/overlapping_jobs_visit_each_folder_once_even_when_parent_was_queued_last_create.sql")).unwrap();
        prepare_directory_work(&handle, &mut conn).unwrap();
        assert_eq!(conn.query_row(include_str!("../sql/native_projection/overlapping_jobs_visit_each_folder_once_even_when_parent_was_queued_last_select.sql"), [], |row| row.get::<_, i64>(0)).unwrap(), 3);
        assert!(!client_library::file_work::has_pending_file_work(&conn).unwrap());
    }

    #[test]
    fn coalesced_pass_preserves_a_child_request_queued_during_publication() {
        let (root, handle, mut conn, parent) = fixture();
        let child = client_library::placements::create_directory(&mut conn, &parent, &"Child".into()).unwrap();
        let initial: i64 = conn.query_row(include_str!("../sql/native_projection/coalesced_pass_preserves_a_child_request_queued_during_publication_select.sql"), [child.id.to_string()], |row| row.get(0)).unwrap();
        conn.execute_batch(include_str!("../sql/native_projection/test_projection_scope_schema.sql")).unwrap();
        conn.execute(include_str!("../sql/native_projection/test_projection_scope_insert.sql"), rusqlite::params![parent.to_string(), child.id.to_string()]).unwrap();
        conn.execute_batch(include_str!("../sql/native_projection/coalesced_pass_preserves_a_child_request_queued_during_publication_query.sql"))
        .unwrap();
        prepare_directory_work(&handle, &mut conn).unwrap();
        let retained: i64 = conn.query_row(include_str!("../sql/native_projection/coalesced_pass_preserves_a_child_request_queued_during_publication_select.sql"), [child.id.to_string()], |row| row.get(0)).unwrap();
        assert!(retained > initial);
        assert!(root.path().join("Source/Child/.biblos_uuid").exists());
        conn.execute_batch(include_str!("../sql/native_projection/coalesced_pass_preserves_a_child_request_queued_during_publication_drop.sql")).unwrap();
        prepare_directory_work(&handle, &mut conn).unwrap();
        assert!(!client_library::file_work::has_pending_file_work(&conn).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn a_blocked_marker_read_does_not_hold_sqlites_writer() {
        use std::io::Write as _;
        let (root, handle, conn, id) = fixture();
        let marker = root.path().join("Source/.biblos_uuid");
        std::fs::remove_file(&marker).unwrap();
        assert!(std::process::Command::new("mkfifo").arg(&marker).status().unwrap().success());
        let (opened_tx, opened_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let writer = std::thread::spawn(move || {
            let mut fifo = std::fs::OpenOptions::new().write(true).open(marker).unwrap();
            opened_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            fifo.write_all(id.to_string().as_bytes()).unwrap();
        });
        let inspection = std::thread::spawn(move || PreparedDirectory::inspect(&handle, &conn, id).map(|_| ()));
        opened_rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        let other = rusqlite::Connection::open(root.path().join("test.db")).unwrap();
        other.busy_timeout(std::time::Duration::ZERO).unwrap();
        let update = other.execute(include_str!("../sql/native_projection/a_blocked_marker_read_does_not_hold_sqlites_writer_insert.sql"), []);
        release_tx.send(()).unwrap();
        writer.join().unwrap();
        inspection.join().unwrap().unwrap();
        assert!(update.is_ok(), "a stalled directory inspection must not block an unrelated write: {update:?}");
    }
}
