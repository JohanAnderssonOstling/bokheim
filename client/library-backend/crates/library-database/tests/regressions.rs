use crate::query_methods::query_rows;
use library_database::{Database, ImportCommit, NativeDirectoryAcknowledgement, NativeDirectoryCompletion};
use library_model::PublishedStagedBook;
use sync_common::{BookId, DirId, ROOT_DIR_ID, StateAddress};

fn database() -> Database {
    let db = Database::open(":memory:").unwrap();
    db.initialize_library().unwrap();
    db
}

fn import() -> ImportCommit {
    let hash = BookId::new(&"9".repeat(64));
    ImportCommit {
        source_identity: None,
        byte_checksum: None,
        audiobook_content_hash: None,
        parent_id: ROOT_DIR_ID,
        file_name: "book.epub".into(),
        format: book_model::BookFormat::Epub,
        book_id: hash,
        size_bytes: 10,
        inspection_version: 1,
        inspection: Some(book_inspection_contract::InspectedBook {
            pdf: None,
            audiobook: None,
            metadata: book_model::BookRecord {
                metadata: book_model::BookDetails { title: "Original".into(), subtitle: None, contributors: vec![book_model::Contributor::new("Original Author", book_model::MarcRelatorCode(*b"aut")).unwrap()], book: Default::default() },
                description: String::new(),
            },
            toc: vec![book_model::BookTocEntry { title: "Chapter".into(), target: "ch1".into(), children: vec![] }],
        }),
        published: PublishedStagedBook { name: "book.epub".into(), relative_path: "/book.epub".into(), published: false, fingerprint: None },
        request_thumbnail: false,
        restore_paths: vec![],
    }
}

#[test]
fn browse_book_detail_projects_only_authors_and_narrators() {
    use book_model::{AUTHOR_MARC_RELATOR_CODE, Contributor, MarcRelatorCode, NARRATOR_MARC_RELATOR_CODE};

    let db = database();
    let mut book = import();
    let hash = book.book_id;
    book.inspection.as_mut().unwrap().metadata.contributors = vec![
        Contributor::new("Author", AUTHOR_MARC_RELATOR_CODE).unwrap(),
        Contributor::new("Translator", MarcRelatorCode(*b"trl")).unwrap(),
        Contributor::new("First narrator", NARRATOR_MARC_RELATOR_CODE).unwrap(),
        Contributor::new("Editor", MarcRelatorCode(*b"edt")).unwrap(),
        Contributor::new("Second narrator", NARRATOR_MARC_RELATOR_CODE).unwrap(),
    ];
    db.commit_import(book).unwrap();

    let detail = db.browse_book_detail(&hash).unwrap();
    assert_eq!(detail.authors.iter().map(|author| author.name.as_str()).collect::<Vec<_>>(), ["Author"]);
    assert_eq!(detail.credits.iter().map(|credit| (credit.name.as_str(), credit.role.as_str())).collect::<Vec<_>>(), [("First narrator", "nrt"), ("Second narrator", "nrt")]);
}

#[test]
fn old_library_schemas_are_refused_without_modification() {
    for version in [8, 9, 10, 11, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35] {
        let db = database();
        db.connection.pragma_update(None, "user_version", version).unwrap();
        assert!(db.initialize_library().unwrap_err().to_string().contains("unsupported library schema version"));
        let retained: i64 = db.connection.pragma_query_value(None, "user_version", |row| row.get(0)).unwrap();
        assert_eq!(retained, version);
    }
}

#[test]
fn reimport_with_catalog_answer_preserves_an_existing_definitive_answer() {
    let db = database();
    let hash = import().book_id;
    let original = import().inspection.unwrap().metadata;
    db.commit_import_with_catalog_answer(import(), None, Some(original.clone())).unwrap();
    let mut changed = original.clone();
    changed.metadata.title = "Later catalog correction".into();
    db.commit_import_with_catalog_answer(import(), None, Some(changed)).unwrap();
    assert_eq!(db.enrichment_cover_record(&hash).unwrap().unwrap(), original);
    assert!(db.enrichment_candidates(&[hash], i64::MAX).unwrap().is_empty());
}

fn materialize(db: &Database, id: DirId, renamed_to: Option<&str>) {
    let snapshot = db.native_directory_snapshot(&id).unwrap().unwrap();
    assert!(snapshot.relative_path.starts_with('/'));
    assert_eq!(db.acknowledge_native_directory(NativeDirectoryCompletion { snapshot, renamed_to: renamed_to.map(str::to_owned), materialized: true }).unwrap(), NativeDirectoryAcknowledgement::Applied);
}

fn projection(db: &Database, id: DirId) -> String {
    db.connection.fixtures_read_local_file_projection_relative_path(&(id.to_string()), |r| r.get(0)).unwrap()
}

#[test]
fn directory_acknowledgement_rolls_back_when_projection_write_fails() {
    let db = database();
    let shelf = db.create_directory(&ROOT_DIR_ID, &"Shelf".into()).unwrap().id;
    materialize(&db, shelf, None);
    db.connection.fixtures_create_ON().unwrap();
    let snapshot = db.native_directory_snapshot(&shelf).unwrap().unwrap();
    assert!(db.acknowledge_native_directory(NativeDirectoryCompletion { snapshot, renamed_to: Some("Changed".into()), materialized: true }).is_err());
    let current = db.native_directory_snapshot(&shelf).unwrap().unwrap();
    assert_eq!(current.name, "Shelf");
    assert_eq!(current.projected_path.as_deref(), Some("/Shelf"));
}

#[test]
fn definitive_enrichment_answers_do_not_expire() {
    use metadata_contract::books::{BookEnrichment, BookEnrichmentStatus};

    for status in [BookEnrichmentStatus::Found, BookEnrichmentStatus::NoMatch] {
        let db = database();
        let book = import();
        let hash = book.book_id;
        db.commit_import(book).unwrap();
        let candidate = db.enrichment_candidates(&[hash], 0).unwrap().pop().unwrap();
        let answer = BookEnrichment { key: String::new(), status, record: (status == BookEnrichmentStatus::Found).then_some(candidate.record), toc: None, cover: None, failure: None, detail: String::new() };
        db.record_enrichment(&hash, &candidate.request_key, answer, 100).unwrap();
        assert!(db.enrichment_candidates(&[hash], 100).unwrap().is_empty());
        assert!(db.enrichment_candidates(&[hash], 10 * 365 * 24 * 60 * 60).unwrap().is_empty());
        assert_eq!(db.enrichment_next_retry().unwrap(), None);
    }
}

#[test]
fn failed_enrichment_retries_until_a_definitive_answer() {
    use metadata_contract::books::{BookEnrichment, BookEnrichmentStatus};

    let db = database();
    let book = import();
    let hash = book.book_id;
    db.commit_import(book).unwrap();
    let candidate = db.enrichment_candidates(&[hash], 0).unwrap().pop().unwrap();
    let mut answer = BookEnrichment { key: String::new(), status: BookEnrichmentStatus::Failed, record: None, toc: None, cover: None, failure: None, detail: String::new() };
    db.record_enrichment(&hash, &candidate.request_key, answer.clone(), 100).unwrap();
    assert_eq!(db.enrichment_next_retry().unwrap(), Some(160));
    assert!(db.enrichment_candidates(&[hash], 159).unwrap().is_empty());
    assert_eq!(db.enrichment_candidates(&[hash], 160).unwrap().len(), 1);
    answer.status = BookEnrichmentStatus::NoMatch;
    db.record_enrichment(&hash, &candidate.request_key, answer, 160).unwrap();
    assert_eq!(db.enrichment_next_retry().unwrap(), None);
    assert!(db.enrichment_candidates(&[hash], i64::MAX).unwrap().is_empty());
}

#[test]
fn enrichment_candidate_deadlines_preserve_scope_order_and_request_inputs() {
    use metadata_contract::books::{BookEnrichment, BookEnrichmentStatus};

    let db = database();
    let hashes = (1..=3).map(|index| import_in_folder(&db, index, ROOT_DIR_ID)).collect::<Vec<_>>();
    let scope = [hashes[2], hashes[0], hashes[1]];
    let original = db.enrichment_candidates(&scope, 0).unwrap();
    assert_eq!(original.iter().map(|candidate| candidate.book_id).collect::<Vec<_>>(), hashes);
    let failure = BookEnrichment { key: String::new(), status: BookEnrichmentStatus::Failed, record: None, toc: None, cover: None, failure: None, detail: String::new() };
    db.record_enrichment(&hashes[0], &original[0].request_key, failure.clone(), 10).unwrap();
    db.record_enrichment(&hashes[1], &original[1].request_key, failure, 25).unwrap();
    // The third draft has no retry deadline and remains immediately eligible.
    for (now, expected) in [(69, vec![hashes[2]]), (70, vec![hashes[0], hashes[2]]), (84, vec![hashes[0], hashes[2]]), (85, hashes.clone())] {
        let candidates = db.enrichment_candidates(&scope, now).unwrap();
        assert_eq!(candidates.iter().map(|candidate| candidate.book_id).collect::<Vec<_>>(), expected);
        for candidate in candidates {
            let expected = original.iter().find(|original| original.book_id == candidate.book_id).unwrap();
            assert_eq!(candidate.request_key, expected.request_key);
            assert_eq!(serde_json::to_value(candidate.record).unwrap(), serde_json::to_value(&expected.record).unwrap());
            assert_eq!(candidate.recording, expected.recording);
            assert_eq!(candidate.filenames, expected.filenames);
            assert_eq!(candidate.audiobook, expected.audiobook);
        }
    }
    assert!(db.enrichment_candidates(&[hashes[1]], 70).unwrap().is_empty());
    let candidates = db.enrichment_candidates(&[hashes[1]], 85).unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].book_id, hashes[1]);
}

#[test]
fn enrichment_defers_corrupt_drafts_until_their_retry_deadline() {
    use metadata_contract::books::{BookEnrichment, BookEnrichmentStatus};

    let db = database();
    let hash = import_in_folder(&db, 1, ROOT_DIR_ID);
    let candidate = db.enrichment_candidates(&[hash], 0).unwrap().pop().unwrap();
    let failure = BookEnrichment { key: String::new(), status: BookEnrichmentStatus::Failed, record: None, toc: None, cover: None, failure: None, detail: String::new() };
    db.record_enrichment(&hash, &candidate.request_key, failure, 100).unwrap();
    db.connection.execute("UPDATE local_book_draft SET body=?1 WHERE content_hash=?2", rusqlite::params![b"invalid draft".as_slice(), hash]).unwrap();
    assert!(db.enrichment_candidates(&[hash], 159).unwrap().is_empty());
    assert!(db.enrichment_candidates(&[hash], 160).is_err());
    assert!(db.enrichment_candidates(&[], 160).unwrap().is_empty());
}

