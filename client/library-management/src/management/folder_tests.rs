use super::*;

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
