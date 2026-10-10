use library_database::Database;
use library_replica::{LibraryStateEntry, StatePublication};
use sync_common::{Delivery, DirId, LibraryRevision, PullStateResponse, ROOT_DIR_ID, ReplicaId, ReplicaSeq, StateDelivery, SyncCursor};

fn database() -> Database {
    let database = Database::open(":memory:").unwrap();
    database.initialize_library().unwrap();
    database
}

fn delivery(publication: &StatePublication) -> StateDelivery {
    Delivery { state: sync_common::transport::representation::<_, sync_common::Versioned<sync_common::OpaqueStateEntry>>(&publication.state).unwrap(), revision: LibraryRevision::new(1).unwrap() }
}

fn pull(database: &Database, changes: Vec<StateDelivery>) {
    database.sync_commit_pull_response(&[], &PullStateResponse { mutations: changes, next_cursor: SyncCursor::default(), has_more: false }).unwrap();
}

#[test]
fn recovery_forwards_the_winning_state_without_authoring_an_edit() {
    let older = LibraryStateEntry::DirectoryName { dir_id: DirId::from_u128(7), name: "Older".into() }.publication(ReplicaId::from_u128(1), 10, ReplicaSeq::new(90).unwrap());
    // The winner's sequence must differ from the local replica's first ones,
    // or forwarding it could not be told apart from a fresh local publication.
    let winner = LibraryStateEntry::DirectoryName { dir_id: DirId::from_u128(7), name: "Winner".into() }.publication(ReplicaId::from_u128(2), 10, ReplicaSeq::new(40).unwrap());
    for changes in [vec![delivery(&older), delivery(&winner)], vec![delivery(&winner), delivery(&older)]] {
        let database = database();
        pull(&database, changes);
        assert!(database.sync_publishable_mutations().unwrap().is_empty());
        let address = winner.state.value.address();
        assert_eq!(database.sync_enqueue_missing_state_cells(&[address.clone()]).unwrap(), 1);
        let first = database.sync_publishable_mutations().unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].state, winner.state);
        assert_ne!(first[0].publisher_seq, winner.publisher_seq);
        assert_eq!(database.sync_publishable_mutations().unwrap(), first);
        assert_eq!(database.sync_enqueue_missing_state_cells(&[address.clone()]).unwrap(), 0);
        database.sync_acknowledge_mutations(&[first[0].publisher_seq]).unwrap();
        assert!(database.sync_publishable_mutations().unwrap().is_empty());
        assert_eq!(database.sync_enqueue_missing_state_cells(&[address]).unwrap(), 1);
        let second = database.sync_publishable_mutations().unwrap();
        assert_eq!(second[0].state, winner.state);
        assert_ne!(second[0].publisher_seq, first[0].publisher_seq);
    }
}

#[test]
fn acknowledging_an_old_publication_cannot_remove_a_new_edit() {
    let database = database();
    let id = DirId::from_u128(7);
    database.create_directory_with_id(&id, &ROOT_DIR_ID, &"First".into()).unwrap();
    let first = database.sync_publishable_mutations().unwrap();
    database.move_directory(&id, None, Some("Second")).unwrap();
    let before_ack = database.sync_publishable_mutations().unwrap();
    let new_name = before_ack.iter().find(|publication| matches!(&publication.state.value, LibraryStateEntry::DirectoryName { name, .. } if name == "Second")).unwrap();
    assert_eq!(new_name.state.version.replica_seq, new_name.publisher_seq);
    database.sync_acknowledge_mutations(&first.iter().map(|publication| publication.publisher_seq).collect::<Vec<_>>()).unwrap();
    let after_ack = database.sync_publishable_mutations().unwrap();
    assert!(after_ack.contains(new_name));
}

