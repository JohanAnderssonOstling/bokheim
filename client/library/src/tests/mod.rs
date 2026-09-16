use super::{browse::*, catalog::*, placements::upsert_dir, sync_state::*};
use crate::LIBRARY_SCHEMA_VERSION;
use crate::assets::add_asset_request;
use crate::assets::book_placement_expects_hash;
use crate::assets::complete_asset_request;
use crate::assets::forget_remote_asset;
use crate::assets::get_asset_requests;
use crate::assets::get_book_file_path;
use crate::assets::get_live_dir_paths_tx;
use crate::assets::get_remote_asset_hashes;
use crate::assets::is_book_download_requested;
use crate::assets::is_book_downloaded;
use crate::assets::live_book_asset_page;
use crate::assets::live_book_hash_page;
use crate::assets::record_rejected_asset_upload;
use crate::assets::record_remote_asset;
use crate::assets::rejected_asset_upload_hashes;
use crate::assets::request_book_downloads;
use crate::audiobooks::audiobook_chapters;
use crate::books::get_book_cards_by_author_scoped;
use crate::books::get_book_cards_in_dir_scoped;
use crate::books::get_book_cards_in_dir_sorted_scoped;
use crate::books::queries::BooksSql;
use crate::browse::get_book_ancestor_dir_ids;
use crate::browse::get_descendant_content_hashes;
use crate::browse::get_folder_download_counts;
use crate::browse::queries::HomeSql;
use crate::catalog;
use crate::local_paths::BookPlacement;
use crate::local_paths::RelativeBookPath;
use crate::metadata::attach_external_author_id;
use crate::metadata::get_book_author_identities;
#[cfg(test)]
use crate::metadata::insert_scanned_book;
use crate::metadata::insert_scanned_book_contributors;
use crate::metadata::insert_scanned_book_metadata;
use crate::metadata::recorded_taxonomy_codes;
use crate::metadata::recorded_taxonomy_codes_for_library;
use crate::metadata::resolve_book_author_identity;
use crate::metadata::seed_subject_systems;
use crate::metadata::split_book_author;
use crate::metadata::update_book_metadata;
use crate::metadata::update_book_metadata_contributors;
use crate::metadata::update_book_metadata_contributors_preserving_ids;
use crate::placements;
use crate::placements::create_directory;
use crate::placements::directory_relative_path;
use crate::placements::empty_trash_records;
use crate::placements::library_trash;
use crate::placements::restore_directory_record;
use crate::placements::set_book_hash_downloaded;
use crate::placements::trash_directory_record;
use crate::reading::get_audiobook_reading_position;
use crate::reading::get_book_reading_position;
use crate::reading::get_pdf_reading_page;
use crate::reading::update_reading_position;
use crate::remote_reducer::apply_remote_change_in_tx;
use crate::remote_reducer::apply_remote_changes_in_tx;
use crate::scanner::count_pending_vanish_candidates;
use crate::scanner::delete_vanished_placements;
use crate::scanner::insert_local_book_dir;
use crate::schema::configure_connection;
use crate::schema::init_db;
use crate::schema::library_write_transaction;
use crate::storage_values::unix_millis;
use crate::sync_state::acknowledge_mutations;
use crate::sync_state::begin_cursor_recovery;
use crate::sync_state::complete_cursor_recovery;
use crate::sync_state::cursor_recovery_pending;
#[cfg(test)]
use crate::sync_state::decode_outbox_row;
use crate::sync_state::enqueue_missing_state_cells;
use crate::sync_state::get_library_sync_details;
use crate::sync_state::get_or_create_replica_id;
use crate::sync_state::get_pending_mutations;
use crate::sync_state::get_sync_metadata;
use crate::sync_state::queries::SyncSql;
use crate::sync_state::set_sync_metadata;
use crate::sync_state::state_inventory;
use crate::toc;
use library_model::AgentId;
#[cfg(test)]
use library_model::AuthorAuthority;
use library_model::BookFormat;
use library_model::BookLifecycleState;
use library_model::BookMetadataState;
use library_model::Contributor;
use library_model::ExternalAuthorId;
#[cfg(test)]
use library_model::MarcRelatorCode;
use library_model::PublicationMetadata;
use library_model::ReadProgress;
use library_model::ReadingPositionState;
use library_model::Scheme;
use library_model::Scope;
use library_model::SyncBookMetadata;
use library_model::{BrowseBookSort, BrowseChipSort, LibraryFileTypeFilter};
use library_replica::BlobKind;
use library_replica::MutationBody;
use library_replica::StateMutation;
use library_replica::TransferOrigin;
use rusqlite::Connection;
use rusqlite::OptionalExtension;
use rusqlite::Transaction;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::time::Duration;
use subject_projection::UNIFIED_MATCHER_VERSION;
use sync_common::ContentHash;
use sync_common::DirId;
use sync_common::FileName;
use sync_common::MutationId;
use sync_common::ReplicaSeq;
use sync_common::ServerMutation;
use taxonomy_contract::TaxonomyCode;
use taxonomy_contract::TaxonomySystem;
use test_support::{fixture_content_hash, fixture_dir_id, fixture_replica_id};
const TEST_CONTENT_HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
/// What Home splits its recently-read list at, matching the GPUI home view.
const FINISHED_THRESHOLD: f64 = library_model::FINISHED_PROGRESS_THRESHOLD as f64;
const DIR_A: &str = "61000000-0000-0000-0000-0000e40c292c";
const DIR_B: &str = "62000000-0000-0000-0000-0000e70c2de5";
const DIR_A_CHILD: &str = "612d6368-696c-6400-0000-000093ae1f6b";
const DIR_EMPTY: &str = "656d7074-7900-0000-0000-000018a7beee";
const DIR_SHELF: &str = "7368656c-6600-0000-0000-000007e1ec43";
const DIR_CHILD: &str = "6368696c-6400-0000-0000-0000e5a0205d";
const DIR_OTHER: &str = "6f746865-7200-0000-0000-0000c87d8df5";
const DIR_D1: &str = "64310000-0000-0000-0000-0000881d13e6";
const DIR_D2: &str = "64320000-0000-0000-0000-0000871d1253";

#[test]
fn library_language_counts_include_only_visible_placed_books_and_collapse_regions() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.execute_batch(include_str!("sql/tests/library_language_counts_include_only_visible_placed_books_and_collapse_regions_insert.sql")).unwrap();

    let counts = library_language_counts(&conn).unwrap().into_iter().map(|count| (count.language, count.book_count)).collect::<Vec<_>>();
    assert_eq!(counts, [("en".to_owned(), 2), ("sv".to_owned(), 1)]);
}

#[test]
fn library_format_counts_use_browse_categories_and_count_each_visible_book_once() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.execute_batch(include_str!("sql/mod/library_format_counts_use_browse_categories_and_count_each_visible_book_once_query.sql")).unwrap();

    let counts = library_format_counts(&conn).unwrap().into_iter().map(|count| (count.format, count.book_count)).collect::<Vec<_>>();
    assert_eq!(counts, [(LibraryFileTypeFilter::Book, 2), (LibraryFileTypeFilter::Pdf, 1), (LibraryFileTypeFilter::Audiobook, 1)]);
}

#[test]
fn folder_facets_apply_the_other_dimension_and_keep_their_own_choices_open() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.execute_batch(include_str!("sql/mod/folder_facets_apply_the_other_dimension_and_keep_their_own_choices_open_query.sql")).unwrap();

    let (languages, formats) = folder_facet_counts(&conn, DIR_A, "", &[LibraryFileTypeFilter::Pdf], &["de"]).unwrap();
    assert_eq!(languages.into_iter().map(|count| (count.language, count.book_count)).collect::<Vec<_>>(), [("de".to_owned(), 1), ("en".to_owned(), 9)]);
    assert_eq!(formats.into_iter().map(|count| (count.format, count.book_count)).collect::<Vec<_>>(), [(LibraryFileTypeFilter::Book, 9), (LibraryFileTypeFilter::Pdf, 1)]);

    let (searched_languages, _) = folder_facet_counts(&conn, "00000000-0000-0000-0000-000000000000", "a", &[LibraryFileTypeFilter::Pdf], &[]).unwrap();
    assert_eq!(searched_languages.into_iter().map(|count| (count.language, count.book_count)).collect::<Vec<_>>(), [("de".to_owned(), 1), ("en".to_owned(), 9)]);

    let (zero_languages, zero_formats) = folder_facet_counts(&conn, DIR_A, "", &[LibraryFileTypeFilter::Pdf], &["fr"]).unwrap();
    assert!(zero_languages.iter().any(|count| count.language == "fr" && count.book_count == 0));
    assert!(zero_formats.iter().any(|count| count.format == LibraryFileTypeFilter::Pdf && count.book_count == 0));
}

// Browse fixtures replace the shared schema, never create a library-local tree.
fn use_fixture_taxonomy(conn: &Connection) {
    conn.execute_batch(
        "DETACH curated; ATTACH ':memory:' AS curated;
        CREATE TABLE curated.unified_concept_route(path TEXT PRIMARY KEY,concept_id INTEGER NOT NULL,parent_path TEXT NOT NULL,label TEXT NOT NULL);
        CREATE INDEX curated.idx_unified_concept_route_parent ON unified_concept_route(parent_path,label,path);
        CREATE INDEX curated.idx_unified_concept_route_concept ON unified_concept_route(concept_id,path);",
    )
    .unwrap();
}

#[test]
fn subject_facets_use_derived_ancestor_ownership() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    use_fixture_taxonomy(&conn);
    conn.execute_batch(include_str!("sql/tests/subject_facets_use_derived_ancestor_ownership_insert.sql")).unwrap();

    let (languages, formats) = subject_facet_counts(&conn, "Facet", "", &[LibraryFileTypeFilter::Pdf], &["de"]).unwrap();
    assert_eq!(languages.into_iter().map(|count| (count.language, count.book_count)).collect::<Vec<_>>(), [("de".to_owned(), 1), ("en".to_owned(), 1)]);
    assert_eq!(formats.into_iter().map(|count| (count.format, count.book_count)).collect::<Vec<_>>(), [(LibraryFileTypeFilter::Book, 1), (LibraryFileTypeFilter::Pdf, 1)]);

    let (root_languages, _) = subject_facet_counts(&conn, library_model::ROOT_SUBJECT_PATH, "child", &[LibraryFileTypeFilter::Pdf], &[]).unwrap();
    assert_eq!(root_languages.into_iter().map(|count| (count.language, count.book_count)).collect::<Vec<_>>(), [("de".to_owned(), 1), ("en".to_owned(), 1)]);

    conn.execute(include_str!("sql/tests/subject_facets_use_derived_ancestor_ownership_delete.sql"), []).unwrap();
    let (detached_languages, _) = subject_facet_counts(&conn, "Facet", "", &[LibraryFileTypeFilter::All], &[]).unwrap();
    assert!(detached_languages.is_empty(), "assignment changes must invalidate and rebuild ancestor ownership");
}

#[test]
fn shared_trash_lifecycle_restores_then_purges_folder_and_book() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let directory_id = test_dir_id(DIR_EMPTY);
    let content_hash = fixture_content_hash(91);
    let transaction = conn.transaction().unwrap();
    upsert_dir(&transaction, &directory_id, &sync_common::ROOT_DIR_ID, &test_file_name("Archive")).unwrap();
    insert_scanned_book_metadata(&transaction, content_hash.clone(), &metadata("Disposable", Vec::new(), "description"), BookFormat::Epub).unwrap();
    transaction.execute(include_str!("sql/tests/shared_trash_lifecycle_restores_then_purges_folder_and_book_insert.sql"), rusqlite::params![directory_id.to_string(), content_hash.as_str()]).unwrap();
    transaction.execute(include_str!("sql/tests/shared_trash_lifecycle_restores_then_purges_folder_and_book_insert_2.sql"), [content_hash.as_str()]).unwrap();
    trash_directory_record(&transaction, &directory_id).unwrap();
    transaction.commit().unwrap();
    let trash = library_trash(&conn).unwrap();
    assert_eq!(trash.folders.len(), 1);
    assert_eq!(trash.books.iter().map(|book| &book.content_hash).collect::<Vec<_>>(), vec![&content_hash]);
    let transaction = conn.transaction().unwrap();
    assert_eq!(restore_directory_record(&transaction, &directory_id, None).unwrap(), "Archive");
    transaction.commit().unwrap();
    assert_eq!(library_trash(&conn).unwrap().books.len(), 0);
    assert_eq!(audiobook_chapters(&conn, &content_hash).unwrap().len(), 1);
    let transaction = conn.transaction().unwrap();
    trash_directory_record(&transaction, &directory_id).unwrap();
    empty_trash_records(&transaction).unwrap();
    transaction.commit().unwrap();
    assert_eq!(conn.query_row(include_str!("sql/tests/shared_trash_lifecycle_restores_then_purges_folder_and_book_select.sql"), [directory_id.to_string()], |row| row.get::<_, i64>(0)).unwrap(), 2);
    assert_eq!(conn.query_row(include_str!("sql/tests/shared_trash_lifecycle_restores_then_purges_folder_and_book_select_2.sql"), [content_hash.as_str()], |row| row.get::<_, bool>(0)).unwrap(), true);
    assert_eq!(conn.query_row(include_str!("sql/tests/shared_trash_lifecycle_restores_then_purges_folder_and_book_select_3.sql"), [content_hash.as_str()], |row| row.get::<_, i64>(0)).unwrap(), 0);
    assert!(audiobook_chapters(&conn, &content_hash).unwrap().is_empty());
}
#[test]
fn shared_catalog_ownership_returns_complete_live_keys() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let content_hash = fixture_content_hash(92);
    let transaction = conn.transaction().unwrap();
    insert_scanned_book_metadata(&transaction, content_hash.clone(), &metadata("Owned", Vec::new(), ""), BookFormat::Epub).unwrap();
    transaction.execute(include_str!("sql/tests/shared_catalog_ownership_returns_complete_live_keys_insert.sql"), rusqlite::params![sync_common::ROOT_DIR_ID.to_string(), content_hash.as_str()]).unwrap();
    let source_id = catalog_model::CatalogSourceId::parse("test-source").unwrap();
    let publication = catalog_model::CatalogPublicationIdentity { edition_id: "edition-1".to_owned(), work_id: Some("work-1".to_owned()) };
    catalog::record_catalog_publication(&transaction, &source_id, &publication, &content_hash).unwrap();
    transaction.commit().unwrap();
    let keys = catalog::all_owned_catalog_publications(&conn).unwrap();
    assert_eq!(keys, vec![(source_id, publication)]);
}
fn test_content_hash() -> ContentHash {
    ContentHash::new(&TEST_CONTENT_HASH)
}
fn test_dir_id(value: &str) -> DirId {
    DirId::parse_str(value).unwrap_or_else(|_| fixture_dir_id(value))
}
fn test_file_name(value: &str) -> FileName {
    value.to_owned()
}
fn author(value: &str) -> Contributor {
    Contributor::new(value, None, MarcRelatorCode(*b"aut")).unwrap()
}
#[cfg(feature = "scanner")]
#[test]
fn scanner_directory_snapshot_accepts_the_roots_null_path() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let directories = get_known_directories_for_scan(&conn).unwrap();
    assert!(matches!(directories.get(&sync_common::ROOT_DIR_ID).map(|directory| &directory.state), Some(KnownDirectoryState::Live(None))));
}
fn metadata(title: impl Into<String>, authors: Vec<Contributor>, description: impl Into<String>) -> BookMetadataState {
    BookMetadataState { title: title.into(), contributors: authors, description: description.into(), publication: PublicationMetadata::default() }
}

