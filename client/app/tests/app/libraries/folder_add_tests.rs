use super::*;
use crate::LibraryFolderStep;
use crate::app::data::AppDataLocation;
use std::{fs, path::PathBuf};

#[tokio::test]
async fn adding_registered_folder_retries_a_failed_open() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("app-data");
    fs::create_dir_all(&data).unwrap();
    let books = root.path().join("My Library");
    fs::create_dir(&books).unwrap();
    let context = BackendContext::initialize(AppDataLocation::native_path(&data)).unwrap();
    let entry = context.registry.create_with_asset_storage("My Library", Some(books.to_string_lossy().into_owned()), false).unwrap();
    let db = PathBuf::from(context.registry.library_database_locator(entry.library_id(), entry.storage_locator()));
    fs::create_dir_all(db.parent().unwrap()).unwrap();
    fs::write(&db, b"this is not a SQLite database").unwrap();
    let backend = AppBackend::new(context).await.unwrap();
    for _ in 0..2 {
        let (added, failures) = backend.library_service.create_libraries_from_paths_with_asset_storage(vec![books.clone()], false).await.unwrap();
        assert!(added.is_empty());
        assert_eq!(failures.len(), 1, "a registered but unavailable library must not be silently skipped");
        assert_eq!(failures[0].folder_name.as_deref(), Some("My Library"));
        assert_eq!(failures[0].step, LibraryFolderStep::OpenLibrary);
    }
}