fn import_book(database: &Database, folder: DirId, number: u64, name: &str) -> sync_common::ContentHash {
    let hash = sync_common::ContentHash::new(&format!("{number:064x}"));
    database
        .commit_import(library_database::ImportCommit {
            parent_id: folder,
            file_name: name.into(),
            format: book_model::BookFormat::Epub,
            content_hash: hash,
            size_bytes: 100,
            inspection_version: 1,
            inspection: Some(book_inspection::InspectedBook {
                pdf: None,
                audiobook: None,
                metadata: book_model::BookRecord { metadata: book_model::BookDetails { title: format!("Book {number}"), ..Default::default() }, description: "Description".into() },
                toc: vec![],
            }),
            published: library_model::PublishedStagedBook { name: name.into(), relative_path: format!("/{name}"), published: false, fingerprint: None },
            request_thumbnail: false,
            restore_paths: vec![],
        })
        .unwrap();
    hash
}

fn acknowledge(database: &Database) {
    let ids = database.sync_publishable_mutations().unwrap().iter().map(|p| p.publisher_seq).collect::<Vec<_>>();
    database.sync_acknowledge_mutations(&ids).unwrap();
}

#[test]
fn reader_metadata_imports_publish_complete_documents_once_and_skip_identical_reinspection() {
    let database = database();
    let hash = sync_common::ContentHash::new(&format!("{:064x}", 1));
    let pdf = pdf_view_common::PdfReaderMetadata {
        version: 1,
        content_hash: hash.to_string(),
        pages: vec![pdf_view_common::PdfStoredPage { width: 600.0, height: 800.0, rotation: 0 }],
        skippable_pages: None,
        annotations: vec![],
        dependencies: None,
    };
    let pdf_import = || library_database::ImportCommit {
        parent_id: ROOT_DIR_ID,
        file_name: "book.pdf".into(),
        format: book_model::BookFormat::Pdf,
        content_hash: hash,
        size_bytes: 100,
        inspection_version: 1,
        inspection: Some(book_inspection::InspectedBook {
            pdf: Some(pdf.clone()),
            audiobook: None,
            metadata: book_model::BookRecord { metadata: book_model::BookDetails { title: "Reader metadata".into(), ..Default::default() }, description: String::new() },
            toc: vec![book_model::BookTocEntry { title: "Page 1".into(), target: "page:1".into(), children: vec![] }],
        }),
        published: library_model::PublishedStagedBook { name: "book.pdf".into(), relative_path: "/book.pdf".into(), published: false, fingerprint: None },
        request_thumbnail: false,
        restore_paths: vec![],
    };
    database.commit_import(pdf_import()).unwrap();
    complete_no_match(&database, &hash);
    assert_eq!(database.pdf_metadata(&hash).unwrap(), Some(pdf.clone()));
    assert_eq!(database.sync_publishable_mutations().unwrap().iter().filter(|p| matches!(p.state.value, LibraryStateEntry::BookAdd { ref value, .. } if value.pdf.is_some())).count(), 1);
    acknowledge(&database);
    database.commit_import(pdf_import()).unwrap();
    complete_no_match(&database, &hash);
    assert!(database.sync_publishable_mutations().unwrap().is_empty());

    let hash = sync_common::ContentHash::new(&format!("{:064x}", 2));
    let tracks = vec![book_model::AudiobookTrack { name: "chapter.mp3".into(), start_ms: 0, end_ms: 1000, offset: 0, length: 100, archive_hash: Some(hash) }];
    let metadata = book_inspection::AudiobookMetadata {
        title: "Audio".into(),
        authors: vec![],
        description: String::new(),
        duration_ms: 1000,
        chapters: vec![book_model::AudiobookChapter { title: "Chapter".into(), start_ms: 0, end_ms: 1000 }],
        tracks: tracks.clone(),
        chapter_origin: book_model::ChapterOrigin::TrackFiles,
        recording: Default::default(),
    };
    let audio_import = || library_database::ImportCommit {
        parent_id: ROOT_DIR_ID,
        file_name: "audio.zip".into(),
        format: book_model::BookFormat::Mp3Folder,
        content_hash: hash,
        size_bytes: 100,
        inspection_version: 1,
        inspection: Some(book_inspection::InspectedBook {
            pdf: None,
            audiobook: Some(metadata.clone()),
            metadata: book_model::BookRecord { metadata: book_model::BookDetails { title: "Reader metadata".into(), ..Default::default() }, description: String::new() },
            toc: vec![],
        }),
        published: library_model::PublishedStagedBook { name: "audio.zip".into(), relative_path: "/audio.zip".into(), published: false, fingerprint: None },
        request_thumbnail: false,
        restore_paths: vec![],
    };
    database.commit_import(audio_import()).unwrap();
    complete_no_match(&database, &hash);
    let docs = database.sync_publishable_mutations().unwrap();
    let docs = docs
        .iter()
        .filter_map(|p| match &p.state.value {
            LibraryStateEntry::BookAdd { content_hash, value } if *content_hash == hash => value.navigation.as_ref(),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].duration_ms, Some(1000));
    assert_eq!(docs[0].tracks.as_ref(), Some(&tracks));
    assert_eq!(database.audiobook_tracks(&hash).unwrap(), tracks);
    acknowledge(&database);
    database.commit_import(audio_import()).unwrap();
    complete_no_match(&database, &hash);
    assert!(database.sync_publishable_mutations().unwrap().is_empty());
    let mut changed_import = audio_import();
    let changed = changed_import.inspection.as_mut().unwrap().audiobook.as_mut().unwrap();
    changed.duration_ms = 2000;
    changed.tracks[0].end_ms = 2000;
    changed.chapters[0].end_ms = 2000;
    changed.recording.narrators = vec!["Changed narrator".into()];
    database.commit_import(changed_import).unwrap();
    assert!(database.sync_publishable_mutations().unwrap().is_empty());
    assert_eq!(database.audiobook_tracks(&hash).unwrap(), tracks);
    let playback = database.audiobook_playback_metadata(&hash).unwrap().unwrap();
    assert_eq!(playback.duration_ms, 1000);
    assert_eq!(playback.narrator, None);
}