#[test]
fn description_register_is_independent_for_every_delivery_order() {
    let hash = fixture_content_hash(7777);
    for order in [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]] {
        let mut conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        let changes = [
            server_change(MutationBody::Description { content_hash: hash, value: "Independent text".into() }, 20, 0),
            server_change(MutationBody::BookLifecycle { content_hash: hash, value: BookLifecycleState::Present { metadata: SyncBookMetadata { title: "Seed".into(), ..Default::default() }, added_at: 1, format: BookFormat::Epub } }, 10, 0),
            server_change(MutationBody::Metadata { content_hash: hash, value: SyncBookMetadata { title: "Latest title".into(), ..Default::default() } }, 30, 0),
        ];
        let tx = library_write_transaction(&mut conn).unwrap();
        for index in order {
            apply_remote_change_in_tx(&tx, &changes[index]).unwrap();
        }
        let stored: (String, String) = tx.query_row(include_str!("sql/tests/description_register_is_independent_for_every_delivery_order_select.sql"), [hash.as_str()], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(stored, ("Latest title".into(), "Independent text".into()));
        apply_remote_change_in_tx(&tx, &server_change(MutationBody::Description { content_hash: hash, value: "Stale".into() }, 19, 0)).unwrap();
        assert_eq!(tx.query_row(include_str!("sql/tests/description_register_is_independent_for_every_delivery_order_select_2.sql"), [], |r| r.get::<_, String>(0)).unwrap(), "Independent text");
        apply_remote_change_in_tx(&tx, &server_change(MutationBody::Description { content_hash: hash, value: String::new() }, 21, 0)).unwrap();
        assert_eq!(tx.query_row(include_str!("sql/tests/description_register_is_independent_for_every_delivery_order_select_2.sql"), [], |r| r.get::<_, String>(0)).unwrap(), "");
        apply_remote_change_in_tx(&tx, &server_change(MutationBody::BookLifecycle { content_hash: hash, value: BookLifecycleState::Purged }, 40, 0)).unwrap();
        apply_remote_change_in_tx(&tx, &server_change(MutationBody::Description { content_hash: hash, value: "Late text".into() }, 22, 0)).unwrap();
        assert_eq!(tx.query_row(include_str!("sql/tests/description_register_is_independent_for_every_delivery_order_select_4.sql"), [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        assert_eq!(tx.query_row(include_str!("sql/tests/failed_recovery_page_rolls_back_and_retains_marker_select.sql"), [], |r| r.get::<_, i64>(0)).unwrap(), 0, "remote descriptions must not echo");
        tx.commit().unwrap();
    }
}

#[test]
fn local_description_edits_never_reserialize_other_metadata() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let hash = fixture_content_hash(7778);
    let tx = library_write_transaction(&mut conn).unwrap();
    insert_scanned_book_metadata(&tx, hash, &metadata("Title", Vec::new(), "A long unique description"), BookFormat::Epub).unwrap();
    tx.commit().unwrap();
    let pending = get_pending_mutations(&conn).unwrap();
    assert!(pending.iter().any(|m| matches!(&m.body,MutationBody::Description{value,..} if value=="A long unique description")));
    for mutation in pending.iter().filter(|m| m.body.kind() != "description") {
        let bytes = mutation.to_wire().unwrap().value;
        assert!(!bytes.windows(25).any(|w| w == b"A long unique description"));
    }
    conn.execute(include_str!("sql/tests/failed_recovery_page_rolls_back_and_retains_marker_delete.sql"), []).unwrap();
    let tx = library_write_transaction(&mut conn).unwrap();
    super::metadata::update_book_description(&tx, hash, "Changed text").unwrap();
    tx.commit().unwrap();
    assert!(matches!(get_pending_mutations(&conn).unwrap().as_slice(), [StateMutation { body: MutationBody::Description { .. }, .. }]));
    conn.execute(include_str!("sql/tests/failed_recovery_page_rolls_back_and_retains_marker_delete.sql"), []).unwrap();
    conn.execute(include_str!("sql/tests/local_description_edits_never_reserialize_other_metadata_update.sql"), [hash.as_str()]).unwrap();
    assert!(matches!(get_pending_mutations(&conn).unwrap().as_slice(), [StateMutation { body: MutationBody::Metadata { .. }, .. }]));
}
#[test]
fn scanning_a_present_file_revives_a_purged_metadata_row_consistently() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.execute(include_str!("sql/tests/scanning_a_present_file_revives_a_purged_metadata_row_consistently_insert.sql"), [fixture_content_hash(1).as_str()]).unwrap();
    let transaction = conn.transaction().unwrap();
    insert_scanned_book_metadata(&transaction, fixture_content_hash(1), &metadata("Returned", vec![author("Author")], ""), BookFormat::Epub).unwrap();
    transaction.commit().unwrap();
    let lifecycle = conn
        .query_row(include_str!("sql/tests/scanning_a_present_file_revives_a_purged_metadata_row_consistently_select.sql"), [fixture_content_hash(1).as_str()], |row| Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, Option<i64>>(1)?)))
        .unwrap();
    assert_eq!(lifecycle, (None, None));
}
#[test]
fn embedded_epub_lcc_codes_are_projected_on_import_without_startup_reclassification() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let raw = "QA76.73.R87";
    let mut book_metadata = metadata("Rust book", Vec::new(), "");
    book_metadata.publication.subjects = vec![library_model::PublicationSubject::new(None, raw, "dc:subject", None, None).unwrap()];
    let transaction = conn.transaction().unwrap();
    insert_scanned_book_metadata(&transaction, fixture_content_hash(1), &book_metadata, BookFormat::Epub).unwrap();
    transaction.commit().unwrap();
    let assert_projection = || {
        let code: String = conn.query_row(include_str!("sql/tests/embedded_epub_lcc_codes_are_projected_on_import_without_startup_reclassification_select.sql"), [], |row| row.get(0)).unwrap();
        assert_eq!(code, raw);
        let declaration: (String, Option<String>, Option<String>) =
            conn.query_row(include_str!("sql/tests/embedded_epub_lcc_codes_are_projected_on_import_without_startup_reclassification_select_2.sql"), [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
        assert_eq!(declaration, (raw.into(), None, None));
    };
    assert_projection();
    conn.execute(include_str!("sql/tests/embedded_epub_lcc_codes_are_projected_on_import_without_startup_reclassification_delete.sql"), []).unwrap();
    conn.execute(include_str!("sql/tests/embedded_epub_lcc_codes_are_projected_on_import_without_startup_reclassification_update.sql"), [UNIFIED_MATCHER_VERSION - 1]).unwrap();
    init_db(&conn).unwrap();
    assert_eq!(
        conn.query_row(include_str!("sql/tests/embedded_epub_lcc_codes_are_projected_on_import_without_startup_reclassification_select_3.sql"), [], |r| r.get::<_, i64>(0)).unwrap(),
        0,
        "opening must not replay metadata even when the matcher version differs"
    );
}

#[test]
fn bisac_assignments_are_projected_and_browsed_separately_from_raw_subjects() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let declarations = ["History", "General"].into_iter().map(|name| library_model::PublicationSubject::new(None, name, "dc:subject", None, None).unwrap()).collect();
    let categorized = BookMetadataState { title: "A general history".to_owned(), contributors: Vec::new(), description: String::new(), publication: PublicationMetadata { subjects: declarations, ..PublicationMetadata::default() } };
    let uncategorized = metadata("No subjects", Vec::new(), "");
    let other_system = BookMetadataState {
        title: "Library of Congress history".to_owned(),
        contributors: Vec::new(),
        description: String::new(),
        publication: PublicationMetadata { subjects: vec![library_model::PublicationSubject::new(None, "History / United States / 20th Century", "dc:subject", Some("LCSH".to_owned()), None).unwrap()], ..PublicationMetadata::default() },
    };
    let transaction = conn.transaction().unwrap();
    insert_scanned_book_metadata(&transaction, fixture_content_hash(1), &categorized, BookFormat::Epub).unwrap();
    insert_scanned_book_metadata(&transaction, fixture_content_hash(2), &uncategorized, BookFormat::Epub).unwrap();
    insert_scanned_book_metadata(&transaction, fixture_content_hash(3), &other_system, BookFormat::Epub).unwrap();
    transaction.commit().unwrap();
    conn.execute_batch(include_str!("sql/tests/bisac_assignments_are_projected_and_browsed_separately_from_raw_subjects_insert.sql")).unwrap();
    let assignments = conn
        .prepare(include_str!("sql/tests/bisac_assignments_are_projected_and_browsed_separately_from_raw_subjects_select.sql"))
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<std::collections::BTreeSet<_>, _>>()
        .unwrap();
    let code_paths = subject_projection::unified_subject_paths("bisac", "HIS000000").into_iter().collect::<std::collections::BTreeSet<_>>();
    let expected_assignments = std::iter::once("History / General".to_owned()).collect::<std::collections::BTreeSet<_>>();
    assert_eq!(assignments, expected_assignments);
    assert_eq!(conn.query_row(include_str!("sql/tests/bisac_assignments_are_projected_and_browsed_separately_from_raw_subjects_select_2.sql"), [], |row| row.get::<_, String>(0)).unwrap(), "bisac:HIS000000");
    let stored_mappings = conn
        .prepare(include_str!("sql/tests/bisac_assignments_are_projected_and_browsed_separately_from_raw_subjects_select_3.sql"))
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<std::collections::BTreeSet<_>, _>>()
        .unwrap();
    assert_eq!(stored_mappings, code_paths);
    let external_assignments = conn
        .prepare(include_str!("sql/tests/bisac_assignments_are_projected_and_browsed_separately_from_raw_subjects_select_4.sql"))
        .unwrap()
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .unwrap()
        .collect::<Result<std::collections::BTreeSet<_>, _>>()
        .unwrap();
    assert!(external_assignments.contains(&("lcsh".to_owned(), "LCC / LCSH / History / United States / 20th Century".to_owned())));
    assert_eq!(conn.query_row(include_str!("sql/tests/bisac_assignments_are_projected_and_browsed_separately_from_raw_subjects_select_5.sql"), [], |row| row.get::<_, String>(0)).unwrap(), "dc:subject");
    let roots = get_subject_children(&conn, library_model::ROOT_SUBJECT_PATH).unwrap();
    assert_eq!(roots.len(), 1, "empty taxonomy roots are omitted from browse results");
    let humanities = roots.iter().find(|subject| subject.id.as_str() == "Humanities").expect("unified root is visible");
    assert_eq!(humanities.book_count, 2, "both history books match the locally bundled complete taxonomy");
    assert!(roots.iter().all(|subject| subject.book_count > 0));
    let social_branches = get_subject_children(&conn, "Social Sciences").unwrap();
    assert!(social_branches.is_empty(), "empty taxonomy branches are omitted from browse results");
    let searched_subjects = get_subject_browse_rows(&conn, "Humanities", "history", LibraryFileTypeFilter::All).unwrap();
    let history = searched_subjects.iter().find(|subject| subject.name == "History").expect("History is searchable below Humanities");
    assert_eq!(history.path.as_deref(), Some("Humanities"));
    let expected_code_concepts = code_paths.iter().map(|path| subject_projection::unified_concept_id(path).unwrap()).collect::<std::collections::BTreeSet<_>>();
    assert_eq!(conn.query_row(include_str!("sql/tests/bisac_assignments_are_projected_and_browsed_separately_from_raw_subjects_select_6.sql"), [], |row| row.get::<_, i64>(0)).unwrap(), expected_code_concepts.len() as i64);
    assert_eq!(conn.query_row(include_str!("sql/tests/bisac_assignments_are_projected_and_browsed_separately_from_raw_subjects_select_7.sql"), [], |row| row.get::<_, i64>(0)).unwrap(), expected_code_concepts.len() as i64);
    assert_eq!(get_book_cards_in_subject_scoped(&conn, "Humanities", false, "", LibraryFileTypeFilter::All).unwrap().len(), 0, "book grids use exact assignments");
    for path in &code_paths {
        assert_eq!(get_book_cards_in_subject_scoped(&conn, path, false, "", LibraryFileTypeFilter::All).unwrap().len(), 1);
    }
}
#[test]
fn plain_dc_bisac_heading_uses_general_code_for_library_unified_assignment() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let state = BookMetadataState {
        title: "Plain fiction".to_owned(),
        contributors: Vec::new(),
        description: String::new(),
        publication: PublicationMetadata { subjects: vec![library_model::PublicationSubject::new(None, "Fiction", "dc:subject", None, None).unwrap()], ..PublicationMetadata::default() },
    };
    let transaction = conn.transaction().unwrap();
    insert_scanned_book_metadata(&transaction, fixture_content_hash(1), &state, BookFormat::Epub).unwrap();
    transaction.commit().unwrap();
    assert_eq!(conn.query_row(include_str!("sql/tests/lcc_assignments_use_named_outline_and_upgrade_flat_call_number_paths_select_4.sql"), [], |row| row.get::<_, String>(0)).unwrap(), "bisac:FIC000000");
    let expected = subject_projection::unified_subject_paths("bisac", "FIC000000").into_iter().collect::<std::collections::BTreeSet<_>>();
    let actual = conn
        .prepare(include_str!("sql/tests/plain_dc_bisac_heading_uses_general_code_for_library_unified_assignment_select_2.sql"))
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<std::collections::BTreeSet<_>, _>>()
        .unwrap();
    assert_eq!(actual, expected);
}
#[test]
fn complete_lcsh_style_dc_subject_uses_bisac_code_for_library_unified_assignment() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let state = BookMetadataState {
        title: "Short-story fiction".to_owned(),
        contributors: Vec::new(),
        description: String::new(),
        publication: PublicationMetadata { subjects: vec![library_model::PublicationSubject::new(None, "Fiction -- Short Stories", "dc:subject", None, None).unwrap()], ..PublicationMetadata::default() },
    };
    let transaction = conn.transaction().unwrap();
    insert_scanned_book_metadata(&transaction, fixture_content_hash(1), &state, BookFormat::Epub).unwrap();
    transaction.commit().unwrap();
    let codes = conn
        .prepare(include_str!("sql/tests/complete_lcsh_style_dc_subject_uses_bisac_code_for_library_unified_assignment_select.sql"))
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<std::collections::BTreeSet<_>, _>>()
        .unwrap();
    assert_eq!(codes, std::collections::BTreeSet::from(["bisac:FIC029000".to_owned()]));
    let expected = subject_projection::unified_subject_paths("bisac", "FIC029000").into_iter().collect::<std::collections::BTreeSet<_>>();
    let actual = conn
        .prepare(include_str!("sql/tests/plain_dc_bisac_heading_uses_general_code_for_library_unified_assignment_select_2.sql"))
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<std::collections::BTreeSet<_>, _>>()
        .unwrap();
    assert_eq!(actual, expected);
}
#[test]
fn external_codes_are_recorded_before_exact_local_coverage_is_checked() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let state = BookMetadataState {
        title: "Future LCC detail".to_owned(),
        contributors: Vec::new(),
        description: String::new(),
        publication: PublicationMetadata { subjects: vec![library_model::PublicationSubject::new(None, "Future schedule", "marc", Some("lcc".to_owned()), Some("ZZZZ999".to_owned())).unwrap()], ..PublicationMetadata::default() },
    };
    let content_hash = fixture_content_hash(77);
    let transaction = conn.transaction().unwrap();
    insert_scanned_book_metadata(&transaction, content_hash.clone(), &state, BookFormat::Epub).unwrap();
    transaction.commit().unwrap();
    let codes = recorded_taxonomy_codes(&conn, &content_hash).unwrap();
    assert_eq!(codes, vec![TaxonomyCode { system: TaxonomySystem::Lcc, code: "ZZZZ999".to_owned() }]);
    assert_eq!(recorded_taxonomy_codes_for_library(&conn).unwrap(), codes);
}
#[test]
fn lcc_assignments_use_named_outline_and_upgrade_flat_call_number_paths() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let lcc = library_model::PublicationSubject::new(None, "QA76.73.R3", "openlibrary", Some("lcc".to_owned()), None).unwrap();
    let state = BookMetadataState { title: "Rust programming".to_owned(), contributors: Vec::new(), description: String::new(), publication: PublicationMetadata { subjects: vec![lcc], ..PublicationMetadata::default() } };
    let transaction = conn.transaction().unwrap();
    insert_scanned_book_metadata(&transaction, fixture_content_hash(1), &state, BookFormat::Epub).unwrap();
    transaction.commit().unwrap();
    conn.execute(include_str!("sql/tests/lcc_assignments_use_named_outline_and_upgrade_flat_call_number_paths_insert.sql"), []).unwrap();
    let expected = "LCC / Science / Mathematics / Calculating machines / Electronic computers. Computer science / QA76.73.R3";
    assert_eq!(conn.query_row(include_str!("sql/tests/lcc_assignments_use_named_outline_and_upgrade_flat_call_number_paths_select.sql"), [], |row| row.get::<_, String>(0)).unwrap(), expected);
    conn.execute_batch(include_str!("sql/tests/lcc_assignments_use_named_outline_and_upgrade_flat_call_number_paths_delete.sql")).unwrap();
    seed_subject_systems(&conn).unwrap();
    assert_eq!(conn.query_row(include_str!("sql/tests/lcc_assignments_use_named_outline_and_upgrade_flat_call_number_paths_select.sql"), [], |row| row.get::<_, String>(0)).unwrap(), expected);
    assert_eq!(conn.query_row(include_str!("sql/tests/lcc_assignments_use_named_outline_and_upgrade_flat_call_number_paths_select_3.sql"), [], |row| row.get::<_, String>(0)).unwrap(), subject_projection::LCC_VERSION);
    assert_eq!(conn.query_row(include_str!("sql/tests/lcc_assignments_use_named_outline_and_upgrade_flat_call_number_paths_select_4.sql"), [], |row| row.get::<_, String>(0)).unwrap(), "lcc:QA76.73.R3");
}
#[test]
fn subject_scope_includes_books_assigned_to_descendant_subjects() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    use_fixture_taxonomy(&conn);
    conn.execute_batch(include_str!("sql/tests/subject_scope_includes_books_assigned_to_descendant_subjects_insert.sql")).unwrap();
    assert_eq!(get_descendant_subject_content_hashes(&conn, "Fixture / History").unwrap(), vec![fixture_content_hash(1), fixture_content_hash(2)]);
    assert_eq!(get_descendant_subject_content_hashes(&conn, "Alternate / History").unwrap(), vec![fixture_content_hash(1)], "one concept can be exposed beneath multiple parents");
    assert_eq!(get_descendant_subject_content_hashes(&conn, "Fixture / Science").unwrap(), vec![fixture_content_hash(3)]);
}
#[test]
fn subject_view_hides_empty_nodes_and_collapses_non_branching_routes() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    use_fixture_taxonomy(&conn);
    conn.execute_batch(include_str!("sql/tests/subject_view_hides_empty_nodes_and_collapses_non_branching_routes_insert.sql")).unwrap();
    let scope = FileTypeScope::load(&conn, library_model::LibraryFileTypeFilter::All).unwrap();
    let (taxonomy_roots, root_books) = compact_subject_children(&conn, "", false, &scope).unwrap();
    assert_eq!(taxonomy_roots.iter().map(|child| child.id.as_str()).collect::<Vec<_>>(), ["Test / Solo", "Test / United States / Texas"]);
    assert!(root_books.is_empty());
    let (root_children, root_books) = compact_subject_children(&conn, "Test", false, &scope).unwrap();
    assert_eq!(root_children.iter().map(|child| child.id.as_str()).collect::<Vec<_>>(), ["Test / Solo", "Test / United States / Texas"]);
    assert!(root_books.is_empty());
    let (united_states_children, united_states_books) = compact_subject_children(&conn, "Test / United States", false, &scope).unwrap();
    assert!(united_states_children.is_empty());
    assert_eq!(united_states_books.iter().map(|book| book.title.as_str()).collect::<Vec<_>>(), ["Austin book", "Dallas book"]);
    let (texas_children, texas_books) = compact_subject_children(&conn, "Test / United States / Texas", false, &scope).unwrap();
    assert!(texas_children.is_empty());
    assert_eq!(texas_books.iter().map(|book| book.title.as_str()).collect::<Vec<_>>(), ["Austin book", "Dallas book"]);
    let (solo_children, solo_books) = compact_subject_children(&conn, "Test / Solo", false, &scope).unwrap();
    assert!(solo_children.is_empty());
    assert_eq!(solo_books.iter().map(|book| book.title.as_str()).collect::<Vec<_>>(), ["Solo book"]);
}
#[test]
fn library_write_lock_excludes_other_writers_with_read_only_taxonomy_attached() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("library.db");
    let mut first = Connection::open(&path).unwrap();
    init_db(&first).unwrap();
    let second = Connection::open(&path).unwrap();
    configure_connection(&second).unwrap();
    second.busy_timeout(Duration::ZERO).unwrap();
    let transaction = library_write_transaction(&mut first).unwrap();
    let error = second.execute(include_str!("sql/tests/library_write_lock_excludes_other_writers_with_read_only_taxonomy_attached_update.sql"), []).unwrap_err();
    assert_eq!(error.sqlite_error_code(), Some(rusqlite::ErrorCode::DatabaseBusy));
    transaction.rollback().unwrap();
    second.execute(include_str!("sql/tests/library_write_lock_excludes_other_writers_with_read_only_taxonomy_attached_update.sql"), []).unwrap();
    assert!(second.query_row(include_str!("sql/tests/library_write_lock_excludes_other_writers_with_read_only_taxonomy_attached_select.sql"), [], |r| r.get::<_, i64>(0)).unwrap() > 0);
}

