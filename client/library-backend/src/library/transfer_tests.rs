//! Integration coverage across two independent libraries and asset stores.
use super::concurrency_tests::fixture;
use super::*;
use crate::client::TransferSelection;
use crate::DirId;
use crate::LibraryClient;
use book_model::{Annotation, AnnotationAnchor, AnnotationStyle};
use std::sync::Arc;
use sync_common::{BookId, ROOT_DIR_ID};

const EPUB: &[u8] = include_bytes!("../../crates/book-inspection/tests/fixtures/back-matter-evidence.epub");
const OTHER_EPUB: &[u8] = include_bytes!("../../crates/book-inspection/tests/fixtures/malformed-language.epub");

async fn seed(session: &LibraryResources, parent: DirId, name: &str, bytes: &[u8]) -> BookId {
    let input = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(input.path(), bytes).unwrap();
    session.import(&*session.state.write().await).import_stored_book(parent, name.into(), input.path().into()).await.unwrap()
}
fn selection(folders: Vec<DirId>, books: Vec<BookId>, source: Option<DirId>) -> TransferSelection {
    TransferSelection { folders, books, source }
}
fn clients(source: &LibraryHandle, destination: &LibraryHandle) -> (LibraryClient, LibraryClient) {
    (LibraryClient::new(source.clone(), Arc::new(())), LibraryClient::new(destination.clone(), Arc::new(())))
}

#[tokio::test]
async fn cross_library_copy_preserves_nested_empty_folders_bytes_and_reader_state() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let (source_service, source) = fixture(first.path(), "http://127.0.0.1:1".parse().unwrap()).await;
    let (destination_service, destination) = fixture(second.path(), "http://127.0.0.1:1".parse().unwrap()).await;
    let src = source_service.clone();
    let dst = destination_service.clone();
    let shelf = src.state.write().await.create_directory(&ROOT_DIR_ID, "Shelf").await.unwrap().id;
    let nested = src.state.write().await.create_directory(&shelf, "Nested").await.unwrap().id;
    src.state.write().await.create_directory(&nested, "Empty").await.unwrap();
    let hash = seed(&src, nested, "Novel.epub", EPUB).await;
    src.state.write().await.copy_book_to_directory(&hash, Some(&nested), &shelf).await.unwrap();
    src.state.read().await.database().update_reading_position(&hash, "epubcfi(/6/2)", Some(42.0), None).await.unwrap();
    let note = Annotation {
        id: "transfer-note".into(),
        book_id: hash,
        anchor: AnnotationAnchor::epub_cfi("epubcfi(/6/2)"),
        exact_text: "Text".into(),
        style: AnnotationStyle::Underline,
        color: 0xffffff,
        note: "Remember".into(),
        created_at: 7,
        toc_ordinal: None,
        progress: Some(42.0),
        modified_at: 9,
        deleted: false,
    };
    src.state.read().await.database().upsert_annotation(&note).await.unwrap();
    let original = src.state.read().await.database().book_transfer_state(hash).await.unwrap();
    let (from, to) = clients(&source, &destination);
    let exercise = async {
        // Selecting both an ancestor and child copies the tree only once.
        let outcome = to.transfer_from(&from, selection(vec![shelf, nested], vec![], None), ROOT_DIR_ID, false).await;
        assert!(outcome.error.is_none(), "{:?}", outcome.error);
        assert_eq!(outcome.created_folders.len(), 1);
        let folders = dst.state.read().await.database().folder_destinations().await.unwrap();
        assert_eq!(folders.len(), 4);
        assert!(folders.iter().any(|folder| folder.label == "Empty"));
        let destination_hash = dst.state.read().await.database().book_for_local_content(blake3::hash(EPUB).into()).await.unwrap().unwrap();
        let expected = original.retarget(destination_hash).unwrap();
        let transferred = dst.state.read().await.database().book_transfer_state(destination_hash).await.unwrap();
        assert_eq!(transferred.addition, expected.addition);
        assert_eq!(transferred.reading, expected.reading);
        assert_eq!(transferred.annotations, expected.annotations);
        let copied = dst.state.read().await.database().transfer_plan(selection(outcome.created_folders.clone(), vec![], None)).await.unwrap();
        assert_eq!(copied.books.len(), 2, "repeated placements must survive");
        for path in dst.state.read().await.database().book_paths(&destination_hash).await.unwrap() {
            assert_eq!(std::fs::read(second.path().join(path.as_str().trim_start_matches('/'))).unwrap(), EPUB);
        }
        assert!(src.state.read().await.database().is_live_destination(&shelf).await.unwrap());
        source.shutdown();
        destination.shutdown();
    };
    exercise.await;
}

