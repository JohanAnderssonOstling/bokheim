use library_database::{Database, ImportCommit, NativeDirectoryAcknowledgement, NativeDirectoryCompletion, PublishedImport};
use sync_common::{ContentHash, DirId, ROOT_DIR_ID, StateAddress};

fn database() -> Database {
    let db = Database::open(":memory:").unwrap();
    db.initialize_library().unwrap();
    db
}

fn import() -> ImportCommit {
    let hash = ContentHash::new(&"9".repeat(64));
    ImportCommit {
        parent_id: ROOT_DIR_ID,
        file_name: "book.epub".into(),
        format: book_model::BookFormat::Epub,
        content_hash: hash,
        checksum: hash,
        size_bytes: 10,
        inspection_version: 1,
        inspection: Some(book_metadata::InspectedBook {
            pdf: None,
            audiobook: None,
            metadata: book_model::BookRecord { metadata: book_model::BookDetails { title: "Original".into(), subtitle: None, contributors: vec![book_model::Contributor::new("Original Author", book_model::MarcRelatorCode(*b"aut")).unwrap()], book: Default::default() }, description: String::new() },
            toc: vec![book_model::BookTocEntry { title: "Chapter".into(), target: "ch1".into(), children: vec![] }],
        }),
        published: PublishedImport { name: "book.epub".into(), relative_path: "/book.epub".into(), published: false, fingerprint: None },
        request_thumbnail: false,
        restore_paths: vec![],
    }
}

#[test]
#[cfg(feature = "scanner")]
fn scan_recovery_finishes_only_with_a_readable_current_final_batch() {
    use library_database::{FilesystemScan, InspectedFilesystemScan, ScanApplyResult};
    let db = database();
    let discovery = FilesystemScan { readable: true, ..Default::default() };
    let inspected = InspectedFilesystemScan { readable: true, ..Default::default() };
    let mut snapshot = db.scan_snapshot().unwrap();
    assert!(db.needs_scan().unwrap());
    db.apply_filesystem_scan_batch(&mut snapshot, &discovery, &inspected, false).unwrap();
    assert!(({ use library_database::{DatabaseError, LibraryWork}; use library_database::assets::ActivitySql; let database = &db; (|| -> Result<LibraryWork, DatabaseError> {
        let mut work = None;
        database.connection.activity_progress(|row| {
            let count = |index| row.get::<_, i64>(index).map(|value| value.max(0) as u64);
            work = Some(LibraryWork {
                total_books: count(0)?, uploaded_books: count(1)?,
                pending_uploads: count(2)?, pending_changes: count(3)?,
                pending_thumbnails: count(4)?, pending_covers: count(5)?,
                scan_pending: row.get(6)?, scan_failed: row.get(7)?, sync_configured: row.get(8)?,
            });
            Ok(())
        })?;
        work.ok_or_else(|| DatabaseError::message("library activity query returned no row"))
    })() }).unwrap().scan_pending);
    let current = db.scan_snapshot().unwrap();
    assert_eq!(db.apply_filesystem_scan(&snapshot, &discovery, &inspected).unwrap(), ScanApplyResult::Stale);
    assert!(db.needs_scan().unwrap());
    db.apply_filesystem_scan(&current, &discovery, &inspected).unwrap();
    assert!(!db.needs_scan().unwrap());
    assert!(!({ use library_database::{DatabaseError, LibraryWork}; use library_database::assets::ActivitySql; let database = &db; (|| -> Result<LibraryWork, DatabaseError> {
        let mut work = None;
        database.connection.activity_progress(|row| {
            let count = |index| row.get::<_, i64>(index).map(|value| value.max(0) as u64);
            work = Some(LibraryWork {
                total_books: count(0)?, uploaded_books: count(1)?,
                pending_uploads: count(2)?, pending_changes: count(3)?,
                pending_thumbnails: count(4)?, pending_covers: count(5)?,
                scan_pending: row.get(6)?, scan_failed: row.get(7)?, sync_configured: row.get(8)?,
            });
            Ok(())
        })?;
        work.ok_or_else(|| DatabaseError::message("library activity query returned no row"))
    })() }).unwrap().scan_pending);
    let snapshot = db.scan_snapshot().unwrap();
    db.apply_filesystem_scan(&snapshot, &FilesystemScan::default(), &InspectedFilesystemScan::default()).unwrap();
    let work = ({ use library_database::{DatabaseError, LibraryWork}; use library_database::assets::ActivitySql; let database = &db; (|| -> Result<LibraryWork, DatabaseError> {
        let mut work = None;
        database.connection.activity_progress(|row| {
            let count = |index| row.get::<_, i64>(index).map(|value| value.max(0) as u64);
            work = Some(LibraryWork {
                total_books: count(0)?, uploaded_books: count(1)?,
                pending_uploads: count(2)?, pending_changes: count(3)?,
                pending_thumbnails: count(4)?, pending_covers: count(5)?,
                scan_pending: row.get(6)?, scan_failed: row.get(7)?, sync_configured: row.get(8)?,
            });
            Ok(())
        })?;
        work.ok_or_else(|| DatabaseError::message("library activity query returned no row"))
    })() }).unwrap();
    assert!(work.scan_pending && work.scan_failed);
    let snapshot = db.scan_snapshot().unwrap();
    db.apply_filesystem_scan(&snapshot, &discovery, &inspected).unwrap();
    let work = ({ use library_database::{DatabaseError, LibraryWork}; use library_database::assets::ActivitySql; let database = &db; (|| -> Result<LibraryWork, DatabaseError> {
        let mut work = None;
        database.connection.activity_progress(|row| {
            let count = |index| row.get::<_, i64>(index).map(|value| value.max(0) as u64);
            work = Some(LibraryWork {
                total_books: count(0)?, uploaded_books: count(1)?,
                pending_uploads: count(2)?, pending_changes: count(3)?,
                pending_thumbnails: count(4)?, pending_covers: count(5)?,
                scan_pending: row.get(6)?, scan_failed: row.get(7)?, sync_configured: row.get(8)?,
            });
            Ok(())
        })?;
        work.ok_or_else(|| DatabaseError::message("library activity query returned no row"))
    })() }).unwrap();
    assert!(!work.scan_pending && !work.scan_failed);
}

#[test]
fn activity_reads_report_pending_work_without_writing_lifecycle_records() {
    let db = database();
    let book = import();
    let hash = book.content_hash;
    db.commit_import(book).unwrap();
    db.connection.execute("INSERT INTO book_transfer(content_hash,upload_id,upload_checksum,upload_size) VALUES(?1,1,?1,10) ON CONFLICT(content_hash) DO UPDATE SET upload_id=COALESCE(upload_id,1),upload_checksum=COALESCE(upload_checksum,excluded.upload_checksum),upload_size=COALESCE(upload_size,10)", [hash.as_str()]).unwrap();
    let before = db.connection.total_changes();
    for _ in 0..3 {
        let work = ({ use library_database::{DatabaseError, LibraryWork}; use library_database::assets::ActivitySql; let database = &db; (|| -> Result<LibraryWork, DatabaseError> {
        let mut work = None;
        database.connection.activity_progress(|row| {
            let count = |index| row.get::<_, i64>(index).map(|value| value.max(0) as u64);
            work = Some(LibraryWork {
                total_books: count(0)?, uploaded_books: count(1)?,
                pending_uploads: count(2)?, pending_changes: count(3)?,
                pending_thumbnails: count(4)?, pending_covers: count(5)?,
                scan_pending: row.get(6)?, scan_failed: row.get(7)?, sync_configured: row.get(8)?,
            });
            Ok(())
        })?;
        work.ok_or_else(|| DatabaseError::message("library activity query returned no row"))
    })() }).unwrap();
        assert_eq!(work.total_books, 1);
        assert_eq!(work.pending_uploads, 1);
        assert!(work.pending_changes > 0);
        assert!(!work.sync_configured);
    }
    assert_eq!(db.connection.total_changes(), before);
    { use library_database::assets::ActivitySql; db.connection.activity_set_last_successful_sync_at("123").unwrap(); }
    let work = ({ use library_database::{DatabaseError, LibraryWork}; use library_database::assets::ActivitySql; let database = &db; (|| -> Result<LibraryWork, DatabaseError> {
        let mut work = None;
        database.connection.activity_progress(|row| {
            let count = |index| row.get::<_, i64>(index).map(|value| value.max(0) as u64);
            work = Some(LibraryWork {
                total_books: count(0)?, uploaded_books: count(1)?,
                pending_uploads: count(2)?, pending_changes: count(3)?,
                pending_thumbnails: count(4)?, pending_covers: count(5)?,
                scan_pending: row.get(6)?, scan_failed: row.get(7)?, sync_configured: row.get(8)?,
            });
            Ok(())
        })?;
        work.ok_or_else(|| DatabaseError::message("library activity query returned no row"))
    })() }).unwrap();
    assert!(work.sync_configured);
    assert_eq!(work.pending_uploads, 1);
    assert!(work.pending_changes > 0, "a sync checkpoint does not erase unfinished work");
}