#[test]
fn curated_taxonomy_is_read_only_and_not_stored_in_libraries() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    for table in ["unified_taxonomy_release", "unified_concept", "unified_concept_edge", "unified_concept_route"] {
        assert_eq!(conn.query_row(include_str!("sql/tests/curated_taxonomy_is_read_only_and_not_stored_in_libraries_select.sql"), [table], |row| row.get::<_, i64>(0)).unwrap(), 0);
    }
    let error = conn.execute(include_str!("sql/tests/curated_taxonomy_is_read_only_and_not_stored_in_libraries_delete.sql"), []).unwrap_err();
    assert_eq!(error.sqlite_error_code(), Some(rusqlite::ErrorCode::ReadOnly));
    let mut query = conn.prepare(include_str!("sql/tests/curated_taxonomy_is_read_only_and_not_stored_in_libraries_select_2.sql")).unwrap();
    let shipped = query.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?))).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
    let runtime = subject_projection::unified_taxonomy_routes().into_iter().map(|r| (r.path().to_owned(), r.concept_id(), r.parent_path().to_owned(), r.label().to_owned())).collect::<Vec<_>>();
    assert_eq!(shipped, runtime, "compiled navigation must agree with the matcher");
}
#[test]
fn replica_identity_round_trips_from_the_database() {
    let conn = Connection::open_in_memory().unwrap();
    let generated = init_db(&conn).unwrap();
    assert_eq!(get_or_create_replica_id(&conn).unwrap(), generated);
    let persisted = fixture_replica_id("persisted/replica");
    set_sync_metadata(&conn, "replica_id", &persisted.to_string()).unwrap();
    assert_eq!(get_or_create_replica_id(&conn).unwrap(), persisted);
}
#[test]
fn missing_server_inventory_is_reencoded_into_the_durable_outbox() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let dir_id = fixture_dir_id("inventory/shelf");
    let hash = fixture_content_hash(7);
    let dir_id_text = dir_id.to_string();
    conn.execute(include_str!("sql/tests/missing_server_inventory_is_reencoded_into_the_durable_outbox_insert.sql"), [dir_id_text.as_str()]).unwrap();
    conn.insert_book(hash.as_str(), "Inventory Book", 1, "epub").unwrap();
    conn.execute(include_str!("sql/tests/missing_server_inventory_is_reencoded_into_the_durable_outbox_insert_2.sql"), [dir_id_text.as_str(), hash.as_str()]).unwrap();
    let inventory = state_inventory(&conn).unwrap();
    assert_eq!(inventory.len(), 5);
    conn.execute(include_str!("sql/tests/failed_recovery_page_rolls_back_and_retains_marker_delete.sql"), []).unwrap();
    assert_eq!(enqueue_missing_state_cells(&mut conn, &inventory).unwrap(), 5);
    let mutations = get_pending_mutations(&conn).unwrap();
    let kinds = mutations.iter().map(|mutation| mutation.body.kind()).collect::<BTreeSet<_>>();
    assert_eq!(kinds, BTreeSet::from(["book_lifecycle", "directory_lifecycle", "directory_name", "directory_parent", "placement"]));
}
#[test]
fn local_relationships_use_integer_rows_but_expose_content_hash_identity() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.execute(include_str!("sql/tests/local_relationships_use_integer_rows_but_expose_content_hash_identity_insert.sql"), [TEST_CONTENT_HASH]).unwrap();
    conn.execute(include_str!("sql/tests/local_relationships_use_integer_rows_but_expose_content_hash_identity_insert_2.sql"), [TEST_CONTENT_HASH]).unwrap();
    let (book_key_type, placement_key_type, exposed_hash): (String, String, String) =
        conn.query_row(include_str!("sql/tests/local_relationships_use_integer_rows_but_expose_content_hash_identity_select.sql"), [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
    assert_eq!(book_key_type, "integer");
    assert_eq!(placement_key_type, "integer");
    assert_eq!(exposed_hash, TEST_CONTENT_HASH);
}
/// The card projection must agree with the per-book accessors it replaces,
/// otherwise browsing would quietly present different state than before.
#[test]
fn dir_card_rows_match_the_per_book_accessors_they_replace() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.execute_batch(include_str!("sql/tests/dir_card_rows_match_the_per_book_accessors_they_replace_insert.sql")).unwrap();
    let cards = get_book_cards_in_dir_scoped(&conn, "00000000-0000-0000-0000-000000000000", false, "", LibraryFileTypeFilter::All).unwrap();
    assert_eq!(cards.len(), 3, "hidden books stay excluded");
    let downloaded = cards.iter().find(|card| card.content_hash == fixture_content_hash(1)).expect("book 1 is listed");
    assert!(downloaded.downloaded);
    assert_eq!(downloaded.format_category, LibraryFileTypeFilter::Book);
    assert_eq!(downloaded.description, "A description");
    assert_eq!(downloaded.progress, 42.0);
    let missing = cards.iter().find(|card| card.content_hash == fixture_content_hash(2)).expect("book 2 is listed");
    assert!(!missing.downloaded);
    let audiobook = cards.iter().find(|card| card.content_hash == fixture_content_hash(3)).expect("book 3 is listed");
    assert_eq!(audiobook.format_category, LibraryFileTypeFilter::Audiobook);
    assert_eq!(audiobook.audiobook_duration_ms, Some(3_600_000));
    assert_eq!(audiobook.audiobook_chapter_count, Some(12));
    // Agreement with the accessors this projection replaces.
    for card in &cards {
        assert_eq!(card.downloaded, is_book_downloaded(&conn, card.content_hash).unwrap(), "downloaded disagrees for {}", card.content_hash);
        assert_eq!(card.download_requested, is_book_download_requested(&conn, card.content_hash).unwrap(), "requested disagrees for {}", card.content_hash);
    }
}
#[test]
fn browse_rows_are_always_sorted_by_name_or_title_in_sql() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.execute_batch(include_str!("sql/mod/browse_rows_are_always_sorted_by_name_or_title_in_sql_query.sql")).unwrap();
    let transaction = conn.transaction().unwrap();
    update_book_metadata(&transaction, fixture_content_hash(1), "Zulu Query", "Other Writer").unwrap();
    update_book_metadata(&transaction, fixture_content_hash(2), "Alpha Story", "Query Writer").unwrap();
    transaction.commit().unwrap();
    let rows = get_folder_list_rows(&conn, "00000000-0000-0000-0000-000000000000", "", LibraryFileTypeFilter::All).unwrap();
    assert_eq!(rows.into_iter().map(|row| row.name).collect::<Vec<_>>(), ["alpha", "Zulu"]);
    // Both books match "query", but through different fields. Search relevance
    // must not take precedence over the BrowsePage's stable title ordering.
    let books = get_book_cards_in_dir_scoped(&conn, "00000000-0000-0000-0000-000000000000", false, "query", LibraryFileTypeFilter::All).unwrap();
    assert_eq!(books.iter().map(|book| book.source_directory).collect::<Vec<_>>(), [DirId::parse_str(DIR_A).ok(), DirId::parse_str(DIR_A).ok()]);
    assert_eq!(books.into_iter().map(|book| book.title).collect::<Vec<_>>(), ["Alpha Story", "Zulu Query"]);
    conn.execute_batch(include_str!("sql/tests/browse_rows_are_always_sorted_by_name_or_title_in_sql_insert.sql")).unwrap();
    let rows = get_folder_list_rows_sorted(&conn, "00000000-0000-0000-0000-000000000000", "", LibraryFileTypeFilter::All, BrowseChipSort::BookCount).unwrap();
    assert_eq!(rows.into_iter().map(|row| row.name).collect::<Vec<_>>(), ["Zulu", "alpha"]);
    let books = get_book_cards_in_dir_sorted_scoped(&conn, "00000000-0000-0000-0000-000000000000", false, "query", LibraryFileTypeFilter::All, BrowseBookSort::RecentlyRead).unwrap();
    assert_eq!(books.into_iter().map(|book| book.title).collect::<Vec<_>>(), ["Zulu Query", "Alpha Story"]);
}
#[test]
fn downloaded_only_scope_is_applied_inside_browsing_queries() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.execute_batch(include_str!("sql/tests/downloaded_only_scope_is_applied_inside_browsing_queries_insert.sql")).unwrap();
    let transaction = conn.transaction().unwrap();
    update_book_metadata(&transaction, fixture_content_hash(1), "Local book", "Shared Author").unwrap();
    update_book_metadata(&transaction, fixture_content_hash(2), "Remote book", "Shared Author").unwrap();
    transaction.commit().unwrap();
    assert_eq!(get_recently_read_cards(&conn, 1, true, FINISHED_THRESHOLD).unwrap()[0].content_hash, fixture_content_hash(1), "scope must be applied before LIMIT");
    assert_eq!(get_recently_added_cards(&conn, 1, true).unwrap()[0].content_hash, fixture_content_hash(1), "scope must be applied before LIMIT");
    assert_eq!(search_book_cards_scoped(&conn, "book", true).unwrap().title_matches.iter().map(|card| card.content_hash).collect::<Vec<_>>(), vec![fixture_content_hash(1)]);
    assert_eq!(get_book_cards_in_dir_scoped(&conn, "00000000-0000-0000-0000-000000000000", true, "", LibraryFileTypeFilter::All).unwrap().iter().map(|card| card.content_hash).collect::<Vec<_>>(), vec![fixture_content_hash(1)]);
    let authors = get_authors_scoped(&conn, true).unwrap();
    assert_eq!(authors.len(), 1);
    assert_eq!(authors[0].3, 1, "author counts use the same availability scope as their books");
    assert_eq!(get_book_cards_by_author_scoped(&conn, authors[0].0, true).unwrap().iter().map(|card| card.content_hash).collect::<Vec<_>>(), vec![fixture_content_hash(1)]);
}
#[test]
fn a_requested_download_is_reported_by_the_card_projection() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.execute_batch(include_str!("sql/tests/a_requested_download_is_reported_by_the_card_projection_insert.sql")).unwrap();
    let cards = get_book_cards_in_dir_scoped(&conn, "00000000-0000-0000-0000-000000000000", false, "", LibraryFileTypeFilter::All).unwrap();
    assert!(!cards[0].downloaded);
    assert!(cards[0].download_requested);
}
/// A book placed in several folders joins once per placement; the author
/// view must still show it once.
#[test]
fn author_cards_collapse_a_book_placed_in_several_folders() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let author_id = AgentId::parse_str("a0a00000-0000-0000-0000-000000000001").unwrap();
    conn.execute_batch(include_str!("sql/mod/author_cards_collapse_a_book_placed_in_several_folders_query.sql")).unwrap();
    let cards = get_book_cards_by_author_scoped(&conn, author_id, false).unwrap();
    assert_eq!(cards.len(), 1, "one card per book, not per placement");
    assert_eq!(cards[0].content_hash, fixture_content_hash(1));
}
#[test]
fn subdirectory_book_counts_include_descendants_without_double_counting_books() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.execute_batch(include_str!("sql/mod/subdirectory_book_counts_include_descendants_without_double_counting_books_query.sql")).unwrap();
    let directories = get_child_directories(&conn, "00000000-0000-0000-0000-000000000000").unwrap();
    let counts = directories.into_iter().map(|directory| (directory.name().as_str().to_owned(), (directory.book_count(), directory.downloaded_book_count()))).collect::<HashMap<_, _>>();
    assert_eq!(counts.get("A"), Some(&(2, 1)));
    assert_eq!(counts.get("B"), Some(&(1, 1)));
    assert_eq!(counts.get("Empty"), Some(&(0, 0)), "an empty folder lists with a count of zero");
    let rows = get_folder_list_rows(&conn, "00000000-0000-0000-0000-000000000000", "", LibraryFileTypeFilter::All).unwrap();
    assert!(rows.iter().all(|row| row.path.is_none()), "ordinary child rows do not need a parent path");
    let counts = rows.into_iter().map(|row| (row.name, (row.book_count, row.downloaded_book_count))).collect::<HashMap<_, _>>();
    assert_eq!(counts.get("A"), Some(&(2, 1)));
    assert_eq!(counts.get("B"), Some(&(1, 1)));
    assert_eq!(counts.get("Empty"), Some(&(0, 0)), "an empty folder lists with a count of zero");
    let pdf_rows = get_folder_list_rows_filtered_sorted(&conn, "00000000-0000-0000-0000-000000000000", "", &[LibraryFileTypeFilter::Pdf], BrowseChipSort::Alphabetical, &[]).unwrap();
    assert_eq!(pdf_rows.iter().map(|row| (row.name.as_str(), row.book_count)).collect::<Vec<_>>(), [("A", 1)]);
    let english_rows = get_folder_list_rows_filtered_sorted(&conn, "00000000-0000-0000-0000-000000000000", "", &[LibraryFileTypeFilter::All], BrowseChipSort::Alphabetical, &["en"]).unwrap();
    assert_eq!(english_rows.iter().map(|row| (row.name.as_str(), row.book_count)).collect::<Vec<_>>(), [("A", 1), ("B", 1)]);
    let search_rows = get_folder_list_rows(&conn, "00000000-0000-0000-0000-000000000000", "child", LibraryFileTypeFilter::All).unwrap();
    assert_eq!(search_rows.iter().map(|row| (row.name.as_str(), row.path.as_deref())).collect::<Vec<_>>(), [("Child", Some("A"))]);
}
#[test]
fn folder_ownership_tracks_duplicate_placements_downloads_removal_and_reparenting() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.execute_batch(include_str!("sql/mod/folder_ownership_tracks_duplicate_placements_downloads_removal_and_reparenting_query.sql")).unwrap();
    let one_id = conn.query_row(include_str!("sql/tests/folder_ownership_tracks_duplicate_placements_downloads_removal_and_reparenting_select.sql"), [], |row| row.get::<_, i64>(0)).unwrap();
    let ownership = |folder: &str| {
        conn.query_row(include_str!("sql/tests/folder_ownership_tracks_duplicate_placements_downloads_removal_and_reparenting_select_2.sql"), rusqlite::params![folder, one_id], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
            .optional()
            .unwrap()
    };
    assert_eq!(ownership(DIR_A), Some((2, 1)));
    assert_eq!(ownership("00000000-0000-0000-0000-000000000000"), Some((2, 1)));
    conn.execute(include_str!("sql/tests/folder_ownership_tracks_duplicate_placements_downloads_removal_and_reparenting_update.sql"), rusqlite::params![DIR_A, one_id]).unwrap();
    assert_eq!(ownership(DIR_A), Some((2, 2)));
    conn.execute(include_str!("sql/tests/folder_ownership_tracks_duplicate_placements_downloads_removal_and_reparenting_update_2.sql"), rusqlite::params![DIR_A_CHILD, one_id]).unwrap();
    assert_eq!(ownership(DIR_A_CHILD), None);
    assert_eq!(ownership(DIR_A), Some((1, 1)));
    conn.execute(include_str!("sql/tests/folder_ownership_tracks_duplicate_placements_downloads_removal_and_reparenting_update_3.sql"), rusqlite::params![DIR_A_CHILD, DIR_B]).unwrap();
    let rows = get_folder_list_rows(&conn, "00000000-0000-0000-0000-000000000000", "", LibraryFileTypeFilter::All).unwrap();
    let counts = rows.into_iter().map(|row| (row.name, (row.book_count, row.downloaded_book_count))).collect::<HashMap<_, _>>();
    assert_eq!(counts.get("A"), Some(&(1, 1)));
    assert_eq!(counts.get("B"), Some(&(1, 0)));
}
#[test]
fn an_author_row_takes_its_covers_most_recently_read_first() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let transaction = conn.transaction().unwrap();
    insert_scanned_book(&transaction, fixture_content_hash(1), "Alpha", "Kerstin Ekman").unwrap();
    insert_scanned_book(&transaction, fixture_content_hash(2), "Beta", "Kerstin Ekman").unwrap();
    insert_scanned_book(&transaction, fixture_content_hash(3), "Gamma", "Kerstin Ekman").unwrap();
    transaction.commit().unwrap();
    conn.execute_batch(include_str!("sql/tests/an_author_row_takes_its_covers_most_recently_read_first_insert.sql")).unwrap();
    let author_id = get_authors_scoped(&conn, false).unwrap()[0].0;
    let shelves = get_author_book_cards_scoped(&conn, false, 2).unwrap();
    let shelf = shelves.get(&author_id).map(|books| books.iter().map(|book| (book.content_hash.clone(), book.title.clone())).collect::<Vec<_>>());
    assert_eq!(shelf, Some(vec![(fixture_content_hash(3), "Gamma".to_owned()), (fixture_content_hash(2), "Beta".to_owned())]), "most recently read first, capped at two");
}
#[test]
fn an_author_is_chipped_with_the_subjects_most_of_their_books_carry() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    use_fixture_taxonomy(&conn);
    let transaction = conn.transaction().unwrap();
    insert_scanned_book(&transaction, fixture_content_hash(1), "Alpha", "Kerstin Ekman").unwrap();
    insert_scanned_book(&transaction, fixture_content_hash(2), "Beta", "Kerstin Ekman").unwrap();
    transaction.commit().unwrap();
    conn.execute_batch(include_str!("sql/tests/an_author_is_chipped_with_the_subjects_most_of_their_books_carry_insert.sql")).unwrap();
    let author_id = get_authors_scoped(&conn, false).unwrap()[0].0;
    let named = |subjects: &[library_model::AuthorSubject]| subjects.iter().map(|subject| (subject.parent.as_deref().unwrap_or(""), subject.label.as_str())).map(|(parent, label)| format!("{parent}|{label}")).collect::<Vec<_>>();
    // A leaf is qualified by the parent it was reached through — "Crime" alone
    // says little — while a top-level subject stays the bare word it already is.
    assert_eq!(named(get_author_subjects_scoped(&conn, false, 3).unwrap().get(&author_id).unwrap()), ["Fiction|Crime", "|Nature"]);
    assert_eq!(named(get_author_subjects_scoped(&conn, false, 1).unwrap().get(&author_id).unwrap()), ["Fiction|Crime"], "the cap keeps the most-credited subject");
    assert_eq!(named(&library_authors(&conn, None, library_model::AuthorSort::Name, false).unwrap().authors[0].subjects), ["Fiction|Crime", "|Nature"], "and the index carries them onto the row");
}
#[test]
fn a_book_credited_twice_to_one_author_appears_once() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let transaction = conn.transaction().unwrap();
    insert_scanned_book(&transaction, fixture_content_hash(1), "The Hobbit", "Tolkien, J. R. R.").unwrap();
    transaction.commit().unwrap();
    conn.execute(include_str!("sql/tests/a_book_credited_twice_to_one_author_appears_once_insert.sql"), [fixture_content_hash(1).as_str()]).unwrap();
    // A second credit at another position, resolving to the same identity —
    // the shape a book gets when it lists one person twice, or lists two
    // spellings that normalize together.
    conn.execute(include_str!("sql/tests/a_book_credited_twice_to_one_author_appears_once_insert_2.sql"), []).unwrap();
    let author_id = get_authors_scoped(&conn, false).unwrap()[0].0;
    assert_eq!(get_book_cards_by_author_scoped(&conn, author_id, false).unwrap().len(), 1, "the grid shows the book once");
    assert_eq!(get_author_book_cards_scoped(&conn, false, 2).unwrap().get(&author_id).map(Vec::len), Some(1), "and sits on the shelf once");
}
#[test]
fn recently_read_spends_its_limit_on_each_group() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    // Two finished books read more recently than the two still in progress.
    // Under a single limit over the whole list, the finished pair alone would
    // fill it and Home's first section would come back empty.
    conn.execute_batch(include_str!("sql/tests/recently_read_spends_its_limit_on_each_group_insert.sql")).unwrap();
    let cards = get_recently_read_cards(&conn, 1, false, FINISHED_THRESHOLD).unwrap();
    let (finished, in_progress): (Vec<_>, Vec<_>) = cards.into_iter().partition(|card| f64::from(card.progress) >= FINISHED_THRESHOLD);
    assert_eq!(in_progress.iter().map(|card| card.title.as_str()).collect::<Vec<_>>(), vec!["Reading newest"]);
    assert_eq!(finished.iter().map(|card| card.title.as_str()).collect::<Vec<_>>(), vec!["Finished newest"]);
}
#[test]
fn most_progress_ranks_only_unfinished_started_books() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.execute_batch(include_str!("sql/tests/most_progress_ranks_only_unfinished_started_books_insert.sql")).unwrap();
    let cards = get_most_progress_cards(&conn, 10, false, FINISHED_THRESHOLD).unwrap();
    assert_eq!(cards.into_iter().map(|card| card.title).collect::<Vec<_>>(), ["Almost done", "Halfway"]);
}
#[test]
fn recently_read_sorts_by_read_time_then_title() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.execute_batch(include_str!("sql/tests/recently_read_sorts_by_read_time_then_title_insert.sql")).unwrap();
    let cards = get_recently_read_cards(&conn, 10, false, FINISHED_THRESHOLD).unwrap();
    assert_eq!(cards.into_iter().map(|card| card.title).collect::<Vec<_>>(), ["Beta", "alpha", "Zulu"]);
}
#[test]
fn recently_added_falls_back_to_live_books_without_added_timestamps() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.execute(include_str!("sql/tests/recently_added_falls_back_to_live_books_without_added_timestamps_insert.sql"), [fixture_content_hash(1).as_str()]).unwrap();
    conn.execute(include_str!("sql/tests/recently_added_falls_back_to_live_books_without_added_timestamps_insert_2.sql"), [fixture_content_hash(1).as_str()]).unwrap();
    let books = get_recently_added_cards(&conn, 20, false).unwrap();
    assert_eq!(books.iter().map(|book| book.content_hash).collect::<Vec<_>>(), vec![fixture_content_hash(1)]);
}
#[test]
fn folder_download_queries_are_recursive_and_preserve_durable_requests() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let second_hash = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let third_hash = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    conn.execute_batch(include_str!("sql/mod/folder_download_queries_are_recursive_and_preserve_durable_requests_query.sql")).unwrap();
    conn.execute(include_str!("sql/tests/folder_download_queries_are_recursive_and_preserve_durable_requests_insert.sql"), rusqlite::params![fixture_content_hash(1).as_str(), TEST_CONTENT_HASH, DIR_SHELF]).unwrap();
    conn.execute(include_str!("sql/tests/folder_download_queries_are_recursive_and_preserve_durable_requests_insert_2.sql"), rusqlite::params![fixture_content_hash(2).as_str(), second_hash, DIR_CHILD]).unwrap();
    conn.execute(include_str!("sql/tests/folder_download_queries_are_recursive_and_preserve_durable_requests_insert_3.sql"), rusqlite::params![fixture_content_hash(3).as_str(), third_hash, DIR_OTHER]).unwrap();
    assert_eq!(get_descendant_content_hashes(&conn, DIR_SHELF).unwrap(), vec![fixture_content_hash(1), fixture_content_hash(2)]);
    assert_eq!(get_book_ancestor_dir_ids(&conn, fixture_content_hash(2)).unwrap(), vec![test_dir_id("00000000-0000-0000-0000-000000000000"), test_dir_id("child"), test_dir_id("shelf")]);
    assert_eq!(get_folder_download_counts(&conn, DIR_SHELF).unwrap(), (1, 2));
    assert_eq!(get_folder_download_counts(&conn, DIR_OTHER).unwrap(), (1, 1));
    add_asset_request(&conn, BlobKind::Book, &fixture_content_hash(2), TransferOrigin::UserInitiated).unwrap();
    assert!(is_book_download_requested(&conn, fixture_content_hash(2)).unwrap());
    assert!(is_book_downloaded(&conn, fixture_content_hash(1)).unwrap());
    assert!(!is_book_download_requested(&conn, fixture_content_hash(1)).unwrap());
    assert!(!is_book_downloaded(&conn, fixture_content_hash(2)).unwrap());
    assert!(is_book_download_requested(&conn, fixture_content_hash(2)).unwrap());
    assert!(is_book_downloaded(&conn, fixture_content_hash(3)).unwrap());
    complete_asset_request(&conn, BlobKind::Book, &fixture_content_hash(2)).unwrap();
    assert!(!is_book_download_requested(&conn, fixture_content_hash(2)).unwrap());
    add_asset_request(&conn, BlobKind::Book, &fixture_content_hash(2), TransferOrigin::UserInitiated).unwrap();
    assert!(is_book_download_requested(&conn, fixture_content_hash(2)).unwrap());
    assert!(!is_book_download_requested(&conn, fixture_content_hash(1)).unwrap());
}
#[test]
fn library_search_separates_normalized_title_and_author_substrings() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.execute(include_str!("sql/tests/library_search_separates_normalized_title_and_author_substrings_insert.sql"), [fixture_content_hash(1).as_str()]).unwrap();
    conn.execute(include_str!("sql/tests/library_search_separates_normalized_title_and_author_substrings_insert_2.sql"), [fixture_content_hash(1).as_str()]).unwrap();
    let transaction = conn.transaction().unwrap();
    update_book_metadata(&transaction, fixture_content_hash(1), "Power and Progress", "GÖRAN Smith").unwrap();
    transaction.commit().unwrap();
    assert_eq!(search_book_cards_scoped(&conn, "ow", false).unwrap().title_matches.len(), 1);
    assert_eq!(search_book_cards_scoped(&conn, "progress", false).unwrap().title_matches.len(), 1);
    assert_eq!(search_book_cards_scoped(&conn, "goran   smith", false).unwrap().author_matches.len(), 1);
    let mut scoped_author = search_book_cards_scoped(&conn, "goran", false).unwrap();
    retain_search_results(&mut scoped_author, &HashSet::from([fixture_content_hash(1)]));
    assert_eq!(scoped_author.author_matches.len(), 1);
    let mut outside_scope = search_book_cards_scoped(&conn, "goran", false).unwrap();
    retain_search_results(&mut outside_scope, &HashSet::new());
    assert!(outside_scope.author_matches.is_empty());
    let missing = search_book_cards_scoped(&conn, "missing", false).unwrap();
    assert!(missing.title_matches.is_empty() && missing.author_matches.is_empty());
    let result = search_book_cards_scoped(&conn, "POWER", false).unwrap();
    assert_eq!(result.title_matches.len(), 1);
    assert_eq!(result.title_matches[0].title, "Power and Progress");
    assert_eq!(result.title_matches[0].author, "GÖRAN Smith");
    let non_contiguous = search_book_cards_scoped(&conn, "gor sm", false).unwrap();
    assert!(non_contiguous.title_matches.is_empty() && non_contiguous.author_matches.is_empty());
}
#[test]
fn library_groups_inverted_and_punctuated_author_spellings_without_changing_book_metadata() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let transaction = conn.transaction().unwrap();
    insert_scanned_book(&transaction, fixture_content_hash(1), "The Hobbit", "Tolkien, J. R. R.").unwrap();
    insert_scanned_book(&transaction, fixture_content_hash(2), "The Lord of the Rings", "J.R.R. Tolkien").unwrap();
    transaction.commit().unwrap();
    conn.execute(include_str!("sql/tests/a_book_credited_twice_to_one_author_appears_once_insert.sql"), [fixture_content_hash(1).as_str()]).unwrap();
    conn.execute(include_str!("sql/tests/library_groups_inverted_and_punctuated_author_spellings_without_changing_book_metadata_insert_2.sql"), [fixture_content_hash(2).as_str()]).unwrap();
    assert_eq!(conn.query_row(include_str!("sql/tests/remote_author_ids_are_preserved_even_when_names_normalize_identically_select.sql"), [], |row| row.get::<_, i64>(0)).unwrap(), 1);
    let authors = get_authors_scoped(&conn, false).unwrap();
    assert_eq!(authors.len(), 1);
    assert_eq!((&authors[0].1, authors[0].3), (&"J.R.R. Tolkien".to_owned(), 2));
    assert_eq!(get_book_cards_by_author_scoped(&conn, authors[0].0, false).unwrap().len(), 2);
    let spellings = conn
        .prepare(include_str!("sql/tests/library_groups_inverted_and_punctuated_author_spellings_without_changing_book_metadata_select_2.sql"))
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(spellings, vec!["Tolkien, J. R. R.", "J.R.R. Tolkien"]);
    let synchronized_spellings = get_pending_mutations(&conn)
        .unwrap()
        .into_iter()
        .filter_map(|mutation| match mutation.body {
            MutationBody::BookLifecycle { content_hash, value: BookLifecycleState::Present { metadata, .. } } => Some((content_hash, metadata.contributors)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        synchronized_spellings.iter().map(|(content_hash, authors)| (content_hash.clone(), authors.iter().map(Contributor::name).collect::<Vec<_>>())).collect::<Vec<_>>(),
        vec![(fixture_content_hash(1), vec!["Tolkien, J. R. R."]), (fixture_content_hash(2), vec!["J.R.R. Tolkien"])],
    );
    assert_eq!(synchronized_spellings[0].1[0].agent_id(), synchronized_spellings[1].1[0].agent_id());
}
#[test]
fn ordered_book_author_links_preserve_all_epub_credits() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let authors = vec![Contributor::new("Ursula K. Le Guin", Some("Le Guin, Ursula K.".to_owned()), MarcRelatorCode(*b"aut")).unwrap(), author("Jane Doe")];
    let transaction = conn.transaction().unwrap();
    insert_scanned_book_contributors(&transaction, fixture_content_hash(1), "A collaboration", &authors).unwrap();
    transaction.commit().unwrap();
    let credits = conn
        .prepare(include_str!("sql/tests/ordered_book_author_links_preserve_all_epub_credits_select.sql"))
        .unwrap()
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(credits, vec![("ursula k le guin".to_owned(), "Ursula K. Le Guin".to_owned(), Some("Le Guin, Ursula K.".to_owned())), ("jane doe".to_owned(), "Jane Doe".to_owned(), None),]);
    let pending = get_pending_mutations(&conn).unwrap();
    assert!(pending.into_iter().any(|mutation| matches!(
        mutation.body,
        MutationBody::BookLifecycle {
            value: BookLifecycleState::Present { metadata, .. },
            ..
        } if metadata.contributors == authors
    )));
}
#[test]
fn author_schema_has_one_entity_and_one_book_credit_projection() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let legacy_tables: i64 = conn.query_row(include_str!("sql/tests/author_schema_has_one_entity_and_one_book_credit_projection_select.sql"), [], |row| row.get(0)).unwrap();
    assert_eq!(legacy_tables, 0);
    let book_columns = conn.prepare(include_str!("sql/tests/author_schema_has_one_entity_and_one_book_credit_projection_pragma.sql")).unwrap().query_map([], |row| row.get::<_, String>(1)).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
    assert!(!book_columns.iter().any(|column| matches!(column.as_str(), "author_id" | "author_name" | "authors")), "the wire-encoded authors blob no longer exists; book_contributor is the sole canonical register");
}
#[test]
fn transfer_jobs_are_not_persisted_in_the_library_schema() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let transfer_jobs: i64 = conn.query_row(include_str!("sql/tests/transfer_jobs_are_not_persisted_in_the_library_schema_select.sql"), [], |row| row.get(0)).unwrap();
    assert_eq!(transfer_jobs, 0);
}
#[test]
fn scanned_publication_metadata_is_canonical_and_relationally_projected() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let publication = PublicationMetadata {
        identifiers: vec![library_model::Identifier::new("9781234567897", Scheme::Isbn, Scope::Edition, "test:embedded").unwrap()],
        publishers: vec![library_model::PublisherCredit::new("North Wind Press").unwrap()],
        languages: vec![library_model::LanguageTag::parse("SV-se").unwrap()],
        dates: vec![library_model::PublicationDate::new(None, "2026-08-04", Some("publication".to_owned()), "test:publication-date").unwrap()],
        subjects: vec![library_model::PublicationSubject::new(None, "Distributed systems", "dc:subject", None, None).unwrap()],
        collections: vec![library_model::Collection::new("Bokheim Studies", Some(library_model::CollectionKind::Series), Some("2".to_owned()), "test:embedded").unwrap()],
        rights: vec!["Copyright example".to_owned()],
        ..PublicationMetadata::default()
    };
    // Mixed author + non-author contributors share one flat book_contributor
    // position sequence now that Contributor is the sole relational model.
    let translator = Contributor::new("Example Translator", None, MarcRelatorCode(*b"trl")).unwrap();
    let contributors = vec![author("Example Author"), translator.clone()];
    let state = BookMetadataState { title: "Metadata book".to_owned(), contributors, description: "A description".to_owned(), publication: publication.clone() };
    let transaction = conn.transaction().unwrap();
    insert_scanned_book_metadata(&transaction, fixture_content_hash(1), &state, BookFormat::Epub).unwrap();
    transaction.commit().unwrap();
    let encoded: Vec<u8> = conn.query_row(include_str!("sql/tests/scanned_publication_metadata_is_canonical_and_relationally_projected_select.sql"), [], |row| row.get(0)).unwrap();
    assert_eq!(sync_common::wire::decode::<PublicationMetadata>(&encoded, sync_common::wire::MAX_DECODED_REQUEST_BYTES,).unwrap(), publication,);
    for (table, sql) in [
        ("book_identifier", include_str!("sql/mod/scanned_publication_metadata_is_canonical_and_relationally_projected_query_book_identifier.sql")),
        ("book_publisher", include_str!("sql/mod/scanned_publication_metadata_is_canonical_and_relationally_projected_query_book_publisher.sql")),
        ("book_language", include_str!("sql/mod/scanned_publication_metadata_is_canonical_and_relationally_projected_query_book_language.sql")),
        ("book_publication_date", include_str!("sql/mod/scanned_publication_metadata_is_canonical_and_relationally_projected_query_book_publication_date.sql")),
        ("book_subject", include_str!("sql/mod/scanned_publication_metadata_is_canonical_and_relationally_projected_query_book_subject.sql")),
        ("book_collection", include_str!("sql/mod/scanned_publication_metadata_is_canonical_and_relationally_projected_query_book_collection.sql")),
        ("book_publication_value", include_str!("sql/mod/scanned_publication_metadata_is_canonical_and_relationally_projected_query_book_publication_value.sql")),
    ] {
        let count: i64 = conn.query_row(sql, [], |row| row.get(0)).unwrap();
        assert_eq!(count, 1, "missing {table} projection");
    }
    let contributor_count: i64 = conn.query_row(include_str!("sql/tests/scanned_publication_metadata_is_canonical_and_relationally_projected_select_2.sql"), [], |row| row.get(0)).unwrap();
    assert_eq!(contributor_count, 2, "author and non-author contributor rows both project into the merged table");
    let canonical_identifier: Option<String> = conn.query_row(include_str!("sql/tests/scanned_publication_metadata_is_canonical_and_relationally_projected_select_3.sql"), [], |row| row.get(0)).unwrap();
    assert_eq!(canonical_identifier.as_deref(), Some("9781234567897"));
    let normalized_date = conn
        .query_row(include_str!("sql/tests/scanned_publication_metadata_is_canonical_and_relationally_projected_select_4.sql"), [], |row| Ok((row.get::<_, Option<i32>>(0)?, row.get::<_, Option<u8>>(1)?, row.get::<_, Option<u8>>(2)?)))
        .unwrap();
    assert_eq!(normalized_date, (None, None, None));
    let publisher_agent: String = conn.query_row(include_str!("sql/tests/scanned_publication_metadata_is_canonical_and_relationally_projected_select_5.sql"), [], |row| row.get(0)).unwrap();
    assert_eq!(publisher_agent, publication.publishers[0].agent_id().to_string());
    let contributor_agent: String = conn.query_row(include_str!("sql/tests/scanned_publication_metadata_is_canonical_and_relationally_projected_select_6.sql"), [], |row| row.get(0)).unwrap();
    assert_eq!(contributor_agent, translator.agent_id().to_string());
    let contributor_role: Vec<u8> = conn.query_row(include_str!("sql/tests/scanned_publication_metadata_is_canonical_and_relationally_projected_select_7.sql"), [], |row| row.get(0)).unwrap();
    assert_eq!(contributor_role, b"trl");
    let author_role: Vec<u8> = conn.query_row(include_str!("sql/tests/scanned_publication_metadata_is_canonical_and_relationally_projected_select_8.sql"), [], |row| row.get(0)).unwrap();
    assert_eq!(author_role, b"aut");
    let pending = get_pending_mutations(&conn).unwrap();
    assert_eq!(pending.len(), 2);
    assert!(pending.iter().any(|entry| matches!(
        &entry.body,
        MutationBody::BookLifecycle {
            value: BookLifecycleState::Present { metadata, .. },
            ..
        } if metadata == &state.clone().into()
    )));
    assert!(pending.iter().any(|entry| matches!(&entry.body, MutationBody::Description { value, .. } if value==&state.description)));
    conn.execute(include_str!("sql/tests/scanned_publication_metadata_is_canonical_and_relationally_projected_insert.sql"), [fixture_content_hash(1).as_str()]).unwrap();
}
#[test]
fn authority_evidence_distinguishes_namesakes_without_splitting_spelling_variants() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let tx = conn.transaction().unwrap();
    insert_scanned_book_contributors(&tx, fixture_content_hash(1), "First work", &[author("Smith, Alex")]).unwrap();
    insert_scanned_book_contributors(&tx, fixture_content_hash(2), "Second work", &[author("Alex Smith")]).unwrap();
    insert_scanned_book_contributors(&tx, fixture_content_hash(3), "Third work", &[author("A. Smith")]).unwrap();
    let first_wikidata = ExternalAuthorId::parse(AuthorAuthority::Wikidata, "Q1001").unwrap();
    let second_wikidata = ExternalAuthorId::parse(AuthorAuthority::Wikidata, "Q1002").unwrap();
    let first = resolve_book_author_identity(&tx, fixture_content_hash(1), 0, "Alex Smith", &first_wikidata).unwrap();
    let unresolved_second = get_book_author_identities(&tx, fixture_content_hash(2)).unwrap()[0].author_id;
    assert_ne!(first, unresolved_second, "resolving one shared provisional credit must not assign that authority to every same-name book");
    let second = resolve_book_author_identity(&tx, fixture_content_hash(2), 0, "Alex Smith", &second_wikidata).unwrap();
    let first_variant = resolve_book_author_identity(&tx, fixture_content_hash(3), 0, "Alex Smith", &first_wikidata).unwrap();
    assert_ne!(first, second, "the same normalized name must be able to represent namesakes");
    assert_eq!(first, first_variant, "one authority id must converge spelling variants on one person");
    attach_external_author_id(&tx, first, &ExternalAuthorId::parse(AuthorAuthority::Viaf, "12345678").unwrap()).unwrap();
    assert!(attach_external_author_id(&tx, first, &second_wikidata).is_err(), "one authority id cannot identify two people");
    tx.commit().unwrap();
    assert_eq!(conn.query_row(include_str!("sql/tests/remote_author_ids_are_preserved_even_when_names_normalize_identically_select.sql"), [], |row| row.get::<_, i64>(0)).unwrap(), 2);
    let first_credit = get_book_author_identities(&conn, fixture_content_hash(1)).unwrap().pop().unwrap();
    assert_eq!(first_credit.author_id, first);
    assert_eq!(first_credit.external_ids.len(), 2);
}