#[test]
fn enrichment_reads_identifiers_from_the_stored_metadata_document() {
    use book_model::{Scope, push_isbn};

    let db = database();
    let mut book = import();
    let hash = book.book_id;
    let identifiers = &mut book.inspection.as_mut().unwrap().metadata.book.identifiers;
    push_isbn(identifiers, "9780393243277", Scope::Book).unwrap();
    let expected = identifiers.clone();
    db.commit_import(book).unwrap();

    let candidates = db.enrichment_candidates(&[hash], 0).unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].record.book.identifiers, expected);
}

#[test]
fn navigation_on_book_distinguishes_missing_and_empty_and_replaces_playback_fields() {
    use crate::books::toc::TocSql;

    let db = database();
    let hash = BookId::new(&"a".repeat(64));
    db.connection.fixtures_insert_book_7(&(&hash)).unwrap();
    let read_toc = || {
        let mut toc = None;
        db.connection
            .book_toc_select(&hash, |row| {
                toc = Some(row.get::<_, Vec<u8>>(0)?);
                Ok(())
            })
            .unwrap();
        toc
    };
    assert_eq!(read_toc(), None);

    db.connection.navigation_upsert(&hash, 0, b"[]", Some(1000), Some(b"[]")).unwrap();
    assert_eq!(read_toc().as_deref(), Some(b"[]".as_slice()));
    db.connection.navigation_upsert(&hash, 0, b"[]", None, None).unwrap();
    let playback: (Option<i64>, Option<String>) = db.connection.fixtures_read_book_duration_ms_tracks_json(&(&hash), |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    assert_eq!(playback, (None, None));
    assert_eq!(read_toc().as_deref(), Some(b"[]".as_slice()));
}

#[test]
fn audiobook_detail_preserves_current_chapter_boundaries_and_fallback_titles() {
    use crate::books::toc::TocSql;
    use book_model::{AudiobookChapter, BookFormat, BookTocEntry, audiobook_reading_position, audiobook_toc_target};

    let db = database();
    let mut book = import();
    let hash = book.book_id;
    book.format = BookFormat::M4b;
    book.inspection.as_mut().unwrap().audiobook = Some(book_inspection_contract::AudiobookMetadata {
        title: "Audio".into(),
        authors: Vec::new(),
        description: String::new(),
        duration_ms: 3000,
        chapters: vec![AudiobookChapter { title: "Opening".into(), start_ms: 0, end_ms: 3000 }],
        tracks: Vec::new(),
        chapter_origin: book_model::ChapterOrigin::EmbeddedM4b,
        recording: Default::default(),
    });
    db.commit_import(book).unwrap();
    // Legacy navigation can contain entries with no usable audio timestamp.
    // Display numbering includes them; interval numbering counts only chapters.
    let toc = vec![
        BookTocEntry { title: "Legacy link".into(), target: "unresolved".into(), children: vec![] },
        BookTocEntry { title: "Opening".into(), target: audiobook_toc_target(0), children: vec![] },
        BookTocEntry { title: "3".into(), target: audiobook_toc_target(1000), children: vec![] },
        BookTocEntry { title: "Ending".into(), target: audiobook_toc_target(2000), children: vec![] },
    ];
    db.connection.navigation_upsert(&hash, 4, &crate::stored_document::encode(&toc).unwrap(), Some(3000), None).unwrap();
    assert_eq!(
        db.audiobook_playback_metadata(&hash).unwrap().unwrap().chapters,
        vec![AudiobookChapter { title: "Opening".into(), start_ms: 0, end_ms: 1000 }, AudiobookChapter { title: "3".into(), start_ms: 1000, end_ms: 2000 }, AudiobookChapter { title: "Ending".into(), start_ms: 2000, end_ms: 3000 },]
    );
    for (position, expected) in [(0, Some(("Opening", 0))), (999, Some(("Opening", 0))), (1000, Some(("3", 1000))), (1999, Some(("3", 1000))), (2000, Some(("Ending", 2000))), (3000, None)] {
        db.update_reading_position(&hash, &audiobook_reading_position(position), None, None).unwrap();
        let detail = db.browse_book_detail(&hash).unwrap();
        assert_eq!(detail.current_entry.as_deref(), expected.map(|(title, _)| title));
        assert_eq!(detail.current_target, expected.map(|(_, start)| audiobook_toc_target(start)));
        assert_eq!(detail.toc[2].title, "Chapter 3");
        assert_eq!(detail.audiobook_chapter_count, Some(4));
    }
    db.connection.navigation_upsert(&hash, 4, &crate::stored_document::encode(&toc).unwrap(), None, None).unwrap();
    let detail = db.browse_book_detail(&hash).unwrap();
    assert_eq!(detail.current_entry, None);
    assert_eq!(detail.current_target, None);
    assert_eq!(detail.toc[2].title, "Chapter 3");
}

fn copy_fixture(db: &Database) -> (BookId, DirId, DirId) {
    let source = db.create_directory(&ROOT_DIR_ID, &"A".into()).unwrap().id;
    let destination = db.create_directory(&ROOT_DIR_ID, &"Z".into()).unwrap().id;
    let mut book = import();
    let hash = book.book_id;
    book.parent_id = source;
    book.published.relative_path = "/A/book.epub".into();
    db.commit_import(book).unwrap();
    db.copy_book_to_directory(&hash, Some(&source), &destination).unwrap();
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
fn placement_paths_preserve_root_nested_hidden_and_invalid_cases() {
    use crate::placements::{FolderPaths, placement_path};

    let db = database();
    let shelf = db.create_directory(&ROOT_DIR_ID, &"Shelf".into()).unwrap().id;
    let nested = db.create_directory(&shelf, &"Nested".into()).unwrap().id;
    let mut folders = FolderPaths::new(&db.connection);
    for (parent, expected) in [(ROOT_DIR_ID, "/book.epub"), (nested, "/Shelf/Nested/book.epub")] {
        assert_eq!(db.native_book_path(parent, "book.epub").unwrap().unwrap().as_str(), expected);
        assert_eq!(placement_path(&db.connection, &parent, "book.epub").unwrap().as_str(), expected);
        assert_eq!(folders.placement(&parent.to_string(), "book.epub").unwrap().unwrap().as_str(), expected);
        for invalid in ["", "../escape.epub", "bad\\name.epub"] {
            assert!(db.native_book_path(parent, invalid).unwrap_err().to_string().contains("stored book path"));
            assert!(placement_path(&db.connection, &parent, invalid).unwrap_err().to_string().contains("stored book path"));
            assert!(folders.placement(&parent.to_string(), invalid).unwrap_err().to_string().contains("stored book path"));
        }
    }
    drop(folders);
    db.trash_directory(&shelf).unwrap();
    let mut folders = FolderPaths::new(&db.connection);
    for parent in [nested, DirId::from_u128(u128::MAX)] {
        for name in ["book.epub", "../escape.epub"] {
            assert!(db.native_book_path(parent, name).unwrap().is_none());
            assert!(placement_path(&db.connection, &parent, name).unwrap_err().to_string().contains("placement directory has no path"));
            assert!(folders.placement(&parent.to_string(), name).unwrap().is_none());
        }
    }
}

#[test]
fn stored_path_reads_reject_corrupt_projections_and_recover_after_repair() {
    use library_database::rusqlite::types::Value;
    use std::collections::HashSet;

    let db = database();
    let (hash, _, destination) = copy_fixture(&db);
    let work = db.native_file_work().unwrap().into_iter().find(|work| work.operation == "materialize").unwrap();
    for corrupt in [Value::Text("/zz/../../outside.epub".into()), Value::Blob(vec![255])] {
        db.connection
            .execute("INSERT INTO local_file_projection(content_hash,dir_id,relative_path) VALUES(?1,?2,?3) ON CONFLICT(content_hash,dir_id) DO UPDATE SET relative_path=excluded.relative_path", (hash, destination.to_string(), corrupt))
            .unwrap();
        assert!(db.book_paths(&hash).is_err());
        assert!(db.local_book_paths().is_err());
        assert!(db.native_file_work_snapshot(work.clone()).is_err());
        assert!(db.transfer_snapshot(&[hash]).is_err());
        assert!(db.connection.is_autocommit());
    }
    db.connection.execute("UPDATE local_file_projection SET relative_path='/retained/book.epub' WHERE content_hash=?1 AND dir_id=?2", (hash, destination.to_string())).unwrap();
    let paths = db.book_paths(&hash).unwrap();
    assert_eq!(paths.len(), 4);
    assert_eq!(paths[..2].iter().map(|path| path.as_str()).collect::<HashSet<_>>(), ["/A/book.epub", "/retained/book.epub"].into_iter().collect());
    assert_eq!(paths[2..].iter().map(|path| path.as_str()).collect::<HashSet<_>>(), ["/A/book.epub", "/Z/book.epub"].into_iter().collect());
    assert_eq!(db.local_book_paths().unwrap().len(), 3);
    let snapshot = db.native_file_work_snapshot(work).unwrap().unwrap();
    assert_eq!(snapshot.book_paths.iter().map(|path| path.as_str()).collect::<Vec<_>>(), ["/A/book.epub", "/retained/book.epub"]);
    assert_eq!(db.transfer_snapshot(&[hash]).unwrap().asset_for(&hash).unwrap().upload_paths.len(), 4);
}

#[test]
fn file_work_reads_preserve_queue_order_and_fail_on_a_later_invalid_path() {
    let db = database();
    let (hash, _, _) = copy_fixture(&db);
    let before = db.native_file_work().unwrap();
    let expected = before.iter().map(|work| (work.id, work.operation.clone(), work.book_id, work.directory, work.cleanup_path.clone())).collect::<Vec<_>>();
    for invalid in ["/zz/../../outside.epub", "/zz/../another.epub"] {
        db.connection.execute("INSERT INTO local_file_work(operation,content_hash,relative_path) VALUES('remove',?1,?2)", (hash, invalid)).unwrap();
        let id = db.connection.last_insert_rowid();
        assert!(db.native_file_work().is_err());
        db.connection.execute("DELETE FROM local_file_work WHERE id=?1", [id]).unwrap();
        assert_eq!(db.native_file_work().unwrap().into_iter().map(|work| (work.id, work.operation, work.book_id, work.directory, work.cleanup_path)).collect::<Vec<_>>(), expected);
    }
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
    let projections: i64 = db.connection.fixtures_read_local_file_projection_count(&(destination.to_string()), |row| row.get(0)).unwrap();
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

    fn fixture() -> (Database, BookId) {
        let db = database();
        let mut book = import();
        let hash = book.book_id;
        book.published.published = true;
        book.published.fingerprint = Some("1".into());
        db.commit_import(book).unwrap();
        drain_work(&db);
        (db, hash)
    }

    fn drain_work(db: &Database) {
        db.connection.fixtures_delete_local_file_work().unwrap();
    }

    fn observation(directory: DirId, name: &str, fingerprint: u64) -> ScanFileObservation {
        ScanFileObservation { directory, name: name.into(), path: name.into(), fingerprint }
    }

    fn replacement() -> ScannedBook {
        let hash = BookId::new(&"b".repeat(64));
        ScannedBook {
            source_identity: None,
            audiobook_content_hash: None,
            byte_checksum: None,
            directory: ROOT_DIR_ID,
            name: "book.epub".into(),
            path: "book.epub".into(),
            fingerprint: 2,
            book_id: hash,
            size_bytes: 20,
            extension: "epub".into(),
            inspection: import().inspection.unwrap(),
            inspection_version: 1,
        }
    }

    fn local_state(db: &Database, hash: BookId, directory: DirId) -> (bool, bool, String, i64) {
        db.connection.fixtures_read_local_file_projection_bd_deleted_at_bd(&(&hash), &(directory.to_string()), |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).unwrap()
    }

    #[test]
    fn invalid_inventory_directory_ids_roll_back_the_scan_and_allow_repair() {
        for trashed in [false, true] {
            let (db, hash) = fixture();
            let state = || db.connection.query_row("SELECT scan_seq,scan_state FROM sync_metadata CROSS JOIN library_local_state", [], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))).unwrap();
            let before = state();
            db.connection.execute_batch("DELETE FROM local_file_work; DELETE FROM local_book_work; DELETE FROM local_directory_work; PRAGMA foreign_keys=OFF;").unwrap();
            db.connection.execute("UPDATE book_dir SET dir_id='invalid-directory',deleted_at=?1", [trashed.then_some(1)]).unwrap();

            assert!(db.scan_snapshot(1).is_err());
            assert!(db.connection.is_autocommit());
            assert_eq!(state(), before, "failed inventory reads must not advance the scan or its activity state");

            db.connection.execute("UPDATE book_dir SET dir_id=?1", [ROOT_DIR_ID.to_string()]).unwrap();
            db.connection.pragma_update(None, "foreign_keys", true).unwrap();
            let repaired = db.scan_snapshot(1).unwrap();
            assert_eq!(repaired.scan_id, before.0 + 1);
            if trashed {
                assert!(repaired.books.is_empty());
                assert_eq!(repaired.tombstoned_files, [(ROOT_DIR_ID, "book.epub".into())]);
            } else {
                assert_eq!(repaired.books.len(), 1);
                assert_eq!((repaired.books[0].book_id, repaired.books[0].directory, repaired.books[0].fingerprint), (hash, ROOT_DIR_ID, 1));
                db.connection.execute("UPDATE book_dir SET local_hash='outdated-fingerprint'", []).unwrap();
                let skipped = db.scan_snapshot(1).unwrap();
                assert!(skipped.books.is_empty());
                assert_eq!(skipped.placements.len(), 1, "unreadable fingerprints do not hide live placements");
            }
        }
    }

    #[test]
    fn pending_parent_work_excludes_descendants_remembered_at_other_physical_paths() {
        let (db, _) = fixture();
        let shelf = db.create_directory(&ROOT_DIR_ID, &"Shelf".into()).unwrap().id;
        let nested = db.create_directory(&shelf, &"Nested".into()).unwrap().id;
        db.connection.execute("DELETE FROM local_directory_work", []).unwrap();
        db.connection.execute("INSERT INTO local_directory_work(dir_id) VALUES(?1)", [shelf.to_string()]).unwrap();
        db.connection.execute("INSERT INTO local_directory_projection(dir_id,relative_path) VALUES(?1,'Elsewhere') ON CONFLICT(dir_id) DO UPDATE SET relative_path=excluded.relative_path", [nested.to_string()]).unwrap();

        let snapshot = db.scan_snapshot(1).unwrap();

        assert!(snapshot.excluded_paths.iter().any(|path| path == "Shelf"));
        assert!(snapshot.excluded_paths.iter().any(|path| path == "Shelf/Nested"));
        assert!(snapshot.excluded_paths.iter().any(|path| path == "Elsewhere"));
    }

    #[test]
    fn scan_batches_publish_books_before_final_missing_file_cleanup() {
        let (db, missing) = fixture();
        let mut snapshot = db.scan_snapshot(1).unwrap();
        let mut first = replacement();
        first.name = "first.epub".into();
        first.path = first.name.clone();
        let first_hash = db.resolve_scanned_content(first.book_id.content_hash().unwrap(), None).unwrap();
        let discovery = FilesystemScan { readable: true, files: vec![observation(ROOT_DIR_ID, "first.epub", 2), observation(ROOT_DIR_ID, "second.epub", 2)], ..Default::default() };
        assert_eq!(db.apply_filesystem_scan_batch(&mut snapshot, &discovery, InspectedFilesystemScan { readable: true, books: vec![first] }, false, None).unwrap(), ScanApplyResult::Applied);
        assert_eq!(
            ({
                use library_database::DatabaseError;
                use library_database::browse::{CountsSql, HomeSql};
                let database = &db;
                (|| -> Result<usize, DatabaseError> {
                    let count = database.connection.count_all_books(|row| row.get::<_, i64>(0))?;
                    Ok(usize::try_from(count).map_err(|_| DatabaseError::message("library book count must not be negative"))?)
                })()
            })
            .unwrap(),
            2,
            "first batch is immediately browsable"
        );
        assert!(local_state(&db, first_hash, ROOT_DIR_ID).1);
        assert!(local_state(&db, missing, ROOT_DIR_ID).1, "incomplete scans must not clear missing files");
        let mut second = replacement();
        second.name = "second.epub".into();
        second.path = second.name.clone();
        second.book_id = BookId::new(&"c".repeat(64));

        assert_eq!(db.apply_filesystem_scan_batch(&mut snapshot, &discovery, InspectedFilesystemScan { readable: true, books: vec![second] }, false, None).unwrap(), ScanApplyResult::Applied);
        assert_eq!(
            ({
                use library_database::DatabaseError;
                use library_database::browse::{CountsSql, HomeSql};
                let database = &db;
                (|| -> Result<usize, DatabaseError> {
                    let count = database.connection.count_all_books(|row| row.get::<_, i64>(0))?;
                    Ok(usize::try_from(count).map_err(|_| DatabaseError::message("library book count must not be negative"))?)
                })()
            })
            .unwrap(),
            3
        );
        assert_eq!(db.apply_filesystem_scan_batch(&mut snapshot, &discovery, InspectedFilesystemScan { readable: true, books: vec![] }, true, None).unwrap(), ScanApplyResult::Applied);
        assert!(!local_state(&db, missing, ROOT_DIR_ID).1);
        assert!(local_state(&db, first_hash, ROOT_DIR_ID).1);
    }

    #[test]
    fn ordinary_import_still_rejects_an_occupied_filename() {
        let (db, old) = fixture();
        let mut book = import();
        book.book_id = BookId::new(&"b".repeat(64));
        assert!(db.commit_import(book).is_err());
        assert_eq!(local_state(&db, old, ROOT_DIR_ID), (true, true, "1".into(), 1));
    }
}

