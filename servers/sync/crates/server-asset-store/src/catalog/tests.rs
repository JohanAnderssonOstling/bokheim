use super::*;
use crate::AssetKind;
use std::io::Cursor;
use tempfile::TempDir;
use tokio::io::AsyncReadExt;

async fn store() -> (TempDir, FsAssetStore, CatalogStorage, BookId) {
    let root = TempDir::new().unwrap();
    let catalog = CatalogStorage { books: root.path().join("public/books"), pins: root.path().join("private-pins") };
    std::fs::create_dir_all(&catalog.books).unwrap();
    let hash = BookId::from_bytes(*blake3::hash(b"abc").as_bytes());
    std::fs::write(catalog.books.join(format!("{hash}.epub")), b"abc").unwrap();
    let store = FsAssetStore::new(root.path().join("sync/books"), root.path().join("sync/covers")).with_catalog(catalog.clone());
    store.initialize().await.unwrap();
    (root, store, catalog, hash)
}

#[tokio::test]
async fn retained_catalog_inode_replaces_duplicates_and_survives_unpublishing() {
    let (_root, store, catalog, hash) = store().await;
    store.put("alice", AssetKind::Book, &hash, Some(3), Box::new(Cursor::new(b"abc"))).await.unwrap();
    let duplicate = store.path("", AssetKind::Book, &hash).unwrap();
    assert!(duplicate.exists());
    let prepared = store.prepare_catalog_book(&hash).await.unwrap().unwrap();
    assert_eq!(prepared.size_bytes(), 3);
    prepared.publish().unwrap();
    assert!(!duplicate.exists(), "existing sync bytes are consolidated");
    let public = catalog.books.join(format!("{hash}.epub"));
    let pin = store.catalog_pin(&hash).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(std::fs::metadata(&public).unwrap().ino(), std::fs::metadata(&pin).unwrap().ino());
    }
    // An older client that sends the complete file still must not recreate a copy.
    store.put("bob", AssetKind::Book, &hash, Some(3), Box::new(Cursor::new(b"abc"))).await.unwrap();
    assert!(!duplicate.exists());
    std::fs::remove_file(&public).unwrap();
    assert!(store.prepare_catalog_book(&hash).await.unwrap().is_none(), "a private retained pin does not grant public access");
    assert_eq!(store.book_hashes().await.unwrap(), vec![hash]);
    assert_eq!(store.size("alice", AssetKind::Book, &hash).await.unwrap(), Some(3));
    let mut asset = store.open_from("alice", AssetKind::Book, &hash, 1).await.unwrap().unwrap();
    let mut bytes = Vec::new();
    asset.reader.read_to_end(&mut bytes).await.unwrap();
    assert_eq!(bytes, b"bc");
    store.remove_book(&hash).unwrap();
    assert!(!pin.exists());
}

#[tokio::test]
async fn cleanup_unlinks_only_the_private_pin_and_rejected_preparations_leave_no_pin() {
    let (_root, store, catalog, hash) = store().await;
    drop(store.prepare_catalog_book(&hash).await.unwrap().unwrap());
    assert_eq!(std::fs::read_dir(&catalog.pins).unwrap().count(), 0);
    store.prepare_catalog_book(&hash).await.unwrap().unwrap().publish().unwrap();
    store.remove_book(&hash).unwrap();
    assert_eq!(std::fs::read(catalog.books.join(format!("{hash}.epub"))).unwrap(), b"abc");
    assert_eq!(std::fs::read_dir(&catalog.pins).unwrap().count(), 0);
    let other = BookId::from_bytes(*blake3::hash(b"different").as_bytes());
    assert!(store.prepare_catalog_book(&other).await.unwrap().is_none());
    std::fs::write(catalog.books.join(format!("{other}.epub")), b"abc").unwrap();
    assert!(matches!(store.prepare_catalog_book(&other).await, Err(AssetError::HashMismatch)));
    assert_eq!(std::fs::read_dir(&catalog.pins).unwrap().count(), 0);
}

#[cfg(unix)]
#[tokio::test]
async fn public_symlinks_cannot_adopt_private_files() {
    let (root, store, catalog, hash) = store().await;
    let public = catalog.books.join(format!("{hash}.epub"));
    std::fs::remove_file(&public).unwrap();
    let private = root.path().join("private.epub");
    std::fs::write(&private, b"abc").unwrap();
    std::os::unix::fs::symlink(private, &public).unwrap();
    assert!(matches!(store.prepare_catalog_book(&hash).await, Err(AssetError::InvalidInput(_))));
    assert_eq!(std::fs::read_dir(&catalog.pins).unwrap().count(), 0);
}

#[tokio::test]
async fn catalog_pins_cannot_be_placed_inside_the_public_books_directory() {
    let root = TempDir::new().unwrap();
    let books = root.path().join("public/books");
    std::fs::create_dir_all(&books).unwrap();
    let store = FsAssetStore::new(root.path().join("sync/books"), root.path().join("sync/covers")).with_catalog(CatalogStorage { pins: books.join("pins"), books });
    assert!(matches!(store.initialize().await, Err(AssetError::Storage(_))));
}

#[tokio::test]
async fn catalog_pins_cannot_alias_private_upload_storage_or_public_covers() {
    let root = TempDir::new().unwrap();
    let books = root.path().join("public/books");
    std::fs::create_dir_all(&books).unwrap();
    for pins in [root.path().join("sync/books"), root.path().join("public/covers")] {
        let store = FsAssetStore::new(root.path().join("sync/books"), root.path().join("sync/covers")).with_catalog(CatalogStorage { books: books.clone(), pins });
        assert!(matches!(store.initialize().await, Err(AssetError::Storage(_))));
    }
}

#[tokio::test]
async fn crash_recovery_removes_only_expired_preparation_links() {
    let (_root, store, catalog, hash) = store().await;
    let public = catalog.books.join(format!("{hash}.epub"));
    let stale = catalog.pins.join(format!(".prepare-1-{}", uuid::Uuid::new_v4()));
    std::fs::hard_link(&public, &stale).unwrap();
    let active = store.prepare_catalog_book(&hash).await.unwrap().unwrap();
    let active_path = active.temp.path().to_owned();
    store.remove_stale_catalog_preparations().await.unwrap();
    assert!(!stale.exists());
    assert!(active_path.exists());
    assert!(public.exists());
    active.publish().unwrap();
    store.remove_stale_catalog_preparations().await.unwrap();
    assert!(store.catalog_pin(&hash).unwrap().exists());
}