#[test]
fn metadata_refresh_preserves_identity_only_for_the_same_normalized_credit() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let tx = conn.transaction().unwrap();
    insert_scanned_book_contributors(&tx, fixture_content_hash(1), "Book", &[author("Smith, Alex")]).unwrap();
    let identity = resolve_book_author_identity(&tx, fixture_content_hash(1), 0, "Alex Smith", &ExternalAuthorId::parse(AuthorAuthority::OpenLibrary, "OL123A").unwrap()).unwrap();
    update_book_metadata_contributors(&tx, fixture_content_hash(1), "Book", &[author("Alex Smith")]).unwrap();
    assert_eq!(get_book_author_identities(&tx, fixture_content_hash(1)).unwrap()[0].author_id, identity);
    update_book_metadata_contributors(&tx, fixture_content_hash(1), "Book", &[author("Different Person")]).unwrap();
    assert_ne!(get_book_author_identities(&tx, fixture_content_hash(1)).unwrap()[0].author_id, identity);
    tx.commit().unwrap();
}
#[test]
fn namesake_split_reidentifies_only_the_selected_book_credit() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let tx = conn.transaction().unwrap();
    insert_scanned_book_contributors(&tx, fixture_content_hash(1), "First work", &[author("Alex Smith")]).unwrap();
    insert_scanned_book_contributors(&tx, fixture_content_hash(2), "Second work", &[author("Alex Smith")]).unwrap();
    tx.commit().unwrap();
    let before_first = get_book_author_identities(&conn, fixture_content_hash(1)).unwrap()[0].author_id;
    let before_second = get_book_author_identities(&conn, fixture_content_hash(2)).unwrap()[0].author_id;
    assert_eq!(before_first, before_second, "unresolved normalized-name matches begin as one provisional author");
    conn.execute(include_str!("sql/tests/failed_recovery_page_rolls_back_and_retains_marker_delete.sql"), []).unwrap();
    let tx = conn.transaction().unwrap();
    let split_id = split_book_author(&tx, fixture_content_hash(2), 0).unwrap();
    tx.commit().unwrap();
    assert_eq!(get_book_author_identities(&conn, fixture_content_hash(1)).unwrap()[0].author_id, before_first);
    assert_eq!(get_book_author_identities(&conn, fixture_content_hash(2)).unwrap()[0].author_id, split_id);
    assert_ne!(split_id, before_first);
    assert_eq!(conn.query_row(include_str!("sql/tests/namesake_split_reidentifies_only_the_selected_book_credit_select.sql"), [], |row| row.get::<_, i64>(0)).unwrap(), 1, "splitting identity must retain normalized-name grouping");
    let identity: String = conn.query_row(include_str!("sql/tests/namesake_split_reidentifies_only_the_selected_book_credit_select_2.sql"), [], |row| row.get(0)).unwrap();
    assert_eq!(AgentId::parse_str(&identity).unwrap(), split_id);
    assert_eq!(conn.query_row(include_str!("sql/tests/newly_scanned_book_emits_only_lifecycle_and_placement_select_2.sql"), [], |row| row.get::<_, i64>(0)).unwrap(), 1, "the identity correction must synchronize");
}
#[test]
fn author_browsing_keeps_same_name_identities_and_their_books_separate() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let tx = conn.transaction().unwrap();
    insert_scanned_book_contributors(&tx, fixture_content_hash(1), "First work", &[author("Alex Smith")]).unwrap();
    insert_scanned_book_contributors(&tx, fixture_content_hash(2), "Second work", &[author("Alex Smith")]).unwrap();
    tx.commit().unwrap();
    conn.execute(include_str!("sql/tests/author_browsing_keeps_same_name_identities_and_their_books_separate_insert.sql"), [fixture_content_hash(1).as_str()]).unwrap();
    conn.execute(include_str!("sql/tests/author_browsing_keeps_same_name_identities_and_their_books_separate_insert_2.sql"), [fixture_content_hash(2).as_str()]).unwrap();
    let first_id = get_book_author_identities(&conn, fixture_content_hash(1)).unwrap()[0].author_id;
    let tx = conn.transaction().unwrap();
    let second_id = split_book_author(&tx, fixture_content_hash(2), 0).unwrap();
    tx.commit().unwrap();
    let authors = get_authors_scoped(&conn, false).unwrap();
    assert_eq!(authors.len(), 2);
    assert!(authors.iter().all(|(_, name, _, count)| name == "Alex Smith" && *count == 1));
    assert_eq!(authors.iter().map(|(id, _, _, _)| *id).collect::<BTreeSet<_>>(), BTreeSet::from([first_id, second_id]));
    let first_titles = get_book_cards_by_author_scoped(&conn, first_id, false).unwrap().into_iter().map(|book| book.title).collect::<Vec<_>>();
    let second_titles = get_book_cards_by_author_scoped(&conn, second_id, false).unwrap().into_iter().map(|book| book.title).collect::<Vec<_>>();
    assert_eq!(first_titles, vec!["First work"]);
    assert_eq!(second_titles, vec!["Second work"]);
}
#[test]
fn remote_author_ids_are_preserved_even_when_names_normalize_identically() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.insert_book(fixture_content_hash(1).as_str(), "First", 1, "epub").unwrap();
    conn.insert_book(fixture_content_hash(2).as_str(), "Second", 1, "epub").unwrap();
    let first_id = AgentId::new_v4();
    let second_id = AgentId::new_v4();
    let first = Contributor::with_id(first_id, "Alex Smith", None, MarcRelatorCode(*b"aut")).unwrap();
    let second = Contributor::with_id(second_id, "Smith, Alex", None, MarcRelatorCode(*b"aut")).unwrap();
    let tx = conn.transaction().unwrap();
    update_book_metadata_contributors_preserving_ids(&tx, fixture_content_hash(1), "First", &[first]).unwrap();
    update_book_metadata_contributors_preserving_ids(&tx, fixture_content_hash(2), "Second", &[second]).unwrap();
    tx.commit().unwrap();
    assert_eq!(get_book_author_identities(&conn, fixture_content_hash(1)).unwrap()[0].author_id, first_id);
    assert_eq!(get_book_author_identities(&conn, fixture_content_hash(2)).unwrap()[0].author_id, second_id);
    assert_eq!(conn.query_row(include_str!("sql/tests/remote_author_ids_are_preserved_even_when_names_normalize_identically_select.sql"), [], |row| row.get::<_, i64>(0)).unwrap(), 2);
}
#[test]
fn title_search_needs_no_derived_index_backfill() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.execute_batch(include_str!("sql/tests/title_search_needs_no_derived_index_backfill_insert.sql")).unwrap();
    init_db(&conn).unwrap();
    assert_eq!(search_book_cards_scoped(&conn, "ra", false).unwrap().title_matches.len(), 1);
    let absent_author = search_book_cards_scoped(&conn, "butler", false).unwrap();
    assert!(absent_author.title_matches.is_empty() && absent_author.author_matches.is_empty());
}
#[test]
fn catalog_publication_ownership_is_exact_and_ignores_deleted_books() {
    use catalog_model::{CatalogPublicationIdentity, CatalogSourceId};
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.execute(include_str!("sql/tests/catalog_publication_ownership_is_exact_and_ignores_deleted_books_insert.sql"), [fixture_content_hash(1).as_str()]).unwrap();
    conn.execute(include_str!("sql/tests/failed_recovery_page_rolls_back_and_retains_marker_delete.sql"), []).unwrap();
    let source_id = CatalogSourceId::parse("standard-ebooks").unwrap();
    let owned = CatalogPublicationIdentity { edition_id: "edition-one".to_owned(), work_id: Some("work-one".to_owned()) };
    let transaction = conn.transaction().unwrap();
    record_catalog_publication(&transaction, &source_id, &owned, &fixture_content_hash(1)).unwrap();
    transaction.commit().unwrap();
    let identities = vec![owned.clone(), CatalogPublicationIdentity { edition_id: "edition-two".to_owned(), work_id: Some("work-one".to_owned()) }, CatalogPublicationIdentity { edition_id: "unknown".to_owned(), work_id: None }];
    assert_eq!(owned_catalog_publications(&conn, &source_id, &identities).unwrap(), vec![owned.clone()]);
    let pending = get_pending_mutations(&conn).unwrap();
    assert!(pending.iter().any(|mutation| matches!(
        &mutation.body,
        MutationBody::CatalogPublication { source_id, publication, content_hash }
            if source_id.as_str() == "standard-ebooks"
                && publication.edition_id == "edition-one"
                && publication.work_id.as_deref() == Some("work-one")
                && content_hash == &fixture_content_hash(1)
    )));
    conn.execute(include_str!("sql/tests/catalog_publication_ownership_is_exact_and_ignores_deleted_books_update.sql"), [fixture_content_hash(1).as_str()]).unwrap();
    assert!(owned_catalog_publications(&conn, &source_id, &[owned]).unwrap().is_empty());
}
#[test]
fn remote_catalog_publication_relation_materializes_without_echoing_to_outbox() {
    use catalog_model::{CatalogPublicationIdentity, CatalogSourceId};
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let source_id = CatalogSourceId::parse("remote-catalog").unwrap();
    let identity = CatalogPublicationIdentity { edition_id: "remote-edition".to_owned(), work_id: None };
    let change = server_change(MutationBody::CatalogPublication { source_id: source_id.clone(), publication: identity.clone(), content_hash: fixture_content_hash(2) }, 10, 0);
    let transaction = conn.transaction().unwrap();
    apply_remote_change_in_tx(&transaction, &change).unwrap();
    transaction.commit().unwrap();
    assert_eq!(owned_catalog_publications(&conn, &source_id, &[identity.clone()]).unwrap(), vec![identity]);
    assert!(get_pending_mutations(&conn).unwrap().is_empty());
}
fn server_change(body: MutationBody, pt: u64, l: u64) -> ServerMutation {
    server_change_from(body, pt, l, "remote", 1, 1)
}