#[test]
#[cfg(feature = "scanner")]
fn scan_recovery_survives_reopening_between_batches_without_activity_reads() {
    use library_database::{FilesystemScan, InspectedFilesystemScan};
    let root = std::env::temp_dir().join(format!("scan-recovery-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("library.sqlite");
    let discovery = FilesystemScan { readable: true, ..Default::default() };
    let inspected = InspectedFilesystemScan { readable: true, ..Default::default() };
    {
        let db = Database::open(&path).unwrap();
        db.initialize_library().unwrap();
        let mut snapshot = db.scan_snapshot().unwrap();
        db.apply_filesystem_scan_batch(&mut snapshot, &discovery, &inspected, false).unwrap();
    }
    {
        let db = Database::open(&path).unwrap();
        db.initialize_library().unwrap();
        assert!(db.needs_scan().unwrap());
        let snapshot = db.scan_snapshot().unwrap();
        db.apply_filesystem_scan(&snapshot, &discovery, &inspected).unwrap();
    }
    {
        let db = Database::open(&path).unwrap();
        db.initialize_library().unwrap();
        assert!(!db.needs_scan().unwrap());
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn old_library_schemas_are_refused_without_modification() {
    for version in [8, 9, 10, 11] {
        let db = database();
        db.connection.pragma_update(None, "user_version", version).unwrap();
        assert!(db.initialize_library().unwrap_err().to_string().contains("unsupported library schema version"));
        let retained: i64 = db.connection.pragma_query_value(None, "user_version", |row| row.get(0)).unwrap();
        assert_eq!(retained, version);
    }
}

/// Imports are the only local metadata writer: a later import with changed
/// facts must publish them.
fn assert_title_edit_syncs(db: &Database) {
    db.clear_sync_outbox().unwrap();
    let mut edited = import();
    edited.inspection.as_mut().unwrap().metadata.title = "Edited".into();
    db.commit_import(edited).unwrap();
    let mutations = db.sync_publishable_mutations().unwrap();
    assert!(mutations.iter().any(|mutation| matches!(&mutation.state.value, library_replica::LibraryStateEntry::Metadata { value, .. } if value.title == "Edited")));
}

#[test]
fn successful_import_publishes_contributors_and_restores_metadata_sync() {
    let db = database();
    db.commit_import(import()).unwrap();
    let mutations = db.sync_publishable_mutations().unwrap();
    assert!(mutations.iter().any(|mutation| matches!(mutation.state.value, library_replica::LibraryStateEntry::BookLifecycle { value: library_replica::BookLifecycleState::Present, .. })));
    let metadata = mutations
        .iter()
        .find_map(|mutation| match &mutation.state.value {
            library_replica::LibraryStateEntry::Metadata { value, .. } => Some(value),
            _ => None,
        })
        .expect("import must publish its metadata register");
    assert_eq!(metadata.title, "Original");
    assert_eq!(metadata.contributors.len(), 1);
    assert_eq!(metadata.contributors[0].name(), "Original Author");
    assert_title_edit_syncs(&db);
}

#[test]
fn import_without_contributors_still_publishes_metadata() {
    let db = database();
    let mut book = import();
    book.inspection.as_mut().unwrap().metadata.contributors.clear();
    db.commit_import(book).unwrap();
    assert!(db.sync_publishable_mutations().unwrap().iter().any(|mutation| matches!(&mutation.state.value,
        library_replica::LibraryStateEntry::Metadata { value, .. } if value.title == "Original" && value.contributors.is_empty())));
}

#[test]
fn failed_contributor_write_rolls_back_and_restores_metadata_sync() {
    let db = database();
    db.commit_import(import()).unwrap();
    let queued_before = db.sync_publishable_mutations().unwrap();
    let identities_before: i64 = db.connection.query_row("SELECT count(*) FROM author_identity", [], |row| row.get(0)).unwrap();
    db.connection.execute_batch("CREATE TRIGGER reject_contributor BEFORE INSERT ON book_contributor BEGIN SELECT RAISE(ABORT,'injected contributor failure'); END;").unwrap();
    let changed_import = || {
        let mut next = import();
        next.inspection.as_mut().unwrap().metadata.title = "Changed title".into();
        // Unchanged contributors skip the INSERT entirely, so change the author
        // to ensure the injected failure exercises the transaction boundary.
        next.inspection.as_mut().unwrap().metadata.contributors = vec![book_model::Contributor::new("Changed author", book_model::MarcRelatorCode(*b"aut")).unwrap()];
        next
    };
    let error = db.commit_import(changed_import()).unwrap_err();
    assert!(error.to_string().contains("injected contributor failure"), "unexpected import failure: {error}");
    assert_eq!(db.connection.query_row("SELECT title FROM book", [], |r| r.get::<_, String>(0)).unwrap(), "Original");
    assert_eq!(db.connection.query_row("SELECT name FROM book_contributor", [], |r| r.get::<_, String>(0)).unwrap(), "Original Author");
    assert_eq!(db.connection.query_row("SELECT count(*) FROM author_identity", [], |row| row.get::<_, i64>(0)).unwrap(), identities_before, "failed import must not leave a new author identity");
    assert_eq!(db.sync_publishable_mutations().unwrap(), queued_before, "failed import must not change pending sync records");
    assert_title_edit_syncs(&db);
    db.connection.execute_batch("DROP TRIGGER reject_contributor;").unwrap();
    db.commit_import(changed_import()).unwrap();
    assert_eq!(db.connection.query_row("SELECT title FROM book", [], |r| r.get::<_, String>(0)).unwrap(), "Changed title");
    assert_eq!(db.connection.query_row("SELECT name FROM book_contributor", [], |r| r.get::<_, String>(0)).unwrap(), "Changed author");
}

fn recovery_payloads(db: &Database) -> Vec<(String, String, Vec<u8>)> {
    db.sync_publishable_mutations().unwrap();
    db.connection
        .prepare("SELECT state_kind,state_subkey,body FROM sync_outbox WHERE state_kind IN ('book_toc','pdf_reader_metadata') ORDER BY state_kind,state_subkey")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn recovery_republishes_toc_and_each_pdf_checksum() {
    let db = database();
    let book = import();
    let hash = book.content_hash;
    db.commit_import(book).unwrap();
    let mut cells = vec![StateAddress { kind: "book_toc".into(), entity_key: hash.to_string(), entity_subkey: String::new() }];
    for (checksum, width) in [("a".repeat(64), 100.0), ("b".repeat(64), 200.0)] {
        let metadata =
            pdf_view_common::PdfReaderMetadata { version: 1, checksum: checksum.clone(), pages: vec![pdf_view_common::PdfStoredPage { width, height: 300.0, rotation: 0 }], skippable_pages: None, annotations: vec![], dependencies: None };
        let mut inspected = import();
        inspected.checksum = ContentHash::new(&checksum);
        inspected.inspection.as_mut().unwrap().pdf = Some(metadata);
        db.commit_import(inspected).unwrap();
        cells.push(StateAddress { kind: "pdf_reader_metadata".into(), entity_key: hash.to_string(), entity_subkey: checksum });
    }
    let expected = recovery_payloads(&db);
    assert_eq!(expected.len(), 3);
    db.clear_sync_outbox().unwrap();
    assert_eq!(db.sync_enqueue_missing_state_cells(&cells).unwrap(), 3);
    assert_eq!(recovery_payloads(&db), expected);
    assert_eq!(db.sync_enqueue_missing_state_cells(&cells).unwrap(), 0);
    db.clear_sync_outbox().unwrap();
    db.sync_begin_cursor_recovery().unwrap();
    let mut after = None;
    loop {
        let (next, _) = db.sync_recover_state_page(after.as_ref()).unwrap();
        if next.is_none() {
            break;
        }
        after = next;
    }
    assert_eq!(recovery_payloads(&db), expected);
    assert!(!db.sync_cursor_recovery_pending().unwrap());
    let missing = StateAddress { kind: "pdf_reader_metadata".into(), entity_key: hash.to_string(), entity_subkey: "c".repeat(64) };
    assert_eq!(db.sync_enqueue_missing_state_cells(&[missing]).unwrap(), 0);
}

fn materialize(db: &Database, id: DirId, renamed_to: Option<&str>) {
    let snapshot = db.native_directory_snapshot(&id).unwrap().unwrap();
    assert!(snapshot.relative_path.starts_with('/'));
    assert_eq!(db.acknowledge_native_directory(NativeDirectoryCompletion { snapshot, renamed_to: renamed_to.map(str::to_owned), materialized: true }).unwrap(), NativeDirectoryAcknowledgement::Applied);
}

fn projection(db: &Database, id: DirId) -> String {
    db.connection.query_row("SELECT relative_path FROM local_file_projection WHERE dir_id=?1", [id.to_string()], |r| r.get(0)).unwrap()
}

#[test]
fn directory_renames_move_descendants_but_not_similarly_named_siblings() {
    let db = database();
    let shelf = db.create_directory(&ROOT_DIR_ID, &"Shelf".into()).unwrap().id;
    let child = db.create_directory(&shelf, &"Child".into()).unwrap().id;
    let sibling = db.create_directory(&ROOT_DIR_ID, &"Shelfish".into()).unwrap().id;
    let hash = ContentHash::new(&"9".repeat(64));
    for (id, path) in [(shelf, "/Shelf/book.epub"), (child, "/Shelf/Child/book.epub"), (sibling, "/Shelfish/book.epub")] {
        materialize(&db, id, None);
        db.seed_file_projection(&hash, &id, path);
    }
    materialize(&db, shelf, Some("Shelf (2)"));
    assert_eq!(projection(&db, shelf), "/Shelf (2)/book.epub");
    assert_eq!(projection(&db, child), "/Shelf (2)/Child/book.epub");
    assert_eq!(projection(&db, sibling), "/Shelfish/book.epub");
    assert_eq!(db.native_directory_snapshot(&child).unwrap().unwrap().projected_path.as_deref(), Some("/Shelf (2)/Child"));
    db.move_directory(&shelf, None, Some("Moved")).unwrap();
    materialize(&db, shelf, None);
    assert_eq!(projection(&db, child), "/Moved/Child/book.epub");
}

#[test]
fn directory_acknowledgement_rolls_back_when_projection_write_fails() {
    let db = database();
    let shelf = db.create_directory(&ROOT_DIR_ID, &"Shelf".into()).unwrap().id;
    materialize(&db, shelf, None);
    db.connection.execute_batch("CREATE TRIGGER reject_projection BEFORE UPDATE ON local_directory_projection BEGIN SELECT RAISE(ABORT,'injected projection failure'); END;").unwrap();
    let snapshot = db.native_directory_snapshot(&shelf).unwrap().unwrap();
    assert!(db.acknowledge_native_directory(NativeDirectoryCompletion { snapshot, renamed_to: Some("Changed".into()), materialized: true }).is_err());
    let current = db.native_directory_snapshot(&shelf).unwrap().unwrap();
    assert_eq!(current.name, "Shelf");
    assert_eq!(current.projected_path.as_deref(), Some("/Shelf"));
}

#[test]
fn stored_audiobook_navigation_syncs_with_its_duration() {
    let db = database();
    let hash = ContentHash::new(&"a".repeat(64));
    db.connection.execute("INSERT INTO book(content_hash) VALUES(?1)", [hash.as_str()]).unwrap();
    let entries = vec![book_model::BookTocEntry { title: "Only".into(), target: book_model::audiobook_toc_target(0), children: Vec::new() }];
    db.clear_sync_outbox().unwrap();
    db.commit_audiobook_metadata(&hash, &book_metadata::AudiobookMetadata { title: "Only".into(), authors: Vec::new(), description: String::new(), duration_ms: 1000, chapters: vec![book_metadata::AudiobookChapterMetadata { title: "Only".into(), start_ms: 0, end_ms: 1000 }], tracks: Vec::new(), chapter_origin: book_model::ChapterOrigin::EmbeddedM4b, recording: Default::default() }).unwrap();
    assert!(db.sync_publishable_mutations().unwrap().iter().any(|mutation| matches!(&mutation.state.value, library_replica::LibraryStateEntry::BookToc { value, .. } if value.entries == entries && value.duration_ms == Some(1000))));
}

fn copy_fixture(db: &Database) -> (ContentHash, DirId, DirId) {
    let source = db.create_directory(&ROOT_DIR_ID, &"A".into()).unwrap().id;
    let destination = db.create_directory(&ROOT_DIR_ID, &"Z".into()).unwrap().id;
    let mut book = import();
    let hash = book.content_hash;
    book.parent_id = source;
    book.published.relative_path = "/A/book.epub".into();
    db.commit_import(book).unwrap();
    db.transfer_book_placement(&hash, &source, &destination, false).unwrap();
    (hash, source, destination)
}

fn restore_snapshot(db: &Database) -> library_database::NativeFileWorkSnapshot {
    let work = db.native_file_work().unwrap().into_iter().find(|work| work.operation == "materialize").unwrap();
    db.native_file_work_snapshot(work).unwrap().unwrap()
}

fn restore_completion(snapshot: library_database::NativeFileWorkSnapshot) -> library_database::NativeFileWorkCompletion {
    library_database::NativeFileWorkCompletion { final_path: snapshot.resolved_path.to_string(), snapshot, renamed: None, published: true, thumbnail_set_exists: true }
}

#[test]
fn copy_restores_and_acknowledges_the_queued_destination() {
    let db = database();
    let (_, source, destination) = copy_fixture(&db);
    let snapshot = restore_snapshot(&db);
    assert_eq!(snapshot.resolved_path.as_str(), "/Z/book.epub");
    assert_eq!(snapshot.placement.as_ref().unwrap().0, destination.to_string());
    db.acknowledge_native_file_work(restore_completion(snapshot)).unwrap();
    assert_eq!(projection(&db, source), "/A/book.epub");
    assert_eq!(projection(&db, destination), "/Z/book.epub");
    assert!(db.native_file_work().unwrap().is_empty());
}

#[test]
fn directory_move_keeps_queued_restore_bound_to_its_destination() {
    let db = database();
    let (_, _, destination) = copy_fixture(&db);
    materialize(&db, destination, None);
    db.move_directory(&destination, None, Some("Moved")).unwrap();
    materialize(&db, destination, None);
    let snapshot = restore_snapshot(&db);
    assert_eq!(snapshot.resolved_path.as_str(), "/Moved/book.epub");
    assert_eq!(snapshot.placement.as_ref().unwrap().0, destination.to_string());
    db.acknowledge_native_file_work(restore_completion(snapshot)).unwrap();
    assert_eq!(projection(&db, destination), "/Moved/book.epub");
}

#[test]
fn stale_restore_does_not_overwrite_projection_or_consume_work() {
    let db = database();
    let (_, _, destination) = copy_fixture(&db);
    let snapshot = restore_snapshot(&db);
    db.move_directory(&destination, None, Some("Moved")).unwrap();
    assert!(db.acknowledge_native_file_work(restore_completion(snapshot)).is_err());
    assert!(!db.native_file_work().unwrap().is_empty());
    let projections: i64 = db.connection.query_row("SELECT count(*) FROM local_file_projection WHERE dir_id=?1", [destination.to_string()], |row| row.get(0)).unwrap();
    assert_eq!(projections, 0);
}

#[test]
fn trash_work_does_not_target_another_live_copy() {
    let db = database();
    let (hash, source, destination) = copy_fixture(&db);
    db.acknowledge_native_file_work(restore_completion(restore_snapshot(&db))).unwrap();
    db.remove_book_placement(&hash, &source).unwrap();
    assert!(db.native_file_work().unwrap().is_empty(), "soft removal must retain physical copies");
    assert_eq!(projection(&db, source), "/A/book.epub");
    assert_eq!(projection(&db, destination), "/Z/book.epub");
}

#[cfg(feature = "scanner")]
mod scanner {
    use super::*;
    use library_database::{FilesystemScan, InspectedFilesystemScan, ScanApplyResult, ScanFileObservation, ScannedBook};

    fn fixture() -> (Database, ContentHash) {
        let db = database();
        let mut book = import();
        let hash = book.content_hash;
        book.published.published = true;
        book.published.fingerprint = Some("1".into());
        db.commit_import(book).unwrap();
        drain_work(&db);
        (db, hash)
    }

    fn drain_work(db: &Database) {
        db.connection.execute_batch("DELETE FROM local_file_work; DELETE FROM local_book_work; DELETE FROM local_directory_work;").unwrap();
    }

    fn observation(directory: DirId, name: &str, fingerprint: u64) -> ScanFileObservation {
        ScanFileObservation { directory, name: name.into(), path: name.into(), fingerprint }
    }

    fn replacement() -> ScannedBook {
        let hash = ContentHash::new(&"b".repeat(64));
        ScannedBook {
            directory: ROOT_DIR_ID,
            name: "book.epub".into(),
            path: "book.epub".into(),
            fingerprint: 2,
            content_hash: hash,
            checksum: hash,
            size_bytes: 20,
            extension: "epub".into(),
            inspection: import().inspection.unwrap(),
            inspection_version: 1,
        }
    }

    fn local_state(db: &Database, hash: ContentHash, directory: DirId) -> (bool, bool, String, i64) {
        db.connection.query_row("SELECT bd.deleted_at IS NULL,bd.is_downloaded,bd.local_hash,(SELECT count(*) FROM local_file_projection p WHERE p.content_hash=b.content_hash AND p.dir_id=bd.dir_id) FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id WHERE b.content_hash=?1 AND bd.dir_id=?2", library_database::rusqlite::params![hash.as_str(), directory.to_string()], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).unwrap()
    }

    #[test]
    fn scan_batches_publish_books_before_final_missing_file_cleanup() {
        let (db, missing) = fixture();
        let mut snapshot = db.scan_snapshot().unwrap();
        let mut first = replacement();
        first.name = "first.epub".into();
        first.path = first.name.clone();
        let first_hash = first.content_hash;
        let discovery = FilesystemScan { readable: true, files: vec![observation(ROOT_DIR_ID, "first.epub", 2), observation(ROOT_DIR_ID, "second.epub", 2)], ..Default::default() };
        assert_eq!(db.apply_filesystem_scan_batch(&mut snapshot, &discovery, &InspectedFilesystemScan { failures: vec![], readable: true, books: vec![first] }, false).unwrap(), ScanApplyResult::Applied);
        assert_eq!(({ use library_database::browse::{CountsSql,HomeSql}; use library_database::DatabaseError; use library_model::LibraryHomeView; let database = &db;  (|| -> Result<usize, DatabaseError> {
        let mut count = 0_i64;
        database.connection.count_all_books(|row| {
            count = row.get(0)?;
            Ok(())
        })?;
        Ok(usize::try_from(count).map_err(|_| DatabaseError::message("library book count must not be negative"))?)
    })() }).unwrap(), 2, "first batch is immediately browsable");
        assert!(local_state(&db, first_hash, ROOT_DIR_ID).1);
        assert!(local_state(&db, missing, ROOT_DIR_ID).1, "incomplete scans must not clear missing files");
        let mut second = replacement();
        second.name = "second.epub".into();
        second.path = second.name.clone();
        second.content_hash = ContentHash::new(&"c".repeat(64));
        second.checksum = second.content_hash;
        assert_eq!(db.apply_filesystem_scan_batch(&mut snapshot, &discovery, &InspectedFilesystemScan { failures: vec![], readable: true, books: vec![second] }, false).unwrap(), ScanApplyResult::Applied);
        assert_eq!(({ use library_database::browse::{CountsSql,HomeSql}; use library_database::DatabaseError; use library_model::LibraryHomeView; let database = &db;  (|| -> Result<usize, DatabaseError> {
        let mut count = 0_i64;
        database.connection.count_all_books(|row| {
            count = row.get(0)?;
            Ok(())
        })?;
        Ok(usize::try_from(count).map_err(|_| DatabaseError::message("library book count must not be negative"))?)
    })() }).unwrap(), 3);
        assert_eq!(db.apply_filesystem_scan_batch(&mut snapshot, &discovery, &InspectedFilesystemScan { failures: vec![], readable: true, books: vec![] }, true).unwrap(), ScanApplyResult::Applied);
        assert!(!local_state(&db, missing, ROOT_DIR_ID).1);
        assert!(local_state(&db, first_hash, ROOT_DIR_ID).1);
    }

    #[test]
    fn replacement_updates_only_the_old_placement_and_commits_other_discoveries() {
        let (db, old) = fixture();
        let other = db.create_directory(&ROOT_DIR_ID, &"Other".into()).unwrap().id;
        db.transfer_book_placement(&old, &ROOT_DIR_ID, &other, false).unwrap();
        db.acknowledge_native_file_work(restore_completion(restore_snapshot(&db))).unwrap();
        drain_work(&db);
        let snapshot = db.scan_snapshot().unwrap();
        let replaced = replacement();
        let new_hash = replaced.content_hash;
        let mut extra = replacement();
        extra.name = "another.epub".into();
        extra.path = extra.name.clone();
        extra.content_hash = ContentHash::new(&"c".repeat(64));
        extra.checksum = extra.content_hash;
        let discovery = FilesystemScan {
            readable: true,
            directories: vec![],
            files: vec![observation(ROOT_DIR_ID, "book.epub", 2), observation(ROOT_DIR_ID, "another.epub", 2), ScanFileObservation { path: "Other/book.epub".into(), ..observation(other, "book.epub", 1) }],
        };
        assert_eq!(db.apply_filesystem_scan(&snapshot, &discovery, &InspectedFilesystemScan { failures: vec![], readable: true, books: vec![replaced, extra] }).unwrap(), ScanApplyResult::Applied);
        assert_eq!(local_state(&db, old, ROOT_DIR_ID), (false, false, String::new(), 0));
        assert!(local_state(&db, old, other).0);
        assert!(local_state(&db, old, other).1);
        assert_eq!(local_state(&db, new_hash, ROOT_DIR_ID), (true, true, "2".into(), 1));
        assert_eq!(db.connection.query_row("SELECT count(*) FROM book WHERE deleted_at IS NULL", [], |r| r.get::<_, i64>(0)).unwrap(), 3);
    }

    #[test]
    fn failed_replacement_rolls_back_old_placement_and_projection() {
        let (db, old) = fixture();
        let snapshot = db.scan_snapshot().unwrap();
        db.connection.execute_batch("CREATE TRIGGER fail_scan_import BEFORE INSERT ON book BEGIN SELECT RAISE(ABORT,'injected import failure'); END;").unwrap();
        let result = db.apply_filesystem_scan(
            &snapshot,
            &FilesystemScan { readable: true, files: vec![observation(ROOT_DIR_ID, "book.epub", 2)], ..Default::default() },
            &InspectedFilesystemScan { failures: vec![], readable: true, books: vec![replacement()] },
        );
        assert!(result.is_err());
        assert_eq!(local_state(&db, old, ROOT_DIR_ID), (true, true, "1".into(), 1));
    }

    #[test]
    fn ordinary_import_still_rejects_an_occupied_filename() {
        let (db, old) = fixture();
        let mut book = import();
        book.content_hash = ContentHash::new(&"b".repeat(64));
        assert!(db.commit_import(book).is_err());
        assert_eq!(local_state(&db, old, ROOT_DIR_ID), (true, true, "1".into(), 1));
    }

    #[test]
    fn complete_scan_clears_only_missing_local_availability_without_sync_deletions() {
        let (db, hash) = fixture();
        // Downloads can have an empty fingerprint and must still be reconciled.
        db.connection.execute("UPDATE book_dir SET local_hash=''", []).unwrap();
        db.clear_sync_outbox().unwrap();
        let snapshot = db.scan_snapshot().unwrap();
        assert!(snapshot.excluded_paths.is_empty());
        assert_eq!(db.apply_filesystem_scan(&snapshot, &FilesystemScan { readable: true, ..Default::default() }, &InspectedFilesystemScan { readable: true, ..Default::default() }).unwrap(), ScanApplyResult::Applied);
        assert_eq!(local_state(&db, hash, ROOT_DIR_ID), (true, false, String::new(), 0));
        assert!(!db.is_book_downloaded(&hash).unwrap());
        assert!(db.sync_publishable_mutations().unwrap().is_empty());
        assert_eq!(db.browse_book_detail(&hash).unwrap().book.title, "Original");
    }

    #[test]
    fn incomplete_or_excluded_scans_preserve_local_availability() {
        for (discovery_readable, inspection_readable, excluded) in [(false, true, false), (true, false, false), (true, true, true)] {
            let (db, hash) = fixture();
            let mut snapshot = db.scan_snapshot().unwrap();
            if excluded {
                snapshot.excluded_paths.push("book.epub".into());
            }
            db.apply_filesystem_scan(&snapshot, &FilesystemScan { readable: discovery_readable, ..Default::default() }, &InspectedFilesystemScan { readable: inspection_readable, ..Default::default() }).unwrap();
            assert_eq!(local_state(&db, hash, ROOT_DIR_ID), (true, true, "1".into(), 1));
        }
    }

    #[test]
    fn missing_copy_does_not_clear_an_observed_copy() {
        let (db, hash) = fixture();
        let other = db.create_directory(&ROOT_DIR_ID, &"Other".into()).unwrap().id;
        db.transfer_book_placement(&hash, &ROOT_DIR_ID, &other, false).unwrap();
        db.acknowledge_native_file_work(restore_completion(restore_snapshot(&db))).unwrap();
        drain_work(&db);
        let snapshot = db.scan_snapshot().unwrap();
        db.apply_filesystem_scan(
            &snapshot,
            &FilesystemScan { readable: true, files: vec![ScanFileObservation { path: "Other/book.epub".into(), ..observation(other, "book.epub", 1) }], ..Default::default() },
            &InspectedFilesystemScan { readable: true, ..Default::default() },
        )
        .unwrap();
        assert!(!local_state(&db, hash, ROOT_DIR_ID).1);
        assert!(local_state(&db, hash, other).1);
        assert!(db.is_book_downloaded(&hash).unwrap());
    }

    #[test]
    fn local_change_after_snapshot_rejects_scan_without_cleanup() {
        let (db, hash) = fixture();
        let snapshot = db.scan_snapshot().unwrap();
        db.connection.execute("UPDATE book_dir SET local_hash='newer'", []).unwrap();
        assert_eq!(db.apply_filesystem_scan(&snapshot, &FilesystemScan { readable: true, ..Default::default() }, &InspectedFilesystemScan { failures: vec![], readable: true, books: vec![replacement()] }).unwrap(), ScanApplyResult::Stale);
        assert_eq!(local_state(&db, hash, ROOT_DIR_ID), (true, true, "newer".into(), 1));
    }
}

#[test]
fn cloud_presence_reset_handles_empty_queues_and_persists_policy() {
    let db = database();
    for enabled in [false, true] {
        db.set_asset_storage_policy(enabled, true).unwrap();
        assert_eq!(db.connection.query_row("SELECT cloud_storage_enabled FROM sync_metadata", [], |r| r.get::<_, bool>(0)).unwrap(), enabled);
        assert!(({ use library_database::*; use library_database::assets::*; use library_database::shared_sql::*; use library_model::{BookUploadIntent,BookPlacement,RelativeBookPath}; use sync_common::ContentHash; let database = &db; let after = 0; let limit = 10; (|| -> Result<Vec<BookUploadIntent>, library_database::DatabaseError> {
        let mut intents = Vec::new();
        database.connection.transfer_pending_book_uploads(after, limit, library_database::assets::rejection_cutoff()?, |row| {
            intents.push(BookUploadIntent { id: row.get(0)?, content_hash: ContentHash::new(&row.get::<_, String>(1)?), checksum: ContentHash::new(&row.get::<_, String>(2)?), size_bytes: row.get::<_, i64>(3)? as u64 });
            Ok(())
        })?;
        Ok(intents)
    })() }).unwrap().is_empty());
    }
}

#[test]
fn cloud_presence_reset_requeues_books() {
    use library_model::BlobKind;
    let db = database();
    let mut book = import();
    let hash = book.content_hash;
    book.published.published = true;
    book.published.fingerprint = Some("1".into());
    db.commit_import(book).unwrap();
    db.record_remote_asset(BlobKind::Book, &hash).unwrap();
    db.settle_rejected_asset(&hash, library_database::RejectedAsset::BookUpload { intent: None }, "quota").unwrap();
    let before = db.book_upload_intent(&hash).unwrap().unwrap();

    db.set_asset_storage_policy(true, true).unwrap();
    let after = db.book_upload_intent(&hash).unwrap().unwrap();
    assert!(after.id > before.id);
    assert_eq!(after.checksum, before.checksum);
    assert_eq!(after.size_bytes, before.size_bytes);
    assert!(!db.remote_asset_hashes(BlobKind::Book).unwrap().contains(&hash));
    assert!(db.book_upload_rejection_reason(&hash).unwrap().is_none());

    // An already-uploaded book has no intent, but its local baseline must
    // recreate one when the cloud-presence cache is reset.
    db.connection.execute("UPDATE book_transfer SET upload_id=NULL,upload_checksum=NULL,upload_size=NULL WHERE content_hash=?1", [hash.as_str()]).unwrap();
    db.record_remote_asset(BlobKind::Book, &hash).unwrap();
    db.set_asset_storage_policy(true, true).unwrap();
    let prepared = db.upload_preparation_snapshot().unwrap();
    db.finish_upload_preparation(&prepared).unwrap();
    let requeued = ({ use library_database::*; use library_database::assets::*; use library_database::shared_sql::*; use library_model::{BookUploadIntent,BookPlacement,RelativeBookPath}; use sync_common::ContentHash; let database = &db; let after = 0; let limit = 10; (|| -> Result<Vec<BookUploadIntent>, library_database::DatabaseError> {
        let mut intents = Vec::new();
        database.connection.transfer_pending_book_uploads(after, limit, library_database::assets::rejection_cutoff()?, |row| {
            intents.push(BookUploadIntent { id: row.get(0)?, content_hash: ContentHash::new(&row.get::<_, String>(1)?), checksum: ContentHash::new(&row.get::<_, String>(2)?), size_bytes: row.get::<_, i64>(3)? as u64 });
            Ok(())
        })?;
        Ok(intents)
    })() }).unwrap();
    assert_eq!(requeued.len(), 1);
    assert_eq!(requeued[0].content_hash, hash);
    assert_eq!(requeued[0].checksum, before.checksum);
    assert_eq!(requeued[0].size_bytes, before.size_bytes);
}

fn assert_uninitialized(db: &Database) {
    assert!(db.connection.is_autocommit(), "failed initialization must release its transaction");
    assert_eq!(db.connection.query_row("PRAGMA main.user_version", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    assert_eq!(db.connection.query_row("SELECT count(*) FROM main.sqlite_schema", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
}

#[test]
fn interrupted_initialization_rolls_back_and_retries_after_reopen() {
    struct TestDirectory(std::path::PathBuf);
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let directory = TestDirectory(std::env::temp_dir().join(format!("library-init-{}", uuid::Uuid::new_v4())));
    std::fs::create_dir(&directory.0).unwrap();
    for budget in [500, 5000] {
        let path = directory.0.join(format!("{budget}.sqlite"));
        {
            let db = Database::open(&path).unwrap();
            let mut operations = 0;
            // Interrupt once, leaving rollback free to finish normally.
            db.connection
                .progress_handler(
                    100,
                    Some(move || {
                        operations += 100;
                        operations == budget
                    }),
                )
                .unwrap();
            let result = db.initialize_library();
            db.connection.progress_handler(0, None::<fn() -> bool>).unwrap();
            assert!(result.is_err(), "expected initialization interruption at {budget}");
            assert_uninitialized(&db);
        }
        let replica = {
            let db = Database::open(&path).unwrap();
            assert_uninitialized(&db);
            let replica = db.initialize_library().unwrap();
            assert_eq!(db.initialize_library().unwrap(), replica);
            db.commit_import(import()).unwrap();
            replica
        };
        let reopened = Database::open(&path).unwrap();
        assert_eq!(reopened.initialize_library().unwrap(), replica);
        assert_eq!(reopened.browse_book_detail(&import().content_hash).unwrap().book.title, "Original");
    }
}

#[test]
fn failing_final_schema_version_write_rolls_back_schema_and_replica() {
    use library_database::rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    let db = Database::open(":memory:").unwrap();
    db.connection
        .authorizer(Some(|context: AuthContext<'_>| match context.action {
            AuthAction::Pragma { pragma_name: "user_version", pragma_value: Some(_) } => Authorization::Deny,
            _ => Authorization::Allow,
        }))
        .unwrap();
    let result = db.initialize_library();
    db.connection.authorizer(None::<fn(AuthContext<'_>) -> Authorization>).unwrap();
    assert!(result.is_err());
    assert_uninitialized(&db);
    let replica = db.initialize_library().unwrap();
    assert_eq!(db.initialize_library().unwrap(), replica);
}

#[test]
fn subject_browsing_reads_assignments_without_cached_membership() {
    let db = database();
    let mut book = import();
    let hash = book.content_hash;
    book.inspection.as_mut().unwrap().metadata.book.subjects = vec![book_model::BookSubject::new(None, "History / United States / 20th Century", "dc:subject", Some("bisac".into()), Some("HIS036060".into())).unwrap()];
    db.commit_import(book).unwrap();
    let (path, parent): (i64, i64) = db
        .connection
        .query_row("SELECT route.route_id,route.parent_route_id FROM book_unified_concept assignment JOIN curated.unified_concept_route route ON route.concept_id=assignment.concept_id WHERE route.route_id!=route.parent_route_id LIMIT 1", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap();
    let mut query = library_model::LibraryBrowseQuery {
        location: library_model::BrowseLocation::Subject(parent),
        search: String::new(),
        file_types: vec![],
        chip_sort: library_model::BrowseChipSort::Alphabetical,
        book_sort: library_model::BrowseBookSort::Alphabetical,
        languages: vec![],
        hide_finished: false,
        include_direct_child_books: false,
    };
    let parent_contents = db.browse_contents(&query, false).unwrap();
    assert!(parent_contents.children.is_empty());
    assert!(parent_contents.books.is_empty());
    query.location = library_model::BrowseLocation::Subject(path);
    let contents = db.browse_contents(&query, false).unwrap();
    assert!(contents.books.is_empty());
    query.location = library_model::BrowseLocation::Subject(0);
    query.search.clear();
    let root_contents = db.browse_contents(&query, false).unwrap();
    assert_eq!(root_contents.books[0].content_hash, hash);
    query.search = "Original".into();
    let searched = db.browse_contents(&query, false).unwrap();
    assert!(searched.children.is_empty());
    assert_eq!(searched.books.len(), 1);
}

fn subject_query(location: library_model::BrowseLocation) -> library_model::LibraryBrowseQuery {
    library_model::LibraryBrowseQuery {
        location,
        search: String::new(),
        file_types: vec![],
        languages: vec![],
        chip_sort: library_model::BrowseChipSort::Alphabetical,
        book_sort: library_model::BrowseBookSort::Alphabetical,
        hide_finished: false,
        include_direct_child_books: false,
    }
}

fn subject_fixture() -> (Database, String, String) {
    let db = database();
    for index in 1..=4 {
        let mut book = import();
        book.content_hash = ContentHash::new(&format!("{index:064x}"));
        book.checksum = book.content_hash;
        book.file_name = format!("book{index}.epub");
        book.published.name = book.file_name.clone();
        book.published.relative_path = format!("/{}", book.file_name);
        book.inspection.as_mut().unwrap().metadata.title = format!("Book {index}");
        book.inspection.as_mut().unwrap().metadata.book.subjects = vec![book_model::BookSubject::new(None, "History / United States / 20th Century", "dc:subject", Some("bisac".into()), Some("HIS036060".into())).unwrap()];
        db.commit_import(book).unwrap();
    }
    let (path, parent): (String, String) = db
        .connection
        .query_row("SELECT path,parent_path FROM curated.unified_concept_paths WHERE concept_id IN (SELECT concept_id FROM book_unified_concept) AND path!=parent_path AND parent_path!='Subject' LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    // Subsequent browse-only edits explicitly operate on the derived fixture.

    (db, path, parent)
}

#[test]
fn subject_browsing_is_read_only_and_follows_placement_and_assignment_edits() {
    let (db, _, _) = subject_fixture();
    let root = db.browse_contents(&subject_query(library_model::BrowseLocation::Subject(0)), false).unwrap();
    let query = subject_query(root.children[0].id);
    let initial = db.browse_contents(&query, false).unwrap();
    assert!(initial.children.is_empty());
    assert_eq!(initial.books.len(), 4);
    let changes = db.connection.total_changes();
    db.connection.pragma_update(None, "query_only", true).unwrap();
    assert_eq!(db.browse_contents(&query, false).unwrap().children.len(), initial.children.len());
    db.browse(&query, false).unwrap();
    db.connection.pragma_update(None, "query_only", false).unwrap();
    assert_eq!(db.connection.total_changes(), changes);
    db.connection.execute("UPDATE book_dir SET deleted_at=10 WHERE book_row_id=(SELECT row_id FROM book WHERE title='Book 4')", []).unwrap();
    let shrunk = db.browse_contents(&query, false).unwrap();
    assert!(shrunk.children.is_empty());
    assert_eq!(shrunk.books.len(), 3);
    db.connection.execute("UPDATE book_dir SET deleted_at=NULL", []).unwrap();
    assert_eq!(db.browse_contents(&query, false).unwrap().books.len(), 4);
    db.connection.execute("DELETE FROM book_unified_concept", []).unwrap();
    let empty = db.browse_contents(&query, false).unwrap();
    assert!(empty.children.is_empty() && empty.books.is_empty());
}

#[test]
fn failed_subject_read_releases_snapshot_and_can_retry() {
    use library_database::rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    let (db, _, _) = subject_fixture();
    db.connection
        .authorizer(Some(|context: AuthContext<'_>| match context.action {
            AuthAction::Read { table_name: "book_unified_concept", .. } => Authorization::Deny,
            _ => Authorization::Allow,
        }))
        .unwrap();
    assert!(db.browse_contents(&subject_query(library_model::BrowseLocation::Subject(0)), false).is_err());
    assert!(db.connection.is_autocommit());
    db.connection.authorizer(None::<fn(AuthContext<'_>) -> Authorization>).unwrap();
    assert!(!db.browse_contents(&subject_query(library_model::BrowseLocation::Subject(0)), false).unwrap().children.is_empty());
}

fn browse_folder(db: &Database, dir: DirId, search: &str, languages: &[&str]) -> library_model::BrowseContents {
    let mut query = subject_query(library_model::BrowseLocation::Folder(dir));
    query.search = search.into();
    query.languages = languages.iter().map(|language| (*language).to_owned()).collect();
    db.browse_contents(&query, false).unwrap()
}

fn import_in_folder(db: &Database, index: u64, dir: DirId) -> ContentHash {
    let mut book = import();
    book.parent_id = dir;
    book.content_hash = ContentHash::new(&format!("{index:064x}"));
    book.checksum = book.content_hash;
    book.file_name = format!("book{index}.epub");
    book.published.name = book.file_name.clone();
    book.published.relative_path = format!("/{}", book.file_name);
    book.inspection.as_mut().unwrap().metadata.title = format!("Title {index}");
    let hash = book.content_hash;
    db.commit_import(book).unwrap();
    hash
}

fn place_book(db: &Database, hash: ContentHash, dir: DirId, downloaded: bool) {
    db.connection
        .execute(
            "INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded) SELECT ?1,row_id,'copy.epub','',?3 FROM book WHERE content_hash=?2",
            library_database::rusqlite::params![dir.to_string(), hash.as_str(), downloaded],
        )
        .unwrap();
}

#[test]
fn folder_counts_include_all_placements_and_search_sources() {
    let db = database();
    let shelf = db.create_directory(&ROOT_DIR_ID, &"Shelf".into()).unwrap().id;
    let nested = db.create_directory(&shelf, &"Nested".into()).unwrap().id;
    let branch = db.create_directory(&shelf, &"Branch".into()).unwrap().id;
    let empty = db.create_directory(&ROOT_DIR_ID, &"Empty".into()).unwrap().id;
    let first = import_in_folder(&db, 101, nested);
    let second = import_in_folder(&db, 102, branch);
    let third = import_in_folder(&db, 103, shelf);
    // Exercise derived browse rows without producing unrelated canonical edits.

    db.connection.execute("UPDATE book_dir SET is_downloaded=0", []).unwrap();
    place_book(&db, first, shelf, false);
    place_book(&db, first, branch, true);
    db.connection.execute("UPDATE book_dir SET is_downloaded=1 WHERE book_row_id IN (SELECT row_id FROM book WHERE content_hash IN (?1,?2))", [second.as_str(), third.as_str()]).unwrap();
    db.connection.execute("UPDATE book SET format=2 WHERE content_hash=?1", [third.as_str()]).unwrap();
    db.connection.execute("INSERT OR IGNORE INTO book_language(book_row_id,language) SELECT row_id,'en' FROM book WHERE content_hash!=?1", [third.as_str()]).unwrap();
    let view = browse_folder(&db, ROOT_DIR_ID, "", &[]);
    let chip = view.children.iter().find(|c| c.id == library_model::BrowseLocation::Folder(shelf)).unwrap();
    assert_eq!((chip.book_count, chip.downloaded_book_count), (5, 3));
    assert_eq!(view.children.iter().find(|c| c.id == library_model::BrowseLocation::Folder(empty)).unwrap().book_count, 0);
    assert!(view.books.is_empty(), "ordinary browsing only shows direct placements");
    let direct = browse_folder(&db, shelf, "", &[]);
    assert_eq!(direct.books.len(), 2);
    assert!(direct.books.iter().all(|b| b.source_directory == Some(shelf)));
    let searched = browse_folder(&db, shelf, "Title", &[]);
    assert_eq!(searched.books.len(), 5, "search returns each descendant placement");
    assert_eq!(searched.books.iter().filter(|b| b.content_hash == first).count(), 3);
    let source = [shelf, nested, branch].into_iter().min_by_key(|id| id.to_string()).unwrap();
    assert_eq!(searched.books.iter().find(|b| b.content_hash == first).unwrap().source_directory, Some(source));
    let filtered = browse_folder(&db, ROOT_DIR_ID, "", &["en"]);
    assert_eq!(filtered.children.len(), 1);
    assert_eq!(filtered.children[0].book_count, 4);
    assert_eq!(browse_folder(&db, shelf, "Title", &["EN_us"]).books.len(), 4);
    let mut facet_query = subject_query(library_model::BrowseLocation::Folder(shelf));
    facet_query.languages = vec!["en".into()];
    let facets = db.browse(&facet_query, false).unwrap();
    let (languages, formats) = (facets.language_counts, facets.format_counts);
    assert_eq!(languages.iter().find(|l| l.language == "en").unwrap().book_count, 2);
    assert_eq!(formats.iter().map(|f| f.book_count).sum::<u32>(), 3);
    // A folder-name search still finds matching descendant chips and breadcrumbs.
    let nested_search = browse_folder(&db, ROOT_DIR_ID, "Nested", &[]);
    assert_eq!(nested_search.children.len(), 1);
    assert_eq!(nested_search.children[0].path.as_deref(), Some("Shelf"));
    assert_eq!(nested_search.children[0].book_count, 1);
    // Every removed or moved placement changes the subtree count.
    db.connection.execute("DELETE FROM book_dir WHERE dir_id=?1 AND book_row_id=(SELECT row_id FROM book WHERE content_hash=?2)", [shelf.to_string(), first.to_string()]).unwrap();
    assert_eq!(browse_folder(&db, ROOT_DIR_ID, "", &[]).children.iter().find(|c| c.id == library_model::BrowseLocation::Folder(shelf)).unwrap().book_count, 4);
    db.connection.execute("UPDATE book_dir SET dir_id=?1 WHERE dir_id=?2 AND book_row_id=(SELECT row_id FROM book WHERE content_hash=?3)", [empty.to_string(), nested.to_string(), first.to_string()]).unwrap();
    let moved = browse_folder(&db, ROOT_DIR_ID, "", &[]);
    assert_eq!(moved.children.iter().find(|c| c.id == library_model::BrowseLocation::Folder(shelf)).unwrap().book_count, 3);
    assert_eq!(moved.children.iter().find(|c| c.id == library_model::BrowseLocation::Folder(empty)).unwrap().book_count, 1);
    assert_eq!(browse_folder(&db, shelf, "Title", &[]).books.iter().find(|b| b.content_hash == first).unwrap().source_directory, Some(branch));
}

#[test]
fn folder_counts_follow_writes_and_browsing_is_read_only() {
    let db = database();
    let shelf = db.create_directory(&ROOT_DIR_ID, &"Shelf".into()).unwrap().id;
    let branch = db.create_directory(&shelf, &"Branch".into()).unwrap().id;
    let other = db.create_directory(&ROOT_DIR_ID, &"Other".into()).unwrap().id;
    let hash = import_in_folder(&db, 201, branch);
    let counts = || {
        let view = browse_folder(&db, ROOT_DIR_ID, "", &[]);
        let folder = view.children.iter().find(|row| row.id == library_model::BrowseLocation::Folder(shelf)).unwrap();
        (folder.book_count, folder.downloaded_book_count)
    };
    assert_eq!(counts(), (1, 1));
    for downloaded in [0, 1, 0] {
        db.connection.execute("UPDATE book_dir SET is_downloaded=?1", [downloaded]).unwrap();
        assert_eq!(counts(), (1, downloaded as usize));
    }
    let tx = db.connection.unchecked_transaction().unwrap();
    tx.execute("UPDATE book_dir SET deleted_at=1", []).unwrap();
    assert_eq!(db.connection.query_row("SELECT count(*) FROM book_dir WHERE deleted_at IS NULL", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    tx.rollback().unwrap();
    assert_eq!(counts(), (1, 0));
    db.connection.pragma_update(None, "query_only", true).unwrap();
    assert_eq!(browse_folder(&db, branch, "", &[]).books.len(), 1);
    db.connection.pragma_update(None, "query_only", false).unwrap();

    db.connection.execute("UPDATE dir SET name='Moved',parent_id=?1 WHERE id=?2", [other.to_string(), branch.to_string()]).unwrap();

    let view = browse_folder(&db, ROOT_DIR_ID, "", &[]);
    assert_eq!(view.children.iter().find(|c| c.id == library_model::BrowseLocation::Folder(shelf)).unwrap().book_count, 0);
    assert_eq!(view.children.iter().find(|c| c.id == library_model::BrowseLocation::Folder(other)).unwrap().book_count, 1);
    db.connection.execute("UPDATE book_dir SET deleted_at=1 WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1)", [hash.as_str()]).unwrap();
    assert_eq!(browse_folder(&db, ROOT_DIR_ID, "", &[]).children.iter().find(|c| c.id == library_model::BrowseLocation::Folder(other)).unwrap().book_count, 0);
    db.connection.execute("UPDATE book_dir SET deleted_at=NULL", []).unwrap();
    db.connection.execute("UPDATE dir SET deleted_at=1 WHERE id=?1", [other.to_string()]).unwrap();
    assert!(browse_folder(&db, ROOT_DIR_ID, "Title", &[]).books.is_empty(), "deleted ancestors block membership");
    assert_eq!(browse_folder(&db, branch, "", &[]).books.len(), 1, "a live detached subtree retains its own membership");
}

#[test]
fn folder_sections_use_subtree_counts_and_normalized_languages() {
    let db = database();
    let shelf = db.create_directory(&ROOT_DIR_ID, &"Shelf".into()).unwrap().id;
    let child = db.create_directory(&shelf, &"Child".into()).unwrap().id;
    let empty = db.create_directory(&shelf, &"Empty".into()).unwrap().id;
    let branch = db.create_directory(&child, &"Branch".into()).unwrap().id;
    import_in_folder(&db, 301, child);
    import_in_folder(&db, 302, branch);
    db.connection.execute("INSERT OR IGNORE INTO book_language(book_row_id,language) SELECT row_id,'en' FROM book", []).unwrap();
    let mut query = subject_query(library_model::BrowseLocation::Folder(ROOT_DIR_ID));
    let unfiltered = db.browse(&query, false).unwrap();
    assert_eq!(unfiltered.sections.len(), 1);
    assert_eq!(unfiltered.sections[0].parent.id, library_model::BrowseLocation::Folder(shelf));
    assert_eq!(unfiltered.sections[0].children.iter().map(|row| row.id.clone()).collect::<Vec<_>>(), [library_model::BrowseLocation::Folder(child), library_model::BrowseLocation::Folder(empty)]);
    let downloaded = db.browse(&query, true).unwrap();
    assert_eq!(downloaded.sections[0].children.len(), 1);
    assert_eq!(downloaded.sections[0].children[0].id, library_model::BrowseLocation::Folder(child));
    query.languages = vec!["en".into()];
    let page = db.browse(&query, false).unwrap();
    assert_eq!(page.sections.len(), 1);
    let contents = &page.sections[0];
    assert_eq!(contents.children.len(), 1);
    assert_eq!(contents.children[0].id, library_model::BrowseLocation::Folder(child));
    assert_eq!(contents.children[0].book_count, 2);
    query.languages = vec!["sv".into()];
    let no_matches = db.browse(&query, false).unwrap();
    assert!(no_matches.contents.children.is_empty());
    assert!(no_matches.sections.is_empty());
}

#[test]
fn folder_sections_preserve_wrapper_order_and_direct_search_results() {
    let db = database();
    let wrapper = db.create_directory(&ROOT_DIR_ID, &"A wrapper".into()).unwrap().id;
    let nested = db.create_directory(&wrapper, &"Nested".into()).unwrap().id;
    let plain = db.create_directory(&ROOT_DIR_ID, &"Z plain".into()).unwrap().id;
    import_in_folder(&db, 303, nested);
    import_in_folder(&db, 304, plain);
    let query = subject_query(library_model::BrowseLocation::Folder(ROOT_DIR_ID));
    let page = db.browse(&query, false).unwrap();
    assert_eq!(page.contents.children.iter().map(|row| row.id.clone()).collect::<Vec<_>>(), [library_model::BrowseLocation::Folder(wrapper), library_model::BrowseLocation::Folder(plain)]);
    assert_eq!(page.sections.iter().map(|section| section.parent.id.clone()).collect::<Vec<_>>(), [library_model::BrowseLocation::Folder(plain), library_model::BrowseLocation::Folder(wrapper)]);
    assert_eq!(page.sections[1].children[0].id, library_model::BrowseLocation::Folder(nested));
    assert_eq!(page.sections[0].books.len(), 1);
    assert!(page.sections[1].books.is_empty());
    let search = library_model::LibraryBrowseQuery { search: "Nested".into(), ..query };
    let searched = db.browse(&search, false).unwrap();
    assert!(searched.sections.is_empty());
    assert_eq!(searched.contents.children[0].id, library_model::BrowseLocation::Folder(nested));
    assert_eq!(searched.contents.children[0].path.as_deref(), Some("A wrapper"));
}

#[test]
fn downloaded_folder_sorting_uses_scoped_counts_and_normalized_names() {
    let db = database();
    let mut index = 9000;
    for (name, downloads, remote) in [("Écology", 1, 0), ("Zoology", 1, 2), ("Most", 2, 0), ("Remote", 0, 1), ("Empty", 0, 0)] {
        let directory = db.create_directory(&ROOT_DIR_ID, &name.into()).unwrap().id;
        for book in 0..downloads + remote {
            index += 1;
            let hash = import_in_folder(&db, index, directory);
            db.connection.execute("UPDATE book_dir SET is_downloaded=?1 WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?2)", library_database::rusqlite::params![book < downloads, hash.as_str()]).unwrap();
        }
    }
    db.connection.execute("INSERT OR IGNORE INTO book_language(book_row_id,language) SELECT row_id,'en' FROM book", []).unwrap();
    for filtered in [false, true] {
        for search in ["", "ology"] {
            for sort in [library_model::BrowseChipSort::Alphabetical, library_model::BrowseChipSort::BookCount] {
                let mut query = subject_query(library_model::BrowseLocation::Folder(ROOT_DIR_ID));
                query.search = search.into();
                query.chip_sort = sort;
                if filtered {
                    query.languages = vec!["en".into()];
                    query.file_types = vec![library_model::LibraryFileTypeFilter::Book];
                }
                let contents = db.browse_contents(&query, true).unwrap();
                let expected = if !search.is_empty() {
                    vec![("Écology", 1), ("Zoology", 1)]
                } else if sort == library_model::BrowseChipSort::BookCount {
                    vec![("Most", 2), ("Écology", 1), ("Zoology", 1)]
                } else {
                    vec![("Écology", 1), ("Most", 2), ("Zoology", 1)]
                };
                assert_eq!(contents.children.iter().map(|row| (row.name.as_str(), row.book_count)).collect::<Vec<_>>(), expected);
                assert!(contents.children.iter().all(|row| row.book_count == row.downloaded_book_count));
            }
        }
    }
}

#[test]
fn subject_cards_and_global_facets_use_normalized_languages() {
    let (db, _path, _) = subject_fixture();
    db.connection.execute("INSERT OR IGNORE INTO book_language(book_row_id,language) SELECT row_id,'en' FROM book", []).unwrap();
    let root = db.browse_contents(&subject_query(library_model::BrowseLocation::Subject(0)), false).unwrap();
    let mut query = subject_query(root.children[0].id);
    query.languages = vec!["en".into()];
    assert_eq!(db.browse_contents(&query, false).unwrap().books.len(), 4);
    let facets = db.browse(&query, false).unwrap();
    let (languages, formats) = (facets.language_counts, facets.format_counts);
    assert_eq!(languages.iter().find(|l| l.language == "en").unwrap().book_count, 4);
    assert_eq!(formats.iter().map(|f| f.book_count).sum::<u32>(), 4);
}

#[test]
fn browse_progress_is_derived_without_rewriting_sync_registers_and_audio_is_book_scoped() {
    let db = database();
    let hash = import_in_folder(&db, 401, ROOT_DIR_ID);
    // Browse progress comes from the dedicated register, independent of the location.

    for (position, progress, expected) in [("0.25", 0., 0.), ("35", 0., 0.), ("chapter.xhtml", 0., 0.), ("0.25", 70., 70.), ("101", 0., 0.), (" 0.5suffix", 0., 0.)] {
        db.connection.execute("UPDATE book SET read_pos=?1,read_progress=?2 WHERE content_hash=?3", library_database::rusqlite::params![position, progress, hash.as_str()]).unwrap();
        assert_eq!(db.browse_book_detail(&hash).unwrap().book.progress, f64::from(expected) as f32);
        let stored: (String, f64) = db.connection.query_row("SELECT read_pos,read_progress FROM book WHERE content_hash=?1", [hash.as_str()], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(stored, (position.to_owned(), progress));
    }
    db.connection.execute("UPDATE book_navigation SET duration_ms=12345 WHERE content_hash=?1", [hash.as_str()]).unwrap();
    db.connection.execute("UPDATE book_navigation SET entry_count=7 WHERE content_hash=?1", [hash.as_str()]).unwrap();
    let folder = db.create_directory(&ROOT_DIR_ID, &"Copy".into()).unwrap().id;
    place_book(&db, hash, folder, false);
    let detail = db.browse_book_detail(&hash).unwrap();
    assert_eq!(detail.book.audiobook_duration_ms, Some(12345));
    assert_eq!(detail.audiobook_chapter_count, Some(7));
    db.connection.execute("UPDATE book_navigation SET duration_ms=NULL WHERE content_hash=?1", [hash.as_str()]).unwrap();
    let detail = db.browse_book_detail(&hash).unwrap();
    assert_eq!(detail.book.audiobook_duration_ms, None);
    assert_eq!(detail.audiobook_chapter_count, None);
}

/// Books a search from the root finds, by title or author.
fn searched(db: &Database, search: &str) -> Vec<ContentHash> {
    browse_folder(db, ROOT_DIR_ID, search, &[]).books.into_iter().map(|book| book.content_hash).collect()
}

#[test]
fn search_reads_stored_keys_without_normalizing_metadata_again() {
    let db = database();
    let hash = import_in_folder(&db, 701, ROOT_DIR_ID);
    // Queries still normalize the user's input in Rust. No database scalar call
    // should be needed to search/sort title and author metadata after its write.
    db.connection
        .create_scalar_function("normalize_search_text", 1, library_database::rusqlite::functions::FunctionFlags::SQLITE_UTF8, |_| -> library_database::rusqlite::Result<String> { Err(library_database::rusqlite::Error::InvalidQuery) })
        .unwrap();
    assert_eq!(searched(&db, "title")[0], hash);
    assert_eq!(searched(&db, "original author")[0], hash);
}

#[test]
fn folder_count_queries_do_not_scan_unrelated_library_books() {
    use library_database::rusqlite::{StatementStatus, named_params};
    let db = database();
    let small = db.create_directory(&ROOT_DIR_ID, &"Small".into()).unwrap().id;
    let child = db.create_directory(&small, &"Child".into()).unwrap().id;
    let big = db.create_directory(&ROOT_DIR_ID, &"Big".into()).unwrap().id;
    import_in_folder(&db, 901, child);

    let tx = db.connection.unchecked_transaction().unwrap();
    tx.execute_batch("WITH RECURSIVE numbers(n) AS (VALUES(1000) UNION ALL SELECT n+1 FROM numbers WHERE n<2999) INSERT INTO book(content_hash,title,format) SELECT printf('%064x',n),'Other book',1 FROM numbers;").unwrap();
    tx.execute("INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded) SELECT ?1,row_id,content_hash || '.epub','',1 FROM book WHERE title='Other book'", [big.to_string()]).unwrap();
    tx.execute(
        "WITH RECURSIVE numbers(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM numbers WHERE n<1000)
        INSERT INTO dir(id,parent_id,name) SELECT printf('10000000-0000-0000-0000-%012x',n),?1,printf('Unrelated %d',n) FROM numbers",
        [big.to_string()],
    )
    .unwrap();
    tx.commit().unwrap();
    browse_folder(&db, ROOT_DIR_ID, "", &[]);
    let source = include_str!("../src/browse/sql/folders.sql");
    let query = source.split("-- name: folder_direct_counts?\n").nth(1).unwrap();
    let directories = serde_json::to_string(&[child.to_string()]).unwrap();
    let mut direct = db.connection.prepare(query).unwrap();
    let count: i64 = direct.query_row(named_params! {":directory_ids":directories,":file_types":0,":languages":""}, |r| r.get(1)).unwrap();
    assert_eq!(count, 1);
    assert!(direct.get_status(StatementStatus::FullscanStep) < 100, "direct counts must not scan unrelated books");
    db.connection.execute("INSERT OR IGNORE INTO book_language(book_row_id,language) SELECT row_id,'en' FROM book WHERE title!='Other book'", []).unwrap();
    for (formats, languages) in [(1, ""), (2, ""), (3, ""), (0, ",en,"), (1, ",en,"), (1, ",sv,")] {
        direct.reset_status(StatementStatus::FullscanStep);
        let actual: i64 = direct.query_row(named_params! {":directory_ids":directories,":file_types":formats,":languages":languages}, |r| r.get(1)).unwrap();
        assert_eq!(actual, i64::from((formats == 0 || formats & 1 != 0) && (languages.is_empty() || languages == ",en,")));
        assert!(direct.get_status(StatementStatus::FullscanStep) < 100, "filtered counts must not scan unrelated books");
    }
    assert_eq!(browse_folder(&db, ROOT_DIR_ID, "", &[]).children.iter().find(|row| row.id == library_model::BrowseLocation::Folder(small)).unwrap().book_count, 1);
}

#[test]
fn read_time_directory_counts_follow_deletes_and_physical_cascades() {
    let db = database();
    let shelf = db.create_directory(&ROOT_DIR_ID, &"Shelf".into()).unwrap().id;
    let branch = db.create_directory(&shelf, &"Branch".into()).unwrap().id;
    let hash = import_in_folder(&db, 901, branch);
    place_book(&db, hash, shelf, true);
    let root_count = || browse_folder(&db, ROOT_DIR_ID, "", &[]).children.iter().map(|row| row.book_count).sum::<usize>();
    assert_eq!(root_count(), 2);
    db.connection.execute("UPDATE book SET deleted_at=1 WHERE content_hash=?1", [hash.as_str()]).unwrap();
    assert_eq!(root_count(), 0);
    db.connection.execute("UPDATE book SET deleted_at=NULL WHERE content_hash=?1", [hash.as_str()]).unwrap();
    assert_eq!(root_count(), 2);
    db.connection.execute("DELETE FROM dir WHERE id=?1", [branch.to_string()]).unwrap();
    assert_eq!(root_count(), 1);
    db.connection.execute("DELETE FROM book WHERE content_hash=?1", [hash.as_str()]).unwrap();
    assert_eq!(root_count(), 0);
    import_in_folder(&db, 902, shelf);
    db.connection.execute("DELETE FROM dir WHERE id=?1", [shelf.to_string()]).unwrap();
    assert_eq!(root_count(), 0);
}

#[cfg(feature = "scanner")]
#[test]
fn scan_file_failures_finish_and_remain_visible_until_next_scan() {
    use library_database::{FilesystemScan, InspectedFilesystemScan};
    let db = database();
    let mut snapshot = db.scan_snapshot().unwrap();
    let discovery = FilesystemScan { readable: true, ..Default::default() };
    let failures = vec![("Broken.epub".to_owned(), "Invalid ZIP header".to_owned())];
    // Even a batch containing only failed files must persist their diagnostics.
    db.apply_filesystem_scan_batch(&mut snapshot, &discovery, &InspectedFilesystemScan { failures: failures.clone(), readable: false, books: vec![] }, false).unwrap();
    db.apply_filesystem_scan_batch(&mut snapshot, &discovery, &InspectedFilesystemScan { readable: false, ..Default::default() }, true).unwrap();
    assert!(!db.needs_scan().unwrap());
    for _ in 0..2 {
        assert!(!({ use library_database::{DatabaseError, LibraryWork}; use library_database::assets::ActivitySql; let database = &db; (|| -> Result<LibraryWork, DatabaseError> {
        let mut work = None;
        database.connection.activity_progress(|row| {
            let count = |index| row.get::<_, i64>(index).map(|value| value.max(0) as u64);
            work = Some(LibraryWork {
                total_books: count(0)?, uploaded_books: count(1)?,
                pending_uploads: count(2)?, pending_changes: count(3)?,
                pending_thumbnails: count(4)?, pending_covers: count(5)?,
                scan_pending: row.get(6)?, scan_failed: row.get(7)?, sync_configured: row.get(8)?,
            });
            Ok(())
        })?;
        work.ok_or_else(|| DatabaseError::message("library activity query returned no row"))
    })() }).unwrap().scan_pending);
        assert_eq!(db.scan_failures().unwrap(), failures);
    }
    let snapshot = db.scan_snapshot().unwrap();
    db.apply_filesystem_scan(&snapshot, &discovery, &InspectedFilesystemScan { readable: true, ..Default::default() }).unwrap();
    assert!(db.scan_failures().unwrap().is_empty());
}

fn record_found_answer(db: &Database, hash: ContentHash) {
    let candidate = ({ use library_database::enrichment::facts::*; use library_database::DatabaseError; use library_replica::LibraryStateEntry; use metadata_contract::books::{BookEnrichmentStatus,BookFacts}; let database = db; let scope = &[hash]; let now = 0; (|| -> Result<Vec<EnrichmentCandidate>, DatabaseError> {
        let scope = serde_json::to_string(scope).map_err(DatabaseError::operation)?;
        let mut rows = Vec::new();
        database.connection.facts_candidates(&scope, |row| {
            rows.push((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?, row.get::<_, Option<String>>(2)?, row.get::<_, Option<String>>(3)?, row.get::<_, Option<i64>>(4)?));
            Ok(())
        })?;
        let mut candidates = Vec::new();
        for (hash, format, stored_key, status, retry_at) in rows {
            let Some((body, recording)) = facts_input(&database.connection, &hash)? else { continue };
            let key = request_key(&body, recording.as_deref());
            let waiting = retry_at.is_some_and(|at| at > now);
            if waiting && (stored_key.as_deref() == Some(key.as_str()) || status.as_deref() == Some("failed")) {
                continue;
            }
            let LibraryStateEntry::Metadata { value, .. } = sync_common::wire::decode::<LibraryStateEntry>(&body, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)? else { continue };
            let mut filenames = Vec::new();
            database.connection.facts_filenames(&hash, |row| {
                filenames.push(row.get(0)?);
                Ok(())
            })?;
            let recording = recording.map(|json| serde_json::from_str(&json)).transpose().map_err(DatabaseError::operation)?;
            let audiobook = matches!(format.and_then(book_model::BookFormat::from_code), Some(book_model::BookFormat::M4b | book_model::BookFormat::Mp3Folder));
            let record = book_model::BookRecord { metadata: value, description: String::new() };
            candidates.push(EnrichmentCandidate { content_hash: ContentHash::new(&hash), request_key: key, facts: BookFacts { key: String::new(), record, audiobook, filenames, recording } });
        }
        Ok(candidates)
    })() }).unwrap().pop().expect("an unanswered book is a candidate");
    let mut record = candidate.facts.record.clone();
    record.book.subjects.push(book_model::BookSubject::new(None, "History", "openlibrary:metadata:work", Some("lcc".into()), Some("DC38".into())).unwrap());
    record.contributors[0] = book_model::Contributor::with_id(book_model::agent_id_from_migration_seed(b"external-author:open_library:OL1A"), "Original Author", book_model::MarcRelatorCode(*b"aut")).unwrap();
    record.description = "Server description.".into();
    let answer = metadata_contract::books::BookEnrichment { key: "0".into(), status: metadata_contract::books::BookEnrichmentStatus::Found, record: Some(record), toc: None, cover: None, detail: String::new() };
    ({ use library_database::enrichment::facts::*; use library_database::DatabaseError; use library_replica::LibraryStateEntry; use metadata_contract::books::{BookEnrichmentStatus,BookFacts}; let database = db; let hash = &hash; let request_key = &candidate.request_key; let result = &answer; let now = 0; (|| -> Result<(), DatabaseError> {
        let transaction = database.connection.unchecked_transaction()?;
        match result.status {
            BookEnrichmentStatus::Failed => {
                transaction.facts_record_failure(hash.as_str(), request_key, now + FAILURE_RETRY_SECONDS)?;
            }
            BookEnrichmentStatus::Found => {
                let response = serde_json::to_string(result).map_err(DatabaseError::operation)?;
                transaction.facts_record(hash.as_str(), request_key, "found", Some(response.as_str()), now + ANSWER_LIFETIME_SECONDS)?;
                transaction.facts_mark_dirty(hash.as_str())?;
            }
            BookEnrichmentStatus::NoMatch => {
                transaction.facts_record(hash.as_str(), request_key, "no_match", None, now + ANSWER_LIFETIME_SECONDS)?;
                transaction.facts_mark_dirty(hash.as_str())?;
            }
        }
        library_database::commit(transaction)?;
        Ok(())
    })() }).unwrap();
}

fn projected_subject_codes(db: &Database) -> Vec<String> {
    db.connection.prepare("SELECT code FROM book_subject WHERE code IS NOT NULL").unwrap().query_map([], |row| row.get(0)).unwrap().collect::<Result<_, _>>().unwrap()
}





#[test]
fn reimport_keeps_the_file_author_identity_under_an_enriched_projection() {
    let db = database();
    let input = import();
    let hash = input.content_hash;
    db.commit_import(input).unwrap();
    let file_author = db.connection.query_row("SELECT stable_id FROM author_identity", [], |row| row.get::<_, String>(0)).unwrap();
    record_found_answer(&db, hash);
    let projected: String = db.connection.query_row("SELECT a.stable_id FROM book_contributor c JOIN author_identity a ON a.id=c.author_identity_id", [], |row| row.get(0)).unwrap();
    assert_ne!(projected, file_author, "the answer's authority identity is projected");

    db.clear_sync_outbox().unwrap();
    db.commit_import(import()).unwrap();
    let metadata = db.sync_publishable_mutations().unwrap().into_iter().find_map(|mutation| match mutation.state.value {
        library_replica::LibraryStateEntry::Metadata { value, .. } => Some(value),
        _ => None,
    });
    if let Some(metadata) = metadata {
        assert_eq!(metadata.contributors[0].agent_id().to_string(), file_author, "the register never takes the projected identity");
    }
}
