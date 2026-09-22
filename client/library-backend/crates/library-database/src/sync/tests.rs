//! Sync concern tests: apply, state, and outbox.

#[cfg(test)]
mod apply_tests {
    use crate::*;
    use library_replica::{MutationBody, ReadingPositionState, StateMutation, SyncBookMetadata};
    use sync_common::{LibraryRevision, MutationId, PullStateResponse, ReplicaSeq, StateCell, SyncCursor};

    struct TestDatabase {
        database: Database,
        path: std::path::PathBuf,
    }

    impl Drop for TestDatabase {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    fn open_test_database() -> TestDatabase {
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("library-database-sync-apply-{}-{id}", std::process::id()));
        let database = Database::open(&path).expect("open test database");
        database.initialize_library().expect("initialize test library");
        TestDatabase { database, path }
    }

    fn mutation_id(tag: &str) -> MutationId {
        MutationId::parse(&format!("12345678-1234-1234-1234-{tag:0>12}")).unwrap()
    }

    fn placement_change(hash: sync_common::ContentHash, changed_at: u64) -> sync_common::ServerMutation {
        let dir_id = uuid::Uuid::parse_str("12345678-1234-1234-1234-333333333333").unwrap();
        let body = MutationBody::Placement { dir_id, content_hash: hash, present: true, origin_folder_id: None };
        let mutation = StateMutation { mutation_id: mutation_id("333333333333"), body, changed_at, replica_seq: ReplicaSeq::new(1).unwrap() };
        sync_common::ServerMutation { mutation: mutation.to_wire().unwrap(), replica_id: uuid::Uuid::new_v4(), revision: LibraryRevision::new(1).unwrap() }
    }

    fn reading_change(hash: sync_common::ContentHash, cfi: &str, progress: f32, changed_at: u64) -> sync_common::ServerMutation {
        let body = MutationBody::ReadingPosition { content_hash: hash, value: ReadingPositionState { location: book_model::ReadingPosition::parse(cfi).unwrap(), progress } };
        let mutation = StateMutation { mutation_id: mutation_id("111111111111"), body, changed_at, replica_seq: ReplicaSeq::new(1).unwrap() };
        sync_common::ServerMutation { mutation: mutation.to_wire().unwrap(), replica_id: uuid::Uuid::new_v4(), revision: LibraryRevision::new(1).unwrap() }
    }

    fn metadata_change(hash: sync_common::ContentHash, title: &str, author: &str, changed_at: u64) -> sync_common::ServerMutation {
        let body = MutationBody::Metadata {
            content_hash: hash,
            value: SyncBookMetadata { title: title.to_owned(), subtitle: None, contributors: vec![book_model::Contributor::new(author, book_model::MarcRelatorCode(*b"aut")).unwrap()], book: book_model::BookMetadata::default() },
        };
        let mutation = StateMutation { mutation_id: mutation_id("222222222222"), body, changed_at, replica_seq: ReplicaSeq::new(1).unwrap() };
        sync_common::ServerMutation { mutation: mutation.to_wire().unwrap(), replica_id: uuid::Uuid::new_v4(), revision: LibraryRevision::new(1).unwrap() }
    }

    #[test]
    fn pull_commit_applies_reading_advances_cursor_and_holds_stale() {
        let holder = open_test_database();
        let database = &holder.database;
        let hash = sync_common::ContentHash::new(&"7".repeat(64));
        let next = SyncCursor { state_revision: None, reading_revision: Some(LibraryRevision::new(3).unwrap()) };
        let page = PullStateResponse { mutations: vec![placement_change(hash, 90), reading_change(hash, "epubcfi(/6/4)", 0.5, 100)], next_cursor: next.clone(), has_more: false };
        assert!(database.sync_commit_pull_response(&[], &page).expect("commit reading page"));
        assert_eq!(database.sync_pull_cursor().expect("read cursor"), next);
        assert_eq!(database.browse_book_detail(hash).expect("read detail").book.progress, 0.5);
        let stale = PullStateResponse { mutations: vec![reading_change(hash, "epubcfi(/6/2)", 0.9, 50)], next_cursor: next.clone(), has_more: false };
        assert!(database.sync_commit_pull_response(&[], &stale).expect("commit stale page"));
        assert_eq!(database.browse_book_detail(hash).expect("reread detail").book.progress, 0.5);
    }

    fn toc_change(hash: sync_common::ContentHash, changed_at: u64) -> sync_common::ServerMutation {
        let body = MutationBody::BookToc {
            content_hash: hash,
            value: book_model::BookTocDocument {
                entries: vec![
                    book_model::BookTocEntry { title: "Opening".into(), target: book_model::audiobook_toc_target(0), children: Vec::new() },
                    book_model::BookTocEntry { title: "Conclusion".into(), target: book_model::audiobook_toc_target(400), children: Vec::new() },
                ],
                duration_ms: Some(1000),
            },
        };
        batch_mutation("444444444444".to_owned(), body, changed_at, 7)
    }