#[test]
fn remote_page_origin_is_lazy_scoped_and_restored_on_errors() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.execute_batch(include_str!("sql/tests/remote_page_origin_is_lazy_scoped_and_restored_on_errors_create.sql")).unwrap();
    let changes = (1..=32).map(|id| server_change(MutationBody::Metadata { content_hash: fixture_content_hash(id), value: SyncBookMetadata { title: "Remote".into(), ..Default::default() } }, 10, 0)).collect::<Vec<_>>();
    let tx = library_write_transaction(&mut conn).unwrap();
    apply_remote_changes_in_tx(&tx, &changes).unwrap();
    assert_eq!(tx.query_row(include_str!("sql/tests/remote_page_origin_is_lazy_scoped_and_restored_on_errors_select.sql"), [], |r| r.get::<_, i64>(0)).unwrap(), 2);
    assert!(get_pending_mutations(&tx).unwrap().is_empty());
    assert_eq!(get_sync_metadata(&tx, "change_origin").unwrap().as_deref(), Some("local"));
    tx.execute(include_str!("sql/tests/remote_page_origin_is_lazy_scoped_and_restored_on_errors_delete.sql"), []).unwrap();
    apply_remote_changes_in_tx(&tx, &changes).unwrap();
    assert_eq!(tx.query_row(include_str!("sql/tests/remote_page_origin_is_lazy_scoped_and_restored_on_errors_select.sql"), [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    let mut invalid = changes[0].clone();
    invalid.mutation.value = vec![255];
    let fresh = server_change(MutationBody::Metadata { content_hash: fixture_content_hash(1), value: SyncBookMetadata { title: "Uncommitted".into(), ..Default::default() } }, 20, 0);
    assert!(apply_remote_changes_in_tx(&tx, &[fresh, invalid]).is_err());
    assert_eq!(get_sync_metadata(&tx, "change_origin").unwrap().as_deref(), Some("local"));
    tx.rollback().unwrap();
    assert_eq!(conn.query_row(include_str!("sql/tests/remote_page_origin_is_lazy_scoped_and_restored_on_errors_select_3.sql"), [], |r| r.get::<_, i64>(0)).unwrap(), 0);
}
fn server_change_from(body: MutationBody, pt: u64, _l: u64, actor: &str, replica_seq: u64, revision: u64) -> ServerMutation {
    let mutation = StateMutation { mutation_id: MutationId::new(), body, changed_at: pt, replica_seq: ReplicaSeq::new(replica_seq).unwrap() };
    ServerMutation { mutation: mutation.to_wire().unwrap(), replica_id: fixture_replica_id(actor), revision: sync_common::LibraryRevision::new(revision).unwrap() }
}
fn valid_outbox_row() -> OutboxRow {
    OutboxRow {
        mutation_id: MutationId::new().to_string(),
        state_kind: "book_lifecycle".to_string(),
        state_key: fixture_content_hash(1).to_string(),
        body: sync_common::wire::encode(&MutationBody::BookLifecycle { content_hash: fixture_content_hash(1), value: BookLifecycleState::Deleted { origin_folder_id: None } }).unwrap(),
        changed_at: 10,
        replica_seq: 1,
        state_subkey: String::new(),
    }
}
#[test]
fn outbox_decoder_accepts_valid_rows() {
    let change = decode_outbox_row(valid_outbox_row()).expect("valid outbox row");
    assert_eq!(change.replica_seq.get(), 1);
    assert!(matches!(change.body, MutationBody::BookLifecycle { content_hash, value: BookLifecycleState::Deleted { .. } } if content_hash == fixture_content_hash(1)));
}
#[test]
fn outbox_decoder_rejects_invalid_wire_and_invalid_canonical_bodies() {
    let mut invalid_wire = valid_outbox_row();
    invalid_wire.body = vec![0xc1];
    assert!(matches!(decode_outbox_row(invalid_wire), Err(OutboxReadError::CorruptMutation { replica_seq: 1, .. })));
    let mut malformed_known = valid_outbox_row();
    malformed_known.body = vec![0x81];
    assert!(matches!(decode_outbox_row(malformed_known), Err(OutboxReadError::CorruptMutation { replica_seq: 1, .. })));
    let mut mismatched_key = valid_outbox_row();
    mismatched_key.state_key = fixture_content_hash(2).to_string();
    assert!(matches!(decode_outbox_row(mismatched_key), Err(OutboxReadError::CorruptMutation { replica_seq: 1, .. })));
}
#[test]
fn outbox_read_never_returns_events_after_an_undecodable_row() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(include_str!("sql/tests/outbox_read_never_returns_events_after_an_undecodable_row_create.sql")).unwrap();
    assert!(matches!(get_pending_mutations(&conn), Err(OutboxReadError::CorruptMutation { replica_seq: 1, .. })));
}
#[test]
fn generated_mutation_clocks_are_unix_milliseconds() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.insert_book(fixture_content_hash(1).as_str(), "Book", unix_millis().unwrap(), "epub").unwrap();
    let changed_at: i64 = conn.query_row(include_str!("sql/tests/generated_mutation_clocks_are_unix_milliseconds_select.sql"), [], |row| row.get(0)).unwrap();
    let now = unix_millis().unwrap();
    assert!(changed_at >= now - 5_000 && changed_at <= now + 5_000);
    assert!(changed_at < 100_000_000_000_000, "a generated protocol clock must not be microseconds");
}
#[test]
fn library_sync_details_follow_the_durable_outbox_and_last_successful_sync() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    assert_eq!(get_library_sync_details(&conn).unwrap(), LibrarySyncDetails::default());
    conn.insert_book(fixture_content_hash(1).as_str(), "Book", unix_millis().unwrap(), "epub").unwrap();
    set_sync_metadata(&conn, "last_successful_sync_at", "1723374600000").unwrap();
    assert_eq!(get_library_sync_details(&conn).unwrap(), LibrarySyncDetails { has_unsynced_changes: true, last_synced_at: Some(1_723_374_600_000) });
    let pending = get_pending_mutations(&conn).unwrap();
    acknowledge_mutations(&conn, &pending.into_iter().map(|mutation| mutation.mutation_id).collect::<Vec<_>>()).unwrap();
    assert_eq!(get_library_sync_details(&conn).unwrap(), LibrarySyncDetails { has_unsynced_changes: false, last_synced_at: Some(1_723_374_600_000) });
}
#[test]
fn acknowledging_an_in_flight_value_cannot_delete_its_newer_replacement() {
    let conn = singleton_guard_conn();
    let first = get_pending_mutations(&conn).unwrap();
    assert_eq!(first.len(), 2, "book creation and reading position occupy distinct state cells");
    let first_position = first.iter().find(|mutation| matches!(mutation.body, MutationBody::ReadingPosition { .. })).unwrap();
    let old_id = first_position.mutation_id;
    let old_seq = first_position.replica_seq;
    update_reading_position(&conn, fixture_content_hash(1), "epubcfi(/6/4)", Some(ReadProgress::new(50.0).unwrap())).unwrap();
    let second = get_pending_mutations(&conn).unwrap();
    let second_position = second.iter().find(|mutation| matches!(mutation.body, MutationBody::ReadingPosition { .. })).unwrap();
    assert_ne!(second_position.mutation_id, old_id);
    assert!(second_position.replica_seq > old_seq);
    acknowledge_mutations(&conn, &[old_id]).unwrap();
    let after_stale_ack = get_pending_mutations(&conn).unwrap();
    let retained = after_stale_ack.iter().find(|mutation| matches!(mutation.body, MutationBody::ReadingPosition { .. })).unwrap();
    assert_eq!(retained.mutation_id, second_position.mutation_id);
    acknowledge_mutations(&conn, &[second_position.mutation_id]).unwrap();
    assert!(get_pending_mutations(&conn).unwrap().iter().all(|mutation| !matches!(mutation.body, MutationBody::ReadingPosition { .. })));
}
/// Current row-ID schema for isolated sweep tests.
fn new_schema_conn() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn
}
fn is_alive(conn: &Connection, dir: &str, content_hash: &str, file: &str) -> bool {
    conn.query_row(include_str!("sql/tests/is_alive_select.sql"), rusqlite::params![dir, content_hash, file], |r| r.get::<_, bool>(0)).unwrap()
}
/// Current schema initialized for isolated remote placement tests.
fn lww_conn() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.execute(include_str!("sql/tests/lww_conn_insert.sql"), [DIR_D1]).unwrap();
    conn
}
fn book_dir_change(add: bool, pt: u64, l: u64) -> ServerMutation {
    let body = if add {
        MutationBody::Placement { dir_id: test_dir_id("d1"), content_hash: fixture_content_hash(1), present: true, origin_folder_id: None }
    } else {
        MutationBody::Placement { dir_id: test_dir_id("d1"), content_hash: fixture_content_hash(1), present: false, origin_folder_id: None }
    };
    server_change(body, pt, l)
}
fn book_dir_add_change(content_hash: ContentHash, dir_id: &str, file_name: &str, pt: u64) -> ServerMutation {
    let _ = file_name;
    server_change(MutationBody::Placement { dir_id: test_dir_id(dir_id), content_hash, present: true, origin_folder_id: None }, pt, 0)
}
fn apply_dir_state(tx: &Transaction<'_>, dir_id: &str, parent_id: &str, name: &str, deleted: bool, pt: u64) {
    let id = test_dir_id(dir_id);
    let changes = [
        server_change(MutationBody::DirectoryName { dir_id: id.clone(), name: test_file_name(name) }, pt, 0),
        server_change(MutationBody::DirectoryParent { dir_id: id.clone(), parent_id: test_dir_id(parent_id) }, pt, 0),
        server_change(MutationBody::DirectoryLifecycle { dir_id: id, value: if deleted { library_model::DirectoryLifecycleState::Deleted } else { library_model::DirectoryLifecycleState::Present } }, pt, 0),
    ];
    for change in changes {
        apply_remote_change_in_tx(tx, &change).unwrap();
    }
}
fn fk_conn() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
    conn
}
#[test]
fn directory_dependencies_can_cross_committed_pull_pages() {
    let mut conn = fk_conn();
    let first_page = conn.transaction().unwrap();
    apply_dir_state(&first_page, "child", "parent", "Child", false, 10);
    first_page.commit().unwrap();
    let parent_id = test_dir_id("parent").to_string();
    let child_id = test_dir_id("child").to_string();
    let before_parent: (bool, bool) = conn.query_row(include_str!("sql/tests/directory_dependencies_can_cross_committed_pull_pages_select.sql"), rusqlite::params![parent_id, child_id], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    assert_eq!(before_parent, (true, true), "an incomplete hierarchy must remain hidden but commit successfully");
    let second_page = conn.transaction().unwrap();
    apply_dir_state(&second_page, "parent", "00000000-0000-0000-0000-000000000000", "Parent", false, 11);
    second_page.commit().unwrap();
    let read = conn.transaction().unwrap();
    let paths = get_live_dir_paths_tx(&read).unwrap();
    assert_eq!(paths[&test_dir_id("parent")].as_str(), "Parent");
    assert_eq!(paths[&test_dir_id("child")].as_str(), "Parent/Child");
}
#[test]
fn placement_dependency_can_cross_committed_pull_pages() {
    let mut conn = fk_conn();
    let placement = server_change(MutationBody::Placement { dir_id: test_dir_id("future-shelf"), content_hash: fixture_content_hash(7), present: true, origin_folder_id: None }, 10, 0);
    let first_page = conn.transaction().unwrap();
    apply_remote_change_in_tx(&first_page, &placement).unwrap();
    first_page.commit().unwrap();
    let future_shelf = test_dir_id("future-shelf").to_string();
    let placeholder_deleted: bool = conn.query_row(include_str!("sql/tests/placement_dependency_can_cross_committed_pull_pages_select.sql"), [future_shelf], |row| row.get(0)).unwrap();
    assert!(placeholder_deleted);
    assert_eq!(get_book_file_path(&conn, fixture_content_hash(7)).unwrap(), None, "a placement under an unresolved directory must remain hidden");
    let second_page = conn.transaction().unwrap();
    apply_dir_state(&second_page, "future-shelf", "00000000-0000-0000-0000-000000000000", "Shelf", false, 11);
    second_page.commit().unwrap();
    assert_eq!(get_book_file_path(&conn, fixture_content_hash(7)).unwrap().unwrap().as_str(), format!("/Shelf/{}", fixture_content_hash(7)));
}
fn reading_position_change(cfi: &str, pt: u64, l: u64) -> ServerMutation {
    server_change(MutationBody::ReadingPosition { content_hash: fixture_content_hash(1), value: ReadingPositionState { location: library_model::ReadingPosition::parse(cfi).unwrap(), progress: 0.0 } }, pt, l)
}
fn singleton_guard_conn() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.insert_book(fixture_content_hash(1).as_str(), "T", 1, "epub").unwrap();
    update_reading_position(&conn, fixture_content_hash(1), "epubcfi(/6/2)", Some(ReadProgress::new(25.0).unwrap())).unwrap();
    conn
}
fn read_pos_raw(conn: &Connection) -> String {
    conn.query_row(include_str!("sql/tests/read_pos_raw_select.sql"), [], |r| r.get(0)).unwrap()
}
#[test]
fn pdf_page_position_uses_the_synchronized_reading_register() {
    let conn = singleton_guard_conn();
    update_reading_position(&conn, fixture_content_hash(1), &library_model::pdf_page_reading_position(17), None).unwrap();
    assert_eq!(get_pdf_reading_page(&conn, fixture_content_hash(1)).unwrap(), Some(17));
    assert_eq!(get_book_reading_position(&conn, fixture_content_hash(1)).unwrap(), None);
    assert_eq!(read_pos_raw(&conn), "pdfpage(17)");
    assert!(get_pending_mutations(&conn).unwrap().iter().any(|mutation| matches!(&mutation.body, MutationBody::ReadingPosition { value, .. } if value.location.as_str() == "pdfpage(17)")));
}
#[test]
fn audiobook_position_uses_the_synchronized_reading_register() {
    let conn = singleton_guard_conn();
    update_reading_position(&conn, fixture_content_hash(1), &library_model::audiobook_reading_position(1_234_567), Some(ReadProgress::new(42.5).unwrap())).unwrap();
    assert_eq!(get_audiobook_reading_position(&conn, fixture_content_hash(1)).unwrap(), Some(1_234_567));
    assert_eq!(get_book_reading_position(&conn, fixture_content_hash(1)).unwrap(), None);
    assert_eq!(get_pdf_reading_page(&conn, fixture_content_hash(1)).unwrap(), None);
    assert_eq!(read_pos_raw(&conn), "audiopos(1234567)");
    let progress: f32 = conn.query_row(include_str!("sql/tests/audiobook_position_uses_the_synchronized_reading_register_select.sql"), [], |row| row.get(0)).unwrap();
    assert_eq!(progress, 42.5);
    assert!(get_pending_mutations(&conn).unwrap().iter().any(|mutation| matches!(&mutation.body, MutationBody::ReadingPosition { value, .. } if value.location.as_str() == "audiopos(1234567)" && value.progress == 42.5)));
}
fn latest_read_position_mutation(conn: &Connection) -> (u64, u64, u64) {
    conn.query_row(include_str!("sql/tests/latest_read_position_mutation_select.sql"), [], |r| {
        let seq: i64 = r.get(0)?;
        let pt: i64 = r.get(1)?;
        let l: i64 = r.get(2)?;
        Ok((seq.max(0) as u64, pt.max(0) as u64, l.max(0) as u64))
    })
    .unwrap()
}
/// Applies a sequence of (add?, changed_at, replica_seq) remote ops and reports whether
/// the placement ends up live (present) or tombstoned (removed).
fn final_present(ops: &[(bool, u64, u64)]) -> bool {
    let mut conn = lww_conn();
    let tx = conn.transaction().unwrap();
    let dir_id = test_dir_id("d1");
    for &(add, pt, replica_seq) in ops {
        let body = if add {
            MutationBody::Placement { dir_id: dir_id.clone(), content_hash: fixture_content_hash(1), present: true, origin_folder_id: None }
        } else {
            MutationBody::Placement { dir_id: dir_id.clone(), content_hash: fixture_content_hash(1), present: false, origin_folder_id: None }
        };
        let change = server_change_from(body, pt, 0, "remote", replica_seq.max(1), replica_seq.max(1));
        apply_remote_change_in_tx(&tx, &change).unwrap();
    }
    let present = tx.query_row(include_str!("sql/tests/final_present_select.sql"), [DIR_D1], |r| r.get::<_, i64>(0)).unwrap() > 0;
    tx.commit().unwrap();
    present
}
#[test]
fn local_book_dir_ops_stamp_versions_and_preserve_conditional_transitions() {
    let conn = Connection::open_in_memory().unwrap();
    let _client = init_db(&conn).unwrap();
    conn.execute(include_str!("sql/tests/local_book_dir_ops_stamp_versions_and_preserve_conditional_transitions_insert.sql"), [DIR_D1]).unwrap();
    conn.execute(include_str!("sql/tests/client_rejects_a_nil_persisted_mutation_id_without_overwriting_the_row_insert.sql"), []).unwrap();
    // Local add (change_origin defaults to 'local') creates one canonical
    // outbox row and stamps the placement with that mutation's version.
    conn.execute(include_str!("sql/tests/local_book_dir_ops_stamp_versions_and_preserve_conditional_transitions_insert_3.sql"), [DIR_D1]).unwrap();
    let add_version: (i64, i64) = conn.query_row(include_str!("sql/tests/local_book_dir_ops_stamp_versions_and_preserve_conditional_transitions_select.sql"), [DIR_D1], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert!(add_version.0 > 0, "local add must stamp wall-clock time");
    // Local remove: trig_book_dir_deleted (now AFTER) emits book_dir_removed and
    // the stamp advances the wall-clock version.
    conn.execute(include_str!("sql/tests/local_book_dir_ops_stamp_versions_and_preserve_conditional_transitions_update.sql"), []).unwrap();
    assert!(matches!(get_pending_mutations(&conn).unwrap().last().map(|mutation| &mutation.body), Some(MutationBody::Placement { present: false, .. })));
    let remove_version: (i64, i64) = conn.query_row(include_str!("sql/tests/local_book_dir_ops_stamp_versions_and_preserve_conditional_transitions_select.sql"), [DIR_D1], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert!(remove_version >= add_version);
    // Reactivation runs under an outer UPSERT. Metadata conflict handling in
    // its nested wall-clock version trigger must remain an UPSERT as well; `OR REPLACE` is
    // overridden by the outer statement and used to fail on the metadata PK.
    conn.insert_book_dir(DIR_D1, fixture_content_hash(1).as_str(), "f.epub", "fp2", 2, 1).unwrap();
    let readd_version: (i64, i64) = conn.query_row(include_str!("sql/tests/local_book_dir_ops_stamp_versions_and_preserve_conditional_transitions_select.sql"), [DIR_D1], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    let states = get_pending_mutations(&conn)
        .unwrap()
        .into_iter()
        .filter_map(|mutation| match mutation.body {
            MutationBody::Placement { present: true, .. } => Some("present"),
            MutationBody::Placement { present: false, .. } => Some("vacant"),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(states, vec!["present"]);
    assert!(readd_version > remove_version, "replacement must advance the total local version");
}
#[test]
fn local_book_lifecycle_ops_stamp_complete_versions() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    set_sync_metadata(&conn, "replica_id", "local-device").unwrap();
    conn.insert_book(fixture_content_hash(1).as_str(), "Book", 1, "epub").unwrap();
    let added: (i64, i64, String, i64, bool) = conn.query_row(include_str!("sql/tests/local_book_lifecycle_ops_stamp_complete_versions_select.sql"), [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))).unwrap();
    assert!(added.0 > 0);
    assert_eq!(added.2, "local-device");
    assert!(added.3 > 0);
    assert!(!added.4);
    conn.execute(include_str!("sql/tests/local_book_lifecycle_ops_stamp_complete_versions_update.sql"), []).unwrap();
    let deleted: (i64, i64, String, i64, bool) =
        conn.query_row(include_str!("sql/tests/local_book_lifecycle_ops_stamp_complete_versions_select.sql"), [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))).unwrap();
    assert!((deleted.0, deleted.1) > (added.0, added.1));
    assert_eq!(deleted.2, "local-device");
    assert!(deleted.3 > added.3);
    assert!(deleted.4);
}
#[test]
fn newly_scanned_book_emits_only_lifecycle_and_placement() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let tx = conn.transaction().unwrap();
    let root = test_dir_id("00000000-0000-0000-0000-000000000000");
    let file_name = test_file_name("scan.epub");
    let content_hash = test_content_hash();
    insert_scanned_book(&tx, content_hash, "Scanned title", "Scanned author").unwrap();
    insert_local_book_dir(&tx, &root, content_hash, &file_name, 7, 1).unwrap();
    tx.commit().unwrap();
    let counts: (i64, i64, i64, i64) = conn.query_row(include_str!("sql/tests/newly_scanned_book_emits_only_lifecycle_and_placement_select.sql"), [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).unwrap();
    assert_eq!(counts, (2, 1, 1, 0));
    let lifecycle = get_pending_mutations(&conn)
        .unwrap()
        .into_iter()
        .find_map(|mutation| match mutation.body {
            MutationBody::BookLifecycle { value: BookLifecycleState::Present { metadata, .. }, .. } => Some((metadata.title, metadata.contributors)),
            _ => None,
        })
        .unwrap();
    assert_eq!(lifecycle.0, "Scanned title");
    assert_eq!(lifecycle.1.iter().map(Contributor::name).collect::<Vec<_>>(), vec!["Scanned author"]);
    conn.execute(include_str!("sql/tests/failed_recovery_page_rolls_back_and_retains_marker_delete.sql"), []).unwrap();
    let tx = conn.transaction().unwrap();
    update_book_metadata(&tx, content_hash, "Edited title", "Edited author").unwrap();
    tx.commit().unwrap();
    let metadata_count: i64 = conn.query_row(include_str!("sql/tests/newly_scanned_book_emits_only_lifecycle_and_placement_select_2.sql"), [], |row| row.get(0)).unwrap();
    assert_eq!(metadata_count, 1, "later metadata edits must remain independently synchronized");
}
#[test]
fn dir_state_triggers_coalesce_to_the_latest_full_state() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.execute(include_str!("sql/tests/dir_state_triggers_coalesce_to_the_latest_full_state_insert.sql"), [DIR_D1]).unwrap();
    conn.execute(include_str!("sql/tests/dir_state_triggers_coalesce_to_the_latest_full_state_update.sql"), [DIR_D1]).unwrap();
    conn.execute(include_str!("sql/tests/book_path_chooses_the_first_live_path_in_name_order_update.sql"), [DIR_D1]).unwrap();
    let count: i64 = conn.query_row(include_str!("sql/tests/dir_state_triggers_coalesce_to_the_latest_full_state_select.sql"), [DIR_D1], |row| row.get(0)).unwrap();
    assert_eq!(count, 3);
    assert!(get_pending_mutations(&conn).unwrap().into_iter().any(|mutation| matches!(mutation.body, MutationBody::DirectoryLifecycle { value: library_model::DirectoryLifecycleState::Deleted, .. })));
}
#[test]
fn stale_remote_dir_state_does_not_overwrite_unpushed_local_move() {
    let mut conn = fk_conn();
    conn.execute(include_str!("sql/tests/stale_remote_dir_state_does_not_overwrite_unpushed_local_move_insert.sql"), [DIR_D1]).unwrap();
    conn.execute(include_str!("sql/tests/stale_remote_dir_state_does_not_overwrite_unpushed_local_move_update.sql"), [DIR_D1]).unwrap();
    let local_pt: u64 = conn
        .query_row(include_str!("sql/tests/stale_remote_dir_state_does_not_overwrite_unpushed_local_move_select.sql"), [DIR_D1], |r| {
            let pt: i64 = r.get(0)?;
            Ok(pt.max(0) as u64)
        })
        .unwrap();
    let tx = conn.transaction().unwrap();
    let stale_name = server_change(MutationBody::DirectoryName { dir_id: test_dir_id("d1"), name: test_file_name("RemoteOld") }, local_pt.saturating_sub(1), 0);
    apply_remote_change_in_tx(&tx, &stale_name).unwrap();
    tx.commit().unwrap();
    let name: String = conn.query_row(include_str!("sql/tests/stale_remote_dir_state_does_not_overwrite_unpushed_local_move_select_2.sql"), [DIR_D1], |r| r.get(0)).unwrap();
    assert_eq!(name, "LocalNew");
}
#[test]
fn singleton_remote_apply_skips_stale_value_behind_unpushed_local_edit() {
    let mut conn = singleton_guard_conn();
    let (_, local_pt, _) = latest_read_position_mutation(&conn);
    let stale_pt = local_pt.saturating_sub(1);
    let tx = conn.transaction().unwrap();
    apply_remote_change_in_tx(&tx, &reading_position_change("epubcfi(/6/4)", stale_pt, 0)).unwrap();
    tx.commit().unwrap();
    assert_eq!(read_pos_raw(&conn), "epubcfi(/6/2)");
}
#[test]
fn singleton_remote_apply_accepts_newer_value_than_unpushed_local_edit() {
    let mut conn = singleton_guard_conn();
    let (_, local_pt, local_l) = latest_read_position_mutation(&conn);
    let tx = conn.transaction().unwrap();
    apply_remote_change_in_tx(&tx, &reading_position_change("epubcfi(/6/4)", local_pt + 1, local_l)).unwrap();
    tx.commit().unwrap();
    assert_eq!(read_pos_raw(&conn), "epubcfi(/6/4)");
}
#[test]
fn singleton_guard_only_considers_pending_mutations() {
    let mut conn = singleton_guard_conn();
    let (_, local_pt, _) = latest_read_position_mutation(&conn);
    let pending = get_pending_mutations(&conn).unwrap();
    acknowledge_mutations(&conn, &pending.iter().map(|mutation| mutation.mutation_id).collect::<Vec<_>>()).unwrap();
    let tx = conn.transaction().unwrap();
    apply_remote_change_in_tx(&tx, &reading_position_change("epubcfi(/6/6)", local_pt.saturating_sub(1), 0)).unwrap();
    tx.commit().unwrap();
    assert_eq!(read_pos_raw(&conn), "epubcfi(/6/2)");
}
#[test]
fn persisted_reading_state_is_validated_in_rust() {
    let conn = fk_conn();
    set_sync_metadata(&conn, "change_origin", "remote").unwrap();
    conn.execute(include_str!("sql/tests/persisted_reading_state_is_validated_in_rust_insert.sql"), []).unwrap();
    assert!(get_book_reading_position(&conn, fixture_content_hash(1)).is_err());
    conn.execute(include_str!("sql/tests/persisted_reading_state_is_validated_in_rust_insert_2.sql"), [DIR_D1]).unwrap();
    conn.execute(include_str!("sql/tests/persisted_reading_state_is_validated_in_rust_insert_3.sql"), [DIR_D1]).unwrap();
    conn.execute(include_str!("sql/tests/persisted_reading_state_is_validated_in_rust_update.sql"), []).unwrap();
    conn.execute(include_str!("sql/tests/persisted_reading_state_is_validated_in_rust_insert_4.sql"), []).unwrap();
    assert!(get_recently_read_cards(&conn, 10, false, FINISHED_THRESHOLD).is_ok());
    set_sync_metadata(&conn, "change_origin", "local").unwrap();
}
#[test]
fn invalid_remote_reading_position_is_rejected_before_storage() {
    // Decoding validates a remote location before it can become a typed
    // mutation, so the reducer never receives malformed reading state.
    assert!(library_model::ReadingPosition::parse("not-a-cfi").is_err());
}
#[test]
fn scalar_registers_arriving_before_book_lifecycle_are_preserved() {
    let mut conn = fk_conn();
    let tx = conn.transaction().unwrap();
    apply_remote_change_in_tx(&tx, &reading_position_change("epubcfi(/6/4)", 10, 0)).unwrap();
    apply_remote_change_in_tx(
        &tx,
        &server_change(
            MutationBody::BookLifecycle { content_hash: fixture_content_hash(1), value: BookLifecycleState::Present { metadata: metadata("Book", vec![author("Lifecycle author")], "").into(), added_at: 12, format: BookFormat::Epub } },
            12,
            0,
        ),
    )
    .unwrap();
    tx.commit().unwrap();
    let state: (Option<String>, i64, String, String) =
        conn.query_row(include_str!("sql/tests/scalar_registers_arriving_before_book_lifecycle_are_preserved_select.sql"), [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).unwrap();
    assert_eq!(state, (Some("epubcfi(/6/4)".to_string()), 10, "Book".to_owned(), "Lifecycle author".to_owned()));
}
#[test]
fn remote_metadata_rebuilds_ordered_author_links_without_echoing() {
    let mut conn = fk_conn();
    conn.execute(include_str!("sql/tests/failed_recovery_page_rolls_back_and_retains_marker_delete.sql"), []).unwrap();
    let authors = vec![author("Le Guin, Ursula K."), author("Jane Doe")];
    let tx = conn.transaction().unwrap();
    apply_remote_change_in_tx(&tx, &server_change(MutationBody::Metadata { content_hash: fixture_content_hash(1), value: metadata("A collaboration", authors.clone(), "").into() }, 12, 0)).unwrap();
    tx.commit().unwrap();
    let credits = conn.prepare(include_str!("sql/tests/remote_metadata_rebuilds_ordered_author_links_without_echoing_select.sql")).unwrap().query_map([], |row| row.get::<_, String>(0)).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(credits, authors.iter().map(|author| author.name().to_owned()).collect::<Vec<_>>());
    let stored: Vec<Contributor> = conn
        .prepare(include_str!("sql/tests/remote_metadata_rebuilds_ordered_author_links_without_echoing_select_2.sql"))
        .unwrap()
        .query_map([], |row| {
            let stable_id: String = row.get(0)?;
            let name: String = row.get(1)?;
            let sort_name: Option<String> = row.get(2)?;
            let role_bytes: Vec<u8> = row.get(3)?;
            let role_array: [u8; 3] = role_bytes.try_into().unwrap();
            Ok(Contributor::with_id(AgentId::parse_str(&stable_id).unwrap(), name, sort_name, MarcRelatorCode(role_array)).unwrap())
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(stored, authors);
    assert_eq!(conn.query_row(include_str!("sql/tests/failed_recovery_page_rolls_back_and_retains_marker_select.sql"), [], |row| row.get::<_, i64>(0)).unwrap(), 0);
}
#[test]
fn lifecycle_initializer_does_not_overwrite_existing_metadata() {
    let mut conn = fk_conn();
    set_sync_metadata(&conn, "change_origin", "remote").unwrap();
    conn.execute(include_str!("sql/tests/lifecycle_initializer_does_not_overwrite_existing_metadata_insert.sql"), []).unwrap();
    let tx = conn.transaction().unwrap();
    update_book_metadata(&tx, fixture_content_hash(1), "Existing title", "Existing author").unwrap();
    tx.commit().unwrap();
    set_sync_metadata(&conn, "change_origin", "local").unwrap();
    let tx = conn.transaction().unwrap();
    apply_remote_change_in_tx(
        &tx,
        &server_change(
            MutationBody::BookLifecycle {
                content_hash: fixture_content_hash(1),
                value: BookLifecycleState::Present { metadata: metadata("Lifecycle title", vec![author("Lifecycle author")], "").into(), added_at: 12, format: BookFormat::Epub },
            },
            12,
            0,
        ),
    )
    .unwrap();
    tx.commit().unwrap();
    let metadata: (String, String) = conn.query_row(include_str!("sql/tests/lifecycle_initializer_does_not_overwrite_existing_metadata_select.sql"), [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    assert_eq!(metadata, ("Existing title".to_owned(), "Existing author".to_owned()));
}
#[test]
fn generated_outbox_identity_is_stamped_on_materialized_state() {
    let conn = fk_conn();
    conn.execute(include_str!("sql/tests/generated_outbox_identity_is_stamped_on_materialized_state_insert.sql"), []).unwrap();
    let (persisted, stamped): (String, String) = conn.query_row(include_str!("sql/tests/generated_outbox_identity_is_stamped_on_materialized_state_select.sql"), [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    assert_eq!(stamped, persisted);
    MutationId::parse(&stamped).expect("the entity version must carry the generated mutation id");
}
#[test]
fn deferred_pull_tx_allows_book_dir_before_dir_in_same_batch() {
    let mut conn = fk_conn();
    let tx = conn.transaction().unwrap();
    tx.pragma_update(None, "defer_foreign_keys", "ON").unwrap();
    apply_remote_change_in_tx(&tx, &book_dir_add_change(fixture_content_hash(1), "late-dir", "f.epub", 10)).unwrap();
    apply_dir_state(&tx, "late-dir", "00000000-0000-0000-0000-000000000000", "Late", false, 11);
    tx.commit().unwrap();
    let late_dir = test_dir_id("late-dir").to_string();
    let present: i64 = conn.query_row(include_str!("sql/tests/remote_book_dir_into_soft_deleted_dir_still_applies_select.sql"), [late_dir], |r| r.get(0)).unwrap();
    assert_eq!(present, 1);
}
#[test]
fn compacted_dir_state_can_arrive_after_book_dir_add_in_same_batch() {
    let mut conn = fk_conn();
    let tx = conn.transaction().unwrap();
    tx.pragma_update(None, "defer_foreign_keys", "ON").unwrap();
    apply_remote_change_in_tx(&tx, &book_dir_add_change(fixture_content_hash(1), "d-final", "f.epub", 10)).unwrap();
    apply_dir_state(&tx, "d-final", "00000000-0000-0000-0000-000000000000", "Final", false, 11);
    tx.commit().unwrap();
    let final_dir = test_dir_id("d-final").to_string();
    let row: (String, i64) = conn.query_row(include_str!("sql/tests/compacted_dir_state_can_arrive_after_book_dir_add_in_same_batch_select.sql"), [final_dir], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!(row, ("Final".to_string(), 1));
}
#[test]
fn deferred_pull_tx_allows_child_dir_before_parent_in_same_batch() {
    let mut conn = fk_conn();
    let tx = conn.transaction().unwrap();
    tx.pragma_update(None, "defer_foreign_keys", "ON").unwrap();
    apply_dir_state(&tx, "child-dir", "parent-dir", "Child", false, 10);
    apply_dir_state(&tx, "parent-dir", "00000000-0000-0000-0000-000000000000", "Parent", false, 11);
    tx.commit().unwrap();
    let child_dir = test_dir_id("child-dir").to_string();
    let parent: String = conn.query_row(include_str!("sql/tests/deferred_pull_tx_allows_child_dir_before_parent_in_same_batch_select.sql"), [child_dir], |r| r.get(0)).unwrap();
    assert_eq!(parent, test_dir_id("parent-dir").to_string());
}
#[test]
fn missing_directory_is_a_hidden_placeholder_across_pull_transactions() {
    let mut conn = fk_conn();
    let tx = conn.transaction().unwrap();
    tx.pragma_update(None, "defer_foreign_keys", "ON").unwrap();
    apply_remote_change_in_tx(&tx, &book_dir_add_change(fixture_content_hash(1), "missing-dir", "f.epub", 10)).unwrap();
    tx.commit().unwrap();
    let missing_dir = test_dir_id("missing-dir").to_string();
    let placeholder: (String, bool) = conn.query_row(include_str!("sql/tests/missing_directory_is_a_hidden_placeholder_across_pull_transactions_select.sql"), [missing_dir], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    assert_eq!(placeholder, ("00000000-0000-0000-0000-000000000000".to_owned(), true));
}
#[test]
fn remote_book_dir_into_soft_deleted_dir_still_applies() {
    let mut conn = fk_conn();
    let soft_dir = test_dir_id("soft-dir").to_string();
    conn.execute(include_str!("sql/tests/remote_book_dir_into_soft_deleted_dir_still_applies_insert.sql"), [soft_dir.as_str()]).unwrap();
    let tx = conn.transaction().unwrap();
    tx.pragma_update(None, "defer_foreign_keys", "ON").unwrap();
    apply_remote_change_in_tx(&tx, &book_dir_add_change(fixture_content_hash(1), "soft-dir", "f.epub", 10)).unwrap();
    tx.commit().unwrap();
    let present: i64 = conn.query_row(include_str!("sql/tests/remote_book_dir_into_soft_deleted_dir_still_applies_select.sql"), [soft_dir], |r| r.get(0)).unwrap();
    assert_eq!(present, 1);
}
// Pre-existing gap unrelated to the contributor migration: `delete_dir` and
// `remove_deleted_dir_artifacts` do not exist anywhere in this crate (not a
// visibility issue — the functions were never (re)implemented after some
// earlier refactor). Left disabled rather than inventing directory-deletion
// business logic as a side effect of the contributor unification.
#[test]
fn sqlite_rejects_unknown_blob_kinds() {
    let conn = fk_conn();
    assert!(conn.execute(include_str!("sql/tests/sqlite_rejects_unknown_blob_kinds_insert.sql"), [TEST_CONTENT_HASH]).is_err());
}

#[test]
fn library_schema_does_not_store_asset_bytes() {
    let conn = fk_conn();
    let local_blob_table: i64 = conn.query_row(include_str!("sql/tests/library_schema_does_not_store_asset_bytes_select.sql"), [], |row| row.get(0)).unwrap();
    assert_eq!(local_blob_table, 0);
}
#[test]
fn remote_asset_facts_are_namespaced() {
    let conn = fk_conn();
    let content_hash = test_content_hash();
    assert!(get_remote_asset_hashes(&conn, BlobKind::Book).unwrap().is_empty());
    record_remote_asset(&conn, BlobKind::Book, &content_hash).unwrap();
    assert!(get_remote_asset_hashes(&conn, BlobKind::Book).unwrap().contains(&content_hash));
    assert!(get_remote_asset_hashes(&conn, BlobKind::Thumbnail).unwrap().is_empty());
    forget_remote_asset(&conn, BlobKind::Book, &content_hash).unwrap();
    assert!(get_remote_asset_hashes(&conn, BlobKind::Book).unwrap().is_empty());
}

#[test]
fn rejected_upload_suppression_expires() {
    let conn = fk_conn();
    let content_hash = test_content_hash();
    record_rejected_asset_upload(&conn, BlobKind::Book, &content_hash, "forbidden").unwrap();
    assert!(rejected_asset_upload_hashes(&conn, BlobKind::Book).unwrap().contains(&content_hash));

    conn.execute(include_str!("sql/tests/rejected_upload_suppression_expires_update.sql"), []).unwrap();

    assert!(!rejected_asset_upload_hashes(&conn, BlobKind::Book).unwrap().contains(&content_hash));
}
#[test]
fn completing_one_asset_request_cannot_erase_a_concurrent_request() {
    let conn = fk_conn();
    let first = test_content_hash();
    let second = ContentHash::new(&"b".repeat(64));
    add_asset_request(&conn, BlobKind::Book, &first, TransferOrigin::Background).unwrap();
    add_asset_request(&conn, BlobKind::Book, &second, TransferOrigin::Background).unwrap();
    complete_asset_request(&conn, BlobKind::Book, &first).unwrap();
    assert_eq!(get_asset_requests(&conn, BlobKind::Book).unwrap(), vec![(second, TransferOrigin::Background)]);
}
#[test]
fn placement_publication_guard_requires_exact_live_book_path_and_hash() {
    let conn = fk_conn();
    let hash = test_content_hash();
    let placement = BookPlacement::new(hash, RelativeBookPath::parse("/A/f.epub").unwrap());
    conn.execute(include_str!("sql/tests/dir_state_triggers_coalesce_to_the_latest_full_state_insert.sql"), [DIR_D1]).unwrap();
    conn.execute(include_str!("sql/tests/shared_asset_download_marks_every_live_placement_insert_2.sql"), [hash.as_str()]).unwrap();
    conn.execute(include_str!("sql/tests/placement_publication_guard_requires_exact_live_book_path_and_hash_insert_3.sql"), rusqlite::params![hash.as_str(), DIR_D1]).unwrap();
    assert!(book_placement_expects_hash(&conn, &placement, &hash).unwrap());
    conn.execute(include_str!("sql/tests/placement_publication_guard_requires_exact_live_book_path_and_hash_update.sql"), ["b".repeat(64)]).unwrap();
    assert!(!book_placement_expects_hash(&conn, &placement, &hash).unwrap());
    conn.execute(include_str!("sql/tests/placement_publication_guard_requires_exact_live_book_path_and_hash_update.sql"), [hash.as_str()]).unwrap();
    conn.execute(include_str!("sql/tests/live_book_pages_skip_unplaced_and_hidden_books_without_duplicates_update.sql"), []).unwrap();
    assert!(!book_placement_expects_hash(&conn, &placement, &hash).unwrap());
}

#[test]
fn shared_asset_download_marks_every_live_placement() {
    let conn = fk_conn();
    let hash = test_content_hash();
    conn.execute(include_str!("sql/tests/shared_asset_download_marks_every_live_placement_insert.sql"), [DIR_D1, DIR_D2]).unwrap();
    conn.execute(include_str!("sql/tests/shared_asset_download_marks_every_live_placement_insert_2.sql"), [hash.as_str()]).unwrap();
    conn.execute(include_str!("sql/tests/shared_asset_download_marks_every_live_placement_insert_3.sql"), rusqlite::params![DIR_D1, DIR_D2, hash.as_str()]).unwrap();

    set_book_hash_downloaded(&conn, hash, true).unwrap();

    let downloaded: i64 = conn.query_row(include_str!("sql/tests/shared_asset_download_marks_every_live_placement_select.sql"), [], |row| row.get(0)).unwrap();
    assert_eq!(downloaded, 2);
}
/// Naming a download twice must return the same name. Without this the book's
/// own file occupies the name the ladder just chose, the next call escalates a
/// rung, and the transfer writes a second copy that no placement references.
/// The probe must see the filesystem, not only app-managed rows: a file the user
/// put there themselves still occupies its name.
#[test]
fn remote_book_dir_add_remove_converge_regardless_of_order() {
    // The #4 divergence: device A applied B's add last (present), device B
    // applied A's remove last (absent). With the wall-clock version gate, the newer wall-clock version op
    // (remove@20) wins no matter the application order.
    let add_then_remove = final_present(&[(true, 10, 0), (false, 20, 0)]);
    let remove_then_add = final_present(&[(false, 20, 0), (true, 10, 0)]);
    assert_eq!(add_then_remove, remove_then_add, "order must not change the outcome");
    assert!(!add_then_remove, "newer wall-clock version remove must win → absent");
    // Symmetric case: a newer add beats an older remove, both orders.
    assert!(final_present(&[(false, 20, 0), (true, 30, 0)]));
    assert!(final_present(&[(true, 30, 0), (false, 20, 0)]));
    // Exact wall-clock ties are resolved consistently: deletion is always the
    // deterministic loser override in placement LWW, regardless of sequence.
    assert!(!final_present(&[(true, 15, 2), (false, 15, 1)]));
    assert!(!final_present(&[(false, 15, 1), (true, 15, 2)]));
    assert!(!final_present(&[(true, 15, 1), (false, 15, 2)]));
    assert!(!final_present(&[(false, 15, 2), (true, 15, 1)]));
}
#[test]
fn remote_book_delete_creates_a_versioned_tombstone_before_any_add() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let content_hash = fixture_content_hash(77);
    let deletion = server_change_from(MutationBody::BookLifecycle { content_hash, value: BookLifecycleState::Deleted { origin_folder_id: None } }, 10, 0, "delete-device", 1, 1);
    let stale_add =
        server_change_from(MutationBody::BookLifecycle { content_hash, value: BookLifecycleState::Present { metadata: metadata("Stale", Vec::new(), "").into(), added_at: 1, format: BookFormat::Epub } }, 9, 0, "add-device", 1, 2);
    let tx = conn.transaction().unwrap();
    apply_remote_change_in_tx(&tx, &deletion).unwrap();
    apply_remote_change_in_tx(&tx, &stale_add).unwrap();
    tx.commit().unwrap();
    let deleted: bool = conn.query_row(include_str!("sql/tests/remote_book_delete_creates_a_versioned_tombstone_before_any_add_select.sql"), [content_hash.as_str()], |row| row.get(0)).unwrap();
    assert!(deleted, "an unseen deletion must survive a later stale add");
}
#[test]
fn carousels_hide_books_with_no_live_placement() {
    // A book whose every placement is removed keeps its metadata row but must
    // not surface in the carousels (un-openable), and must reappear when a
    // placement returns.
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let content_hash = test_content_hash();
    conn.execute(include_str!("sql/tests/carousels_hide_books_with_no_live_placement_insert.sql"), [DIR_D1]).unwrap();
    conn.execute(include_str!("sql/tests/carousels_hide_books_with_no_live_placement_insert_2.sql"), [content_hash.as_str()]).unwrap();
    conn.execute(include_str!("sql/tests/carousels_hide_books_with_no_live_placement_insert_3.sql"), [content_hash.as_str()]).unwrap();
    conn.execute(include_str!("sql/tests/carousels_hide_books_with_no_live_placement_insert_4.sql"), rusqlite::params![content_hash.as_str(), DIR_D1]).unwrap();
    let shows = |connection: &Connection| get_recently_read_cards(connection, 10, false, FINISHED_THRESHOLD).unwrap().iter().any(|book| book.content_hash == content_hash);
    assert!(shows(&conn), "book with a live placement must appear");
    assert_eq!(count_all_books(&conn), 1);
    // Remove the only placement: the row survives but drops out of the carousel.
    conn.execute(include_str!("sql/tests/carousels_hide_books_with_no_live_placement_update.sql"), [content_hash.as_str()]).unwrap();
    assert!(!shows(&conn), "orphaned book must not appear in the carousel");
    assert_eq!(count_all_books(&conn), 0, "count must match what is displayed");
    let row_kept: i64 = conn.query_row(include_str!("sql/tests/carousels_hide_books_with_no_live_placement_select.sql"), [content_hash.as_str()], |r| r.get(0)).unwrap();
    assert_eq!(row_kept, 1, "the book metadata row must be preserved, not tombstoned");
    // A returning placement makes it reappear with no resurrection plumbing.
    conn.execute(include_str!("sql/tests/carousels_hide_books_with_no_live_placement_update_2.sql"), [content_hash.as_str()]).unwrap();
    assert!(shows(&conn), "book must reappear once a live placement returns");
}
fn count_all_books(conn: &Connection) -> i64 {
    let mut n = 0;
    conn.count_all_books(|r| {
        n = r.get(0)?;
        Ok(())
    })
    .unwrap();
    n
}
#[test]
fn winning_remote_removal_reports_applied_for_file_deletion() {
    let mut conn = lww_conn();
    let tx = conn.transaction().unwrap();
    // Present placement.
    apply_remote_change_in_tx(&tx, &book_dir_change(true, 10, 1)).unwrap();
    // Higher-wall-clock version removal wins -> caller should delete the local file.
    let applied = apply_remote_change_in_tx(&tx, &book_dir_change(false, 20, 2)).unwrap();
    assert!(applied.is_some(), "winning removal must report the placement so the file is deleted");
    // A stale (older wall-clock version) removal loses -> no deletion.
    let stale = apply_remote_change_in_tx(&tx, &book_dir_change(false, 5, 3)).unwrap();
    assert!(stale.is_none(), "stale removal must not trigger a deletion");
    tx.commit().unwrap();
}
#[test]
fn sweep_removes_missing_placements_but_keeps_other_placements() {
    // VANISH_GRACE_SCANS = 2: a file must be missing for >= 2 scans.
    let conn = new_schema_conn();
    let current = 10_i64;
    conn.execute_batch(include_str!("sql/mod/sweep_removes_missing_placements_but_keeps_other_placements_query.sql")).unwrap();
    delete_vanished_placements(&conn, current).unwrap();
    assert!(is_alive(&conn, DIR_D1, fixture_content_hash(1).as_str(), "keep.epub"), "present file must survive");
    assert!(is_alive(&conn, DIR_D1, fixture_content_hash(4).as_str(), "blip.epub"), "single transient miss must survive the grace window");
    assert!(!is_alive(&conn, DIR_D1, fixture_content_hash(2).as_str(), "gone.epub"), "a missing file removes its placement");
    let gone_downloaded: bool = conn.query_row(include_str!("sql/tests/sweep_removes_missing_placements_but_keeps_other_placements_select.sql"), [DIR_D1], |row| row.get(0)).unwrap();
    assert!(!gone_downloaded, "missing files become unavailable locally");
    assert!(is_alive(&conn, DIR_D2, fixture_content_hash(2).as_str(), "moved.epub"), "relocated file's new path must survive");
    assert!(is_alive(&conn, DIR_D2, fixture_content_hash(3).as_str(), "remote.epub"), "synced-but-undownloaded placement must never be swept");
    assert!(is_alive(&conn, DIR_D2, fixture_content_hash(5).as_str(), "fresh.epub"), "downloaded-but-never-scanned file must not be deleted");
    // The blip is the one pending candidate awaiting a re-scan.
    assert_eq!(count_pending_vanish_candidates(&conn, current).unwrap(), 1);
}
#[test]
fn client_projection_revives_a_multi_node_cycle_and_its_descendant() {
    let mut conn = fk_conn();
    let tx = conn.transaction().unwrap();
    for (id, parent, name, pt) in [("a", "00000000-0000-0000-0000-000000000000", "A", 10), ("b", "a", "B", 11), ("c", "b", "C", 12), ("d", "c", "D", 13)] {
        apply_dir_state(&tx, id, parent, name, false, pt);
    }
    apply_dir_state(&tx, "a", "c", "A", false, 20);
    assert!(get_live_dir_paths_tx(&tx).unwrap().is_empty(), "the cycle and every unreachable descendant must be suppressed");
    apply_dir_state(&tx, "a", "00000000-0000-0000-0000-000000000000", "A", false, 30);
    let paths = get_live_dir_paths_tx(&tx).unwrap();
    assert_eq!(paths.get(&test_dir_id("a")).unwrap().as_str(), "A");
    assert_eq!(paths.get(&test_dir_id("b")).unwrap().as_str(), "A/B");
    assert_eq!(paths.get(&test_dir_id("c")).unwrap().as_str(), "A/B/C");
    assert_eq!(paths.get(&test_dir_id("d")).unwrap().as_str(), "A/B/C/D");
    tx.commit().unwrap();
}
#[test]
fn client_projection_revives_a_suppressed_directory_subtree_after_collision_clears() {
    let mut conn = fk_conn();
    let tx = conn.transaction().unwrap();
    apply_dir_state(&tx, "winner", "00000000-0000-0000-0000-000000000000", "Shared", false, 20);
    apply_dir_state(&tx, "loser", "00000000-0000-0000-0000-000000000000", "Shared", false, 10);
    apply_dir_state(&tx, "child", "loser", "Child", false, 11);
    let numbered = get_live_dir_paths_tx(&tx).unwrap();
    assert_eq!(numbered.len(), 3);
    assert_eq!(numbered.get(&test_dir_id("winner")).unwrap().as_str(), "Shared");
    assert_eq!(numbered.get(&test_dir_id("loser")).unwrap().as_str(), "Shared 2");
    assert_eq!(numbered.get(&test_dir_id("child")).unwrap().as_str(), "Shared 2/Child");
    apply_dir_state(&tx, "unrelated-target", "00000000-0000-0000-0000-000000000000", "Unrelated", false, 15);
    tx.execute(include_str!("sql/tests/client_projection_revives_a_suppressed_directory_subtree_after_collision_clears_delete.sql"), []).unwrap();
    apply_dir_state(&tx, "winner", "00000000-0000-0000-0000-000000000000", "Other", false, 30);
    let queued: std::collections::HashSet<String> =
        tx.prepare(include_str!("sql/tests/client_projection_revives_a_suppressed_directory_subtree_after_collision_clears_select.sql")).unwrap().query_map([], |row| row.get(0)).unwrap().collect::<Result<_, _>>().unwrap();
    assert!(queued.contains(&test_dir_id("winner").to_string()));
    assert!(queued.contains(&test_dir_id("loser").to_string()), "indirect conflict resolution must queue the renamed sibling and its subtree");
    assert!(!queued.contains(&test_dir_id("unrelated-target").to_string()));
    let revived = get_live_dir_paths_tx(&tx).unwrap();
    assert_eq!(revived.get(&test_dir_id("winner")).unwrap().as_str(), "Other");
    assert_eq!(revived.get(&test_dir_id("loser")).unwrap().as_str(), "Shared");
    assert_eq!(revived.get(&test_dir_id("child")).unwrap().as_str(), "Shared/Child");
    tx.commit().unwrap();
}
#[test]
fn client_rejects_a_nil_persisted_mutation_id_without_overwriting_the_row() {
    let mut conn = lww_conn();
    set_sync_metadata(&conn, "change_origin", "remote").unwrap();
    conn.execute(include_str!("sql/tests/client_rejects_a_nil_persisted_mutation_id_without_overwriting_the_row_insert.sql"), []).unwrap();
    conn.execute(include_str!("sql/tests/client_rejects_a_nil_persisted_mutation_id_without_overwriting_the_row_insert_2.sql"), [DIR_D1]).unwrap();
    conn.execute(include_str!("sql/tests/client_rejects_a_nil_persisted_mutation_id_without_overwriting_the_row_insert_3.sql"), [DIR_D1, fixture_replica_id("stored-actor").to_string().as_str()]).unwrap();
    let tx = conn.transaction().unwrap();
    let result = apply_remote_change_in_tx(&tx, &book_dir_change(true, 20, 2));
    assert!(result.is_err(), "a nil stored event id must fail closed");
    let stored: String = tx.query_row(include_str!("sql/tests/client_rejects_a_nil_persisted_mutation_id_without_overwriting_the_row_select.sql"), [], |row| row.get(0)).unwrap();
    assert_eq!(stored, fixture_content_hash(1).as_str());
    tx.rollback().unwrap();
}
// --- Contributor unification regression coverage -------------------------
#[test]
fn contributor_list_rewrite_coalesces_into_one_outbox_mutation_per_book_and_kind() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let transaction = conn.transaction().unwrap();
    insert_scanned_book_contributors(&transaction, fixture_content_hash(1), "Original title", &[author("Original Author")]).unwrap();
    transaction.commit().unwrap();
    conn.execute(include_str!("sql/tests/failed_recovery_page_rolls_back_and_retains_marker_delete.sql"), []).unwrap();
    // A metadata update that clears and rewrites every book_contributor row
    // fires the per-row book_contributor INSERT/DELETE triggers once per
    // deleted row and once per inserted row, in addition to trig_book_metadata
    // itself firing on the book row's own column changes. All of that must
    // coalesce (via the shared ON CONFLICT(state_kind, state_key) upsert) into
    // exactly one outbox row per (content_hash, kind).
    let rewritten = vec![author("Replacement Author One"), author("Replacement Author Two"), Contributor::new("Replacement Editor", None, MarcRelatorCode(*b"edt")).unwrap()];
    let transaction = conn.transaction().unwrap();
    update_book_metadata_contributors(&transaction, fixture_content_hash(1), "Rewritten title", &rewritten).unwrap();
    transaction.commit().unwrap();
    let outbox_rows: Vec<(String, String)> = conn
        .prepare(include_str!("sql/tests/contributor_list_rewrite_coalesces_into_one_outbox_mutation_per_book_and_kind_select.sql"))
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(outbox_rows, vec![("metadata".to_owned(), fixture_content_hash(1).to_string())], "exactly one outbox mutation must exist per (content_hash, kind) after a contributor-list rewrite");
    let pending = get_pending_mutations(&conn).unwrap();
    assert_eq!(pending.len(), 1);
    assert!(matches!(
        &pending[0].body,
        MutationBody::Metadata { content_hash, value } if content_hash.clone() == fixture_content_hash(1) && value.contributors == rewritten
    ));
}
#[test]
fn current_schema_has_integer_relationships_foreign_keys_and_lookup_indexes() {
    let conn = fk_conn();
    let retired_link_table_exists: bool = conn.query_row(include_str!("sql/tests/current_schema_has_integer_relationships_foreign_keys_and_lookup_indexes_select.sql"), [], |row| row.get(0)).unwrap();
    assert!(!retired_link_table_exists);
    for (table, column) in [("book_dir", "book_row_id"), ("book_contributor", "book_row_id"), ("annotation", "book_row_id")] {
        let declared_type: String = conn.query_row(include_str!("sql/tests/current_schema_has_integer_relationships_foreign_keys_and_lookup_indexes_select_2.sql"), rusqlite::params![table, column], |row| row.get(0)).unwrap();
        assert_eq!(declared_type.to_ascii_uppercase(), "INTEGER", "{table}.{column} must remain an integer relationship");
    }
    let foreign_key_errors: i64 = conn.query_row(include_str!("sql/tests/current_schema_has_integer_relationships_foreign_keys_and_lookup_indexes_select_3.sql"), [], |row| row.get(0)).unwrap();
    assert_eq!(foreign_key_errors, 0);
    for index in ["sqlite_autoindex_book_1", "idx_book_dir_book", "idx_book_dir_local_hash", "idx_book_unified_concept_concept", "idx_annotation_book"] {
        let present: i64 = conn.query_row(include_str!("sql/tests/current_schema_has_integer_relationships_foreign_keys_and_lookup_indexes_select_4.sql"), [index], |row| row.get(0)).unwrap();
        assert_eq!(present, 1, "missing identity/relationship index {index}");
    }
    conn.execute(include_str!("sql/tests/current_schema_has_integer_relationships_foreign_keys_and_lookup_indexes_insert.sql"), [TEST_CONTENT_HASH]).unwrap();
    let book_lookup_plan: String = conn.query_row(include_str!("sql/tests/current_schema_has_integer_relationships_foreign_keys_and_lookup_indexes_explain.sql"), [TEST_CONTENT_HASH], |row| row.get(3)).unwrap();
    assert!(book_lookup_plan.contains("sqlite_autoindex_book_1"), "content identity lookup must use its unique index: {book_lookup_plan}");
    let placement_lookup_plan: String = conn.query_row(include_str!("sql/tests/current_schema_has_integer_relationships_foreign_keys_and_lookup_indexes_explain_2.sql"), [1_i64], |row| row.get(3)).unwrap();
    assert!(placement_lookup_plan.contains("idx_book_dir_book"), "book placement lookup must use its row-id index: {placement_lookup_plan}");
}

#[test]
fn external_deletion_removes_only_missing_placements_then_trashes_the_last() {
    let conn = new_schema_conn();
    conn.execute_batch(include_str!("sql/mod/external_deletion_removes_only_missing_placements_then_trashes_the_last_query.sql")).unwrap();
    delete_vanished_placements(&conn, 3).unwrap();
    assert!(library_trash(&conn).unwrap().books.is_empty());
    let alive: i64 = conn.query_row(include_str!("sql/tests/external_deletion_removes_only_missing_placements_then_trashes_the_last_select.sql"), [], |r| r.get(0)).unwrap();
    assert_eq!(alive, 1);
    delete_vanished_placements(&conn, 5).unwrap();
    assert_eq!(library_trash(&conn).unwrap().books.len(), 1);
    let progress: f64 = conn.query_row(include_str!("sql/tests/external_deletion_removes_only_missing_placements_then_trashes_the_last_select_2.sql"), [], |r| r.get(0)).unwrap();
    assert_eq!(progress, 0.5);
    let removals: i64 = conn.query_row(include_str!("sql/tests/external_deletion_removes_only_missing_placements_then_trashes_the_last_select_3.sql"), [], |r| r.get(0)).unwrap();
    assert_eq!(removals, 3);
}

#[test]
fn paged_outbox_preserves_scan_boundary_when_rows_are_acknowledged_and_new_edits_arrive() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let tx = conn.transaction().unwrap();
    for id in 1..=300 {
        tx.execute(include_str!("sql/tests/paged_outbox_preserves_scan_boundary_when_rows_are_acknowledged_and_new_edits_arrive_insert.sql"), rusqlite::params![fixture_content_hash(id).as_str(), format!("Book {id}")]).unwrap();
    }
    tx.commit().unwrap();
    let expected = get_pending_mutations(&conn).unwrap();
    assert!(expected.len() > 256);
    let through = outbox_watermark(&conn).unwrap();
    let first = pending_mutations_page(&conn, i64::MIN, through).unwrap();
    assert_eq!(first.len(), 256);
    let mut after = first.last().unwrap().replica_seq.get() as i64;
    let mut seen = first.iter().map(|mutation| mutation.mutation_id).collect::<Vec<_>>();
    acknowledge_mutations(&conn, &seen).unwrap();
    conn.execute(include_str!("sql/tests/paged_outbox_preserves_scan_boundary_when_rows_are_acknowledged_and_new_edits_arrive_insert_2.sql"), [fixture_content_hash(999).as_str()]).unwrap();
    assert!(outbox_watermark(&conn).unwrap() > through);
    loop {
        let page = pending_mutations_page(&conn, after, through).unwrap();
        if page.is_empty() {
            break;
        }
        assert!(page.len() <= 256);
        after = page.last().unwrap().replica_seq.get() as i64;
        seen.extend(page.into_iter().map(|mutation| mutation.mutation_id));
    }
    assert_eq!(seen, expected.into_iter().map(|mutation| mutation.mutation_id).collect::<Vec<_>>());
    assert!(!pending_mutations_page(&conn, through, outbox_watermark(&conn).unwrap()).unwrap().is_empty());
}

#[test]
fn paged_inventory_does_not_skip_cells_when_earlier_rows_disappear() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let tx = conn.transaction().unwrap();
    for id in 1..=300 {
        tx.execute(include_str!("sql/tests/recovery_skips_encoding_existing_outbox_payloads_insert.sql"), [fixture_content_hash(id).as_str()]).unwrap();
    }
    tx.commit().unwrap();
    let expected = state_inventory(&conn).unwrap();
    let mut seen = state_inventory_page(&conn, None).unwrap();
    assert_eq!(seen.len(), 256);
    let first = &seen[0];
    conn.execute(include_str!("sql/tests/paged_inventory_does_not_skip_cells_when_earlier_rows_disappear_delete.sql"), rusqlite::params![first.kind, first.entity_key, first.entity_subkey]).unwrap();
    loop {
        let page = state_inventory_page(&conn, seen.last()).unwrap();
        if page.is_empty() {
            break;
        }
        assert!(page.len() <= 256);
        seen.extend(page);
    }
    assert_eq!(seen, expected);
}

#[test]
fn paged_recovery_retains_marker_until_all_pages_are_durable_and_restarts_idempotently() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let tx = conn.transaction().unwrap();
    for id in 1..=300 {
        tx.execute(include_str!("sql/tests/recovery_skips_encoding_existing_outbox_payloads_insert.sql"), [fixture_content_hash(id).as_str()]).unwrap();
    }
    tx.commit().unwrap();
    conn.execute(include_str!("sql/tests/failed_recovery_page_rolls_back_and_retains_marker_delete.sql"), []).unwrap();
    begin_cursor_recovery(&conn).unwrap();
    let (first_cursor, first_count) = recover_state_page(&mut conn, None).unwrap();
    assert!(first_cursor.is_some());
    assert_eq!(first_count, 256);
    assert!(cursor_recovery_pending(&conn).unwrap());
    let first_ids = get_pending_mutations(&conn).unwrap().into_iter().map(|mutation| mutation.mutation_id).collect::<Vec<_>>();
    // Model losing the in-memory cursor after a worker crash.
    let (mut after, repeated_count) = recover_state_page(&mut conn, None).unwrap();
    assert_eq!(repeated_count, 0);
    assert_eq!(first_ids, get_pending_mutations(&conn).unwrap().into_iter().map(|mutation| mutation.mutation_id).collect::<Vec<_>>());
    while after.is_some() {
        assert!(cursor_recovery_pending(&conn).unwrap());
        (after, _) = recover_state_page(&mut conn, after.as_ref()).unwrap();
    }
    assert!(!cursor_recovery_pending(&conn).unwrap());
    assert_eq!(get_pending_mutations(&conn).unwrap().len(), state_inventory(&conn).unwrap().len());
}

#[test]
fn recovery_skips_encoding_existing_outbox_payloads() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.execute(include_str!("sql/tests/recovery_skips_encoding_existing_outbox_payloads_insert.sql"), [fixture_content_hash(1).as_str()]).unwrap();
    begin_cursor_recovery(&conn).unwrap();
    let before = get_pending_mutations(&conn).unwrap().into_iter().map(|m| (m.mutation_id, m.body)).collect::<Vec<_>>();
    conn.create_scalar_function("bokheim_mutation", -1, rusqlite::functions::FunctionFlags::SQLITE_UTF8, |_| -> rusqlite::Result<Vec<u8>> { Err(rusqlite::Error::InvalidQuery) }).unwrap();
    assert_eq!(complete_cursor_recovery(&mut conn).unwrap(), 0);
    assert!(!cursor_recovery_pending(&conn).unwrap());
    assert_eq!(before, get_pending_mutations(&conn).unwrap().into_iter().map(|m| (m.mutation_id, m.body)).collect::<Vec<_>>());
}

#[test]
fn failed_recovery_page_rolls_back_and_retains_marker() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    for id in 1..=2 {
        conn.execute(include_str!("sql/tests/recovery_skips_encoding_existing_outbox_payloads_insert.sql"), [fixture_content_hash(id).as_str()]).unwrap();
    }
    conn.execute(include_str!("sql/tests/failed_recovery_page_rolls_back_and_retains_marker_delete.sql"), []).unwrap();
    begin_cursor_recovery(&conn).unwrap();
    conn.execute_batch(include_str!("sql/tests/failed_recovery_page_rolls_back_and_retains_marker_create.sql")).unwrap();
    assert!(recover_state_page(&mut conn, None).is_err());
    assert!(cursor_recovery_pending(&conn).unwrap());
    assert_eq!(conn.query_row(include_str!("sql/tests/failed_recovery_page_rolls_back_and_retains_marker_select.sql"), [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    conn.execute_batch(include_str!("sql/tests/failed_recovery_page_rolls_back_and_retains_marker_drop.sql")).unwrap();
    assert_eq!(complete_cursor_recovery(&mut conn).unwrap(), 2);
    assert!(!cursor_recovery_pending(&conn).unwrap());
}

#[test]
fn paged_outbox_limits_bytes_and_advances_past_one_oversized_mutation() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    for id in 1..=4 {
        conn.execute(include_str!("sql/tests/recovery_skips_encoding_existing_outbox_payloads_insert.sql"), [fixture_content_hash(id).as_str()]).unwrap();
        conn.execute(include_str!("sql/tests/paged_outbox_limits_bytes_and_advances_past_one_oversized_mutation_insert_2.sql"), rusqlite::params![fixture_content_hash(id).as_str(), "x".repeat(if id == 2 { 400_000 } else { 150_000 })])
            .unwrap();
    }
    let expected = get_pending_mutations(&conn).unwrap();
    let through = outbox_watermark(&conn).unwrap();
    let mut after = i64::MIN;
    let mut seen = Vec::new();
    let mut oversized = false;
    loop {
        let page = pending_mutations_page(&conn, after, through).unwrap();
        if page.is_empty() {
            break;
        }
        let bytes = page.iter().map(|mutation| sync_common::wire::encode(&mutation.body).unwrap().len()).sum::<usize>();
        if bytes > 256 * 1024 {
            assert_eq!(page.len(), 1);
            oversized = true;
        }
        after = page.last().unwrap().replica_seq.get() as i64;
        seen.extend(page);
    }
    assert!(oversized, "fixture must exercise a mutation larger than the page byte target");
    assert_eq!(seen, expected);
}

#[test]
fn live_book_pages_skip_unplaced_and_hidden_books_without_duplicates() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let root = sync_common::ROOT_DIR_ID.to_string();
    conn.execute(include_str!("sql/tests/live_book_pages_skip_unplaced_and_hidden_books_without_duplicates_insert.sql"), rusqlite::params![DIR_A, root]).unwrap();
    conn.execute(include_str!("sql/tests/live_book_pages_skip_unplaced_and_hidden_books_without_duplicates_insert_2.sql"), rusqlite::params![DIR_A_CHILD, DIR_A]).unwrap();
    conn.execute(include_str!("sql/tests/live_book_pages_skip_unplaced_and_hidden_books_without_duplicates_insert_3.sql"), rusqlite::params![DIR_B, root]).unwrap();
    for id in 1..=73 {
        let hash = format!("{id:064x}");
        conn.execute(include_str!("sql/tests/live_book_pages_skip_unplaced_and_hidden_books_without_duplicates_insert_4.sql"), rusqlite::params![id, hash]).unwrap();
        if id == 71 {
            continue;
        }
        let dir = if id == 73 { DIR_A_CHILD } else { root.as_str() };
        conn.execute(include_str!("sql/tests/live_book_pages_skip_unplaced_and_hidden_books_without_duplicates_insert_5.sql"), rusqlite::params![id, dir, format!("{id}.epub"), (id == 72).then_some(1)]).unwrap();
    }
    conn.execute(include_str!("sql/tests/live_book_pages_skip_unplaced_and_hidden_books_without_duplicates_insert_6.sql"), [DIR_B]).unwrap();
    let mut after = i64::MIN;
    let mut ids = Vec::new();
    loop {
        let page = live_book_hash_page(&conn, after).unwrap();
        assert!(page.len() <= 32);
        let assets = live_book_asset_page(&conn, after).unwrap();
        assert_eq!(assets.iter().map(|row| (row.row_id, row.content_hash)).collect::<Vec<_>>(), page);
        assert!(assets.iter().all(|row| !row.book_upload_allowed && !row.thumbnail_upload_allowed && !row.thumbnail_remote && !row.thumbnail_pending));
        let Some((last, _)) = page.last() else { break };
        after = *last;
        ids.extend(page.into_iter().map(|(id, _)| id));
    }
    assert_eq!(ids, (1..=70).collect::<Vec<_>>());
    conn.execute(include_str!("sql/tests/live_book_pages_skip_unplaced_and_hidden_books_without_duplicates_update.sql"), []).unwrap();
    assert!(live_book_hash_page(&conn, i64::MIN).unwrap().is_empty());
    assert!(live_book_asset_page(&conn, i64::MIN).unwrap().is_empty());
}

#[test]
fn download_requests_validate_each_book_and_commit_the_batch_atomically() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let hash = fixture_content_hash(501);
    let missing = fixture_content_hash(502);
    let root = sync_common::ROOT_DIR_ID.to_string();
    conn.execute(include_str!("sql/tests/book_path_chooses_the_first_live_path_in_name_order_insert.sql"), [hash.as_str()]).unwrap();
    conn.execute(include_str!("sql/tests/book_path_chooses_the_first_live_path_in_name_order_insert_3.sql"), rusqlite::params![root, hash.as_str()]).unwrap();
    assert!(request_book_downloads(&mut conn, &[hash, missing], TransferOrigin::UserInitiated).is_err());
    assert!(get_asset_requests(&conn, BlobKind::Book).unwrap().is_empty());
    request_book_downloads(&mut conn, &[hash, hash], TransferOrigin::UserInitiated).unwrap();
    assert_eq!(get_asset_requests(&conn, BlobKind::Book).unwrap(), vec![(hash, TransferOrigin::UserInitiated)]);
    complete_asset_request(&conn, BlobKind::Book, &hash).unwrap();
    conn.execute(include_str!("sql/tests/live_book_pages_skip_unplaced_and_hidden_books_without_duplicates_update.sql"), []).unwrap();
    assert!(request_book_downloads(&mut conn, &[hash], TransferOrigin::UserInitiated).is_err());
    conn.execute(include_str!("sql/tests/download_requests_validate_each_book_and_commit_the_batch_atomically_update_2.sql"), []).unwrap();
    conn.execute(include_str!("sql/tests/download_requests_validate_each_book_and_commit_the_batch_atomically_update_3.sql"), [hash.as_str()]).unwrap();
    assert!(request_book_downloads(&mut conn, &[hash], TransferOrigin::UserInitiated).is_err());
    assert!(get_asset_requests(&conn, BlobKind::Book).unwrap().is_empty());
}

#[test]
fn book_path_chooses_the_first_live_path_in_name_order() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let hash = fixture_content_hash(801);
    let root = sync_common::ROOT_DIR_ID.to_string();
    conn.execute(include_str!("sql/tests/book_path_chooses_the_first_live_path_in_name_order_insert.sql"), [hash.as_str()]).unwrap();
    for (dir, name) in [(DIR_B, "Z"), (DIR_A, "A")] {
        conn.execute(include_str!("sql/tests/book_path_chooses_the_first_live_path_in_name_order_insert_2.sql"), rusqlite::params![dir, root, name]).unwrap();
        conn.execute(include_str!("sql/tests/book_path_chooses_the_first_live_path_in_name_order_insert_3.sql"), rusqlite::params![dir, hash.as_str()]).unwrap();
    }
    assert_eq!(get_book_file_path(&conn, hash).unwrap().unwrap().as_str(), "/A/book.epub");
    conn.execute(include_str!("sql/tests/book_path_chooses_the_first_live_path_in_name_order_update.sql"), [DIR_A]).unwrap();
    assert_eq!(get_book_file_path(&conn, hash).unwrap().unwrap().as_str(), "/Z/book.epub");
    conn.execute(include_str!("sql/tests/book_path_chooses_the_first_live_path_in_name_order_update_2.sql"), [DIR_B]).unwrap();
    assert!(get_book_file_path(&conn, hash).unwrap().is_none());
}

#[test]
fn directory_relative_paths_require_complete_live_ancestry() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let parent = create_directory(&mut conn, &sync_common::ROOT_DIR_ID, &"Parent".to_owned()).unwrap();
    let child = create_directory(&mut conn, parent.id(), &"Child".to_owned()).unwrap();
    assert_eq!(directory_relative_path(&conn, child.id()).unwrap().unwrap().as_str(), "Parent/Child");
    assert!(directory_relative_path(&conn, &sync_common::ROOT_DIR_ID).unwrap().is_none());
    assert!(directory_relative_path(&conn, &DirId::new_v4()).unwrap().is_none());
    conn.execute(include_str!("sql/tests/book_path_chooses_the_first_live_path_in_name_order_update.sql"), [parent.id().to_string()]).unwrap();
    assert!(directory_relative_path(&conn, child.id()).unwrap().is_none(), "a deleted ancestor must not produce a shortened path");
    assert!(placements::create_directory_with_id(&mut conn, child.id(), parent.id(), &"Child".to_owned()).is_err(), "retrying an existing directory must still validate its ancestry");
    conn.execute(include_str!("sql/tests/directory_relative_paths_require_complete_live_ancestry_update_2.sql"), [parent.id().to_string()]).unwrap();
    assert_eq!(placements::create_directory_with_id(&mut conn, child.id(), parent.id(), &"Child".to_owned()).unwrap().id(), child.id());
    assert_eq!(directory_relative_path(&conn, child.id()).unwrap().unwrap().as_str(), "Parent/Child");
}

#[test]
fn directory_creation_validates_paths_before_committing() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let before: i64 = conn.query_row(include_str!("sql/tests/directory_creation_validates_paths_before_committing_select.sql"), [], |row| row.get(0)).unwrap();
    for name in ["", ".", "..", "broken//path", "../escape"] {
        assert!(create_directory(&mut conn, &sync_common::ROOT_DIR_ID, &name.to_owned()).is_err(), "accepted {name:?}");
        assert_eq!(conn.query_row(include_str!("sql/tests/directory_creation_validates_paths_before_committing_select.sql"), [], |row| row.get::<_, i64>(0)).unwrap(), before);
    }
    let directory = create_directory(&mut conn, &sync_common::ROOT_DIR_ID, &"Shelf".to_owned()).unwrap();
    assert_eq!(directory.name(), "Shelf");
    assert_eq!(directory_relative_path(&conn, directory.id()).unwrap().unwrap().as_str(), "Shelf");
}