#[test]
fn cloud_presence_reset_handles_empty_queues_and_persists_policy() {
    let db = database();
    for enabled in [false, true] {
        db.set_asset_storage_policy(enabled, true).unwrap();
        assert_eq!(db.connection.fixtures_read_sync_metadata_cloud_storage_enabled(|r| r.get::<_, bool>(0)).unwrap(), enabled);
        assert!(db.pending_book_uploads(0, 10).unwrap().is_empty());
    }
}

fn assert_uninitialized(db: &Database) {
    assert!(db.connection.is_autocommit(), "failed initialization must release its transaction");
    assert_eq!(db.connection.fixtures_check_main_user_version(|r| r.get::<_, i64>(0)).unwrap(), 0);
    assert_eq!(db.connection.fixtures_read_main_sqlite_schema_count(|r| r.get::<_, i64>(0)).unwrap(), 0);
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
    for budget in [500, 3000] {
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
        assert_eq!(reopened.browse_book_detail(&import().book_id).unwrap().book.title, "Original");
    }
}

#[test]
fn reopening_current_library_does_not_commit_database_changes() {
    use library_database::rusqlite::hooks::{AuthAction, AuthContext, Authorization};

    struct TestDirectory(std::path::PathBuf);
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let directory = TestDirectory(std::env::temp_dir().join(format!("library-reopen-{}", uuid::Uuid::new_v4())));
    std::fs::create_dir(&directory.0).unwrap();
    let path = directory.0.join("library.sqlite");
    let db = Database::open(&path).unwrap();
    let replica = db.initialize_library().unwrap();
    let mode: String = db.connection.pragma_query_value(Some("main"), "journal_mode", |row| row.get(0)).unwrap();
    assert_eq!(mode, "wal", "new libraries must establish persistent WAL mode");
    let observer = library_database::rusqlite::Connection::open(&path).unwrap();
    let data_version = || observer.pragma_query_value(None, "data_version", |row| row.get::<_, i64>(0)).unwrap();
    let before = data_version();
    drop(db);

    let reopened = Database::open(&path).unwrap();
    let mode: String = reopened.connection.pragma_query_value(Some("main"), "journal_mode", |row| row.get(0)).unwrap();
    assert_eq!(mode, "wal", "WAL must survive closing and reopening the database");
    reopened
        .connection
        .authorizer(Some(|context: AuthContext<'_>| match context.action {
            AuthAction::Pragma { pragma_name, .. } if pragma_name.eq_ignore_ascii_case("journal_mode") => Authorization::Deny,
            _ => Authorization::Allow,
        }))
        .unwrap();
    // Exercise both ordinary connection configuration and initialization with
    // every journal-mode read/write forbidden, including read-only queries.
    crate::configure_library_connection(&reopened.connection).unwrap();
    assert_eq!(reopened.initialize_library().unwrap(), replica);
    reopened.connection.authorizer(None::<fn(AuthContext<'_>) -> Authorization>).unwrap();
    assert_eq!(data_version(), before, "opening a current library must not commit a write");
}

#[test]
fn failed_wal_setup_leaves_library_uninitialized_and_can_retry() {
    use library_database::rusqlite::hooks::{AuthAction, AuthContext, Authorization};

    let db = Database::open(":memory:").unwrap();
    db.connection
        .authorizer(Some(|context: AuthContext<'_>| match context.action {
            AuthAction::Pragma { pragma_name, .. } if pragma_name.eq_ignore_ascii_case("journal_mode") => Authorization::Deny,
            _ => Authorization::Allow,
        }))
        .unwrap();
    assert!(db.initialize_library().is_err());
    db.connection.authorizer(None::<fn(AuthContext<'_>) -> Authorization>).unwrap();
    assert!(db.connection.is_autocommit());
    assert_uninitialized(&db);
    let replica = db.initialize_library().unwrap();
    assert_eq!(db.initialize_library().unwrap(), replica);
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
    let hash = book.book_id;
    book.inspection.as_mut().unwrap().metadata.book.subjects = vec![book_model::BookSubject::new(None, "History / United States / 20th Century", "dc:subject", Some("bisac".into()), Some("HIS036060".into())).unwrap()];
    db.commit_import(book).unwrap();
    let (path, parent): (i64, i64) = db.connection.fixtures_read_book_unified_concept_route_route_id_route(|row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    let mut query = library_model::LibraryBrowseQuery {
        location: library_model::BrowseLocation::Subject(parent),
        search: String::new(),
        file_types: vec![],
        chip_sort: library_model::BrowseChipSort::Alphabetical,
        book_sort: library_model::BrowseBookSort::Alphabetical,
        languages: vec![],
        hide_finished: false,
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
    assert_eq!(root_contents.books[0].book_id(), Some(hash));
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
    }
}

fn subject_fixture() -> (Database, String, String) {
    let db = database();
    for index in 1..=4 {
        let mut book = import();
        book.book_id = BookId::new(&format!("{index:064x}"));

        book.file_name = format!("book{index}.epub");
        book.published.name = book.file_name.clone();
        book.published.relative_path = format!("/{}", book.file_name);
        book.inspection.as_mut().unwrap().metadata.title = format!("Book {index}");
        book.inspection.as_mut().unwrap().metadata.book.subjects = vec![book_model::BookSubject::new(None, "History / United States / 20th Century", "dc:subject", Some("bisac".into()), Some("HIS036060".into())).unwrap()];
        db.commit_import(book).unwrap();
    }
    let (path, parent): (String, String) = db.connection.fixtures_read_curated_unified_concept_paths_path_parent_path(|r| Ok((r.get(0)?, r.get(1)?))).unwrap();
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
    db.connection.fixtures_update_book_dir_deleted_at().unwrap();
    let shrunk = db.browse_contents(&query, false).unwrap();
    assert!(shrunk.children.is_empty());
    assert_eq!(shrunk.books.len(), 3);
    db.connection.fixtures_update_book_dir_deleted_at_2().unwrap();
    assert_eq!(db.browse_contents(&query, false).unwrap().books.len(), 4);
    db.connection.fixtures_delete_book_unified_concept_4().unwrap();
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

fn import_in_folder(db: &Database, index: u64, dir: DirId) -> BookId {
    let mut book = import();
    book.parent_id = dir;
    book.book_id = BookId::new(&format!("{index:064x}"));

    book.file_name = format!("book{index}.epub");
    book.published.name = book.file_name.clone();
    book.published.relative_path = format!("/{}", book.file_name);
    book.inspection.as_mut().unwrap().metadata.title = format!("Title {index}");
    let hash = book.book_id;
    db.commit_import(book).unwrap();
    hash
}

fn place_book(db: &Database, hash: BookId, dir: DirId, downloaded: bool) {
    db.connection.fixtures_insert_book_dir_4(&(dir.to_string()), &(&hash), &downloaded).unwrap();
}

#[test]
fn all_books_lists_unique_live_books_across_folders() {
    use library_model::BrowseLocation;

    let db = database();
    let query = subject_query(BrowseLocation::Books);
    assert!(db.browse(&query, false).unwrap().contents.books.is_empty());
    let shelf = db.create_directory(&ROOT_DIR_ID, &"Shelf".into()).unwrap().id;
    let nested = db.create_directory(&shelf, &"Nested".into()).unwrap().id;
    let second = import_in_folder(&db, 102, nested);
    let first = import_in_folder(&db, 101, ROOT_DIR_ID);
    place_book(&db, first, shelf, true);
    let deleted = import_in_folder(&db, 103, shelf);
    db.connection.fixtures_update_book_deleted_at(&(&deleted)).unwrap();
    let unplaced = import_in_folder(&db, 104, shelf);
    db.connection.fixtures_delete_book_dir_2(&(shelf.to_string()), &unplaced).unwrap();

    let page = db.browse(&query, false).unwrap();
    assert_eq!(page.contents.books.iter().map(|book| book.book_id()).collect::<Vec<_>>(), [Some(first), Some(second)]);
    assert!(page.contents.books.iter().all(|book| book.source_directory.is_none()));
    assert!(page.contents.children.is_empty());
    assert!(page.sections.is_empty());
    assert!(page.path.is_empty());
    assert!(page.ancestors.is_empty());
    assert_eq!(page.contents.books.len(), db.book_count().unwrap());
    assert_eq!(page.format_counts.iter().map(|count| count.book_count).sum::<u32>(), 2);
    assert_eq!(db.browse_contents(&query, false).unwrap(), page.contents);
}

#[test]
fn all_books_uses_shared_search_filters_and_book_order() {
    use crate::assets::TransfersSql;
    use crate::projection::ProjectionSql;
    use library_model::{BrowseBookSort, BrowseLocation, LibraryFileTypeFilter};

    let db = database();
    let shelf = db.create_directory(&ROOT_DIR_ID, &"Shelf".into()).unwrap().id;
    let first = import_in_folder(&db, 101, ROOT_DIR_ID);
    let second = import_in_folder(&db, 102, shelf);
    let third = import_in_folder(&db, 103, shelf);
    db.connection.fixtures_update_book_dir_is_downloaded().unwrap();
    db.connection.fixtures_update_book_dir_is_downloaded_2(&(&first), &(&second)).unwrap();
    db.connection.asset_clear_book_paths(&(&third)).unwrap();
    db.connection.fixtures_update_book_format(&(&third)).unwrap();
    db.connection.fixtures_insert_book_language_4(&(&third)).unwrap();
    // Derived reading state provides the recent-reading sort order.
    db.connection.projection_set_book_reading(&first, Some("reading"), 50.0_f32, Some(20)).unwrap();
    db.connection.projection_set_book_reading(&second, Some("finished"), 100.0_f32, Some(30)).unwrap();
    let mut query = subject_query(BrowseLocation::Books);
    let hashes = |query: &library_model::LibraryBrowseQuery, downloaded_only| db.browse_contents(query, downloaded_only).unwrap().books.into_iter().map(|book| book.book_id().unwrap()).collect::<Vec<_>>();
    assert_eq!(hashes(&query, true), [first, second]);
    query.search = "TITLE 102".into();
    assert_eq!(hashes(&query, false), [second]);
    query.search = "original author".into();
    assert_eq!(hashes(&query, false), [first, second, third]);
    query.search = "Shelf".into();
    assert!(hashes(&query, false).is_empty());
    query.search.clear();
    query.file_types = vec![LibraryFileTypeFilter::Pdf];
    assert_eq!(hashes(&query, false), [third]);
    query.file_types.clear();
    query.languages = vec!["EN_us".into()];
    assert_eq!(hashes(&query, false), [first, second]);
    query.languages.clear();
    query.book_sort = BrowseBookSort::RecentlyRead;
    assert_eq!(hashes(&query, false), [second, first, third]);
    query.hide_finished = true;
    assert_eq!(hashes(&query, false), [first, third]);
    query.search = "no matches".into();
    assert!(hashes(&query, false).is_empty());
}

#[test]
fn empty_folders_remain_visible_in_local_and_paged_browsing() {
    use library_model::{BrowseLocation, BrowsePageRequest};

    let db = database();
    let empty = db.create_directory(&ROOT_DIR_ID, &"Empty".into()).unwrap().id;
    let parent = db.create_directory(&ROOT_DIR_ID, &"Projects".into()).unwrap().id;
    let nested = db.create_directory(&parent, &"Empty nested".into()).unwrap().id;
    let remote = db.create_directory(&ROOT_DIR_ID, &"Remote books".into()).unwrap().id;
    import_in_folder(&db, 401, remote);
    db.connection.fixtures_update_book_dir_is_downloaded_3(&0).unwrap();
    let local = db.create_directory(&ROOT_DIR_ID, &"Local books".into()).unwrap().id;
    import_in_folder(&db, 402, local);
    let query = subject_query(BrowseLocation::Folder(ROOT_DIR_ID));

    for downloaded_only in [false, true] {
        let views = [db.browse(&query, downloaded_only).unwrap(), db.browse_page(&query, downloaded_only, BrowsePageRequest::default()).unwrap()];
        for view in views {
            for id in [empty, parent, local] {
                assert!(view.contents.children.iter().any(|row| row.id == BrowseLocation::Folder(id)), "folder {id} disappeared with downloaded_only={downloaded_only}");
            }
            assert_eq!(view.contents.children.iter().any(|row| row.id == BrowseLocation::Folder(remote)), !downloaded_only);
            let section = view.sections.iter().find(|section| section.parent.id == BrowseLocation::Folder(empty)).unwrap();
            assert_eq!(section.parent.book_count, 0);
            assert!(section.books.is_empty());
            let container = view.sections.iter().find(|section| section.parent.id == BrowseLocation::Folder(parent)).unwrap();
            assert_eq!(container.children[0].id, BrowseLocation::Folder(nested));
        }
        let contents = db.browse_contents(&query, downloaded_only).unwrap();
        assert!(contents.children.iter().any(|row| row.id == BrowseLocation::Folder(empty)));

        let nested_view = db.browse(&subject_query(BrowseLocation::Folder(parent)), downloaded_only).unwrap();
        assert_eq!(nested_view.contents.children[0].id, BrowseLocation::Folder(nested));

        let mut search = query.clone();
        search.search = "EMPTY".into();
        let found = db.browse_contents(&search, downloaded_only).unwrap();
        assert_eq!(found.children.len(), 2);
        assert_eq!(found.children.iter().find(|row| row.id == BrowseLocation::Folder(nested)).unwrap().path.as_deref(), Some("Projects"));
    }
}

#[test]
fn empty_folders_still_respect_explicit_book_filters() {
    use library_model::{BrowseLocation, LibraryFileTypeFilter};

    let db = database();
    let empty = db.create_directory(&ROOT_DIR_ID, &"Empty".into()).unwrap().id;
    let mut query = subject_query(BrowseLocation::Folder(ROOT_DIR_ID));
    query.file_types = vec![LibraryFileTypeFilter::Book];
    assert!(db.browse_contents(&query, true).unwrap().children.is_empty());
    query.file_types.clear();
    query.languages = vec!["en".into()];
    assert!(db.browse_contents(&query, true).unwrap().children.is_empty());
    query.languages.clear();
    query.hide_finished = true;
    assert!(db.browse_contents(&query, true).unwrap().children.is_empty());
    query.hide_finished = false;
    assert_eq!(db.browse_contents(&query, true).unwrap().children[0].id, BrowseLocation::Folder(empty));
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

    db.connection.fixtures_update_book_dir_is_downloaded().unwrap();
    place_book(&db, first, shelf, false);
    place_book(&db, first, branch, true);
    db.connection.fixtures_update_book_dir_is_downloaded_2(&(&second), &(&third)).unwrap();
    db.connection.fixtures_update_book_format(&(&third)).unwrap();
    db.connection.fixtures_insert_book_language_4(&(&third)).unwrap();
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
    assert_eq!(searched.books.iter().filter(|b| b.book_id() == Some(first)).count(), 3);
    let source = [shelf, nested, branch].into_iter().min_by_key(|id| id.to_string()).unwrap();
    assert_eq!(searched.books.iter().find(|b| b.book_id() == Some(first)).unwrap().source_directory, Some(source));
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
    db.connection.fixtures_delete_book_dir_2(&(shelf.to_string()), &first).unwrap();
    assert_eq!(browse_folder(&db, ROOT_DIR_ID, "", &[]).children.iter().find(|c| c.id == library_model::BrowseLocation::Folder(shelf)).unwrap().book_count, 4);
    db.connection.fixtures_update_book_dir_dir_id(&(empty.to_string()), &(nested.to_string()), &first).unwrap();
    let moved = browse_folder(&db, ROOT_DIR_ID, "", &[]);
    assert_eq!(moved.children.iter().find(|c| c.id == library_model::BrowseLocation::Folder(shelf)).unwrap().book_count, 3);
    assert_eq!(moved.children.iter().find(|c| c.id == library_model::BrowseLocation::Folder(empty)).unwrap().book_count, 1);
    assert_eq!(browse_folder(&db, shelf, "Title", &[]).books.iter().find(|b| b.book_id() == Some(first)).unwrap().source_directory, Some(branch));
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
        db.connection.fixtures_update_book_dir_is_downloaded_3(&downloaded).unwrap();
        assert_eq!(counts(), (1, downloaded as usize));
    }
    let tx = db.connection.unchecked_transaction().unwrap();
    tx.fixtures_update_book_dir_deleted_at_3().unwrap();
    assert_eq!(db.connection.fixtures_read_book_dir_count(|r| r.get::<_, i64>(0)).unwrap(), 0);
    tx.rollback().unwrap();
    assert_eq!(counts(), (1, 0));
    db.connection.pragma_update(None, "query_only", true).unwrap();
    assert_eq!(browse_folder(&db, branch, "", &[]).books.len(), 1);
    db.connection.pragma_update(None, "query_only", false).unwrap();

    db.connection.fixtures_update_dir_name_parent_id(&(other.to_string()), &(branch.to_string())).unwrap();

    let view = browse_folder(&db, ROOT_DIR_ID, "", &[]);
    assert_eq!(view.children.iter().find(|c| c.id == library_model::BrowseLocation::Folder(shelf)).unwrap().book_count, 0);
    assert_eq!(view.children.iter().find(|c| c.id == library_model::BrowseLocation::Folder(other)).unwrap().book_count, 1);
    db.connection.fixtures_update_book_dir_deleted_at_4(&(&hash)).unwrap();
    assert_eq!(browse_folder(&db, ROOT_DIR_ID, "", &[]).children.iter().find(|c| c.id == library_model::BrowseLocation::Folder(other)).unwrap().book_count, 0);
    db.connection.fixtures_update_book_dir_deleted_at_2().unwrap();
    db.connection.fixtures_update_dir_deleted_at(&(other.to_string())).unwrap();
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
    db.connection.fixtures_insert_book_language_5().unwrap();
    let mut query = subject_query(library_model::BrowseLocation::Folder(ROOT_DIR_ID));
    let unfiltered = db.browse(&query, false).unwrap();
    assert_eq!(unfiltered.sections.len(), 1);
    assert_eq!(unfiltered.sections[0].parent.id, library_model::BrowseLocation::Folder(shelf));
    assert_eq!(unfiltered.sections[0].children.iter().map(|row| row.id).collect::<Vec<_>>(), [library_model::BrowseLocation::Folder(child), library_model::BrowseLocation::Folder(empty)]);
    let downloaded = db.browse(&query, true).unwrap();
    assert_eq!(downloaded.sections[0].children.iter().map(|row| row.id).collect::<Vec<_>>(), [library_model::BrowseLocation::Folder(child), library_model::BrowseLocation::Folder(empty)]);
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
    assert_eq!(page.contents.children.iter().map(|row| row.id).collect::<Vec<_>>(), [library_model::BrowseLocation::Folder(wrapper), library_model::BrowseLocation::Folder(plain)]);
    assert_eq!(page.sections.iter().map(|section| section.parent.id).collect::<Vec<_>>(), [library_model::BrowseLocation::Folder(plain), library_model::BrowseLocation::Folder(wrapper)]);
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
            db.connection.fixtures_update_book_dir_is_downloaded_4(&(book < downloads), &(&hash)).unwrap();
        }
    }
    db.connection.fixtures_insert_book_language_5().unwrap();
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
                let mut expected = if !search.is_empty() {
                    vec![("Écology", 1), ("Zoology", 1)]
                } else if sort == library_model::BrowseChipSort::BookCount {
                    vec![("Most", 2), ("Écology", 1), ("Zoology", 1)]
                } else {
                    vec![("Écology", 1), ("Most", 2), ("Zoology", 1)]
                };
                if !filtered && search.is_empty() {
                    let position = if sort == library_model::BrowseChipSort::BookCount { expected.len() } else { 1 };
                    expected.insert(position, ("Empty", 0));
                }
                assert_eq!(contents.children.iter().map(|row| (row.name.as_str(), row.book_count)).collect::<Vec<_>>(), expected);
                assert!(contents.children.iter().all(|row| row.book_count == row.downloaded_book_count));
            }
        }
    }
}

