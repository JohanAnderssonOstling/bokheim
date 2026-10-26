#![cfg(not(target_arch = "wasm32"))]

use library_files::AssetStore;
use std::io::{Read, Seek};
use sync_common::BookId;

fn path(value: &str) -> library_model::RelativeBookPath {
    library_model::RelativeBookPath::parse(value).unwrap()
}

#[tokio::test]
async fn readers_and_availability_use_only_selected_paths() {
    let root = tempfile::tempdir().unwrap();
    let assets = AssetStore::open(root.path().to_str().unwrap()).unwrap();
    let bytes = b"selected book";
    let hash = BookId::from(blake3::hash(bytes));
    std::fs::write(root.path().join("book.epub"), bytes).unwrap();
    assert!(!assets.book_exists(&hash, &[]).unwrap());
    assert!(assets.open_book_reader(&hash, Vec::new()).await.unwrap().is_none());
    let paths = vec![path("missing.epub"), path("book.epub")];
    assert!(assets.book_exists(&hash, &paths).unwrap());
    let mut reader = assets.open_book_reader(&hash, paths.clone()).await.unwrap().unwrap();
    let mut actual = Vec::new();
    reader.read_to_end(&mut actual).unwrap();
    assert_eq!(actual, bytes);
    assert!(assets.verified_book_reader(hash, paths).await.unwrap().is_some());
    std::fs::remove_file(root.path().join("book.epub")).unwrap();
    assert!(!assets.book_exists(&hash, &[path("book.epub")]).unwrap());
    assert!(assets.open_book_reader(&hash, vec![path("book.epub")]).await.unwrap().is_none());
}

#[tokio::test]
async fn common_book_opener_archives_folder_tracks_and_cover() {
    let root = tempfile::tempdir().unwrap();
    let assets = AssetStore::open(root.path().to_str().unwrap()).unwrap();
    let folder = root.path().join("Story");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("01.mp3"), b"opening").unwrap();
    std::fs::write(folder.join("cover.jpg"), b"folder artwork").unwrap();
    let hash = BookId::from(audiobook_folder::folder_hash(&folder).unwrap());
    assert!(assets.book_exists(&hash, &[path("Story")]).unwrap());
    let mut reader = assets.open_book_reader(&hash, vec![path("Story")]).await.unwrap().unwrap();
    assert_eq!(audiobook_archive::archived_cover(&mut reader).unwrap(), Some(b"folder artwork".to_vec()));
    reader.rewind().unwrap();
    let mut archive = zip::ZipArchive::new(reader).unwrap();
    let mut bytes = Vec::new();
    archive.by_name("01.mp3").unwrap().read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"opening");
}

#[tokio::test]
async fn m4b_cover_uses_the_selected_book_path() {
    let root = tempfile::tempdir().unwrap();
    let assets = AssetStore::open(root.path().to_str().unwrap()).unwrap();
    std::fs::write(root.path().join("Story.m4b"), include_bytes!("fixtures/embedded-cover.m4b")).unwrap();
    std::fs::write(root.path().join("Story.jpg"), b"selected artwork").unwrap();
    assert!(assets.m4b_sidecar_cover(Vec::new()).await.unwrap().is_none());
    assert_eq!(assets.m4b_sidecar_cover(vec![path("Story.m4b")]).await.unwrap(), Some(b"selected artwork".to_vec()));
}