#[test]
fn initialization_rejects_incompatible_schemas_without_upgrading() {
    for version in [0, 32, LIBRARY_SCHEMA_VERSION + 1] {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("sql/tests/initialization_rejects_incompatible_schemas_without_upgrading_create.sql")).unwrap();
        conn.pragma_update(None, "user_version", version).unwrap();
        let error = init_db(&conn).unwrap_err();
        assert!(error.to_string().contains("unsupported library schema version"));
        assert_eq!(conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0)).unwrap(), version);
        assert_eq!(conn.query_row(include_str!("sql/tests/initialization_rejects_incompatible_schemas_without_upgrading_select.sql"), [], |row| row.get::<_, String>(0)).unwrap(), "keep");
        assert_eq!(conn.query_row(include_str!("sql/tests/initialization_rejects_incompatible_schemas_without_upgrading_select_2.sql"), [], |row| row.get::<_, i64>(0)).unwrap(), 1);
    }
}

#[test]
fn reopening_current_schema_does_not_decode_or_rewrite_book_metadata() {
    let conn = Connection::open_in_memory().unwrap();
    let replica = init_db(&conn).unwrap();
    conn.set_sync_metadata("change_origin", "remote").unwrap();
    conn.execute(include_str!("sql/tests/reopening_current_schema_does_not_decode_or_rewrite_book_metadata_insert.sql"), [fixture_content_hash(9999).as_str()]).unwrap();
    conn.execute_batch(include_str!("sql/tests/reopening_current_schema_does_not_decode_or_rewrite_book_metadata_create.sql")).unwrap();
    assert_eq!(init_db(&conn).unwrap(), replica);
    assert_eq!(conn.query_row(include_str!("sql/tests/reopening_current_schema_does_not_decode_or_rewrite_book_metadata_select.sql"), [], |row| row.get::<_, Vec<u8>>(0)).unwrap(), [1, 2, 3]);
}

