#![cfg(not(target_arch = "wasm32"))]

use library_files::filesystem::place_into_directory;

#[test]
fn external_move_preserves_colliding_files_and_can_retry_in_place() {
    let temporary = tempfile::tempdir().unwrap();
    let library = temporary.path().join("Library");
    std::fs::create_dir(&library).unwrap();
    let source = temporary.path().join("book.epub");
    std::fs::write(&source, b"dropped book").unwrap();
    std::fs::write(library.join("book.epub"), b"existing book").unwrap();
    let moved = place_into_directory(&source, &library, &[], true).unwrap();
    assert!(!source.exists());
    assert_eq!(moved, library.join("book 2.epub"));
    assert_eq!(std::fs::read(library.join("book.epub")).unwrap(), b"existing book");
    assert_eq!(std::fs::read(&moved).unwrap(), b"dropped book");
    assert_eq!(place_into_directory(&moved, &library, &[], true).unwrap(), moved);
    assert!(!library.join("book 3.epub").exists());
}

#[test]
fn external_move_moves_the_complete_folder_including_sidecars() {
    let temporary = tempfile::tempdir().unwrap();
    let library = temporary.path().join("Library");
    let source = temporary.path().join("Books");
    std::fs::create_dir(&library).unwrap();
    std::fs::create_dir_all(source.join("Nested")).unwrap();
    std::fs::write(source.join("Nested/book.epub"), b"book").unwrap();
    std::fs::write(source.join("cover.jpg"), b"cover").unwrap();
    std::fs::create_dir(library.join("Books")).unwrap();
    let moved = place_into_directory(&source, &library, &[], true).unwrap();
    assert!(!source.exists());
    assert_eq!(moved, library.join("Books 2"));
    assert_eq!(std::fs::read(moved.join("Nested/book.epub")).unwrap(), b"book");
    assert_eq!(std::fs::read(moved.join("cover.jpg")).unwrap(), b"cover");
}

#[test]
fn external_move_respects_logical_names_and_rejects_moving_into_itself() {
    let temporary = tempfile::tempdir().unwrap();
    let library = temporary.path().join("Library");
    std::fs::create_dir(&library).unwrap();
    let source = temporary.path().join("book.epub");
    std::fs::write(&source, b"book").unwrap();
    assert_eq!(place_into_directory(&source, &library, &["book.epub".into()], true).unwrap(), library.join("book 2.epub"));
    let before = std::fs::read(library.join("book 2.epub")).unwrap();
    assert!(place_into_directory(temporary.path(), &library, &[], true).is_err());
    assert_eq!(std::fs::read(library.join("book 2.epub")).unwrap(), before);
}

#[test]
fn external_copy_preserves_originals_and_places_complete_folders() {
    let temporary = tempfile::tempdir().unwrap();
    let library = temporary.path().join("Library");
    let source = temporary.path().join("Books");
    std::fs::create_dir(&library).unwrap();
    std::fs::create_dir_all(source.join("Nested")).unwrap();
    std::fs::write(source.join("Nested/book.epub"), b"book").unwrap();
    std::fs::write(source.join("cover.jpg"), b"cover").unwrap();
    std::fs::write(source.join(".notes"), b"hidden sidecar").unwrap();
    std::fs::create_dir(library.join("Books")).unwrap();
    let copied = place_into_directory(&source, &library, &[], false).unwrap();
    assert_eq!(copied, library.join("Books 2"));
    for name in ["Nested/book.epub", "cover.jpg", ".notes"] {
        assert_eq!(std::fs::read(source.join(name)).unwrap(), std::fs::read(copied.join(name)).unwrap());
    }
    assert_eq!(place_into_directory(&copied, &library, &[], false).unwrap(), copied);
    assert!(!library.join("Books 3").exists());
}

#[test]
fn external_copy_preserves_colliding_files() {
    let temporary = tempfile::tempdir().unwrap();
    let library = temporary.path().join("Library");
    std::fs::create_dir(&library).unwrap();
    let source = temporary.path().join("book.epub");
    std::fs::write(&source, b"copied book").unwrap();
    std::fs::write(library.join("book.epub"), b"existing book").unwrap();
    let copied = place_into_directory(&source, &library, &[], false).unwrap();
    assert_eq!(copied, library.join("book 2.epub"));
    assert_eq!(std::fs::read(&source).unwrap(), b"copied book");
    assert_eq!(std::fs::read(copied).unwrap(), b"copied book");
    assert_eq!(std::fs::read(library.join("book.epub")).unwrap(), b"existing book");
}

#[cfg(unix)]
#[test]
fn failed_folder_copy_never_exposes_a_partial_destination() {
    let temporary = tempfile::tempdir().unwrap();
    let library = temporary.path().join("Library");
    let source = temporary.path().join("Books");
    std::fs::create_dir(&library).unwrap();
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("book.epub"), b"book").unwrap();
    std::os::unix::fs::symlink("missing-target", source.join("unsupported-link")).unwrap();

    assert!(place_into_directory(&source, &library, &[], false).is_err());
    assert!(!library.join("Books").exists());
    assert_eq!(std::fs::read(source.join("book.epub")).unwrap(), b"book");
    assert_eq!(std::fs::read_dir(&library).unwrap().count(), 0);
}
