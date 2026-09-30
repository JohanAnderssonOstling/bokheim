use library_database::{Database, ImportCommit, NativeDirectoryAcknowledgement, NativeDirectoryCompletion, PublishedImport};
use sync_common::{ContentHash, DirId, ROOT_DIR_ID, StateCell};

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
            metadata: book_model::BookRecord {
                title: "Original".into(),
                subtitle: None,
                contributors: vec![book_model::Contributor::new("Original Author", book_model::MarcRelatorCode(*b"aut")).unwrap()],
                description: String::new(),
                book: Default::default(),
            },
            toc: vec![book_model::BookTocEntry { title: "Chapter".into(), target: "ch1".into(), children: vec![] }],
        }),
        published: PublishedImport { name: "book.epub".into(), relative_path: "/book.epub".into(), published: false, fingerprint: None },
        request_thumbnail: false,
        restore_paths: vec![],
    }
}

#[test]
#[cfg(feature = "scanner")]
fn scan_operation_finishes_only_with_a_readable_final_batch() {
    use library_database::{FilesystemScan, InspectedFilesystemScan, OperationActivity, ScanApplyResult};
    use library_replica::LibraryOperationState;
    let db = database();
    let discovery = FilesystemScan { readable: true, ..Default::default() };
    let inspected = InspectedFilesystemScan { failures: vec![], readable: true, books: vec![] };

    // Start without polling Settings: scan lifecycle must not depend on UI reads.
    let mut snapshot = db.scan_snapshot().unwrap();
    assert!(db.needs_scan().unwrap());
    assert_eq!(db.apply_filesystem_scan_batch(&mut snapshot, &discovery, &inspected, false).unwrap(), ScanApplyResult::Applied);
    assert!(db.needs_scan().unwrap());
    assert_ne!(db.operation_status(OperationActivity::default()).unwrap().operation.unwrap().state, LibraryOperationState::Completed);

    // A newer scan invalidates the old result; it cannot finish the new scan.
    let current = db.scan_snapshot().unwrap();
    assert_eq!(db.apply_filesystem_scan(&snapshot, &discovery, &inspected).unwrap(), ScanApplyResult::Stale);
    assert!(db.needs_scan().unwrap());
    assert_eq!(db.apply_filesystem_scan(&current, &discovery, &inspected).unwrap(), ScanApplyResult::Applied);
    assert!(!db.needs_scan().unwrap());
    let files_pending = db.operation_status(OperationActivity { local_busy: true, ..Default::default() }).unwrap().operation.unwrap();
    assert_ne!(files_pending.state, LibraryOperationState::Completed);
    let completed = db.operation_status(OperationActivity::default()).unwrap().operation.unwrap();
    assert_eq!(completed.state, LibraryOperationState::Completed);
    assert!(completed.waiting_reason.is_none());
    assert!(db.operation_status(OperationActivity::default()).unwrap().operation.is_none());

    // An unreadable directory walk still needs another scan on reopen.
    let snapshot = db.scan_snapshot().unwrap();
    let unreadable = InspectedFilesystemScan { failures: vec![], readable: false, books: vec![] };
    db.apply_filesystem_scan(&snapshot, &FilesystemScan { readable: false, ..Default::default() }, &unreadable).unwrap();
    let failed = db.operation_status(OperationActivity::default()).unwrap().operation.unwrap();
    assert!(failed.needs_attention);
    assert!(failed.waiting_reason.unwrap().contains("could not be scanned"));
    assert!(db.needs_scan().unwrap());
    let snapshot = db.scan_snapshot().unwrap();
    db.apply_filesystem_scan(&snapshot, &discovery, &inspected).unwrap();
    assert_eq!(db.operation_status(OperationActivity::default()).unwrap().operation.unwrap().state, LibraryOperationState::Completed);
}

#[test]
fn worker_reconciliation_persists_an_idle_import_completion() {
    use library_database::OperationActivity;
    let db = database();
    db.connection.execute("UPDATE sync_metadata SET operation_id='import',operation_kind='Import',operation_scan_complete=1,operation_completed=0 WHERE singleton=1", []).unwrap();

    // This is the call made by a worker after its final batch, without any
    // Settings transfer-status request.
    db.reconcile_operation(OperationActivity::default()).unwrap();

    assert!(db.connection.query_row("SELECT operation_completed FROM sync_metadata WHERE singleton=1", [], |row| row.get::<_, bool>(0)).unwrap());
}