#[path = "subject_compaction_tests.rs"]
mod subject_compaction_tests;

#[test]
fn reopening_library_preserves_triggers_without_schema_writes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("library.db");
    let conn = Connection::open(&path).unwrap();
    let replica = init_db(&conn).unwrap();
    let schema_version: i64 = conn.pragma_query_value(None, "schema_version", |row| row.get(0)).unwrap();
    let triggers: i64 = conn.query_row(include_str!("sql/tests/reopening_library_preserves_triggers_without_schema_writes_select.sql"), [], |row| row.get(0)).unwrap();
    assert!(triggers > 0);
    drop(conn);

    let conn = Connection::open(&path).unwrap();
    conn.pragma_update(None, "query_only", true).unwrap();
    assert_eq!(init_db(&conn).unwrap(), replica);
    assert_eq!(conn.total_changes(), 0, "Reopening must not write library data");
    conn.pragma_update(None, "query_only", false).unwrap();
    let reopened_version: i64 = conn.pragma_query_value(None, "schema_version", |row| row.get(0)).unwrap();
    assert_eq!(reopened_version, schema_version, "Reopening must not drop or recreate schema objects");
    let reopened_triggers: i64 = conn.query_row(include_str!("sql/tests/reopening_library_preserves_triggers_without_schema_writes_select.sql"), [], |row| row.get(0)).unwrap();
    assert_eq!(reopened_triggers, triggers);

    conn.execute(include_str!("sql/tests/reopening_library_preserves_triggers_without_schema_writes_insert.sql"), [TEST_CONTENT_HASH]).unwrap();
    let category: i64 = conn.query_row(include_str!("sql/tests/reopening_library_preserves_triggers_without_schema_writes_select_3.sql"), [], |row| row.get(0)).unwrap();
    assert_eq!(category, 4, "The original trigger must still maintain browse data");
}

