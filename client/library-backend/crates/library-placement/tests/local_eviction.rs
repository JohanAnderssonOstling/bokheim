#![cfg(not(target_arch = "wasm32"))]

use book_inspection::InspectedBook;
use book_model::{BookDetails, BookFormat, BookRecord};
use library_database::{DatabaseClient, ImportCommit};
use library_files::AssetStore;
use library_model::{BlobKind, PublishedStagedBook};
use library_placement::evict_local_book;
use std::fs;
use sync_common::{BookId, ROOT_DIR_ID};
use tempfile::TempDir;

const BOOK_BYTES: &[u8] = b"the only local copy of an unfinished upload";

async fn local_book() -> (TempDir, AssetStore, DatabaseClient, BookId) {
    let root = TempDir::new().unwrap();
    let assets = AssetStore::open(root.path().to_str().unwrap()).unwrap();
    let database = DatabaseClient::open(":memory:").unwrap();
    database.initialize_library().await.unwrap();
    let hash = BookId::from_bytes(*blake3::hash(BOOK_BYTES).as_bytes());
    let record = BookRecord { metadata: BookDetails { title: "Book".into(), ..Default::default() }, description: String::new() };
    fs::write(root.path().join("book.epub"), BOOK_BYTES).unwrap();
    database
        .commit_import(ImportCommit {
            source_identity: None,
            byte_checksum: None,
            parent_id: ROOT_DIR_ID,
            file_name: "book.epub".into(),
            format: BookFormat::Epub,
            book_id: hash,
            size_bytes: BOOK_BYTES.len() as u64,
            inspection_version: 1,
            inspection: Some(InspectedBook { pdf: None, audiobook: None, metadata: record.clone(), toc: vec![] }),
            published: PublishedStagedBook { name: "book.epub".into(), relative_path: "/book.epub".into(), published: true, fingerprint: None },
            request_thumbnail: false,
            restore_paths: vec![],
        })
        .await
        .unwrap();
    database.record_catalog_answer(&hash, record).await.unwrap();
    assert!(database.is_book_downloaded(&hash).await.unwrap());
    assert!(database.transfer_snapshot(&[hash]).await.unwrap().asset_for(&hash).unwrap().upload.intent.is_some());
    (root, assets, database, hash)
}

#[tokio::test]
async fn unfinished_upload_keeps_its_file_and_queued_changes() {
    let (root, assets, database, hash) = local_book().await;
    let before = database.transfer_snapshot(&[hash]).await.unwrap();

    let error = evict_local_book(&assets, &database, &hash).await.unwrap_err();

    assert!(error.to_string().contains("upload it before removing the download"));
    assert_eq!(fs::read(root.path().join("book.epub")).unwrap(), BOOK_BYTES);
    assert!(database.is_book_downloaded(&hash).await.unwrap());
    assert_eq!(database.transfer_snapshot(&[hash]).await.unwrap().asset_for(&hash).unwrap().upload.intent, before.asset_for(&hash).unwrap().upload.intent);
    assert!(database.stored_sync_status().await.unwrap().has_unsynced_changes);
}

#[tokio::test]
async fn uploaded_cover_does_not_allow_removing_the_only_book_copy() {
    let (root, assets, database, hash) = local_book().await;
    database.record_remote_asset(BlobKind::Thumbnail, &hash).await.unwrap();

    assert!(evict_local_book(&assets, &database, &hash).await.is_err());
    assert_eq!(fs::read(root.path().join("book.epub")).unwrap(), BOOK_BYTES);
    assert!(database.is_book_downloaded(&hash).await.unwrap());
}

#[tokio::test]
async fn server_backed_book_can_remove_its_file_and_keep_its_entry() {
    let (root, assets, database, hash) = local_book().await;
    database.record_remote_asset(BlobKind::Book, &hash).await.unwrap();
    let path = database.book_paths(&hash).await.unwrap().first().unwrap().clone();

    evict_local_book(&assets, &database, &hash).await.unwrap();

    assert!(!root.path().join("book.epub").exists());
    assert!(!database.is_book_downloaded(&hash).await.unwrap());
    assert_eq!(database.book_paths(&hash).await.unwrap(), vec![path]);
    assert_eq!(database.browse_book_detail(&hash).await.unwrap().book.title, "Book");
    evict_local_book(&assets, &database, &hash).await.unwrap();
}