#[tokio::test]
async fn cross_library_failed_copy_preserves_the_entire_source_selection_and_reports_partial_destination_changes() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let (source_service, source) = fixture(first.path(), "http://127.0.0.1:1".parse().unwrap()).await;
    let (destination_service, destination) = fixture(second.path(), "http://127.0.0.1:1".parse().unwrap()).await;
    let src = source_service.clone();
    let dst = destination_service.clone();
    let good = seed(&src, ROOT_DIR_ID, "Good.epub", EPUB).await;
    let broken = seed(&src, ROOT_DIR_ID, "Broken.epub", OTHER_EPUB).await;
    let path = src.state.read().await.database().book_paths(&broken).await.unwrap()[0].clone();
    std::fs::write(first.path().join(path.as_str().trim_start_matches('/')), b"corrupt").unwrap();
    let (from, to) = clients(&source, &destination);
    let exercise = async {
        let outcome = to.transfer_from(&from, selection(vec![], vec![good, broken], Some(ROOT_DIR_ID)), ROOT_DIR_ID, true).await;
        assert!(outcome.error.is_some());
        let destination_hash = dst.state.read().await.database().book_for_local_content(blake3::hash(EPUB).into()).await.unwrap().unwrap();
        assert_eq!(outcome.created_placements, vec![(destination_hash, ROOT_DIR_ID)]);
        assert!(outcome.removed_placements.is_empty());
        assert!(src.state.read().await.database().library_trash().await.unwrap().books.is_empty());
        assert!(src.state.read().await.database().transfer_plan(selection(vec![], vec![good, broken], Some(ROOT_DIR_ID))).await.is_ok());
        assert_eq!(dst.state.read().await.database().book_count().await.unwrap(), 1);
        source.shutdown();
        destination.shutdown();
    };
    exercise.await;
}

#[tokio::test]
async fn cross_library_move_rejects_a_source_tree_changed_during_copy() {
    let root = tempfile::tempdir().unwrap();
    let (service, _handle) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap()).await;
    let shelf = service.state.write().await.create_directory(&ROOT_DIR_ID, "Shelf").await.unwrap().id;
    seed(&service, shelf, "Novel.epub", EPUB).await;
    let plan = service.state.read().await.database().transfer_plan(selection(vec![shelf], vec![], None)).await.unwrap();
    service.state.write().await.create_directory(&shelf, "Added during copy").await.unwrap();
    assert!(service.state.read().await.database().remove_transferred_selection(&plan).await.is_err());
    assert!(service.state.read().await.database().is_live_destination(&shelf).await.unwrap());
    assert!(service.state.read().await.database().library_trash().await.unwrap().books.is_empty());
}

#[tokio::test]
async fn cross_library_move_to_existing_book_preserves_destination_state_and_other_source_placements() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let (source_service, source) = fixture(first.path(), "http://127.0.0.1:1".parse().unwrap()).await;
    let (destination_service, destination) = fixture(second.path(), "http://127.0.0.1:1".parse().unwrap()).await;
    let src = source_service.clone();
    let dst = destination_service.clone();
    let hash = seed(&src, ROOT_DIR_ID, "Novel.epub", EPUB).await;
    let destination_hash = seed(&dst, ROOT_DIR_ID, "Existing.epub", EPUB).await;
    let retained = src.state.write().await.create_directory(&ROOT_DIR_ID, "Retained").await.unwrap().id;
    src.state.write().await.copy_book_to_directory(&hash, Some(&ROOT_DIR_ID), &retained).await.unwrap();
    src.state.read().await.database().update_reading_position(&hash, "epubcfi(/6/2)", Some(20.0), None).await.unwrap();
    dst.state.read().await.database().update_reading_position(&destination_hash, "epubcfi(/6/4)", Some(60.0), None).await.unwrap();
    let original = dst.state.read().await.database().book_transfer_state(destination_hash).await.unwrap();
    let (from, to) = clients(&source, &destination);
    let exercise = async {
        let outcome = to.transfer_from(&from, selection(vec![], vec![hash], None), ROOT_DIR_ID, true).await;
        assert!(outcome.error.is_none(), "{:?}", outcome.error);
        assert!(outcome.created_placements.is_empty(), "existing destination placement is excluded from undo");
        assert_eq!(outcome.removed_placements, vec![(hash, ROOT_DIR_ID)]);
        let after = dst.state.read().await.database().book_transfer_state(destination_hash).await.unwrap();
        assert_eq!(after.addition, original.addition);
        assert_eq!(after.reading, original.reading);
        assert!(second.path().join("Existing.epub").is_file());
        assert!(!second.path().join("Novel.epub").exists());
        assert!(src.state.read().await.database().transfer_plan(selection(vec![], vec![hash], Some(retained))).await.is_ok());
        assert!(src.state.read().await.database().transfer_plan(selection(vec![], vec![hash], Some(ROOT_DIR_ID))).await.is_err());
        assert!(src.state.read().await.database().library_trash().await.unwrap().books.is_empty());
        source.shutdown();
        destination.shutdown();
    };
    exercise.await;
}