#[test]
fn subject_cards_and_global_facets_use_normalized_languages() {
    let (db, _path, _) = subject_fixture();
    db.connection.fixtures_insert_book_language_5().unwrap();
    let root = db.browse_contents(&subject_query(library_model::BrowseLocation::Subject(0)), false).unwrap();
    let mut query = subject_query(root.children[0].id);
    query.languages = vec!["en".into()];
    assert_eq!(db.browse_contents(&query, false).unwrap().books.len(), 4);
    let facets = db.browse(&query, false).unwrap();
    let (languages, formats) = (facets.language_counts, facets.format_counts);
    assert_eq!(languages.iter().find(|l| l.language == "en").unwrap().book_count, 4);
    assert_eq!(db.library_language_counts().unwrap(), languages);
    assert_eq!(formats.iter().map(|f| f.book_count).sum::<u32>(), 4);
}

#[test]
fn browse_progress_is_derived_without_rewriting_sync_registers_and_audio_is_book_scoped() {
    let db = database();
    let hash = import_in_folder(&db, 401, ROOT_DIR_ID);
    // Browse progress comes from the dedicated register, independent of the location.

    for (position, progress, expected) in [("0.25", 0., 0.), ("35", 0., 0.), ("chapter.xhtml", 0., 0.), ("0.25", 70., 70.), ("101", 0., 0.), (" 0.5suffix", 0., 0.)] {
        db.connection.fixtures_update_book_read_pos_read_progress(&position, &progress, &(&hash)).unwrap();
        assert_eq!(db.browse_book_detail(&hash).unwrap().book.progress, f64::from(expected) as f32);
        let stored: (String, f64) = db.connection.fixtures_read_book_read_pos_read_progress(&(&hash), |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(stored, (position.to_owned(), progress));
    }
    db.connection.fixtures_update_book_duration_ms(&(&hash)).unwrap();
    db.connection.fixtures_update_book_entry_count(&(&hash)).unwrap();
    let folder = db.create_directory(&ROOT_DIR_ID, &"Copy".into()).unwrap().id;
    place_book(&db, hash, folder, false);
    let detail = db.browse_book_detail(&hash).unwrap();
    assert_eq!(detail.book.audiobook_duration_ms, Some(12345));
    assert_eq!(detail.audiobook_chapter_count, Some(7));
    db.connection.fixtures_update_book_duration_ms_2(&(&hash)).unwrap();
    let detail = db.browse_book_detail(&hash).unwrap();
    assert_eq!(detail.book.audiobook_duration_ms, None);
    assert_eq!(detail.audiobook_chapter_count, None);
}

/// Books a search from the root finds, by title or author.
fn searched(db: &Database, search: &str) -> Vec<BookId> {
    browse_folder(db, ROOT_DIR_ID, search, &[]).books.into_iter().map(|book| book.book_id().unwrap()).collect()
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
    use crate::browse::FoldersSql;
    use crate::query_methods::active_query_fullscan_steps;
    let db = database();
    let small = db.create_directory(&ROOT_DIR_ID, &"Small".into()).unwrap().id;
    let child = db.create_directory(&small, &"Child".into()).unwrap().id;
    let big = db.create_directory(&ROOT_DIR_ID, &"Big".into()).unwrap().id;
    import_in_folder(&db, 901, child);

    let tx = db.connection.unchecked_transaction().unwrap();
    tx.fixtures_read_numbers_numbers_n_n().unwrap();
    tx.fixtures_insert_book_dir_5(&(big.to_string())).unwrap();
    tx.fixtures_read_numbers_numbers_n_n_2(&(big.to_string())).unwrap();
    tx.commit().unwrap();
    browse_folder(&db, ROOT_DIR_ID, "", &[]);
    let directories = serde_json::to_string(&[child.to_string()]).unwrap();
    let mut count = 0;
    db.connection
        .folder_direct_counts(&directories, 0, "", 0, f64::from(library_model::FINISHED_PROGRESS_THRESHOLD), |row| {
            count = row.get::<_, i64>(1)?;
            assert!(active_query_fullscan_steps(&db.connection) < 100, "direct counts must not scan unrelated books");
            Ok(())
        })
        .unwrap();
    assert_eq!(count, 1);
    db.connection.fixtures_insert_book_language_6().unwrap();
    for (formats, languages) in [(1, ""), (2, ""), (3, ""), (0, ",en,"), (1, ",en,"), (1, ",sv,")] {
        let mut actual = 0;
        db.connection
            .folder_direct_counts(&directories, formats, languages, 0, f64::from(library_model::FINISHED_PROGRESS_THRESHOLD), |row| {
                actual = row.get::<_, i64>(1)?;
                assert!(active_query_fullscan_steps(&db.connection) < 100, "filtered counts must not scan unrelated books");
                Ok(())
            })
            .unwrap();
        assert_eq!(actual, i64::from((formats == 0 || formats & 1 != 0) && (languages.is_empty() || languages == ",en,")));
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
    db.connection.fixtures_update_book_deleted_at(&(&hash)).unwrap();
    assert_eq!(root_count(), 0);
    db.connection.fixtures_update_book_deleted_at_2(&(&hash)).unwrap();
    assert_eq!(root_count(), 2);
    db.connection.fixtures_delete_dir(&(branch.to_string())).unwrap();
    assert_eq!(root_count(), 1);
    db.connection.fixtures_delete_book(&(&hash)).unwrap();
    assert_eq!(root_count(), 0);
    import_in_folder(&db, 902, shelf);
    db.connection.fixtures_delete_dir(&(shelf.to_string())).unwrap();
    assert_eq!(root_count(), 0);
}

fn projected_subject_codes(db: &Database) -> Vec<String> {
    query_rows(|callback| db.connection.fixtures_read_book_subject_code(callback), |row| row.get(0)).unwrap()
}

#[cfg(test)]
use crate::sql_fixtures::FixturesSql;

fn complete_no_match(db: &Database, hash: &BookId) {
    use metadata_contract::books::{BookEnrichment, BookEnrichmentStatus};
    let Some(candidate) = db.enrichment_candidates(&[*hash], 0).unwrap().pop() else { return };
    db.record_enrichment(hash, &candidate.request_key, BookEnrichment { key: String::new(), status: BookEnrichmentStatus::NoMatch, record: None, toc: None, cover: None, failure: None, detail: String::new() }, 0).unwrap();
}

#[test]
fn upload_preparation_excludes_physical_work_and_keeps_batch_membership_fixed() {
    let db = database();
    let branch = db.create_directory(&ROOT_DIR_ID, &"Branch".into()).unwrap().id;
    let nested = db.create_directory(&branch, &"Nested".into()).unwrap().id;
    let hashes: Vec<_> = (980..988).map(|index| import_in_folder(&db, index, if index == 985 { nested } else { ROOT_DIR_ID })).collect();
    db.connection.execute_batch("DELETE FROM local_book_work; DELETE FROM local_file_work; DELETE FROM local_directory_work; UPDATE book_transfer SET upload_id=NULL,upload_prepared=NULL;").unwrap();
    db.connection.execute("INSERT INTO book_transfer(content_hash,upload_id) VALUES(?1,42) ON CONFLICT(content_hash) DO UPDATE SET upload_id=42", [hashes[0]]).unwrap();
    db.connection.execute("INSERT INTO local_book_work(content_hash) VALUES(?1)", [hashes[2]]).unwrap();
    for (hash, operation) in [(hashes[3], "materialize"), (hashes[4], "remove")] {
        if operation == "materialize" {
            db.connection.execute("INSERT INTO local_file_work(operation,content_hash,dir_id) VALUES(?1,?2,?3)", (operation, hash, ROOT_DIR_ID.to_string())).unwrap();
        } else {
            db.connection.execute("INSERT INTO local_file_work(operation,content_hash,relative_path) VALUES(?1,?2,'/book.epub')", (operation, hash)).unwrap();
        }
    }
    db.connection.execute("INSERT INTO local_directory_work(dir_id) VALUES(?1)", [branch.to_string()]).unwrap();
    db.connection.execute("UPDATE local_file_projection SET dir_id=?1,relative_path='/Branch/Nested/remembered.epub' WHERE content_hash=?2", (nested.to_string(), hashes[6])).unwrap();
    db.connection.execute("UPDATE book SET deleted_at=1 WHERE content_hash=?1", [hashes[7]]).unwrap();

    let batch = db.prepared_upload_batch().unwrap();
    assert_eq!(batch.members, [(hashes[0], Some(42)), (hashes[1], None)]);

    let later = import_in_folder(&db, 988, ROOT_DIR_ID);
    db.connection.execute_batch("DELETE FROM local_book_work; DELETE FROM local_file_work; DELETE FROM local_directory_work;").unwrap();
    db.finish_upload_preparation(&batch).unwrap();
    for (index, hash) in hashes.iter().chain([&later]).enumerate() {
        let prepared: Option<i64> = db.connection.query_row("SELECT (SELECT upload_prepared FROM book_transfer WHERE content_hash=?1)", [hash], |row| row.get(0)).unwrap();
        assert_eq!(
            prepared,
            match index {
                0 => Some(42),
                1 => Some(0),
                _ => None,
            }
        );
    }
    let next = db.prepared_upload_batch().unwrap();
    assert_eq!(next.hashes(), hashes[..7].iter().copied().chain([later]).collect());
    assert!(db.connection.is_autocommit());
}

#[test]
fn thumbnail_download_pages_skip_unreachable_books_and_keep_materialized_paths() {
    let db = database();
    let hidden = db.create_directory(&ROOT_DIR_ID, &"Hidden".into()).unwrap().id;
    let live = db.create_directory(&ROOT_DIR_ID, &"Live".into()).unwrap().id;
    let hashes: Vec<_> = (960..966).map(|index| import_in_folder(&db, index, if index == 962 || index == 964 { live } else { hidden })).collect();
    db.record_server_covers(&hashes).unwrap();
    db.connection.execute("DELETE FROM local_file_projection WHERE content_hash!=?1", [hashes[3]]).unwrap();
    db.connection.execute("UPDATE dir SET deleted_at=1 WHERE id=?1", [hidden.to_string()]).unwrap();

    assert_eq!(db.pending_thumbnail_downloads(None, 2).unwrap(), hashes[2..4]);
    assert_eq!(db.pending_thumbnail_downloads(Some(&hashes[3]), 2).unwrap(), [hashes[4]]);
    assert!(db.pending_thumbnail_downloads(Some(&hashes[4]), 2).unwrap().is_empty());
    assert!(db.pending_thumbnail_downloads(None, 0).unwrap().is_empty());
    assert_eq!(db.pending_thumbnail_downloads(None, 6).unwrap(), hashes[2..5]);
    assert!(db.connection.is_autocommit());

    // Planning can also borrow the caller's transaction.
    let transaction = db.connection.unchecked_transaction().unwrap();
    assert_eq!(db.pending_thumbnail_downloads(None, 2).unwrap(), hashes[2..4]);
    assert!(!db.connection.is_autocommit());
    transaction.commit().unwrap();
}

#[test]
fn thumbnail_download_planning_checks_every_path_and_releases_failed_reads() {
    use library_database::rusqlite::types::Value;

    let db = database();
    let shelf = db.create_directory(&ROOT_DIR_ID, &"Shelf".into()).unwrap().id;
    let hash = import_in_folder(&db, 970, ROOT_DIR_ID);
    db.copy_book_to_directory(&hash, Some(&ROOT_DIR_ID), &shelf).unwrap();
    db.record_server_covers(&[hash]).unwrap();
    for corrupt in [Value::Text("/zz/../../outside.epub".into()), Value::Blob(vec![255])] {
        db.connection
            .execute("INSERT INTO local_file_projection(content_hash,dir_id,relative_path) VALUES(?1,?2,?3) ON CONFLICT(content_hash,dir_id) DO UPDATE SET relative_path=excluded.relative_path", (hash, shelf.to_string(), corrupt))
            .unwrap();
        assert!(db.pending_thumbnail_downloads(None, 1).is_err());
        assert!(db.connection.is_autocommit());
    }
    db.connection.execute("UPDATE local_file_projection SET relative_path='/Shelf/book970.epub' WHERE content_hash=?1 AND dir_id=?2", (hash, shelf.to_string())).unwrap();
    // Bypass the name index to simulate corruption already stored on disk.
    db.connection.execute_batch("DROP INDEX idx_book_dir_live_portable_name;").unwrap();
    for corrupt in [Value::Text("../../outside.epub".into()), Value::Blob(vec![255])] {
        db.connection.execute("UPDATE book_dir SET file_name=?1 WHERE dir_id=?2", (corrupt, shelf.to_string())).unwrap();
        assert!(db.pending_thumbnail_downloads(None, 1).is_err(), "materialized paths must not hide corrupt live placements");
        assert!(db.connection.is_autocommit());
    }
    db.connection.execute("UPDATE book_dir SET file_name='book970.epub' WHERE dir_id=?1", [shelf.to_string()]).unwrap();
    assert_eq!(db.pending_thumbnail_downloads(None, 1).unwrap(), [hash]);
}

#[test]
fn transfer_snapshot_preserves_versions_projected_paths_and_requested_order() {
    let db = database();
    let shelf = db.create_directory(&ROOT_DIR_ID, &"Shelf".into()).unwrap().id;
    let first = import_in_folder(&db, 910, ROOT_DIR_ID);
    let second = import_in_folder(&db, 911, shelf);
    let missing = BookId::for_local_content(blake3::hash(b"separate book").into());
    db.copy_book_to_directory(&first, Some(&ROOT_DIR_ID), &shelf).unwrap();
    db.connection.execute("UPDATE book_dir SET local_hash=CASE WHEN dir_id=?1 THEN 'root-version' ELSE 'shelf-version' END WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?2)", (ROOT_DIR_ID.to_string(), first)).unwrap();
    db.connection
        .execute(
            "INSERT INTO local_file_projection(content_hash,dir_id,relative_path) VALUES(?1,?2,'/previous/book.epub') ON CONFLICT(content_hash,dir_id) DO UPDATE SET relative_path=excluded.relative_path",
            (first, ROOT_DIR_ID.to_string()),
        )
        .unwrap();

    let snapshot = db.transfer_snapshot(&[second, missing, first]).unwrap();
    let asset = snapshot.asset_for(&first).unwrap();
    assert_eq!(asset.upload.local_versions, [(ROOT_DIR_ID.to_string(), "book910.epub".into(), "root-version".into()), (shelf.to_string(), "book910.epub".into(), "shelf-version".into())]);
    assert_eq!(asset.upload_paths.iter().map(|path| path.as_str()).collect::<Vec<_>>(), ["/previous/book.epub", "/book910.epub", "/Shelf/book910.epub"]);
    assert!(snapshot.asset_for(&missing).unwrap().upload_paths.is_empty());
    assert_eq!(snapshot.placements.iter().map(|entry| (entry.book_id, entry.rel_path.as_str())).collect::<Vec<_>>(), [(second, "/Shelf/book911.epub"), (first, "/book910.epub"), (first, "/Shelf/book910.epub")]);
    for (entry, directory) in snapshot.placements.iter().zip([shelf, ROOT_DIR_ID, shelf]) {
        let book = entry.book.as_ref().unwrap();
        assert_eq!(book.book_id, entry.book_id);
        assert_eq!(book.relative_path, entry.rel_path);
        assert_eq!(book.directory, directory);
        assert_eq!(book.format, book_model::BookFormat::Epub);
        assert!(book.audiobook_track_titles.is_empty());
        assert!(book.occupied_names.contains(&book.name));
    }

    let duplicate = db.transfer_snapshot(&[first, first]).unwrap();
    assert_eq!(duplicate.asset_for(&first).unwrap().upload.local_versions.len(), 4);
    assert_eq!(duplicate.asset_for(&first).unwrap().upload_paths.len(), 6);
    assert_eq!(duplicate.placements.len(), 8);
}

#[test]
fn transfer_snapshot_releases_its_transaction_after_invalid_projected_paths() {
    let db = database();
    let hash = import_in_folder(&db, 912, ROOT_DIR_ID);
    db.connection
        .execute("INSERT INTO local_file_projection(content_hash,dir_id,relative_path) VALUES(?1,?2,'../outside.epub') ON CONFLICT(content_hash,dir_id) DO UPDATE SET relative_path=excluded.relative_path", (hash, ROOT_DIR_ID.to_string()))
        .unwrap();
    assert!(db.transfer_snapshot(&[hash]).is_err());
    assert!(db.connection.is_autocommit());
    db.connection.execute("UPDATE local_file_projection SET relative_path='/recovered.epub' WHERE content_hash=?1", [hash]).unwrap();
    let snapshot = db.transfer_snapshot(&[hash]).unwrap();
    assert_eq!(snapshot.asset_for(&hash).unwrap().upload_paths.iter().map(|path| path.as_str()).collect::<Vec<_>>(), ["/recovered.epub", "/book912.epub"]);
}

#[test]
fn subtree_walk_visits_each_descendant_once_even_with_repeated_roots_and_a_cycle() {
    let db = database();
    let first = DirId::from_u128(930);
    let second = DirId::from_u128(931);
    let third = DirId::from_u128(932);
    for (id, parent, name) in [(first, ROOT_DIR_ID, "First"), (second, first, "Second"), (third, second, "Third")] {
        db.create_directory_with_id(&id, &parent, &name.into()).unwrap();
    }
    let walk = || crate::placements::walk_down(&db.connection, vec![first.to_string(), first.to_string()], false).unwrap();
    assert_eq!(walk(), [first.to_string(), second.to_string(), third.to_string()]);
    db.connection.execute("UPDATE dir SET parent_id=?1 WHERE id=?2", (third.to_string(), first.to_string())).unwrap();
    assert_eq!(walk(), [first.to_string(), second.to_string(), third.to_string()]);
}

#[test]
fn subtree_trash_and_restore_include_nested_books_and_shared_placements() {
    let db = database();
    let shelf = db.create_directory(&ROOT_DIR_ID, &"Shelf".into()).unwrap().id;
    let nested = db.create_directory(&shelf, &"Nested".into()).unwrap().id;
    let deepest = db.create_directory(&nested, &"Deepest".into()).unwrap().id;
    let only = import_in_folder(&db, 933, deepest);
    let shared = import_in_folder(&db, 934, deepest);
    db.copy_book_to_directory(&shared, Some(&deepest), &ROOT_DIR_ID).unwrap();
    let nested_placement_live =
        || db.connection.query_row("SELECT deleted_at IS NULL FROM book_dir WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1) AND dir_id=?2", (shared, deepest.to_string()), |row| row.get::<_, bool>(0)).unwrap();

    db.trash_directory(&shelf).unwrap();

    let trash = db.library_trash().unwrap();
    assert!(trash.books.iter().any(|book| book.book_id == only));
    assert!(trash.books.iter().all(|book| book.book_id != shared));
    assert!(!nested_placement_live());
    assert_eq!(db.browse_book_detail(&shared).unwrap().book.book_id(), Some(shared));

    db.restore_directory(&shelf, None).unwrap();

    assert!(db.library_trash().unwrap().books.is_empty());
    assert!(nested_placement_live());
    assert_eq!(db.browse_book_detail(&only).unwrap().book.book_id(), Some(only));
    assert_eq!(db.dir_path_components(&deepest).unwrap().iter().map(|part| part.id).collect::<Vec<_>>(), [shelf, nested, deepest]);
}

#[test]
fn restored_nested_books_queue_paths_after_parent_and_name_changes() {
    let db = database();
    let shelf = db.create_directory(&ROOT_DIR_ID, &"Shelf".into()).unwrap().id;
    let nested = db.create_directory(&shelf, &"Nested".into()).unwrap().id;
    let first = import_in_folder(&db, 946, nested);
    let second = import_in_folder(&db, 947, nested);
    db.trash_directory(&shelf).unwrap();
    let destination = db.create_directory(&ROOT_DIR_ID, &"Target".into()).unwrap().id;
    db.create_directory(&destination, &"Shelf".into()).unwrap();
    // Forget the fixture imports' remembered paths so both placements need work.
    db.connection.execute_batch("DELETE FROM local_file_projection; DELETE FROM local_file_work;").unwrap();

    let restored = db.restore_directory(&shelf, Some(&destination)).unwrap();

    assert_ne!(restored, "Shelf", "restoring must resolve the destination's name collision");
    let components = db.dir_path_components(&nested).unwrap();
    assert_eq!(components.iter().map(|part| part.id).collect::<Vec<_>>(), [destination, shelf, nested]);
    let mut work = db
        .native_file_work()
        .unwrap()
        .into_iter()
        .map(|work| {
            let snapshot = db.native_file_work_snapshot(work).unwrap().unwrap();
            (snapshot.work.book_id, snapshot.work.operation, snapshot.resolved_path.to_string())
        })
        .collect::<Vec<_>>();
    work.sort();
    assert_eq!(work, [(first, "materialize".to_owned(), format!("/Target/{restored}/Nested/book946.epub")), (second, "materialize".to_owned(), format!("/Target/{restored}/Nested/book947.epub"))]);
}

#[test]
fn subtree_purge_follows_retained_parents_of_suppressed_descendants() {
    use library_replica::DirectoryLifecycleState;

    for empty_all in [false, true] {
        let db = database();
        let shelf = db.create_directory(&ROOT_DIR_ID, &"Shelf".into()).unwrap().id;
        let nested = db.create_directory(&shelf, &"Nested".into()).unwrap().id;
        let deepest = db.create_directory(&nested, &"Deepest".into()).unwrap().id;
        let unrelated = db.create_directory(&ROOT_DIR_ID, &"Unrelated".into()).unwrap().id;
        db.trash_directory(&shelf).unwrap();
        if empty_all {
            db.empty_trash().unwrap();
        } else {
            db.purge_directory(&shelf).unwrap();
        }
        for id in [shelf, nested, deepest] {
            assert_eq!(crate::placements::canonical_directory(&db.connection, &id).unwrap().lifecycle, Some(DirectoryLifecycleState::Purged));
        }
        assert_eq!(crate::placements::canonical_directory(&db.connection, &unrelated).unwrap().lifecycle, Some(DirectoryLifecycleState::Present));
    }
}

#[test]
fn manifest_settlement_preserves_missing_and_stale_upload_intents() {
    use library_model::BlobKind;

    let db = database();
    let hashes = (1..=3).map(|index| import_in_folder(&db, index, ROOT_DIR_ID)).collect::<Vec<_>>();
    for hash in &hashes {
        db.connection.fixtures_ready_book_upload(hash).unwrap();
        complete_no_match(&db, hash);
    }
    let mut planned = db.pending_book_uploads(0, 10).unwrap();
    assert_eq!(planned.len(), 3);
    // The server owns the first two, but one response describes an obsolete
    // upload version. The current version must remain queued for negotiation.
    let owned = [planned[0].book_id, planned[1].book_id];
    planned[1].size_bytes += 1;
    db.settle_book_manifest(planned.iter().filter(|intent| owned.contains(&intent.book_id))).unwrap();

    assert_eq!(db.remote_asset_hashes(BlobKind::Book).unwrap(), [planned[0].book_id].into_iter().collect());
    let pending = db.pending_book_uploads(0, 10).unwrap();
    assert_eq!(pending.len(), 2);
    assert!(pending.iter().any(|intent| intent.book_id == planned[1].book_id && intent.size_bytes != planned[1].size_bytes));
    assert!(pending.iter().any(|intent| intent == &planned[2]));
    // Repeating the same manifest cannot settle either remaining intent.
    db.settle_book_manifest(planned.iter().filter(|intent| owned.contains(&intent.book_id))).unwrap();
    assert_eq!(db.pending_book_uploads(0, 10).unwrap(), pending);
}

#[test]
fn enrichment_cover_checks_preserve_transfer_state_and_survive_reopening() {
    let path = std::env::temp_dir().join(format!("enrichment-cover-check-{}.db", uuid::Uuid::new_v4()));
    let db = Database::open(&path).unwrap();
    db.initialize_library().unwrap();
    let book = import();
    let hash = book.book_id;
    db.commit_import(book).unwrap();
    db.complete_thumbnail_work(&hash, false).unwrap();
    assert!(db.enrichment_cover_candidates(&[hash]).unwrap().is_empty());
    let candidate = db.enrichment_candidates(&[hash], 0).unwrap().pop().unwrap();
    let answer = metadata_contract::books::BookEnrichment { key: String::new(), status: metadata_contract::books::BookEnrichmentStatus::NoMatch, record: None, toc: None, cover: None, failure: None, detail: String::new() };
    db.record_enrichment(&hash, &candidate.request_key, answer, 0).unwrap();
    assert_eq!(db.enrichment_cover_record(&hash).unwrap().unwrap().title, "Original");
    assert_eq!(db.enrichment_cover_candidates(&[hash]).unwrap(), vec![hash]);
    db.mark_enrichment_cover_checked(&hash).unwrap();
    assert!(db.enrichment_cover_candidates(&[hash]).unwrap().is_empty());
    assert_eq!(db.thumbnail_work_state(&hash).unwrap().as_deref(), Some("no_cover"));
    drop(db);
    let reopened = Database::open(&path).unwrap();
    reopened.initialize_library().unwrap();
    assert!(reopened.enrichment_cover_candidates(&[hash]).unwrap().is_empty());
    assert_eq!(reopened.thumbnail_work_state(&hash).unwrap().as_deref(), Some("no_cover"));
    drop(reopened);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn direct_cover_lookup_uses_file_title_and_enriched_attributes_without_a_server_cover_id() {
    use book_model::{AUTHOR_MARC_RELATOR_CODE, Contributor, Identifier, Scheme, Scope};
    use metadata_contract::books::{BookEnrichment, BookEnrichmentStatus};
    let db = database();
    let book = import();
    let hash = book.book_id;
    db.commit_import(book).unwrap();
    let candidate = db.enrichment_candidates(&[hash], 0).unwrap().pop().unwrap();
    let mut enriched = candidate.record.clone();
    enriched.title = "Provider edition title".into();
    enriched.description = "Provider description".into();
    enriched.contributors = vec![Contributor::new("Matched Author", AUTHOR_MARC_RELATOR_CODE).unwrap()];
    enriched.book.identifiers.push(Identifier::new("9780306406157", Scheme::Isbn, Scope::Book).unwrap());
    db.record_enrichment(&hash, &candidate.request_key, BookEnrichment { key: String::new(), status: BookEnrichmentStatus::Found, record: Some(enriched.clone()), toc: None, cover: None, failure: None, detail: String::new() }, 0).unwrap();
    let record = db.enrichment_cover_record(&hash).unwrap().unwrap();
    assert_eq!(record.title, "Original");
    assert_eq!(record.book.identifiers, enriched.book.identifiers);
    assert_eq!(record.contributors, enriched.contributors);
    assert!(record.description.is_empty());
}

#[test]
fn transfer_plans_preserve_root_order_and_visit_nested_selections_once() {
    use library_database::transfer::TransferSelection;
    use std::collections::HashSet;

    let db = database();
    let alpha = db.create_directory(&ROOT_DIR_ID, &"Alpha".into()).unwrap().id;
    let zulu = db.create_directory(&ROOT_DIR_ID, &"Zulu".into()).unwrap().id;
    let second = db.create_directory(&alpha, &"Second".into()).unwrap().id;
    let first = db.create_directory(&alpha, &"First".into()).unwrap().id;
    let leaf = db.create_directory(&first, &"Leaf".into()).unwrap().id;
    let empty = db.create_directory(&zulu, &"Empty".into()).unwrap().id;
    let other = db.create_directory(&ROOT_DIR_ID, &"Other".into()).unwrap().id;
    let shared = import_in_folder(&db, 910, first);
    db.copy_book_to_directory(&shared, Some(&first), &second).unwrap();
    let direct = import_in_folder(&db, 911, ROOT_DIR_ID);
    import_in_folder(&db, 912, other);
    let selection = TransferSelection { folders: vec![zulu, first, alpha, zulu, leaf], books: vec![direct, direct], source: Some(ROOT_DIR_ID) };
    let writes = db.connection.total_changes();

    let plan = db.transfer_plan(selection.clone()).unwrap();

    assert_eq!(plan.selection, selection);
    assert_eq!(plan.roots, [zulu, alpha]);
    assert_eq!(plan.folders.iter().map(|folder| folder.id).collect::<Vec<_>>(), [zulu, alpha, empty, first, second, leaf]);
    assert_eq!(plan.books.len(), 3);
    assert!(plan.books[..2].iter().all(|book| book.book_id == shared && !book.direct));
    assert_eq!(plan.books[..2].iter().map(|book| book.directory).collect::<HashSet<_>>(), HashSet::from([first, second]));
    assert_eq!((plan.books[2].book_id, plan.books[2].directory, plan.books[2].direct), (direct, ROOT_DIR_ID, true));
    assert_eq!(db.transfer_plan(selection).unwrap(), plan);

    let books_only = db.transfer_plan(TransferSelection { folders: vec![], books: vec![direct, direct], source: Some(ROOT_DIR_ID) }).unwrap();
    assert!(books_only.roots.is_empty() && books_only.folders.is_empty());
    assert_eq!(books_only.books, [plan.books[2].clone()]);
    assert_eq!(db.connection.total_changes(), writes, "planning must leave the source untouched");
}

#[test]
fn direct_transfer_plans_preserve_source_choice_selection_order_and_late_row_errors() {
    use library_database::transfer::TransferSelection;
    use std::collections::HashSet;

    let db = database();
    let before = DirId::from_u128(10);
    let tree = DirId::from_u128(20);
    let after = DirId::from_u128(30);
    for (id, name) in [(before, "Before"), (tree, "Tree"), (after, "After")] {
        db.create_directory_with_id(&id, &ROOT_DIR_ID, &name.into()).unwrap();
    }
    let outside_first = import_in_folder(&db, 930, before);
    db.copy_book_to_directory(&outside_first, Some(&before), &tree).unwrap();
    let inside_first = import_in_folder(&db, 931, tree);
    db.copy_book_to_directory(&inside_first, Some(&tree), &after).unwrap();
    let direct = import_in_folder(&db, 932, before);
    let selection = TransferSelection { folders: vec![tree], books: vec![direct, outside_first, direct, inside_first], source: None };

    let plan = db.transfer_plan(selection).unwrap();

    assert_eq!(plan.books.len(), 4);
    assert!(plan.books[..2].iter().all(|book| !book.direct && book.directory == tree));
    assert_eq!(plan.books[..2].iter().map(|book| book.book_id).collect::<HashSet<_>>(), HashSet::from([outside_first, inside_first]));
    assert_eq!(plan.books[2..].iter().map(|book| (book.book_id, book.directory, book.direct)).collect::<Vec<_>>(), [(direct, before, true), (outside_first, before, true)]);

    let explicit = db.transfer_plan(TransferSelection { folders: vec![tree], books: vec![inside_first, inside_first], source: Some(after) }).unwrap();
    assert_eq!(explicit.books.len(), 3);
    assert_eq!((explicit.books[2].book_id, explicit.books[2].directory, explicit.books[2].direct), (inside_first, after, true));
    assert!(db.transfer_plan(TransferSelection { folders: vec![tree], books: vec![outside_first], source: Some(after) }).is_err());
    assert!(db.transfer_plan(TransferSelection { folders: vec![tree], books: vec![BookId::from_bytes([255; 32])], source: None }).is_err());

    let chosen = TransferSelection { folders: vec![], books: vec![inside_first], source: Some(tree) };
    let expected = db.transfer_plan(chosen.clone()).unwrap();
    // The portable-name index normally rejects this corrupt value on write.
    // Simulate stored corruption in a later, unchosen copy of the selected book.
    db.connection.execute_batch("DROP INDEX idx_book_dir_live_portable_name;").unwrap();
    db.connection.execute("UPDATE book_dir SET file_name=?1 WHERE dir_id=?2", (vec![255_u8], after.to_string())).unwrap();
    assert!(db.transfer_plan(chosen.clone()).is_err());
    assert!(db.connection.is_autocommit());
    db.connection.execute("UPDATE book_dir SET file_name='book931.epub' WHERE dir_id=?1", [after.to_string()]).unwrap();
    assert_eq!(db.transfer_plan(chosen).unwrap(), expected);
}

#[test]
fn flat_book_copy_preserves_each_source_and_file_name() {
    let db = database();
    let first_source = db.create_directory(&ROOT_DIR_ID, &"First".into()).unwrap().id;
    let second_source = db.create_directory(&ROOT_DIR_ID, &"Second".into()).unwrap().id;
    let destination = db.create_directory(&ROOT_DIR_ID, &"Copies".into()).unwrap().id;
    let first = import_in_folder(&db, 901, first_source);
    let second = import_in_folder(&db, 902, second_source);

    for hash in [first, second] {
        assert!(db.copy_book_to_directory(&hash, None, &destination).unwrap());
    }
    assert_eq!(browse_folder(&db, first_source, "", &[]).books[0].book_id(), Some(first));
    assert_eq!(browse_folder(&db, second_source, "", &[]).books[0].book_id(), Some(second));
    assert_eq!(browse_folder(&db, destination, "", &[]).books.len(), 2);
    assert!(db.has_book_named(&destination, "book901.epub").unwrap());
    assert!(db.has_book_named(&destination, "book902.epub").unwrap());
    assert_eq!(db.browse_contents(&subject_query(library_model::BrowseLocation::Books), false).unwrap().books.len(), 2);
}

#[test]
fn flat_book_copy_failure_keeps_the_successful_prefix() {
    let db = database();
    let destination = db.create_directory(&ROOT_DIR_ID, &"Copies".into()).unwrap().id;
    let first = import_in_folder(&db, 905, ROOT_DIR_ID);
    let missing = BookId::new(&"f".repeat(64));
    assert!(db.copy_book_to_directory(&first, None, &destination).unwrap());
    assert!(db.copy_book_to_directory(&missing, None, &destination).is_err());
    assert_eq!(browse_folder(&db, destination, "", &[]).books[0].book_id(), Some(first));
    assert_eq!(browse_folder(&db, ROOT_DIR_ID, "", &[]).books[0].book_id(), Some(first));
}