#[test]
fn offline_addition_waits_for_enrichment_then_publishes_one_complete_document() {
    use book_model::{AUTHOR_MARC_RELATOR_CODE, Contributor};
    use metadata_contract::books::{BookEnrichment, BookEnrichmentStatus};
    let source = database();
    let target = database();
    let hash = import_book(&source, ROOT_DIR_ID, 7, "book.epub");
    assert_eq!(source.browse_book_detail(&hash).unwrap().book.title, "Book 7");
    source.update_reading_position(&hash, "epubcfi(/6/2)", Some(0.25), None).unwrap();
    assert!(source.sync_publishable_mutations().unwrap().is_empty());
    let candidate = source.enrichment_candidates(&[hash], 0).unwrap().pop().unwrap();
    let mut record = candidate.record;
    record.contributors = vec![Contributor::new("Enriched author", AUTHOR_MARC_RELATOR_CODE).unwrap()];
    let answer = BookEnrichment { key: String::new(), status: BookEnrichmentStatus::Found, record: Some(record), toc: None, cover: Some(metadata_contract::books::CoverSource { id: "cover-7".into() }), detail: String::new() };
    source.record_enrichment(&hash, &candidate.request_key, &answer, 0).unwrap();
    let additions = source.sync_publishable_mutations().unwrap();
    assert_eq!(additions.iter().filter(|p| matches!(p.state.value, LibraryStateEntry::BookAdd { .. })).count(), 1);
    assert!(additions.iter().any(|p| matches!(p.state.value, LibraryStateEntry::ReadingPosition { .. })));
    pull(&target, additions.iter().map(delivery).collect());
    assert_eq!(target.browse_book_detail(&hash).unwrap().book.author, "Enriched author");
    assert_eq!(target.reading_position(&hash).unwrap().as_deref(), Some("epubcfi(/6/2)"));
    assert!(target.enrichment_candidates(&[hash], i64::MAX).unwrap().is_empty());
    assert_eq!(target.enrichment_cover_source(&hash).unwrap().unwrap().id, "cover-7");
    assert_eq!(target.enrichment_next_retry().unwrap(), None);
}