#[tokio::test]
async fn cross_library_import_rejects_wrong_bytes_before_publishing() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let (source_service, _) = fixture(first.path(), "http://127.0.0.1:1".parse().unwrap()).await;
    let (destination_service, _) = fixture(second.path(), "http://127.0.0.1:1".parse().unwrap()).await;
    let hash = seed(&source_service, ROOT_DIR_ID, "Novel.epub", EPUB).await;
    let state = source_service.state.read().await.database().book_transfer_state(hash).await.unwrap();
    let result = destination_service.import(&*destination_service.state.write().await).import_transferred_book(ROOT_DIR_ID, "Novel.epub".into(), Box::new(std::io::Cursor::new(b"bad bytes".to_vec())), b"bad bytes".len() as u64, state).await;
    assert!(result.unwrap_err().to_string().contains("verification"));
    assert!(destination_service.state.read().await.database().book_paths(&hash).await.unwrap().is_empty());
    assert!(!second.path().join("Novel.epub").exists());
    assert!(!source_service.state.read().await.database().book_paths(&hash).await.unwrap().is_empty());
}

#[tokio::test]
async fn cross_library_folder_move_uses_fresh_ids_resolves_name_collisions_and_restores_source_on_undo() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let (source_service, source) = fixture(first.path(), "http://127.0.0.1:1".parse().unwrap()).await;
    let (destination_service, destination) = fixture(second.path(), "http://127.0.0.1:1".parse().unwrap()).await;
    let src = source_service.clone();
    let dst = destination_service.clone();
    let shelf = src.state.write().await.create_directory(&ROOT_DIR_ID, "Shelf").await.unwrap().id;
    let hash = seed(&src, shelf, "Novel.epub", EPUB).await;
    let existing = dst.state.write().await.create_directory(&ROOT_DIR_ID, "Shelf").await.unwrap().id;
    let (from, to) = clients(&source, &destination);
    let exercise = async {
        let outcome = to.transfer_from(&from, selection(vec![shelf], vec![], None), ROOT_DIR_ID, true).await;
        assert!(outcome.error.is_none(), "{:?}", outcome.error);
        assert_eq!(outcome.removed_folders, vec![(shelf, Some(ROOT_DIR_ID))]);
        let copy = outcome.created_folders[0];
        assert_ne!(copy, shelf);
        assert_ne!(copy, existing);
        assert!(!src.state.read().await.database().is_live_destination(&shelf).await.unwrap());
        assert_eq!(src.state.read().await.database().library_trash().await.unwrap().books.len(), 1);
        let folders = dst.state.read().await.database().folder_destinations().await.unwrap();
        assert_ne!(folders.iter().find(|folder| folder.id == copy).unwrap().label, "Shelf");
        assert!(dst.state.read().await.database().is_live_destination(&existing).await.unwrap());
        src.run_file_jobs().await.unwrap();
        from.restore_directory(shelf, Some(ROOT_DIR_ID)).await.unwrap();
        to.move_directory_to_trash(copy).await.unwrap();
        src.run_file_jobs().await.unwrap();
        assert!(src.state.read().await.database().is_live_destination(&shelf).await.unwrap());
        assert!(dst.state.read().await.database().is_live_destination(&existing).await.unwrap());
        assert!(!dst.state.read().await.database().is_live_destination(&copy).await.unwrap());
        let path = src.state.read().await.database().book_paths(&hash).await.unwrap()[0].clone();
        assert_eq!(std::fs::read(first.path().join(path.as_str().trim_start_matches('/'))).unwrap(), EPUB);
        source.shutdown();
        destination.shutdown();
    };
    exercise.await;
}
