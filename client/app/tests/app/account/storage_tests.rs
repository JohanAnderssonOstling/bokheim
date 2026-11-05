use super::*;
use crate::app::data::AppDataLocation;
use account_contract::BookStorageUsage;
use library_backend::LibraryClient;
use library_model::BlobKind;
use std::path::Path;
use std::sync::Arc;
use sync_common::{BookId, ROOT_DIR_ID};

#[tokio::test]
async fn account_book_storage_uses_titles_without_downloads_and_ranks_cloud_sizes() {
    let root = tempfile::tempdir().unwrap();
    let context = BackendContext::initialize(AppDataLocation::native_path(root.path())).unwrap();
    let backend = AppBackend::new(context).await.unwrap();
    let entry = backend.library_service.create_with_asset_storage("Local cache".into(), false).await.unwrap();
    let id = *entry.library_id();
    let library = LibraryClient::new(backend.library_service.handle(&id).unwrap(), Arc::new(()));
    let large_bytes = include_bytes!("../../../../library-backend/tests/fixtures/cloud-cover.epub");
    let small_bytes = include_bytes!("../../../../library-backend/crates/book-inspection/tests/fixtures/malformed-language.epub");
    let large = library.import_downloaded_book(ROOT_DIR_ID, "Large.epub".into(), large_bytes.to_vec()).await.unwrap();
    let small = library.import_downloaded_book(ROOT_DIR_ID, "Small.epub".into(), small_bytes.to_vec()).await.unwrap();
    backend.library_service.retire(&id);
    {
        let db = library_database::Database::open(&backend.registry.library_database_locator(&id, entry.storage_locator())).unwrap();
        db.record_remote_asset(BlobKind::Book, &large).unwrap();
        db.evict_local_book(&large).unwrap();
    }
    std::fs::remove_file(Path::new(entry.storage_locator()).join("Large.epub")).unwrap();
    backend.library_service.open(&id).await.unwrap();
    let unknown = BookId::from_bytes([7; 32]);
    let books = backend
        .state
        .library_service
        .resolve_server_book_storage(vec![
            BookStorageUsage { book_id: small, used_bytes: small_bytes.len() as u64 },
            BookStorageUsage { book_id: unknown, used_bytes: 1 },
            BookStorageUsage { book_id: large, used_bytes: large_bytes.len() as u64 },
        ])
        .await
        .unwrap();
    assert_eq!(books.iter().map(|book| book.book_id).collect::<Vec<_>>(), [large, small, unknown]);
    assert_eq!(books[0].title, "Cloud cover fixture");
    assert_eq!(books[1].title, "Language metadata test");
    assert_eq!(books[2].title, "Title unavailable");
    assert!(!Path::new(entry.storage_locator()).join("Large.epub").exists(), "reading titles must not download book files");
}