    #[test]
    fn pull_commit_applies_audiobook_navigation_with_duration() {
        let holder = open_test_database();
        let database = &holder.database;
        let hash = sync_common::ContentHash::new(&"9".repeat(64));
        let next = SyncCursor { state_revision: Some(LibraryRevision::new(5).unwrap()), reading_revision: None };
        let page = PullStateResponse { mutations: vec![placement_change(hash, 90), toc_change(hash, 100)], next_cursor: next, has_more: false };
        assert!(database.sync_commit_pull_response(&[], &page).expect("commit navigation page"));
        // The last chapter runs to the end of the audio: without the synced
        // duration the chapters would not resolve at all.
        let chapters = database.audiobook_chapters(&hash).expect("read applied chapters");
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[1].end_ms, 1000);
        // Remote state must not echo back into the push outbox.
        assert!(database.sync_publishable_mutations().expect("read outbox").iter().all(|mutation| !matches!(&mutation.body, MutationBody::BookToc { .. })));
    }

    #[test]
    fn pull_commit_projects_metadata_identity_and_holds_stale() {
        let holder = open_test_database();
        let database = &holder.database;
        let hash = sync_common::ContentHash::new(&"8".repeat(64));
        let next = SyncCursor { state_revision: Some(LibraryRevision::new(5).unwrap()), reading_revision: None };
        let page = PullStateResponse { mutations: vec![placement_change(hash, 90), metadata_change(hash, "Remote Title", "Remote Author", 100)], next_cursor: next.clone(), has_more: false };
        assert!(database.sync_commit_pull_response(&[], &page).expect("commit metadata page"));
        let detail = database.browse_book_detail(hash).expect("read detail");
        assert_eq!(detail.book.title, "Remote Title");
        assert!(detail.authors.iter().any(|author| author.name == "Remote Author"), "incoming contributor is projected");
        let stale = PullStateResponse { mutations: vec![metadata_change(hash, "Stale Title", "Stale Author", 50)], next_cursor: next.clone(), has_more: false };
        assert!(database.sync_commit_pull_response(&[], &stale).expect("commit stale page"));
        let detail = database.browse_book_detail(hash).expect("reread detail");
        assert_eq!(detail.book.title, "Remote Title");
        assert!(detail.authors.iter().any(|author| author.name == "Remote Author"), "stale contributor never lands");
    }

    fn batch_hash(index: u64) -> sync_common::ContentHash {
        sync_common::ContentHash::new(&format!("{index:064x}"))
    }

    fn batch_metadata(index: u64) -> SyncBookMetadata {
        SyncBookMetadata {
            title: format!("Batch Title {index}"),
            subtitle: None,
            contributors: vec![book_model::Contributor::new(format!("Batch Author {index}"), book_model::MarcRelatorCode(*b"aut")).unwrap()],
            book: book_model::BookMetadata::default(),
        }
    }

    fn batch_mutation(tag: String, body: MutationBody, changed_at: u64, replica: u128) -> sync_common::ServerMutation {
        let mutation = StateMutation { mutation_id: mutation_id(&tag), body, changed_at, replica_seq: ReplicaSeq::new(1).unwrap() };
        sync_common::ServerMutation { mutation: mutation.to_wire().unwrap(), replica_id: uuid::Uuid::from_u128(replica), revision: LibraryRevision::new(1).unwrap() }
    }

    fn addition_change(index: u64, changed_at: u64) -> sync_common::ServerMutation {
        let body = MutationBody::BookLifecycle { content_hash: batch_hash(index), value: library_replica::BookLifecycleState::Present { metadata: batch_metadata(index), added_at: changed_at, format: book_model::BookFormat::Epub } };
        batch_mutation(format!("{index:012}"), body, changed_at, u128::from(index))
    }

    fn removal_change(hash: sync_common::ContentHash, tag: &str, changed_at: u64, purged: bool) -> sync_common::ServerMutation {
        let value = if purged { library_replica::BookLifecycleState::Purged } else { library_replica::BookLifecycleState::Deleted { origin_folder_id: None } };
        batch_mutation(tag.to_owned(), MutationBody::BookLifecycle { content_hash: hash, value }, changed_at, 9_999_999)
    }

    fn state_snapshot(database: &Database) -> (Vec<(String, Option<String>, bool, bool)>, Vec<(String, String, String, i64, i64, i64)>, Vec<String>, Vec<(String, String, i64)>, SyncCursor) {
        let books = database
            .connection
            .prepare("SELECT content_hash, title, deleted_at IS NOT NULL, purged_at IS NOT NULL FROM book ORDER BY content_hash")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let versions = database
            .connection
            .prepare("SELECT state_kind, state_key, state_subkey, changed_at, conflict_rank, replica_seq FROM sync_state_version ORDER BY state_kind, state_key, state_subkey")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let work = database.connection.prepare("SELECT content_hash FROM local_book_work ORDER BY content_hash").unwrap().query_map([], |row| row.get(0)).unwrap().collect::<Result<Vec<String>, _>>().unwrap();
        let contributors = database
            .connection
            .prepare("SELECT b.content_hash, c.name, c.position FROM book_contributor c JOIN book b ON b.row_id = c.book_row_id ORDER BY b.content_hash, c.position")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        (books, versions, work, contributors, database.sync_pull_cursor().expect("read cursor"))
    }

    fn decoy_hash() -> sync_common::ContentHash {
        sync_common::ContentHash::new(&"f".repeat(64))
    }

    #[test]
    fn batched_additions_match_ordered_across_bounded_snapshots() {
        let batched_holder = open_test_database();
        let ordered_holder = open_test_database();
        let batched = &batched_holder.database;
        let ordered = &ordered_holder.database;
        // Seeds land identically through the ordered path on both sides.
        // Index 200 is newer than the page, 201 older, 202 deleted, 203
        // purged, 204 owned by a newer metadata change.
        let seed = vec![
            addition_change(200, 110),
            addition_change(201, 10),
            addition_change(202, 10),
            removal_change(batch_hash(202), "000000000900", 20, false),
            addition_change(203, 10),
            removal_change(batch_hash(203), "000000000901", 20, true),
            addition_change(204, 10),
            metadata_change(batch_hash(204), "Metadata owns this title", "Metadata Author", 120),
            placement_change(decoy_hash(), 90),
        ];
        let next_seed = SyncCursor { state_revision: Some(LibraryRevision::new(2).unwrap()), reading_revision: None };
        for database in [batched, ordered] {
            let page = PullStateResponse { mutations: seed.clone(), next_cursor: next_seed.clone(), has_more: true };
            assert!(database.sync_commit_pull_response(&[], &page).expect("commit seed page"));
        }
        let mut incoming: Vec<sync_common::ServerMutation> = (0..205).map(|index| addition_change(index, 100)).collect();
        let next = SyncCursor { state_revision: Some(LibraryRevision::new(3).unwrap()), reading_revision: None };
        let page = PullStateResponse { mutations: incoming.clone(), next_cursor: next.clone(), has_more: false };
        assert!(batched.sync_commit_pull_response(&[], &page).expect("commit batched page"));
        // One stale change forces the whole page onto the ordered path but
        // loses its version check, so it must not perturb the outcome.
        incoming.push(placement_change(decoy_hash(), 50));
        let page = PullStateResponse { mutations: incoming, next_cursor: next.clone(), has_more: false };
        assert!(ordered.sync_commit_pull_response(&[], &page).expect("commit ordered page"));
        assert_eq!(state_snapshot(batched), state_snapshot(ordered));
        let (books, _, _, _, _) = state_snapshot(batched);
        let title = |index: u64| books.iter().find(|(hash, _, _, _)| *hash == batch_hash(index).as_str()).map(|(_, title, _, _)| title.clone());
        assert_eq!(title(200), Some(Some("Batch Title 200".to_owned())), "seed title survives, page title is stale");
        assert_eq!(title(201), Some(Some("Batch Title 201".to_owned())));
        assert_eq!(title(204), Some(Some("Metadata owns this title".to_owned())), "metadata-owned title survives the top-up");
        assert_eq!(title(203), Some(Some("Batch Title 203".to_owned())), "newer lifecycle wins revive the purged tombstone");
        assert!(books.iter().all(|(hash, _, _, purged)| *hash != batch_hash(203).as_str() || !purged), "revived book is not purged");
    }

    #[test]
    fn batched_metadata_matches_ordered_for_missing_and_existing_books() {
        let batched_holder = open_test_database();
        let ordered_holder = open_test_database();
        let batched = &batched_holder.database;
        let ordered = &ordered_holder.database;
        // Even indexes exist up front; odd indexes arrive through ensure.
        // Index 100 is newer than the page, 101 purged, 102 carries a title
        // that a metadata change must still overwrite.
        let mut seed = Vec::new();
        for index in (0..100).step_by(2) {
            seed.push(placement_change(batch_hash(index), 90));
        }
        seed.push(metadata_change(batch_hash(100), "Newer owns this title", "Newer Author", 110));
        seed.push(addition_change(101, 10));
        seed.push(removal_change(batch_hash(101), "000000000902", 20, true));
        seed.push(addition_change(102, 10));
        seed.push(placement_change(decoy_hash(), 90));
        let next_seed = SyncCursor { state_revision: Some(LibraryRevision::new(2).unwrap()), reading_revision: None };
        for database in [batched, ordered] {
            let page = PullStateResponse { mutations: seed.clone(), next_cursor: next_seed.clone(), has_more: true };
            assert!(database.sync_commit_pull_response(&[], &page).expect("commit seed page"));
        }
        let mut incoming: Vec<sync_common::ServerMutation> = (0..103).map(|index| metadata_change(batch_hash(index), &format!("Batch Title {index}"), &format!("Meta Author {index}"), 100)).collect();
        let next = SyncCursor { state_revision: Some(LibraryRevision::new(3).unwrap()), reading_revision: None };
        let page = PullStateResponse { mutations: incoming.clone(), next_cursor: next.clone(), has_more: false };
        assert!(batched.sync_commit_pull_response(&[], &page).expect("commit batched page"));
        incoming.push(placement_change(decoy_hash(), 50));
        let page = PullStateResponse { mutations: incoming, next_cursor: next.clone(), has_more: false };
        assert!(ordered.sync_commit_pull_response(&[], &page).expect("commit ordered page"));
        assert_eq!(state_snapshot(batched), state_snapshot(ordered));
        let (books, _, _, contributors, _) = state_snapshot(batched);
        let title = |index: u64| books.iter().find(|(hash, _, _, _)| *hash == batch_hash(index).as_str()).map(|(_, title, _, _)| title.clone());
        assert_eq!(title(102), Some(Some("Batch Title 102".to_owned())));
        assert!(contributors.iter().any(|(hash, name, _)| *hash == batch_hash(102).as_str() && *name == "Meta Author 102"), "metadata overwrites despite stored title");
        assert_eq!(title(1), Some(Some("Batch Title 1".to_owned())), "missing book arrives through ensure");
        assert_eq!(title(100), Some(Some("Newer owns this title".to_owned())));
        assert!(books.iter().any(|(hash, title, _, purged)| *hash == batch_hash(101).as_str() && *purged && title.is_none()), "purged book stays purged");
    }

    #[test]
    fn batches_flush_when_chunk_and_page_end_with_stale_changes() {
        for metadata in [false, true] {
            let batched_holder = open_test_database();
            let ordered_holder = open_test_database();
            let change = |index, timestamp| {
                if metadata { metadata_change(batch_hash(index), &format!("Title {index}"), "Author", timestamp) } else { addition_change(index, timestamp) }
            };
            let mut seed: Vec<_> = [99, 199, 204].into_iter().map(|index| change(index, 200)).collect();
            seed.push(placement_change(decoy_hash(), 90));
            let seed_page = PullStateResponse { mutations: seed, next_cursor: SyncCursor::default(), has_more: true };
            for database in [&batched_holder.database, &ordered_holder.database] {
                database.sync_commit_pull_response(&[], &seed_page).unwrap();
            }
            let mut page = PullStateResponse { mutations: (0..205).map(|index| change(index, 100)).collect(), next_cursor: SyncCursor { state_revision: Some(LibraryRevision::new(3).unwrap()), reading_revision: None }, has_more: false };
            batched_holder.database.sync_commit_pull_response(&[], &page).unwrap();
            // A stale placement selects the ordered path without changing state.
            page.mutations.push(placement_change(decoy_hash(), 50));
            ordered_holder.database.sync_commit_pull_response(&[], &page).unwrap();
            assert_eq!(state_snapshot(&batched_holder.database), state_snapshot(&ordered_holder.database), "metadata={metadata}");
            // Replaying an entirely stale batch must preserve the same state.
            page.mutations.pop();
            let before = state_snapshot(&batched_holder.database);
            batched_holder.database.sync_commit_pull_response(&[], &page).unwrap();
            assert_eq!(before, state_snapshot(&batched_holder.database));
        }
    }

    #[test]
    fn failed_pull_rolls_back_acknowledgements_versions_origin_and_projections() {
        for mode in 0..3 {
            let holder = open_test_database();
            let database = &holder.database;
            database.create_directory_with_id(&uuid::Uuid::new_v4(), &sync_common::ROOT_DIR_ID, &"Local folder".to_owned()).unwrap();
            let pending_ids = || database.sync_publishable_mutations().unwrap().iter().map(|mutation| mutation.mutation_id).collect::<Vec<_>>();
            let ids = pending_ids();
            assert!(!ids.is_empty());
            let before = state_snapshot(database);
            let origin = || database.connection.query_row("SELECT change_origin FROM sync_metadata WHERE singleton = 1", [], |row| row.get::<_, String>(0)).unwrap();
            let previous_origin = origin();
            let mutations = match mode {
                0 => vec![addition_change(1, 100)],
                1 => vec![metadata_change(batch_hash(1), "Title", "Author", 100)],
                _ => vec![addition_change(1, 100), reading_change(batch_hash(1), "epubcfi(/6/2!/4/2/1:0)", 0.5, 101)],
            };
            let page = PullStateResponse { mutations, next_cursor: SyncCursor { state_revision: Some(LibraryRevision::new(3).unwrap()), reading_revision: Some(LibraryRevision::new(4).unwrap()) }, has_more: false };
            // Fail in the shared metadata tail, after lifecycle and version writes.
            database.connection.execute_batch("CREATE TEMP TRIGGER reject_pull_contributor BEFORE INSERT ON book_contributor BEGIN SELECT RAISE(ABORT, 'injected pull failure'); END;").unwrap();
            assert!(database.sync_commit_pull_response(&ids, &page).is_err());
            assert_eq!(before, state_snapshot(database));
            assert_eq!(ids, pending_ids());
            assert_eq!(previous_origin, origin());
            database.connection.execute_batch("DROP TRIGGER reject_pull_contributor;").unwrap();
            assert!(database.sync_commit_pull_response(&ids, &page).unwrap());
            assert!(pending_ids().is_empty(), "remote changes must not be echoed into the outbox");
            assert_eq!(page.next_cursor, database.sync_pull_cursor().unwrap());
            assert_eq!(previous_origin, origin());
        }
    }

    #[test]
    fn empty_pull_acknowledges_without_reporting_remote_changes() {
        let holder = open_test_database();
        let database = &holder.database;
        database.create_directory_with_id(&uuid::Uuid::new_v4(), &sync_common::ROOT_DIR_ID, &"Local folder".to_owned()).unwrap();
        let ids: Vec<_> = database.sync_publishable_mutations().unwrap().iter().map(|mutation| mutation.mutation_id).collect();
        assert!(!ids.is_empty());
        let mut page = PullStateResponse { mutations: Vec::new(), next_cursor: database.sync_pull_cursor().unwrap(), has_more: false };
        assert!(!database.sync_commit_pull_response(&ids, &page).unwrap());
        assert!(database.sync_publishable_mutations().unwrap().is_empty());
        assert!(!database.sync_commit_pull_response(&[], &page).unwrap());
        page.next_cursor.state_revision = Some(LibraryRevision::new(3).unwrap());
        assert!(!database.sync_commit_pull_response(&[], &page).unwrap());
        assert_eq!(page.next_cursor, database.sync_pull_cursor().unwrap());
    }

    fn directory_change(body: MutationBody, changed_at: u64, tag: &str) -> sync_common::ServerMutation {
        let mutation = StateMutation { mutation_id: mutation_id(tag), body, changed_at, replica_seq: ReplicaSeq::new(1).unwrap() };
        sync_common::ServerMutation { mutation: mutation.to_wire().unwrap(), replica_id: uuid::Uuid::new_v4(), revision: LibraryRevision::new(1).unwrap() }
    }

    #[test]
    fn pull_commit_applies_directory_intents_and_projection() {
        let holder = open_test_database();
        let database = &holder.database;
        let dir_id = uuid::Uuid::parse_str("12345678-1234-1234-1234-444444444444").unwrap();
        let next = SyncCursor { state_revision: Some(LibraryRevision::new(6).unwrap()), reading_revision: None };
        let page = PullStateResponse {
            mutations: vec![
                directory_change(MutationBody::DirectoryName { dir_id, name: "Shelf".to_owned() }, 10, "444444444441"),
                directory_change(MutationBody::DirectoryParent { dir_id, parent_id: sync_common::ROOT_DIR_ID }, 20, "444444444442"),
                directory_change(MutationBody::DirectoryLifecycle { dir_id, value: library_replica::DirectoryLifecycleState::Present }, 30, "444444444443"),
            ],
            next_cursor: next.clone(),
            has_more: false,
        };
        assert!(database.sync_commit_pull_response(&[], &page).expect("commit directory page"));
        let row: (String, String, i64, String, String) = database
            .connection
            .prepare("SELECT intent_name, intent_parent_id, intent_lifecycle, name, parent_id FROM dir WHERE id = ?1")
            .unwrap()
            .query_row([dir_id.to_string()], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)))
            .unwrap();
        assert_eq!(row.0, "Shelf");
        assert_eq!(row.1, sync_common::ROOT_DIR_ID.to_string());
        assert_eq!(row.2, 0);
        assert_eq!(row.3, "Shelf", "projected name follows the intent");
        assert_eq!(row.4, sync_common::ROOT_DIR_ID.to_string());
    }

    #[test]
    fn pull_commit_stores_description_and_marks_scanned() {
        let holder = open_test_database();
        let database = &holder.database;
        let hash = sync_common::ContentHash::new(&"9".repeat(64));
        let body = MutationBody::Description { content_hash: hash, value: "A\x00noisy\x07 description.".to_owned() };
        let mutation = StateMutation { mutation_id: mutation_id("555555555555"), body, changed_at: 100, replica_seq: ReplicaSeq::new(1).unwrap() };
        let change = sync_common::ServerMutation { mutation: mutation.to_wire().unwrap(), replica_id: uuid::Uuid::new_v4(), revision: LibraryRevision::new(1).unwrap() };
        let next = SyncCursor { state_revision: Some(LibraryRevision::new(7).unwrap()), reading_revision: None };
        let page = PullStateResponse { mutations: vec![change], next_cursor: next, has_more: false };
        assert!(database.sync_commit_pull_response(&[], &page).expect("commit description page"));
        let stored: (String, i64) = database
            .connection
            .prepare("SELECT d.description, b.description_scanned FROM book_description d JOIN book b ON b.row_id = d.book_row_id WHERE b.content_hash = ?1")
            .unwrap()
            .query_row([hash.as_str()], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap();
        assert_eq!(stored.0, "Anoisy description.");
        assert_eq!(stored.1, 1);
    }

    #[test]
    fn pull_commit_projects_subject_classification() {
        let holder = open_test_database();
        let database = &holder.database;
        let hash = sync_common::ContentHash::new(&"a".repeat(64));
        let mut book = book_model::BookMetadata::default();
        book.subjects = vec![book_model::BookSubject::new(None, "Distributed systems", "dc:subject", None, None).unwrap()];
        let body = MutationBody::Metadata {
            content_hash: hash,
            value: SyncBookMetadata { title: "Classified".to_owned(), subtitle: None, contributors: vec![book_model::Contributor::new("Subject Author", book_model::MarcRelatorCode(*b"aut")).unwrap()], book },
        };
        let mutation = StateMutation { mutation_id: mutation_id("666666666666"), body, changed_at: 100, replica_seq: ReplicaSeq::new(1).unwrap() };
        let change = sync_common::ServerMutation { mutation: mutation.to_wire().unwrap(), replica_id: uuid::Uuid::new_v4(), revision: LibraryRevision::new(1).unwrap() };
        let next = SyncCursor { state_revision: Some(LibraryRevision::new(8).unwrap()), reading_revision: None };
        let page = PullStateResponse { mutations: vec![placement_change(hash, 90), change], next_cursor: next, has_more: false };
        assert!(database.sync_commit_pull_response(&[], &page).expect("commit classified page"));
        let subjects: Vec<String> = database
            .connection
            .prepare("SELECT s.name FROM book_subject s JOIN book b ON b.row_id = s.book_row_id WHERE b.content_hash = ?1 ORDER BY s.position")
            .unwrap()
            .query_map([hash.as_str()], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(subjects.iter().any(|subject| subject.contains("Distributed systems")), "subject survives projection, got {subjects:?}");
    }

    #[test]
    fn languages_project_once_and_never_rewrite() {
        use crate::sync::SyncApplySql;
        use crate::sync::apply::project_prepared_book;
        use book_model::LanguageTag;

        let holder = open_test_database();
        let database = &holder.database;
        let hash = "e".repeat(64);
        let tx = database.connection.unchecked_transaction().expect("open tx");
        tx.execute("INSERT INTO book(content_hash, added_at, format) VALUES(?1, 1, 'epub')", [hash.as_str()]).expect("seed book");
        let read_languages = |tx: &rusqlite::Transaction| -> Vec<String> {
            tx.prepare("SELECT language_tag FROM book_language WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1) ORDER BY position")
                .expect("prepare")
                .query_map([hash.as_str()], |row| row.get(0))
                .expect("query")
                .collect::<Result<Vec<String>, _>>()
                .expect("collect")
        };
        let mut first = book_model::BookMetadata::default();
        first.languages = vec![LanguageTag::parse("en").expect("tag")];
        let encoded = sync_common::wire::encode(&first).expect("encode");
        project_prepared_book(&tx, &hash, &first, encoded).expect("project");
        assert_eq!(read_languages(&tx), vec!["en".to_owned()]);
        // A later projection carrying different languages must not rewrite them.
        let mut second = first.clone();
        second.languages = vec![LanguageTag::parse("fr").expect("tag")];
        let encoded = sync_common::wire::encode(&second).expect("encode");
        project_prepared_book(&tx, &hash, &second, encoded).expect("reproject");
        assert_eq!(read_languages(&tx), vec!["en".to_owned()]);
    }

    #[test]
    fn publishers_are_not_projected() {
        use crate::sync::SyncApplySql;
        use crate::sync::apply::project_prepared_book;

        let holder = open_test_database();
        let database = &holder.database;
        let hash = "f".repeat(64);
        let tx = database.connection.unchecked_transaction().expect("open tx");
        tx.execute("INSERT INTO book(content_hash, added_at, format) VALUES(?1, 1, 'epub')", [hash.as_str()]).expect("seed book");
        let mut book = book_model::BookMetadata::default();
        book.publishers = vec![book_model::PublisherCredit::with_id(book_model::agent_id_from_migration_seed(b"test-publisher"), "Acme Press").expect("credit")];
        let encoded = sync_common::wire::encode(&book).expect("encode");
        project_prepared_book(&tx, &hash, &book, encoded).expect("project");
        let tables: i64 = tx.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('publisher', 'book_publisher')", [], |row| row.get(0)).expect("count");
        assert_eq!(tables, 0);
    }
}

#[cfg(test)]
mod state_tests {
    use crate::*;
    use sync_common::{StateCell, SyncCursor};

    struct TestDatabase {
        database: Database,
        path: std::path::PathBuf,
    }

    impl Drop for TestDatabase {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    fn open_test_database() -> TestDatabase {
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("library-database-sync-state-{}-{id}", std::process::id()));
        let database = Database::open(&path).expect("open test database");
        database.initialize_library().expect("initialize test library");
        TestDatabase { database, path }
    }

    #[test]
    fn recovery_flag_lifecycle() {
        let holder = open_test_database();
        let database = &holder.database;
        assert!(!database.sync_cursor_recovery_pending().expect("read flag"));
        database.sync_begin_cursor_recovery().expect("begin recovery");
        assert!(database.sync_cursor_recovery_pending().expect("reread flag"));
        assert_eq!(database.sync_pull_cursor().expect("read cursor"), SyncCursor::default());
        // No version rows exist, so the first page is also the last and clears
        // the marker.
        assert_eq!(database.sync_recover_state_page(None).expect("recover page"), (None, 0));
        assert!(!database.sync_cursor_recovery_pending().expect("cleared flag"));
    }

    #[test]
    fn inventory_checkpoint_round_trip() {
        let holder = open_test_database();
        let database = &holder.database;
        assert_eq!(database.sync_inventory_checkpoint().expect("read checkpoint"), None);
        let cell = StateCell { kind: "directory_name".to_owned(), entity_key: sync_common::ROOT_DIR_ID.to_string(), entity_subkey: String::new() };
        database.sync_save_inventory_checkpoint(Some(cell.clone())).expect("save checkpoint");
        assert_eq!(database.sync_inventory_checkpoint().expect("reread checkpoint"), Some(cell));
        database.sync_save_inventory_checkpoint(None).expect("clear checkpoint");
        assert_eq!(database.sync_inventory_checkpoint().expect("cleared checkpoint"), None);
    }

    #[test]
    fn missing_cells_reencode_current_state() {
        let holder = open_test_database();
        let database = &holder.database;
        let created = database.create_directory_with_id(&uuid::Uuid::new_v4(), &sync_common::ROOT_DIR_ID, &"Repair seed".to_owned()).expect("seed directory");
        let ids: Vec<sync_common::MutationId> = database.sync_publishable_mutations().expect("read snapshot").iter().map(|mutation| mutation.mutation_id).collect();
        database.sync_acknowledge_mutations(&ids).expect("acknowledge");
        let cell = StateCell { kind: "directory_name".to_owned(), entity_key: created.id.to_string(), entity_subkey: String::new() };
        assert_eq!(database.sync_enqueue_missing_state_cells(&[cell]).expect("enqueue"), 1);
        // Already queued cells are skipped without re-encoding.
        let snapshot = database.sync_publishable_mutations().expect("reread snapshot");
        assert_eq!(snapshot.len(), 1);
        assert!(matches!(snapshot[0].body, library_replica::MutationBody::DirectoryName { .. }));
        let duplicate = StateCell { kind: "directory_name".to_owned(), entity_key: created.id.to_string(), entity_subkey: String::new() };
        assert_eq!(database.sync_enqueue_missing_state_cells(&[duplicate]).expect("re-enqueue"), 0);
        // Unknown kinds have no current value and are skipped.
        let unknown = StateCell { kind: "no_such_kind".to_owned(), entity_key: "x".to_owned(), entity_subkey: String::new() };
        assert_eq!(database.sync_enqueue_missing_state_cells(&[unknown]).expect("enqueue unknown"), 0);
    }
}

#[cfg(test)]
mod outbox_tests {
    use crate::*;
    use sync_common::MutationId;

    struct TestDatabase {
        database: Database,
        path: std::path::PathBuf,
    }

    impl Drop for TestDatabase {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    fn open_test_database() -> TestDatabase {
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("library-database-sync-outbox-{}-{id}", std::process::id()));
        let database = Database::open(&path).expect("open test database");
        database.initialize_library().expect("initialize test library");
        TestDatabase { database, path }
    }

    #[test]
    fn publishable_snapshot_round_trips_through_acknowledgement() {
        let holder = open_test_database();
        let database = &holder.database;
        database.create_directory_with_id(&uuid::Uuid::new_v4(), &sync_common::ROOT_DIR_ID, &"Ack seed".to_owned()).expect("seed directory");
        let snapshot = database.sync_publishable_mutations().expect("read snapshot");
        assert_eq!(snapshot.len(), 3);
        let ids: Vec<MutationId> = snapshot.iter().map(|mutation| mutation.mutation_id).collect();
        database.sync_acknowledge_mutations(&ids).expect("acknowledge");
        assert!(database.sync_publishable_mutations().expect("reread snapshot").is_empty());
        database.sync_acknowledge_mutations(&[]).expect("empty acknowledgement is a no-op");
    }

    #[test]
    fn acknowledgement_commits_caller_sized_batches() {
        let holder = open_test_database();
        let database = &holder.database;
        for index in 0..90 {
            database.create_directory_with_id(&uuid::Uuid::new_v4(), &sync_common::ROOT_DIR_ID, &format!("Batch {index}")).expect("seed directory");
        }
        let snapshot = database.sync_publishable_mutations().expect("read snapshot");
        assert_eq!(snapshot.len(), 270);
        let ids: Vec<MutationId> = snapshot.iter().map(|mutation| mutation.mutation_id).collect();
        for chunk in ids.chunks(256) {
            database.sync_acknowledge_mutations(chunk).expect("acknowledge chunk");
        }
        assert!(database.sync_publishable_mutations().expect("reread snapshot").is_empty());
    }
}

mod annotation_tests {
    use crate::*;
    use book_model::{AnnotationAnchor, AnnotationStyle, ReaderAnnotation};
    use library_replica::{MutationBody, StateMutation};
    use sync_common::{LibraryRevision, ReplicaSeq, SyncCursor};

    struct TestDatabase {
        database: Database,
        path: std::path::PathBuf,
    }

    impl Drop for TestDatabase {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    fn open_test_database() -> TestDatabase {
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("library-database-annotations-{id}"));
        let database = Database::open(&path).expect("open test database");
        database.initialize_library().expect("initialize test library");
        TestDatabase { database, path }
    }

    fn seed_book(database: &Database, hash: sync_common::ContentHash) {
        let dir_id = uuid::Uuid::parse_str("12345678-1234-1234-1234-444444444444").unwrap();
        let body = MutationBody::Placement { dir_id, content_hash: hash, present: true, origin_folder_id: None };
        let mutation = StateMutation { mutation_id: sync_common::MutationId::parse("12345678-1234-1234-1234-444444444444").unwrap(), body, changed_at: 90, replica_seq: ReplicaSeq::new(1).unwrap() };
        let page = sync_common::PullStateResponse {
            mutations: vec![sync_common::ServerMutation { mutation: mutation.to_wire().unwrap(), replica_id: uuid::Uuid::new_v4(), revision: LibraryRevision::new(1).unwrap() }],
            next_cursor: SyncCursor { state_revision: Some(LibraryRevision::new(2).unwrap()), reading_revision: None },
            has_more: false,
        };
        assert!(database.sync_commit_pull_response(&[], &page).expect("seed book"));
    }

    fn annotation(id: &str, hash: book_model::ContentHash, ordinal: Option<i64>, progress: Option<f32>, modified_at: i64, note: &str) -> ReaderAnnotation {
        ReaderAnnotation {
            id: id.to_owned(),
            content_hash: hash,
            anchor: AnnotationAnchor::epub_cfi("epubcfi(/6/2!/4,/1:0,/1:4)"),
            exact_text: "Text".to_owned(),
            style: AnnotationStyle::Highlight,
            color: "#f6c945".to_owned(),
            note: note.to_owned(),
            created_at: 10,
            modified_at,
            toc_ordinal: ordinal,
            progress,
        }
    }

    fn annotation_mutations(database: &Database) -> Vec<StateMutation> {
        database.sync_publishable_mutations().expect("read snapshot").into_iter().filter(|mutation| matches!(mutation.body, MutationBody::Annotation { .. })).collect()
    }

    #[test]
    fn stored_annotations_round_trip_detail_and_order_by_reading_position() {
        let holder = open_test_database();
        let database = &holder.database;
        let hash = sync_common::ContentHash::new(&"a".repeat(64));
        seed_book(database, hash);
        let hash = book_model::ContentHash::new(&"a".repeat(64));
        database.upsert_annotation(&annotation("third", hash.clone(), None, None, 30, "ungrouped")).expect("store unpositioned");
        database.upsert_annotation(&annotation("first", hash.clone(), Some(2), Some(10.0), 20, "early")).expect("store early");
        database.upsert_annotation(&annotation("second", hash.clone(), Some(5), Some(60.0), 10, "late")).expect("store late");
        let stored = database.annotations(sync_common::ContentHash::new(&"a".repeat(64))).expect("read annotations");
        assert_eq!(stored.iter().map(|item| item.id.as_str()).collect::<Vec<_>>(), ["first", "second", "third"]);
        assert_eq!(stored[0].note, "early");
        assert_eq!(stored[0].toc_ordinal, Some(2));
        assert_eq!(stored[0].progress, Some(10.0));
        assert_eq!(stored[0].anchor, AnnotationAnchor::epub_cfi("epubcfi(/6/2!/4,/1:0,/1:4)"));
        assert_eq!(stored[0].created_at, 10);
    }

    #[test]
    fn reupsert_preserves_created_at_and_updates_detail() {
        let holder = open_test_database();
        let database = &holder.database;
        let hash = sync_common::ContentHash::new(&"b".repeat(64));
        seed_book(database, hash);
        let hash = book_model::ContentHash::new(&"b".repeat(64));
        database.upsert_annotation(&annotation("one", hash.clone(), Some(1), Some(5.0), 10, "old")).expect("store");
        let mut edited = annotation("one", hash, Some(1), Some(5.0), 20, "new");
        edited.created_at = 99;
        database.upsert_annotation(&edited).expect("restore");
        let stored = database.annotations(sync_common::ContentHash::new(&"b".repeat(64))).expect("read annotations");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].note, "new");
        assert_eq!(stored[0].created_at, 10, "edits must not move creation time");
        assert_eq!(stored[0].modified_at, 20);
    }

    #[test]
    fn delete_hides_row_and_publishes_tombstone() {
        let holder = open_test_database();
        let database = &holder.database;
        let hash = sync_common::ContentHash::new(&"c".repeat(64));
        seed_book(database, hash);
        let hash = book_model::ContentHash::new(&"c".repeat(64));
        database.upsert_annotation(&annotation("one", hash, Some(3), Some(25.0), 10, "note")).expect("store");
        assert_eq!(annotation_mutations(database).len(), 1);
        database.sync_acknowledge_mutations(&annotation_mutations(database).iter().map(|mutation| mutation.mutation_id).collect::<Vec<_>>()).expect("acknowledge");
        database.delete_annotation("one", 11).expect("delete");
        assert!(database.annotations(sync_common::ContentHash::new(&"c".repeat(64))).expect("read annotations").is_empty());
        let tombstones = annotation_mutations(database);
        assert_eq!(tombstones.len(), 1);
        let MutationBody::Annotation { annotation_id, value } = &tombstones[0].body else { panic!("expected annotation mutation") };
        assert_eq!(annotation_id, "one");
        assert!(value.deleted);
        assert_eq!(value.note, "note");
        assert_eq!(value.toc_ordinal, Some(3));
    }

    #[test]
    fn delete_unknown_id_is_an_error() {
        let holder = open_test_database();
        let database = &holder.database;
        assert!(database.delete_annotation("missing", 11).is_err());
    }

    #[test]
    fn remote_annotation_apply_stores_positioned_detail() {
        let holder = open_test_database();
        let database = &holder.database;
        let hash = sync_common::ContentHash::new(&"d".repeat(64));
        let body = MutationBody::Annotation {
            annotation_id: "remote".to_owned(),
            value: book_model::AnnotationState {
                content_hash: book_model::ContentHash::new(&"d".repeat(64)),
                anchor: AnnotationAnchor::epub_cfi("epubcfi(/6/2)"),
                exact_text: "Remote".to_owned(),
                style: AnnotationStyle::Underline,
                color: "#fff".to_owned(),
                note: "remote note".to_owned(),
                created_at: 5,
                modified_at: 15,
                deleted: false,
                toc_ordinal: Some(7),
                progress: Some(80.0),
            },
        };
        let mutation = StateMutation { mutation_id: sync_common::MutationId::parse("12345678-1234-1234-1234-555555555555").unwrap(), body, changed_at: 100, replica_seq: ReplicaSeq::new(1).unwrap() };
        let page = sync_common::PullStateResponse {
            mutations: vec![sync_common::ServerMutation { mutation: mutation.to_wire().unwrap(), replica_id: uuid::Uuid::new_v4(), revision: LibraryRevision::new(1).unwrap() }],
            next_cursor: SyncCursor { state_revision: Some(LibraryRevision::new(3).unwrap()), reading_revision: None },
            has_more: false,
        };
        assert!(database.sync_commit_pull_response(&[], &page).expect("commit annotation page"));
        let stored = database.annotations(hash).expect("read annotations");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].note, "remote note");
        assert_eq!(stored[0].toc_ordinal, Some(7));
        assert_eq!(stored[0].progress, Some(80.0));
        assert_eq!(stored[0].created_at, 5);
    }
}
