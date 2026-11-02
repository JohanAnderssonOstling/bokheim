use super::*;
use crate::app::data::AppDataLocation;

#[test]
fn missing_folder_and_selected_file_are_distinct() {
    let root = tempfile::tempdir().unwrap();
    let (step, error) = check_library_folder(&root.path().join("missing")).unwrap_err();
    assert_eq!(step, LibraryFolderStep::CheckFolder);
    assert_eq!(error.failure(), Some(Failure::LibraryFolderUnavailable));
    let file = root.path().join("book.epub");
    fs::write(&file, "book").unwrap();
    assert_eq!(check_library_folder(&file).unwrap_err().0, LibraryFolderStep::NotAFolder);
}

#[cfg(unix)]
#[test]
fn readable_folder_without_write_permission_passes_the_access_check() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o555)).unwrap();
    let result = check_library_folder(root.path());
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(result.unwrap(), root.path().canonicalize().unwrap());
}

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
    let backend = AppBackend::new_without_libraries(context).unwrap();
    for _ in 0..2 {
        let (added, failures) = backend.create_libraries_from_paths_with_asset_storage(vec![books.clone()], false).unwrap();
        assert!(added.is_empty());
        assert_eq!(failures.len(), 1, "a registered but unavailable library must not be silently skipped");
        assert_eq!(failures[0].folder_name.as_deref(), Some("My Library"));
        assert_eq!(failures[0].step, LibraryFolderStep::OpenLibrary);
    }
}