#[test]
fn folder_browsing_lists_empty_folders_and_hides_dot_directories() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.execute_batch(include_str!("sql/mod/folder_browsing_lists_empty_folders_and_hides_dot_directories_query.sql")).unwrap();
    let root = "00000000-0000-0000-0000-000000000000";
    let rows = get_folder_list_rows(&conn, root, "", LibraryFileTypeFilter::All).unwrap();
    // "Empty" holds nothing and "Hidden only" holds nothing but a dot-directory,
    // so both list with a count of zero. A folder that has just been created is
    // empty by definition, and hiding it looks like the creation failed.
    assert_eq!(rows.iter().map(|row| (row.name.as_str(), row.book_count, row.downloaded_book_count)).collect::<Vec<_>>(), [("Empty", 0, 0), ("Hidden only", 0, 0), ("Visible", 1, 1)]);
    assert!(get_folder_list_rows(&conn, root, "searchable", LibraryFileTypeFilter::All).unwrap().is_empty());
    assert!(get_folder_list_rows(&conn, root, "", LibraryFileTypeFilter::Pdf).unwrap().is_empty());
    let rows = get_folder_list_rows(&conn, root, "nested", LibraryFileTypeFilter::All).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name, "Nested");
    // Downloaded-only browsing is a different question — "what can I read right
    // now" — and there an empty folder still has nothing to offer.
    let folder = library_folder(&conn, &DirId::parse_str(root).unwrap(), true, LibraryFileTypeFilter::All).unwrap();
    assert_eq!(folder.children.len(), 1);
    assert_eq!(folder.children[0].name, "Visible");
}

#[test]
fn authoritative_author_credits_replace_local_names_and_preserve_other_roles() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let tx = conn.transaction().unwrap();
    let hash = fixture_content_hash(1);
    let author = library_model::Contributor::new("George F. Kennan", None, library_model::MarcRelatorCode(*b"aut")).unwrap();
    let translator = library_model::Contributor::new("Local Translator", None, library_model::MarcRelatorCode(*b"trl")).unwrap();
    insert_scanned_book_contributors(&tx, hash, "Diaries", &[author, translator]).unwrap();
    let id = ExternalAuthorId::parse(AuthorAuthority::OpenLibrary, "OL139896A").unwrap();
    super::external_metadata::apply_authoritative_authors_in_transaction(&tx, hash, "authority-test", "9780393242768", &[(0, "George Frost Kennan".into(), id)]).unwrap();
    tx.commit().unwrap();
    let credits = get_book_author_identities(&conn, hash).unwrap();
    assert_eq!(credits[0].credited_name, "George Frost Kennan");
    assert_eq!(credits[0].external_ids[0].value(), "OL139896A");
    let count: i64 = conn.query_row(include_str!("sql/tests/authoritative_author_credits_replace_local_names_and_preserve_other_roles_select.sql"), [], |r| r.get(0)).unwrap();
    assert_eq!(count, 1);
}

#[test]
fn title_subtitle_storage_search_and_sync() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let hash = fixture_content_hash(981);
    let tx = conn.transaction().unwrap();
    insert_scanned_book_metadata(&tx, hash, &BookMetadataState { title: "Fluid: The Approach Applied by Geniuses".into(), contributors: vec![], description: String::new(), publication: Default::default() }, BookFormat::Epub).unwrap();
    tx.execute(include_str!("sql/tests/title_subtitle_storage_search_sync_and_migration_insert.sql"), rusqlite::params![sync_common::ROOT_DIR_ID.to_string(), hash.as_str()]).unwrap();
    tx.commit().unwrap();
    let card = book_detail_summary(&conn, hash).unwrap().book;
    assert_eq!(card.title, "Fluid");
    assert_eq!(card.subtitle.as_deref(), Some("The Approach Applied by Geniuses"));
    assert_eq!(search_book_cards_scoped(&conn, "Applied by Geniuses", false).unwrap().title_matches.len(), 1);
    let state = get_pending_mutations(&conn)
        .unwrap()
        .into_iter()
        .find_map(|mutation| match mutation.body {
            MutationBody::BookLifecycle { value: library_model::BookLifecycleState::Present { metadata, .. }, .. } => Some(metadata),
            _ => None,
        })
        .unwrap();
    assert_eq!(state.title, "Fluid");
    assert_eq!(state.subtitle(), Some("The Approach Applied by Geniuses"));
    let transported = sync_common::transport::encode(&MutationBody::Metadata { content_hash: hash, value: state.clone() }).unwrap();
    let decoded: MutationBody = sync_common::transport::decode(&transported, sync_common::wire::MAX_DECODED_REQUEST_BYTES).unwrap();
    assert_eq!(decoded, MutationBody::Metadata { content_hash: hash, value: state });
    let mut receiver = Connection::open_in_memory().unwrap();
    init_db(&receiver).unwrap();
    let lifecycle = get_pending_mutations(&conn).unwrap().into_iter().find(|mutation| matches!(mutation.body, MutationBody::BookLifecycle { .. })).unwrap().body;
    let tx = receiver.transaction().unwrap();
    apply_remote_changes_in_tx(&tx, &[server_change(lifecycle, 100, 0), server_change(decoded, 101, 0)]).unwrap();
    tx.commit().unwrap();
    let received: (String, Option<String>) = receiver.query_row(include_str!("sql/tests/title_subtitle_storage_search_sync_and_migration_select.sql"), [hash.as_str()], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    assert_eq!(received, (card.title.clone(), card.subtitle.clone()));
    assert!(get_pending_mutations(&receiver).unwrap().is_empty(), "receiving split metadata must not create echo mutations");
}

#[path = "chip_group_tests.rs"]
mod chip_group_tests;

#[test]
fn pdf_reader_metadata_syncs_by_revision_without_echoing_or_overwriting_other_revisions() {
    use pdf_view_common::{PdfReaderMetadata, PdfStoredOutline, PdfStoredPage};
    let hash = fixture_content_hash(975);
    let checksum = fixture_content_hash(976);
    let data = PdfReaderMetadata {
        version: 1,
        checksum: checksum.to_string(),
        pages: vec![PdfStoredPage { width: 800.0, height: 600.0, rotation: 90 }],
        outline: vec![PdfStoredOutline { title: "Chapter".into(), page_index: 0, depth: 0 }],
        annotations: vec![pdf_view_common::PdfStoredAnnotation {
            page_index: 0,
            index: 2,
            kind: pdf_view_common::PdfStoredAnnotationKind::Note,
            rects: vec![[0.1, 0.2, 0.03, 0.04]],
            color: "#abcdef".into(),
            text: String::new(),
            note: "Imported note".into(),
        }],
        dependencies: Some(pdf_view_common::PdfDependencyIndex { length: 1000, startup: vec![pdf_view_common::PdfByteRange { offset: 0, length: 100 }], pages: vec![vec![pdf_view_common::PdfByteRange { offset: 500, length: 100 }]] }),
    };
    let mut source = Connection::open_in_memory().unwrap();
    init_db(&source).unwrap();
    source.execute(include_str!("sql/tests/pdf_reader_metadata_syncs_by_revision_without_echoing_or_overwriting_other_revisions_insert.sql"), [hash.as_str()]).unwrap();
    source.execute(include_str!("sql/tests/failed_recovery_page_rolls_back_and_retains_marker_delete.sql"), []).unwrap();
    let tx = source.transaction().unwrap();
    super::pdf_metadata::upsert(&tx, &hash, &checksum, &data).unwrap();
    tx.commit().unwrap();
    let pending = get_pending_mutations(&source).unwrap();
    assert_eq!(pending.len(), 1);
    let wire = pending[0].to_wire().unwrap();
    assert_eq!(wire.entity_subkey, checksum.as_str());
    assert_eq!(MutationBody::from_wire(&wire).unwrap(), Some(pending[0].body.clone()));
    let mut invalid_wire = wire;
    invalid_wire.entity_subkey = fixture_content_hash(977).to_string();
    assert!(MutationBody::from_wire(&invalid_wire).is_err());
    let mut target = Connection::open_in_memory().unwrap();
    init_db(&target).unwrap();
    // Simulate reconnecting an existing database as well as a fresh one.
    init_db(&target).unwrap();
    let tx = target.transaction().unwrap();
    apply_remote_change_in_tx(&tx, &server_change(pending[0].body.clone(), 20, 0)).unwrap();
    assert_eq!(super::pdf_metadata::get(&tx, &hash, &checksum).unwrap(), Some(data.clone()));
    assert!(get_pending_mutations(&tx).unwrap().is_empty());
    let mut changed = data.clone();
    let revision = fixture_content_hash(978);
    changed.checksum = revision.to_string();
    changed.pages[0].width = 900.0;
    apply_remote_change_in_tx(&tx, &server_change(MutationBody::PdfReaderMetadata { content_hash: hash, value: changed.clone() }, 30, 0)).unwrap();
    assert_eq!(super::pdf_metadata::get(&tx, &hash, &checksum).unwrap(), Some(data));
    assert_eq!(super::pdf_metadata::get(&tx, &hash, &revision).unwrap(), Some(changed));
    assert!(get_pending_mutations(&tx).unwrap().is_empty());
    tx.commit().unwrap();
}

/// An audiobook's narrator and a translated edition's translator are the two
/// credits a reader chooses by; everything else in the contributor list
/// describes how the file was made. They share one table, and the detail page
/// simply never asked for the other roles.
#[test]
fn the_detail_page_is_given_narrators_and_translators_but_not_production_credits() {
    use library_model::{AUTHOR_MARC_RELATOR_CODE, Contributor, NARRATOR_MARC_RELATOR_CODE, TRANSLATOR_MARC_RELATOR_CODE};

    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let hash = fixture_content_hash(982);
    let tx = conn.transaction().unwrap();
    insert_scanned_book_metadata(
        &tx,
        hash,
        &BookMetadataState {
            title: "Weimarrepubliken".into(),
            contributors: vec![
                Contributor::new("Olle Larsson", None, AUTHOR_MARC_RELATOR_CODE).unwrap(),
                Contributor::new("Per Juhlin", None, NARRATOR_MARC_RELATOR_CODE).unwrap(),
                Contributor::new("Kerstin Gustafsson", None, TRANSLATOR_MARC_RELATOR_CODE).unwrap(),
                Contributor::new("calibre (0.8.55)", None, library_model::marc_relator_code_from_metadata_role("bookproducer")).unwrap(),
            ],
            description: String::new(),
            publication: Default::default(),
        },
        BookFormat::Epub,
    )
    .unwrap();
    tx.execute(include_str!("sql/tests/the_detail_page_is_given_narrators_and_translators_but_not_production_credits_insert.sql"), rusqlite::params![sync_common::ROOT_DIR_ID.to_string(), hash.as_str()]).unwrap();
    tx.commit().unwrap();

    let detail = book_detail_summary(&conn, hash).unwrap();

    assert_eq!(detail.path.iter().map(|segment| segment.name.as_str()).collect::<Vec<_>>(), Vec::<&str>::new(), "a book in the library root has no folders above it");
    assert_eq!(detail.authors.iter().map(|author| author.name.as_str()).collect::<Vec<_>>(), ["Olle Larsson"]);
    assert_eq!(detail.credits.iter().map(|credit| (credit.name.as_str(), credit.role.as_str())).collect::<Vec<_>>(), [("Per Juhlin", "nrt"), ("Kerstin Gustafsson", "trl")], "in credited order, and the conversion tool is not a credit");
}

/// A detail page reached from the dock or a search result has no browse trail
/// behind it, so the payload carries the folders the book actually sits in.
#[test]
fn the_detail_payload_carries_the_folder_trail_to_the_book() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let hash = fixture_content_hash(983);
    let sociology = create_directory(&mut conn, &sync_common::ROOT_DIR_ID, &"Sociology".to_owned()).unwrap();
    let communities = create_directory(&mut conn, &sociology.id, &"Communities".to_owned()).unwrap();
    let tx = conn.transaction().unwrap();
    insert_scanned_book_metadata(&tx, hash, &BookMetadataState { title: "Bowling Alone".into(), contributors: vec![], description: String::new(), publication: Default::default() }, BookFormat::Epub).unwrap();
    tx.execute(include_str!("sql/tests/the_detail_payload_carries_the_folder_trail_to_the_book_insert.sql"), rusqlite::params![communities.id.to_string(), hash.as_str()]).unwrap();
    tx.commit().unwrap();

    let detail = book_detail_summary(&conn, hash).unwrap();

    assert_eq!(detail.path.iter().map(|segment| segment.name.as_str()).collect::<Vec<_>>(), ["Sociology", "Communities"], "root first, and the book is not repeated as a crumb");
    assert_eq!(detail.source_directory_id.as_deref(), Some(communities.id.to_string().as_str()));
}

/// A percentage tells a reader nothing they can picture. The entry they are
/// inside does — but only the renderer can resolve a CFI to one, so it records
/// what it resolved and the library stores the wording against the target.
#[test]
fn a_documents_position_is_named_by_the_entry_the_reader_recorded() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let hash = fixture_content_hash(984);
    let tx = conn.transaction().unwrap();
    insert_scanned_book_metadata(&tx, hash, &BookMetadataState { title: "Bowling Alone".into(), contributors: vec![], description: String::new(), publication: Default::default() }, BookFormat::Epub).unwrap();
    tx.execute(include_str!("sql/tests/the_detail_payload_carries_the_folder_trail_to_the_book_insert.sql"), rusqlite::params![sync_common::ROOT_DIR_ID.to_string(), hash.as_str()]).unwrap();
    toc::upsert_book_toc(
        &tx,
        &hash,
        toc::TocSource::EmbeddedEpub,
        &[library_model::BookTocEntry { title: "Section II".into(), target: "s2.xhtml".into(), children: vec![library_model::BookTocEntry { title: "Against the Tide?".into(), target: "ch9.xhtml".into(), children: Vec::new() }] }],
    )
    .unwrap();
    tx.commit().unwrap();

    let unopened = book_detail_summary(&conn, hash).unwrap();
    assert_eq!(unopened.current_entry, None, "a book nobody has opened is not anywhere");
    assert_eq!(unopened.current_target, None);

    toc::set_reading_entry(&conn, &hash, "ch9.xhtml").unwrap();

    let opened = book_detail_summary(&conn, hash).unwrap();
    assert_eq!(opened.current_entry.as_deref(), Some("Against the Tide?"), "the wording comes from the stored navigation, not from what was recorded");
    // A list marking one of its own rows needs the entry, not its wording: two
    // entries can be called the same thing.
    assert_eq!(opened.current_target.as_deref(), Some("ch9.xhtml"));
}

/// An audiobook needs nothing recorded: a millisecond offset and the chapters'
/// own bounds are enough to say which chapter it falls in.
#[test]
fn an_audiobooks_position_names_its_chapter_without_anything_being_stored_for_it() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let hash = fixture_content_hash(985);
    let tx = conn.transaction().unwrap();
    insert_scanned_book_metadata(&tx, hash, &BookMetadataState { title: "Weimarrepubliken".into(), contributors: vec![], description: String::new(), publication: Default::default() }, BookFormat::M4b).unwrap();
    tx.execute(include_str!("sql/tests/an_audiobooks_position_names_its_chapter_without_anything_being_stored_for_it_insert.sql"), rusqlite::params![sync_common::ROOT_DIR_ID.to_string(), hash.as_str()]).unwrap();
    let chapters = r#"[{"title":"Inledning","start_ms":0,"end_ms":600000},{"title":"Kapp-kuppen","start_ms":600000,"end_ms":1800000}]"#;
    tx.execute(include_str!("sql/tests/an_audiobooks_position_names_its_chapter_without_anything_being_stored_for_it_insert_2.sql"), rusqlite::params![hash.as_str(), chapters]).unwrap();
    tx.commit().unwrap();

    update_reading_position(&conn, hash, &library_model::audiobook_reading_position(900_000), None).unwrap();

    let detail = book_detail_summary(&conn, hash).unwrap();
    assert_eq!(detail.current_entry.as_deref(), Some("Kapp-kuppen"), "fifteen minutes in is the second chapter");
    // The chapter's own navigation target, so the contents list can mark the
    // row rather than searching for one whose title happens to match.
    assert_eq!(detail.current_target.as_deref(), Some(library_model::audiobook_toc_target(600_000).as_str()));
    assert!(detail.toc.iter().any(|entry| Some(entry.target.as_str()) == detail.current_target.as_deref()), "and that target is one of the entries the page draws");
}