fn complete_no_match(database: &Database, hash: &sync_common::ContentHash) {
    use metadata_contract::books::{BookEnrichment, BookEnrichmentStatus};
    let Some(candidate) = database.enrichment_candidates(&[*hash], 0).unwrap().pop() else { return };
    database.record_enrichment(hash, &candidate.request_key, &BookEnrichment { key: String::new(), status: BookEnrichmentStatus::NoMatch, record: None, toc: None, cover: None, detail: String::new() }, 0).unwrap();
}

#[test]
fn a_shared_no_match_finishes_enrichment_and_a_late_answer_cannot_replace_it() {
    use metadata_contract::books::{BookEnrichment, BookEnrichmentStatus};

    let source = database();
    let target = database();
    let hash = import_book(&source, ROOT_DIR_ID, 8, "book.epub");
    pull(&target, source.sync_publishable_mutations().unwrap().iter().map(delivery).collect());
    acknowledge(&source);
    let candidate = source.enrichment_candidates(&[hash], 0).unwrap().pop().unwrap();
    let mut answer = BookEnrichment { key: String::new(), status: BookEnrichmentStatus::NoMatch, record: None, toc: None, cover: None, detail: String::new() };
    source.record_enrichment(&hash, &candidate.request_key, &answer, 0).unwrap();
    let original = source.sync_publishable_mutations().unwrap();
    pull(&target, original.iter().map(delivery).collect());
    answer.status = BookEnrichmentStatus::Found;
    answer.record = Some(candidate.record);
    source.record_enrichment(&hash, &candidate.request_key, &answer, 1).unwrap();
    assert_eq!(source.sync_publishable_mutations().unwrap(), original);
    pull(&target, original.iter().map(delivery).collect());
    assert!(target.enrichment_candidates(&[hash], i64::MAX).unwrap().is_empty());
}

#[test]
fn server_accepted_addition_replaces_a_competing_local_addition_even_with_an_older_version() {
    use book_model::{BookFormat, BookRecord};
    use library_replica::BookAddition;

    let source = database();
    let hash = import_book(&source, ROOT_DIR_ID, 9, "book.epub");
    complete_no_match(&source, &hash);
    let original = source.sync_publishable_mutations().unwrap();
    let candidate = original.iter().find(|p| matches!(p.state.value, LibraryStateEntry::BookAdd { .. })).unwrap();
    assert_eq!(candidate.state.version.changed_at, 0);
    let accepted = LibraryStateEntry::BookAdd {
        content_hash: hash,
        value: BookAddition {
            facts: library_replica::BookFacts { added_at: 1, format: BookFormat::Epub },
            record: BookRecord { metadata: library_replica::BookDetails { title: "Accepted".into(), ..Default::default() }, description: String::new() },
            navigation: None,
            pdf: None,
            recording: None,
            enrichment: Some(library_replica::EnrichmentResult::NoMatch),
        },
    }
    .publication(ReplicaId::from_u128(0), 0, ReplicaSeq::new(1).unwrap());
    assert!(accepted.state.version < candidate.state.version);
    pull(&source, vec![delivery(&accepted)]);
    assert_eq!(source.browse_book_detail(&hash).unwrap().book.title, "Accepted");
    let queued = source.sync_publishable_mutations().unwrap();
    let forwarded = queued.iter().find(|p| matches!(p.state.value, LibraryStateEntry::BookAdd { .. })).unwrap();
    assert_eq!(forwarded.state, accepted.state);
    assert_ne!(forwarded.publisher_seq, candidate.publisher_seq);
    source.sync_acknowledge_mutations(&original.iter().map(|p| p.publisher_seq).collect::<Vec<_>>()).unwrap();
    assert_eq!(source.sync_publishable_mutations().unwrap(), vec![forwarded.clone()]);
    assert_eq!(source.sync_enqueue_missing_state_cells(&[accepted.state.value.address()]).unwrap(), 0);
    acknowledge(&source);
    assert_eq!(source.sync_enqueue_missing_state_cells(&[accepted.state.value.address()]).unwrap(), 1);
    assert_eq!(source.sync_publishable_mutations().unwrap()[0].state, accepted.state);
}

