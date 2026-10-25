#![cfg(not(target_arch = "wasm32"))]

use book_inspection::InspectedBook;
use book_model::{BookDetails, BookFormat, BookRecord};
use library_database::{Database, ImportCommit};
use library_files::AssetStore;
use library_model::{BlobKind, PublishedStagedBook};
use library_placement::evict_local_book;
use std::fs;
use sync_common::{BookId, ROOT_DIR_ID};
use tempfile::TempDir;

const BOOK_BYTES: &[u8] = b"the only local copy of an unfinished upload";

fn local_book() -> (TempDir, AssetStore, Database, BookId) {
    let root = TempDir::new().unwrap();
    let assets = AssetStore::open(root.path().to_str().unwrap()).unwrap();
    let database = Database::open(":memory:").unwrap();
    database.initialize_library().unwrap();
    let hash = BookId::from_bytes(*blake3::hash(BOOK_BYTES).as_bytes());
    let record = BookRecord { metadata: BookDetails { title: "Book".into(), ..Default::default() }, description: String::new() };
    fs::write(root.path().join("book.epub"), BOOK_BYTES).unwrap();
    database
        .commit_import(ImportCommit {
            parent_id: ROOT_DIR_ID,
            file_name: "book.epub".into(),
            format: BookFormat::Epub,
            content_hash: hash,
            size_bytes: BOOK_BYTES.len() as u64,
            inspection_version: 1,
            inspection: Some(InspectedBook { pdf: None, audiobook: None, metadata: record.clone(), toc: vec![] }),
            published: PublishedStagedBook { name: "book.epub".into(), relative_path: "/book.epub".into(), published: true, fingerprint: None },
            request_thumbnail: false,
            restore_paths: vec![],
        })
        .unwrap();
    database.record_catalog_answer(&hash, record).unwrap();
    assert!(database.is_book_downloaded(&hash).unwrap());
    assert!(database.transfer_snapshot(&[hash]).unwrap().asset_for(&hash).unwrap().upload.intent.is_some());
    (root, assets, database, hash)
}

#[test]
fn unfinished_upload_keeps_its_file_and_queued_changes() {
    let (root, assets, database, hash) = local_book();
    let before = database.transfer_snapshot(&[hash]).unwrap();

    let error = evict_local_book(&assets, &database, &hash).unwrap_err();

    assert!(error.to_string().contains("upload it before removing the download"));
    assert_eq!(fs::read(root.path().join("book.epub")).unwrap(), BOOK_BYTES);
    assert!(database.is_book_downloaded(&hash).unwrap());
    assert_eq!(database.transfer_snapshot(&[hash]).unwrap().asset_for(&hash).unwrap().upload.intent, before.asset_for(&hash).unwrap().upload.intent);
    assert!(database.stored_sync_status().unwrap().has_unsynced_changes);
}

#[test]
fn uploaded_cover_does_not_allow_removing_the_only_book_copy() {
    let (root, assets, database, hash) = local_book();
    database.record_remote_asset(BlobKind::Thumbnail, &hash).unwrap();

    assert!(evict_local_book(&assets, &database, &hash).is_err());
    assert_eq!(fs::read(root.path().join("book.epub")).unwrap(), BOOK_BYTES);
    assert!(database.is_book_downloaded(&hash).unwrap());
}

#[test]
fn server_backed_book_can_remove_its_file_and_keep_its_entry() {
    let (root, assets, database, hash) = local_book();
    database.record_remote_asset(BlobKind::Book, &hash).unwrap();
    let path = database.book_paths(&hash).unwrap().first().unwrap().clone();

    evict_local_book(&assets, &database, &hash).unwrap();

    assert!(!root.path().join("book.epub").exists());
    assert!(!database.is_book_downloaded(&hash).unwrap());
    assert_eq!(database.book_paths(&hash).unwrap(), vec![path]);
    assert_eq!(database.browse_book_detail(&hash).unwrap().book.title, "Book");
    evict_local_book(&assets, &database, &hash).unwrap();
}