#[test]
#[cfg(feature = "scanner")]
fn scan_operation_recovers_after_reopening_between_batches() {
    use library_database::{FilesystemScan, InspectedFilesystemScan, OperationActivity};
    use library_replica::LibraryOperationState;
    let root = std::env::temp_dir().join(format!("scan-operation-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("library.sqlite");
    let discovery = FilesystemScan { readable: true, ..Default::default() };
    let inspected = InspectedFilesystemScan { failures: vec![], readable: true, books: vec![] };
    {
        let db = Database::open(&path).unwrap();
        db.initialize_library().unwrap();
        let mut snapshot = db.scan_snapshot().unwrap();
        db.apply_filesystem_scan_batch(&mut snapshot, &discovery, &inspected, false).unwrap();
    }
    {
        let db = Database::open(&path).unwrap();
        assert!(db.needs_scan().unwrap());
        let snapshot = db.scan_snapshot().unwrap();
        db.apply_filesystem_scan(&snapshot, &discovery, &inspected).unwrap();
        // Close before Settings reads completion.
    }
    {
        let db = Database::open(&path).unwrap();
        assert!(!db.needs_scan().unwrap());
        assert_eq!(db.operation_status(OperationActivity::default()).unwrap().operation.unwrap().state, LibraryOperationState::Completed);
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn local_operation_does_not_present_future_sync_work_as_pending_uploads() {
    use library_database::OperationActivity;
    use library_replica::LibraryOperationState;
    let db = database();
    let book = import();
    let hash = book.content_hash;
    db.commit_import(book).unwrap();
    db.connection.execute("INSERT INTO book_transfer(content_hash,upload_id,upload_checksum,upload_size) VALUES(?1,1,?1,10) ON CONFLICT(content_hash) DO UPDATE SET upload_id=COALESCE(upload_id,1),upload_checksum=COALESCE(upload_checksum,excluded.upload_checksum),upload_size=COALESCE(upload_size,10)", [hash.as_str()]).unwrap();
    let pending_changes: i64 = db.connection.query_row("SELECT count(*) FROM sync_outbox", [], |row| row.get(0)).unwrap();
    assert!(pending_changes > 0);

    let local = db.operation_status(OperationActivity { scanning: true, ..Default::default() }).unwrap().operation.unwrap();
    assert!(!local.requires_sync);
    assert_eq!(local.books, 1);
    assert_eq!((local.pending_uploads, local.pending_changes), (0, 0));
    assert!(!local.needs_attention);
    assert!(local.waiting_reason.is_none());

    // Finish local work while leaving its future sync records intact.
    db.connection.execute("UPDATE sync_metadata SET operation_scan_complete=1 WHERE singleton=1", []).unwrap();
    let completed = db.operation_status(OperationActivity::default()).unwrap().operation.unwrap();
    assert_eq!(completed.state, LibraryOperationState::Completed);
    assert!(!completed.requires_sync);
    assert_eq!(db.connection.query_row("SELECT count(*) FROM book_transfer WHERE upload_id IS NOT NULL", [], |row| row.get::<_, i64>(0)).unwrap(), 1);
    assert_eq!(db.connection.query_row("SELECT count(*) FROM sync_outbox", [], |row| row.get::<_, i64>(0)).unwrap(), pending_changes);

    let signed_in = db.operation_status(OperationActivity { signed_in: true, ..Default::default() }).unwrap().operation.unwrap();
    assert!(signed_in.requires_sync);
    assert_eq!((signed_in.pending_uploads, signed_in.pending_changes), (1, u64::try_from(pending_changes).unwrap()));

    // A real sync interrupted by sign-out should still ask the user to resume.
    let signed_out = db.operation_status(OperationActivity::default()).unwrap().operation.unwrap();
    assert!(signed_out.requires_sync);
    assert_eq!(signed_out.pending_uploads, 1);
    assert_eq!(signed_out.waiting_reason.as_deref(), Some("Sign in to finish syncing"));
}

fn assert_title_edit_syncs(db: &Database) {
    assert!(!db.connection.query_row("SELECT metadata_batch_flag(NULL)", [], |r| r.get::<_, bool>(0)).unwrap());
    db.clear_sync_outbox().unwrap();
    db.connection.execute("UPDATE book SET title='Edited'", []).unwrap();
    let mutations = db.sync_publishable_mutations().unwrap();
    assert_eq!(mutations.len(), 1);
    assert!(matches!(&mutations[0].body, library_replica::MutationBody::Metadata { value, .. } if value.title == "Edited"));
}

#[test]
fn successful_import_publishes_contributors_and_restores_metadata_sync() {
    let db = database();
    db.commit_import(import()).unwrap();
    let mutations = db.sync_publishable_mutations().unwrap();
    assert!(mutations.iter().any(|mutation| matches!(mutation.body, library_replica::MutationBody::BookLifecycle { value: library_replica::BookLifecycleState::Present, .. })));
    let metadata = mutations
        .iter()
        .find_map(|mutation| match &mutation.body {
            library_replica::MutationBody::Metadata { value, .. } => Some(value),
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
    assert!(db.sync_publishable_mutations().unwrap().iter().any(|mutation| matches!(&mutation.body,
        library_replica::MutationBody::Metadata { value, .. } if value.title == "Original" && value.contributors.is_empty())));
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
    let mut cells = vec![StateCell { kind: "book_toc".into(), entity_key: hash.to_string(), entity_subkey: String::new() }];
    for (checksum, width) in [("a".repeat(64), 100.0), ("b".repeat(64), 200.0)] {
        let metadata =
            pdf_view_common::PdfReaderMetadata { version: 1, checksum: checksum.clone(), pages: vec![pdf_view_common::PdfStoredPage { width, height: 300.0, rotation: 0 }], skippable_pages: None, annotations: vec![], dependencies: None };
        db.connection.execute("INSERT INTO pdf_reader_metadata VALUES (?1,?2,?3)", library_database::rusqlite::params![hash.as_str(), checksum, serde_json::to_vec(&metadata).unwrap()]).unwrap();
        cells.push(StateCell { kind: "pdf_reader_metadata".into(), entity_key: hash.to_string(), entity_subkey: checksum });
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
    let missing = StateCell { kind: "pdf_reader_metadata".into(), entity_key: hash.to_string(), entity_subkey: "c".repeat(64) };
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

fn audible_job(db: &Database) -> (library_database::AudibleBatchJob, metadata_contract::audible::LookupResponse) {
    use metadata_contract::audible::*;
    let book = import();
    let hash = book.content_hash;
    db.commit_import(book).unwrap();
    let request = LookupRequest { title: "Audio".into(), authors: vec![], duration_ms: 1000, recording: Default::default(), chapters: vec![] };
    db.connection.execute("INSERT INTO audiobook_metadata(content_hash,duration_ms) VALUES(?1,1000)", [hash.as_str()]).unwrap();
    db.connection
        .execute(
            "INSERT INTO audible_enrichment_jobs(content_hash,policy,request_json,status,updated_at) VALUES(?1,?2,?3,'pending',0)",
            library_database::rusqlite::params![hash.as_str(), POLICY_VERSION, serde_json::to_string(&request).unwrap()],
        )
        .unwrap();
    let job = db.claim_audible_batch(10, hash).unwrap().unwrap();
    let response = LookupResponse {
        status: LookupStatus::Supported,
        candidates: vec![Candidate {
            asin: "B012345678".into(),
            region: "us".into(),
            title: "Audio".into(),
            subtitle: None,
            authors: vec![],
            narrators: vec![],
            publisher: None,
            description: None,
            release_date: None,
            isbns: vec![],
            abridged: None,
            provider_duration_ms: Some(1000),
            duration_ms: Some(1000),
            chapters: vec![],
            source: DiscoverySource::Api,
            metadata_provider: "audible".into(),
            chapter_provider: None,
        }],
        selected_asin: Some("B012345678".into()),
        selected_region: Some("us".into()),
        used_website_fallback: false,
        detail: String::new(),
        chapter_plan: Some(ChapterPlan {
            method: AlignmentMethod::EqualCountNames,
            chapters: vec![Chapter { title: "Opening".into(), start_ms: 0, end_ms: 400 }, Chapter { title: "Conclusion".into(), start_ms: 400, end_ms: 1000 }],
            renamed: 2,
            detail: String::new(),
        }),
    };
    (job, response)
}

#[test]
fn audible_success_stores_playable_and_synchronized_navigation() {
    let db = database();
    let (job, response) = audible_job(&db);
    db.clear_sync_outbox().unwrap();
    assert!(db.resolve_audible_batch(&job, &response, 400, 20).unwrap());
    let chapters = db.audiobook_chapters(&job.content_hash).unwrap();
    assert_eq!(chapters.len(), 2);
    assert_eq!(chapters[1].title, "Conclusion");
    assert_eq!(chapters[1].start_ms, 400);
    assert_eq!(chapters[1].end_ms, 1000);
    assert!(
        db.sync_publishable_mutations()
            .unwrap()
            .iter()
            .any(|mutation| matches!(&mutation.body, library_replica::MutationBody::BookToc { value, .. } if value.entries.len() == 2 && value.entries[1].target == book_model::audiobook_toc_target(400) && value.duration_ms == Some(1000)))
    );
    assert!(db.audible_batch_candidates(500, &[job.content_hash]).unwrap().is_empty());
}

#[test]
fn stored_audiobook_navigation_syncs_with_its_duration() {
    let db = database();
    let hash = ContentHash::new(&"a".repeat(64));
    db.connection.execute("INSERT INTO book(content_hash) VALUES(?1)", [hash.as_str()]).unwrap();
    let entries = vec![book_model::BookTocEntry { title: "Only".into(), target: book_model::audiobook_toc_target(0), children: Vec::new() }];
    db.clear_sync_outbox().unwrap();
    db.connection.execute("INSERT INTO audiobook_metadata(content_hash,duration_ms) VALUES(?1,1000)", rusqlite::params![hash.to_string()]).unwrap();
    db.connection.execute("INSERT INTO book_toc(content_hash,entry_count,toc_json) VALUES(?1,1,?2)", rusqlite::params![hash.to_string(), serde_json::to_string(&entries).unwrap()]).unwrap();
    assert!(db.sync_publishable_mutations().unwrap().iter().any(|mutation| matches!(&mutation.body, library_replica::MutationBody::BookToc { value, .. } if value.entries == entries && value.duration_ms == Some(1000))));
}

#[test]
fn audible_toc_failure_preserves_claim_and_allows_retry() {
    let db = database();
    let (job, response) = audible_job(&db);
    db.connection.execute_batch("CREATE TRIGGER fail_audible_toc BEFORE UPDATE ON book_toc BEGIN SELECT RAISE(ABORT,'injected TOC failure'); END;").unwrap();
    assert!(db.resolve_audible_batch(&job, &response, 400, 20).is_err());
    let status: String = db.connection.query_row("SELECT status FROM audible_enrichment_jobs", [], |row| row.get(0)).unwrap();
    assert_eq!(status, "resolving");
    assert_eq!(db.audible_batch_candidates(500, &[job.content_hash]).unwrap(), vec![job.content_hash]);
    db.connection.execute_batch("DROP TRIGGER fail_audible_toc;").unwrap();
    assert!(db.resolve_audible_batch(&job, &response, 400, 20).unwrap());
}

#[test]
fn stale_audible_response_does_not_change_navigation() {
    let db = database();
    let (job, mut response) = audible_job(&db);
    assert!(db.resolve_audible_batch(&job, &response, 400, 20).unwrap());
    response.chapter_plan.as_mut().unwrap().chapters[0].title = "Stale".into();
    assert!(!db.resolve_audible_batch(&job, &response, 400, 21).unwrap());
    assert_eq!(db.audiobook_chapters(&job.content_hash).unwrap()[0].title, "Opening");
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
    let work = db.native_file_work().unwrap().into_iter().find(|work| work.operation == "restore").unwrap();
    db.native_file_work_snapshot(work).unwrap().unwrap()
}

fn restore_completion(snapshot: library_database::NativeFileWorkSnapshot) -> library_database::NativeFileWorkCompletion {
    library_database::NativeFileWorkCompletion { final_path: snapshot.resolved_path.clone(), snapshot, renamed: None, published: true, thumbnail_set_exists: true }
}

#[test]
fn copy_restores_and_acknowledges_the_queued_destination() {
    let db = database();
    let (_, source, destination) = copy_fixture(&db);
    let snapshot = restore_snapshot(&db);
    assert_eq!(snapshot.resolved_path, "/Z/book.epub");
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
    assert_eq!(snapshot.resolved_path, "/Moved/book.epub");
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
    let work = db.native_file_work().unwrap().into_iter().find(|work| work.operation == "trash").unwrap();
    let snapshot = db.native_file_work_snapshot(work).unwrap().unwrap();
    assert_eq!(snapshot.resolved_path, "/A/book.epub");
    assert!(!snapshot.live);
    let completion = restore_completion(snapshot);
    db.acknowledge_native_file_work(completion).unwrap();
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
        assert_eq!(db.browse_book_count().unwrap(), 2, "first batch is immediately browsable");
        assert!(local_state(&db, first_hash, ROOT_DIR_ID).1);
        assert!(local_state(&db, missing, ROOT_DIR_ID).1, "incomplete scans must not clear missing files");
        let mut second = replacement();
        second.name = "second.epub".into();
        second.path = second.name.clone();
        second.content_hash = ContentHash::new(&"c".repeat(64));
        second.checksum = second.content_hash;
        assert_eq!(db.apply_filesystem_scan_batch(&mut snapshot, &discovery, &InspectedFilesystemScan { failures: vec![], readable: true, books: vec![second] }, false).unwrap(), ScanApplyResult::Applied);
        assert_eq!(db.browse_book_count().unwrap(), 3);
        assert_eq!(db.apply_filesystem_scan_batch(&mut snapshot, &discovery, &InspectedFilesystemScan { failures: vec![], readable: true, books: vec![] }, true).unwrap(), ScanApplyResult::Applied);
        assert!(!local_state(&db, missing, ROOT_DIR_ID).1);
        assert!(local_state(&db, first_hash, ROOT_DIR_ID).1);
    }

    #[test]
    fn concurrent_changes_reject_later_scan_batches_without_losing_committed_books() {
        let db = database();
        let mut snapshot = db.scan_snapshot().unwrap();
        let book = replacement();
        let hash = book.content_hash;
        let discovery = FilesystemScan { readable: true, files: vec![observation(ROOT_DIR_ID, "book.epub", 2)], ..Default::default() };
        assert_eq!(db.apply_filesystem_scan_batch(&mut snapshot, &discovery, &InspectedFilesystemScan { failures: vec![], readable: true, books: vec![book] }, false).unwrap(), ScanApplyResult::Applied);
        db.queue_directory_work(&ROOT_DIR_ID).unwrap();
        let guard = snapshot.work_revision;
        assert_eq!(db.apply_filesystem_scan_batch(&mut snapshot, &discovery, &InspectedFilesystemScan { failures: vec![], readable: true, books: vec![] }, true).unwrap(), ScanApplyResult::Stale);
        assert_eq!(snapshot.work_revision, guard);
        assert!(local_state(&db, hash, ROOT_DIR_ID).1);
    }

    #[test]
    fn replacement_updates_only_the_old_placement_and_commits_other_discoveries() {
        let (db, old) = fixture();
        let other = db.create_directory(&ROOT_DIR_ID, &"Other".into()).unwrap().id;
        db.transfer_book_placement(&old, &ROOT_DIR_ID, &other, false).unwrap();
        db.seed_file_projection(&old, &other, "/Other/book.epub");
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
        assert_eq!(db.browse_book_detail(hash).unwrap().book.title, "Original");
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
        db.seed_file_projection(&hash, &other, "/Other/book.epub");
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
        assert!(db.pending_book_uploads(0, 10).unwrap().is_empty());
    }
}

#[test]
fn cloud_presence_reset_requeues_books() {
    use library_replica::BlobKind;
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
    let requeued = db.pending_book_uploads(0, 10).unwrap();
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
        assert_eq!(reopened.browse_book_detail(import().content_hash).unwrap().book.title, "Original");
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
    let (path, parent): (String, String) = db
        .connection
        .query_row("SELECT CAST(route.route_id AS TEXT),CAST(route.parent_route_id AS TEXT) FROM book_unified_concept assignment JOIN curated.unified_concept_route route ON route.concept_id=assignment.concept_id WHERE route.route_id!=route.parent_route_id LIMIT 1", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap();
    let mut query = library_model::LibraryBrowseQuery {
        location: parent,
        search: String::new(),
        file_types: vec![],
        chip_sort: library_model::BrowseChipSort::Alphabetical,
        book_sort: library_model::BrowseBookSort::Alphabetical,
        languages: vec![],
        hide_finished: false,
        include_direct_child_books: false,
    };
    let parent_contents = db.browse_subject_contents(&query, false).unwrap();
    assert!(parent_contents.children.is_empty());
    assert!(parent_contents.books.is_empty());
    query.location = path;
    let contents = db.browse_subject_contents(&query, false).unwrap();
    assert!(contents.books.is_empty());
    query.location = "0".into();
    query.search.clear();
    let root_contents = db.browse_subject_contents(&query, false).unwrap();
    assert_eq!(root_contents.books[0].content_hash, hash);
    query.search = "Original".into();
    let searched = db.browse_subject_contents(&query, false).unwrap();
    assert!(searched.children.is_empty());
    assert_eq!(searched.books.len(), 1);
}

fn subject_query(location: &str) -> library_model::LibraryBrowseQuery {
    library_model::LibraryBrowseQuery {
        location: location.into(),
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
    db.connection.execute("UPDATE sync_metadata SET change_origin='remote'", []).unwrap();
    (db, path, parent)
}

#[test]
fn subject_browsing_is_read_only_and_follows_placement_and_assignment_edits() {
    let (db, _, _) = subject_fixture();
    let root = db.browse_subject_contents(&subject_query("0"), false).unwrap();
    let query = subject_query(&root.children[0].id);
    let initial = db.browse_subject_contents(&query, false).unwrap();
    assert!(initial.children.is_empty());
    assert_eq!(initial.books.len(), 4);
    let changes = db.connection.total_changes();
    db.connection.pragma_update(None, "query_only", true).unwrap();
    assert_eq!(db.browse_subject_contents(&query, false).unwrap().children.len(), initial.children.len());
    db.browse_subject(&query, false).unwrap();
    db.connection.pragma_update(None, "query_only", false).unwrap();
    assert_eq!(db.connection.total_changes(), changes);
    db.connection.execute("UPDATE book_dir SET deleted_at=10 WHERE book_row_id=(SELECT row_id FROM book WHERE title='Book 4')", []).unwrap();
    let shrunk = db.browse_subject_contents(&query, false).unwrap();
    assert!(shrunk.children.is_empty());
    assert_eq!(shrunk.books.len(), 3);
    db.connection.execute("UPDATE book_dir SET deleted_at=NULL", []).unwrap();
    assert_eq!(db.browse_subject_contents(&query, false).unwrap().books.len(), 4);
    db.connection.execute("DELETE FROM book_unified_concept", []).unwrap();
    let empty = db.browse_subject_contents(&query, false).unwrap();
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
    assert!(db.browse_subject_contents(&subject_query("0"), false).is_err());
    assert!(db.connection.is_autocommit());
    db.connection.authorizer(None::<fn(AuthContext<'_>) -> Authorization>).unwrap();
    assert!(!db.browse_subject_contents(&subject_query("0"), false).unwrap().children.is_empty());
}

#[test]
fn single_folder_walk_handles_nested_transactions_renames_and_broken_ancestry() {
    let db = database();
    let parent = DirId::new_v4();
    let child = DirId::new_v4();
    db.seed_dir(&parent, &ROOT_DIR_ID, "Shelf");
    db.seed_dir(&child, &parent, "History");
    assert_eq!(db.directory_relative_path_string(&child).unwrap().as_deref(), Some("Shelf/History"));
    assert_eq!(db.dir_path_components(&child).unwrap().iter().map(|r| r.id).collect::<Vec<_>>(), vec![parent, child]);
    let tx = db.connection.unchecked_transaction().unwrap();
    tx.execute("UPDATE dir SET name='Renamed' WHERE id=?1", [parent.to_string()]).unwrap();
    assert_eq!(db.directory_relative_path_string(&child).unwrap().as_deref(), Some("Renamed/History"));
    tx.rollback().unwrap();
    assert_eq!(db.directory_relative_path_string(&child).unwrap().as_deref(), Some("Shelf/History"));
    db.connection.execute("UPDATE dir SET deleted_at=1 WHERE id=?1", [parent.to_string()]).unwrap();
    assert_eq!(db.directory_relative_path_string(&child).unwrap(), None);
    assert!(db.dir_path_components(&child).unwrap().is_empty());
    assert_eq!(db.directory_relative_path_string(&DirId::new_v4()).unwrap(), None);
    db.connection.execute("UPDATE dir SET deleted_at=NULL,parent_id=?1 WHERE id=?2", [child.to_string(), parent.to_string()]).unwrap();
    // A loop never reaches the root, so its folders are not live.
    assert_eq!(db.directory_relative_path_string(&child).unwrap(), None);
    assert!(db.connection.is_autocommit(), "walks must release their snapshot");
}

fn browse_folder(db: &Database, dir: DirId, search: &str, languages: &[&str]) -> library_model::BrowseContents {
    let mut query = subject_query(&dir.to_string());
    query.search = search.into();
    query.languages = languages.iter().map(|language| (*language).to_owned()).collect();
    db.browse_folder_contents(&query, false).unwrap()
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
    db.connection.execute("UPDATE sync_metadata SET change_origin='remote'", []).unwrap();
    db.connection.execute("UPDATE book_dir SET is_downloaded=0", []).unwrap();
    place_book(&db, first, shelf, false);
    place_book(&db, first, branch, true);
    db.connection.execute("UPDATE book_dir SET is_downloaded=1 WHERE book_row_id IN (SELECT row_id FROM book WHERE content_hash IN (?1,?2))", [second.as_str(), third.as_str()]).unwrap();
    db.connection.execute("UPDATE book SET format=2 WHERE content_hash=?1", [third.as_str()]).unwrap();
    db.connection.execute("INSERT OR IGNORE INTO book_language(book_row_id,language) SELECT row_id,'en' FROM book WHERE content_hash!=?1", [third.as_str()]).unwrap();
    let view = browse_folder(&db, ROOT_DIR_ID, "", &[]);
    let chip = view.children.iter().find(|c| c.id == shelf.to_string()).unwrap();
    assert_eq!((chip.book_count, chip.downloaded_book_count), (5, 3));
    assert_eq!(view.children.iter().find(|c| c.id == empty.to_string()).unwrap().book_count, 0);
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
    let mut facet_query = subject_query(&shelf.to_string());
    facet_query.languages = vec!["en".into()];
    let facets = db.browse_folder(&facet_query, false).unwrap();
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
    assert_eq!(browse_folder(&db, ROOT_DIR_ID, "", &[]).children.iter().find(|c| c.id == shelf.to_string()).unwrap().book_count, 4);
    db.connection.execute("UPDATE book_dir SET dir_id=?1 WHERE dir_id=?2 AND book_row_id=(SELECT row_id FROM book WHERE content_hash=?3)", [empty.to_string(), nested.to_string(), first.to_string()]).unwrap();
    let moved = browse_folder(&db, ROOT_DIR_ID, "", &[]);
    assert_eq!(moved.children.iter().find(|c| c.id == shelf.to_string()).unwrap().book_count, 3);
    assert_eq!(moved.children.iter().find(|c| c.id == empty.to_string()).unwrap().book_count, 1);
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
        let folder = view.children.iter().find(|row| row.id == shelf.to_string()).unwrap();
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
    db.connection.execute("UPDATE sync_metadata SET change_origin='remote'", []).unwrap();
    db.connection.execute("UPDATE dir SET name='Moved',parent_id=?1 WHERE id=?2", [other.to_string(), branch.to_string()]).unwrap();
    db.connection.execute("UPDATE sync_metadata SET change_origin='local'", []).unwrap();
    let view = browse_folder(&db, ROOT_DIR_ID, "", &[]);
    assert_eq!(view.children.iter().find(|c| c.id == shelf.to_string()).unwrap().book_count, 0);
    assert_eq!(view.children.iter().find(|c| c.id == other.to_string()).unwrap().book_count, 1);
    db.connection.execute("UPDATE book_dir SET deleted_at=1 WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1)", [hash.as_str()]).unwrap();
    assert_eq!(browse_folder(&db, ROOT_DIR_ID, "", &[]).children.iter().find(|c| c.id == other.to_string()).unwrap().book_count, 0);
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
    let mut query = subject_query(&ROOT_DIR_ID.to_string());
    let unfiltered = db.browse_folder(&query, false).unwrap();
    assert_eq!(unfiltered.sections.len(), 1);
    assert_eq!(unfiltered.sections[0].parent.id, shelf.to_string());
    assert_eq!(unfiltered.sections[0].children.iter().map(|row| row.id.clone()).collect::<Vec<_>>(), [child.to_string(), empty.to_string()]);
    let downloaded = db.browse_folder(&query, true).unwrap();
    assert_eq!(downloaded.sections[0].children.len(), 1);
    assert_eq!(downloaded.sections[0].children[0].id, child.to_string());
    query.languages = vec!["en".into()];
    let page = db.browse_folder(&query, false).unwrap();
    assert_eq!(page.sections.len(), 1);
    let contents = &page.sections[0];
    assert_eq!(contents.children.len(), 1);
    assert_eq!(contents.children[0].id, child.to_string());
    assert_eq!(contents.children[0].book_count, 2);
    query.languages = vec!["sv".into()];
    let no_matches = db.browse_folder(&query, false).unwrap();
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
    let query = subject_query(&ROOT_DIR_ID.to_string());
    let page = db.browse_folder(&query, false).unwrap();
    assert_eq!(page.contents.children.iter().map(|row| row.id.clone()).collect::<Vec<_>>(), [wrapper.to_string(), plain.to_string()]);
    assert_eq!(page.sections.iter().map(|section| section.parent.id.clone()).collect::<Vec<_>>(), [plain.to_string(), wrapper.to_string()]);
    assert_eq!(page.sections[1].children[0].id, nested.to_string());
    assert_eq!(page.sections[0].books.len(), 1);
    assert!(page.sections[1].books.is_empty());
    let search = library_model::LibraryBrowseQuery { search: "Nested".into(), ..query };
    let searched = db.browse_folder(&search, false).unwrap();
    assert!(searched.sections.is_empty());
    assert_eq!(searched.contents.children[0].id, nested.to_string());
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
                let mut query = subject_query(&ROOT_DIR_ID.to_string());
                query.search = search.into();
                query.chip_sort = sort;
                if filtered {
                    query.languages = vec!["en".into()];
                    query.file_types = vec![library_model::LibraryFileTypeFilter::Book];
                }
                let contents = db.browse_folder_contents(&query, true).unwrap();
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
    let root = db.browse_subject_contents(&subject_query("0"), false).unwrap();
    let mut query = subject_query(&root.children[0].id);
    query.languages = vec!["en".into()];
    assert_eq!(db.browse_subject_contents(&query, false).unwrap().books.len(), 4);
    let facets = db.browse_subject(&query, false).unwrap();
    let (languages, formats) = (facets.language_counts, facets.format_counts);
    assert_eq!(languages.iter().find(|l| l.language == "en").unwrap().book_count, 4);
    assert_eq!(formats.iter().map(|f| f.book_count).sum::<u32>(), 4);
}

#[test]
fn browse_progress_is_derived_without_rewriting_sync_registers_and_audio_is_book_scoped() {
    let db = database();
    let hash = import_in_folder(&db, 401, ROOT_DIR_ID);
    // Browse progress comes from the dedicated register, independent of the location.
    db.connection.execute("UPDATE sync_metadata SET change_origin='remote'", []).unwrap();
    for (position, progress, expected) in [("0.25", 0., 0.), ("35", 0., 0.), ("chapter.xhtml", 0., 0.), ("0.25", 70., 70.), ("101", 0., 0.), (" 0.5suffix", 0., 0.)] {
        db.connection.execute("UPDATE book SET read_pos=?1,read_progress=?2 WHERE content_hash=?3", library_database::rusqlite::params![position, progress, hash.as_str()]).unwrap();
        assert_eq!(db.browse_book_detail(hash).unwrap().book.progress, f64::from(expected) as f32);
        let stored: (String, f64) = db.connection.query_row("SELECT read_pos,read_progress FROM book WHERE content_hash=?1", [hash.as_str()], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(stored, (position.to_owned(), progress));
    }
    db.connection.execute("INSERT INTO audiobook_metadata(content_hash,duration_ms) VALUES(?1,12345)", [hash.as_str()]).unwrap();
    db.connection.execute("UPDATE book_toc SET entry_count=7 WHERE content_hash=?1", [hash.as_str()]).unwrap();
    let folder = db.create_directory(&ROOT_DIR_ID, &"Copy".into()).unwrap().id;
    place_book(&db, hash, folder, false);
    let detail = db.browse_book_detail(hash).unwrap();
    assert_eq!(detail.book.audiobook_duration_ms, Some(12345));
    assert_eq!(detail.audiobook_chapter_count, Some(7));
    db.connection.execute("DELETE FROM audiobook_metadata WHERE content_hash=?1", [hash.as_str()]).unwrap();
    let detail = db.browse_book_detail(hash).unwrap();
    assert_eq!(detail.book.audiobook_duration_ms, None);
    assert_eq!(detail.audiobook_chapter_count, None);
}

fn without_navigation(mutations: Vec<library_replica::StateMutation>) -> Vec<Vec<u8>> {
    // Migration reissues transport identities; the domain payloads must survive.
    let mut values = mutations.into_iter().filter(|m| !matches!(m.body, library_replica::MutationBody::BookToc { .. })).map(|m| sync_common::wire::encode(&m.body).unwrap()).collect::<Vec<_>>();
    values.sort();
    values
}

fn navigation_documents(mutations: &[library_replica::StateMutation]) -> Vec<(String, book_model::BookTocDocument)> {
    let mut documents = mutations
        .iter()
        .filter_map(|mutation| match &mutation.body {
            library_replica::MutationBody::BookToc { content_hash, value } => Some((content_hash.to_string(), value.clone())),
            _ => None,
        })
        .collect::<Vec<_>>();
    documents.sort_by(|left, right| left.0.cmp(&right.0));
    documents
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
    db.connection.execute("UPDATE sync_metadata SET change_origin='remote'", []).unwrap();
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
    assert_eq!(browse_folder(&db, ROOT_DIR_ID, "", &[]).children.iter().find(|row| row.id == small.to_string()).unwrap().book_count, 1);
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

#[test]
fn deleted_book_thumbnail_work_is_cancelled_and_restored_when_needed() {
    let db = database();
    let book = import();
    let hash = book.content_hash;
    db.commit_import(book).unwrap();
    db.connection.execute("INSERT INTO book_transfer(content_hash,cover_state) VALUES(?1,'pending') ON CONFLICT(content_hash) DO UPDATE SET cover_state='pending'", [hash.as_str()]).unwrap();
    assert_eq!(db.pending_thumbnail_job_count().unwrap(), 1);
    db.connection.execute("UPDATE book SET deleted_at=1 WHERE content_hash=?1", [hash.as_str()]).unwrap();
    assert_eq!(db.pending_thumbnail_job_count().unwrap(), 0);
    assert_eq!(db.connection.query_row("SELECT count(*) FROM book_transfer WHERE cover_state IS NOT NULL", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    // A late worker retry must not resurrect a deleted book's pending job.
    db.retry_thumbnail_work(&hash).unwrap();
    assert_eq!(db.pending_thumbnail_job_count().unwrap(), 0);
    db.connection.execute("UPDATE book SET deleted_at=NULL WHERE content_hash=?1", [hash.as_str()]).unwrap();
    assert_eq!(db.pending_thumbnail_job_count().unwrap(), 1);
}

#[cfg(feature = "scanner")]
#[test]
fn scan_file_failures_finish_and_remain_visible_until_next_scan() {
    use library_database::{FilesystemScan, InspectedFilesystemScan, OperationActivity};
    use library_replica::LibraryOperationState;
    let db = database();
    let mut snapshot = db.scan_snapshot().unwrap();
    let discovery = FilesystemScan { readable: true, ..Default::default() };
    let failures = vec![("Broken.epub".to_owned(), "Invalid ZIP header".to_owned())];
    // Even a batch containing only failed files must persist their diagnostics.
    db.apply_filesystem_scan_batch(&mut snapshot, &discovery, &InspectedFilesystemScan { failures: failures.clone(), readable: false, books: vec![] }, false).unwrap();
    db.apply_filesystem_scan_batch(&mut snapshot, &discovery, &InspectedFilesystemScan { readable: false, ..Default::default() }, true).unwrap();
    assert!(!db.needs_scan().unwrap());
    for _ in 0..2 {
        let status = db.operation_status(OperationActivity::default()).unwrap().operation.unwrap();
        assert_eq!(status.state, LibraryOperationState::Completed);
        assert!(status.waiting_reason.is_none());
        assert_eq!(status.scan_failures, failures);
        assert_eq!(status.failures, 1);
    }
    let snapshot = db.scan_snapshot().unwrap();
    db.apply_filesystem_scan(&snapshot, &discovery, &InspectedFilesystemScan { readable: true, ..Default::default() }).unwrap();
    assert!(db.operation_status(OperationActivity::default()).unwrap().operation.unwrap().scan_failures.is_empty());
}

#[test]
fn enriched_subjects_survive_later_page_evidence_and_reload() {
    let db = database();
    let input = import();
    let hash = input.content_hash;
    db.commit_import(input).unwrap();
    db.apply_rich_enrichment(library_database::RichEnrichmentOutcome {
        content_hash: hash,
        provider: "test".into(),
        isbn: "9780061804038".into(),
        matched: None,
        subjects: vec![book_model::BookSubject::new(None, "Business", "openlibrary:metadata:work", Some("lcc".into()), Some("HD2796".into())).unwrap()],
        status: "updated".into(),
        detail: None,
        replace_previous_work_classifications: false,
    })
    .unwrap();
    let mut evidence = book_model::BookMetadata::default();
    evidence.identifiers.push(book_model::Identifier::new("9780140441451", book_model::Scheme::Isbn, book_model::Scope::Edition).unwrap());
    assert!(db.merge_evidence_pages(library_database::MergeEvidenceOutcome { content_hash: hash, evidence }).unwrap());
    // A fresh file inspection of the same content must preserve enrichment too.
    db.commit_import(import()).unwrap();
    let refreshed = db.refresh_subject_enrichment(vec![hash], Default::default()).unwrap();
    assert_eq!(refreshed[0].book.subjects[0].code(), Some("HD2796"));
    assert_eq!(refreshed[0].book.identifiers[0].value(), "9780140441451");
    assert_eq!(db.connection.query_row("SELECT count(*) FROM book_subject WHERE code='HD2796'", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    db.apply_rich_enrichment(library_database::RichEnrichmentOutcome {
        content_hash: hash,
        provider: "test".into(),
        isbn: "9780061804038".into(),
        matched: None,
        subjects: vec![],
        status: "updated".into(),
        detail: None,
        replace_previous_work_classifications: true,
    })
    .unwrap();
    assert!(db.refresh_subject_enrichment(vec![hash], Default::default()).unwrap()[0].book.subjects.is_empty());
    assert_eq!(db.connection.query_row("SELECT count(*) FROM book_subject", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
}

#[test]
fn subject_checkpoints_respect_backoff_restart_exhaustion_and_revision() {
    let db = database();
    let input = import();
    let hash = input.content_hash;
    db.commit_import(input).unwrap();
    let expected = db.connection.query_row("SELECT book_metadata FROM book", [], |r| r.get::<_, Vec<u8>>(0)).unwrap();
    let scope = serde_json::json!([hash.as_str()]).to_string();
    db.save_subject_checkpoint(library_database::SubjectCheckpointOutcome {
        content_hash: hash,
        phase: book_enrichment::SubjectPhase::Exhausted,
        expected,
        title: "Original".into(),
        complete: true,
        retry_seconds: 2592000,
        empty_metadata: false,
    })
    .unwrap();
    assert!(db.subject_enrichment_candidates("", &scope).unwrap().is_empty());
    db.connection.execute("UPDATE local_subject_pipeline SET retry_at=unixepoch()-1", []).unwrap();
    let mut jobs = db.subject_enrichment_candidates("", &scope).unwrap();
    assert_eq!(jobs.len(), 1);
    let book = jobs[0].book.clone();
    assert_eq!(jobs[0].pipeline.next_step(&book), book_enrichment::SubjectStep::InspectPages);
    db.connection.execute("UPDATE local_subject_pipeline SET complete=0,retry_at=unixepoch()+60", []).unwrap();
    assert!(db.subject_enrichment_candidates("", &scope).unwrap().is_empty());
    db.connection.execute("UPDATE local_subject_pipeline SET revision=3", []).unwrap();
    let mut jobs = db.subject_enrichment_candidates("", &scope).unwrap();
    assert_eq!(jobs.len(), 1);
    let book = jobs[0].book.clone();
    assert_eq!(jobs[0].pipeline.next_step(&book), book_enrichment::SubjectStep::InspectPages);
}

#[test]
fn rescan_recovers_legacy_projection_only_subjects() {
    let db = database();
    let input = import();
    let hash = input.content_hash;
    db.commit_import(input).unwrap();
    db.connection.execute("INSERT INTO book_subject(book_row_id,position,name,source,authority,code) SELECT row_id,0,'History','metadata:work','lcc','DC38' FROM book", []).unwrap();
    db.commit_import(import()).unwrap();
    let refreshed = db.refresh_subject_enrichment(vec![hash], Default::default()).unwrap();
    assert_eq!(refreshed[0].book.subjects[0].code(), Some("DC38"));
}