#[test]
fn offline_draft_and_reading_progress_survive_reopening_before_enrichment() {
    let path = std::env::temp_dir().join(format!("book-draft-{}.sqlite3", uuid::Uuid::new_v4()));
    let source = Database::open(&path).unwrap();
    source.initialize_library().unwrap();
    source.set_asset_storage_policy(false, false).unwrap();
    let hash = import_book(&source, ROOT_DIR_ID, 10, "pending.epub");
    source.update_reading_position(&hash, "epubcfi(/6/2)", Some(0.5), None).unwrap();
    assert!(source.sync_publishable_mutations().unwrap().is_empty());
    drop(source);
    let source = Database::open(&path).unwrap();
    source.initialize_library().unwrap();
    assert_eq!(source.browse_book_detail(&hash).unwrap().book.title, "Book 10");
    assert_eq!(source.reading_position(&hash).unwrap().as_deref(), Some("epubcfi(/6/2)"));
    assert!(source.sync_publishable_mutations().unwrap().is_empty());
    complete_no_match(&source, &hash);
    let publications = source.sync_publishable_mutations().unwrap();
    assert_eq!(publications.iter().filter(|p| matches!(p.state.value, LibraryStateEntry::BookAdd { .. })).count(), 1);
    assert!(publications.iter().any(|p| matches!(p.state.value, LibraryStateEntry::ReadingPosition { .. })));
    drop(source);
    std::fs::remove_file(path).unwrap();
}


#[test]
fn annotations_round_trip_register_projection_sync_and_worker_transport() {
    use book_model::{Annotation, AnnotationAnchor, AnnotationStyle, PdfAnnotationAnchor, PdfAnnotationRect};
    use client_runtime::wire::{decode_worker_message, encode_worker_message};

    let source = database();
    let target = database();
    let hash = import_book(&source, ROOT_DIR_ID, 100, "Annotated.epub");
    import_book(&target, ROOT_DIR_ID, 100, "Annotated.epub");
    acknowledge(&source);
    acknowledge(&target);
    for (index, anchor) in [
        AnnotationAnchor::epub_cfi("epubcfi(/6/2)"),
        AnnotationAnchor::Pdf(PdfAnnotationAnchor::new(3, vec![PdfAnnotationRect::new(0.1, 0.2, 0.3, 0.4)], None)),
    ].into_iter().enumerate() {
        let annotation = Annotation {
            id: format!("note-{index}"),
            content_hash: hash,
            anchor,
            exact_text: "Text".into(),
            style: AnnotationStyle::Underline,
            color: 0xffffff,
            note: "note".into(),
            created_at: 7,
            toc_ordinal: Some(5),
            progress: Some(42.5),
            modified_at: 9,
            deleted: false,
        };
        source.upsert_annotation(&annotation).unwrap();
    }
    let publications = source.sync_publishable_mutations().unwrap();
    assert_eq!(publications.len(), 2);
    pull(&target, publications.iter().map(delivery).collect());
    let source_rows = source.annotations(hash).unwrap();
    let target_rows = target.annotations(hash).unwrap();
    assert_eq!(target_rows.len(), 2);
    for (source_row, target_row) in source_rows.iter().zip(&target_rows) {
        assert_eq!(source_row, target_row);
    }
    let message = encode_worker_message(&target_rows).unwrap();
    let decoded: Vec<Annotation> = decode_worker_message(&message).unwrap();
    assert_eq!(decoded, target_rows);
}
