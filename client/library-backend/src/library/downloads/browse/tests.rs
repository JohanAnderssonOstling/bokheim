use super::*;
use crate::library::downloads::persistent_download_state;
use crate::library::lifecycle::concurrency_tests::fixture;
use crate::DownloadState;
use library_model::BlobKind;
use sync_common::ROOT_DIR_ID;

const EPUB: &[u8] = include_bytes!("../../../../crates/book-inspection/tests/fixtures/back-matter-evidence.epub");
const OTHER_EPUB: &[u8] = include_bytes!("../../../../crates/book-inspection/tests/fixtures/malformed-language.epub");

fn query(directory: DirId) -> LibraryBrowseQuery {
    LibraryBrowseQuery { location: BrowseLocation::Folder(directory), search: String::new(), file_types: Vec::new(), chip_sort: Default::default(), book_sort: Default::default(), languages: Vec::new(), hide_finished: false }
}

async fn nested_books(session: &LibraryResources) -> (DirId, BookId, BookId) {
    let shelf = session.state.write().await.create_directory(&ROOT_DIR_ID, "Shelf").await.unwrap().id;
    let nested = session.state.write().await.create_directory(&shelf, "Nested").await.unwrap().id;
    let keep = Box::pin(session.import(&*session.state.write().await).import_downloaded_book(nested, "Keep.epub", EPUB.to_vec())).await.unwrap();
    let skip = Box::pin(session.import(&*session.state.write().await).import_downloaded_book(nested, "Skip.epub", OTHER_EPUB.to_vec())).await.unwrap();
    session.state.write().await.copy_book_to_directory(&keep, Some(&nested), &shelf).await.unwrap();
    (shelf, keep, skip)
}

#[tokio::test]
async fn download_browse_preserves_filters_across_nested_placements_and_keeps_existing_requests() {
    let root = tempfile::tempdir().unwrap();
    let (service, _handle) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap()).await;
    let session = &service;
    let (shelf, keep, skip) = nested_books(session).await;
    for hash in [keep, skip] {
        session.state.read().await.database().record_remote_asset(BlobKind::Book, &hash).await.unwrap();
        session.remove_local_book_copy(&*session.state.write().await, hash).await.unwrap();
    }
    let mut query = query(shelf);
    query.search = session.state.read().await.database().browse_book_detail(&keep).await.unwrap().book.title;
    session.queue_browse_downloads(&*session.state.write().await, query.clone()).await.unwrap();
    assert!(matches!(persistent_download_state(&*session.state.read().await.database(), &*session.state.read().await.assets(), keep).await.unwrap(), DownloadState::Queued));
    assert!(matches!(persistent_download_state(&*session.state.read().await.database(), &*session.state.read().await.assets(), skip).await.unwrap(), DownloadState::NotDownloaded));
    let queued = session.state.read().await.database().book_download_requests(-1, None, 10).await.unwrap();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].0, keep);
    session.queue_browse_downloads(&*session.state.write().await, query).await.unwrap();
    assert_eq!(session.state.read().await.database().book_download_requests(-1, None, 10).await.unwrap(), queued);
}

#[tokio::test]
async fn folder_eviction_counts_unique_books_and_keeps_device_only_bytes_and_reader_state() {
    let root = tempfile::tempdir().unwrap();
    let (service, _handle) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap()).await;
    let session = &service;
    let (shelf, remote, local) = nested_books(session).await;
    session.state.read().await.database().record_remote_asset(BlobKind::Book, &remote).await.unwrap();
    session.state.read().await.database().update_reading_position(&remote, "epubcfi(/6/2)", Some(42.0), None).await.unwrap();
    let reading = session.state.read().await.database().reading_position(&remote).await.unwrap();
    assert_eq!(session.evict_folder_downloads(&*session.state.write().await, &shelf).await.unwrap(), 1);
    assert!(!session.state.read().await.assets().book_exists(&remote, &*session.state.read().await.database().book_paths(&remote).await.unwrap()).await.unwrap());
    assert!(session.state.read().await.assets().book_exists(&local, &*session.state.read().await.database().book_paths(&local).await.unwrap()).await.unwrap());
    assert_eq!(session.state.read().await.database().reading_position(&remote).await.unwrap(), reading);
    assert!(session.state.read().await.database().book_format(&remote).await.unwrap().is_some());
    assert_eq!(session.evict_folder_downloads(&*session.state.write().await, &shelf).await.unwrap(), 0);
}
