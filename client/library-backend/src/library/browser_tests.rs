use super::concurrency_tests::fixture;
use crate::LibraryClient;
use std::fs;
use std::sync::Arc;

#[tokio::test]
async fn browser_download_is_registered_before_import_returns() {
    let data = tempfile::tempdir().unwrap();
    let (_service, handle) = fixture(data.path(), "http://127.0.0.1:1".parse().unwrap()).await;
    let library = LibraryClient::new(handle.clone(), Arc::new(()));
    let exercise = async {
        let staging = library.browser_downloads().await.unwrap();
        let path = staging.directory().join("browser.epub");
        let bytes = include_bytes!("../../crates/book-inspection/tests/fixtures/back-matter-evidence.epub");
        fs::write(&path, bytes).unwrap();

        staging.import(path.clone()).await.unwrap();
        assert!(!path.exists());
        assert_eq!(library.book_count().await.unwrap(), 1);
        assert_eq!(fs::read(data.path().join("browser.epub")).unwrap(), bytes);
        handle.shutdown();
    };
    exercise.await;
}

#[tokio::test]
async fn browser_download_reports_import_failures() {
    let data = tempfile::tempdir().unwrap();
    let (_service, handle) = fixture(data.path(), "http://127.0.0.1:1".parse().unwrap()).await;
    let library = LibraryClient::new(handle.clone(), Arc::new(()));
    let exercise = async {
        let staging = library.browser_downloads().await.unwrap();
        let path = staging.directory().join("broken.epub");
        fs::write(&path, b"invalid EPUB").unwrap();

        assert!(staging.import(path).await.is_err());
        assert_eq!(library.book_count().await.unwrap(), 0);
        handle.shutdown();
    };
    exercise.await;
}
