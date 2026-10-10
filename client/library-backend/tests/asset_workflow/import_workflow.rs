#![cfg(not(target_arch = "wasm32"))]
use library_placement as asset_workflow;
use library_files::AssetStore;
#[tokio::test]
async fn import_workflow_preserves_foreign_files_for_staged_and_direct_writes() {
    for direct in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let db = library_database::Database::open(root.path().join("library.db")).unwrap();
        db.initialize_library().unwrap();
        let store = AssetStore::open(root.path().to_str().unwrap()).unwrap();
        let dir = db.create_directory(&sync_common::ROOT_DIR_ID, &"Shelf".into()).unwrap().id;
        asset_workflow::prepare_directory(&store, &db, dir).await.unwrap();
        std::fs::write(root.path().join("Shelf/book.epub"), b"foreign book").unwrap();
        let bytes = vec![7u8; 150_000];
        let options = library_files::asset_store::ImportOptions { direct };
        let (name, staged) = asset_workflow::stage_import(&store, &db, dir, "book.epub".into(), Box::new(std::io::Cursor::new(bytes.clone())), options).await.unwrap();
        let hash = sync_common::ContentHash::from(blake3::hash(&bytes));
        let staged = staged.into_staged(hash, bytes.len() as u64);
        let _lease = store.lease_book(&hash).await.unwrap();
        let published = asset_workflow::publish_import(&store, &db, dir, &name, staged, false).await.unwrap();
        assert_eq!(published.name, "book 2.epub");
        assert_eq!(std::fs::read(root.path().join("Shelf/book.epub")).unwrap(), b"foreign book");
        assert_eq!(std::fs::read(root.path().join("Shelf/book 2.epub")).unwrap(), bytes);
    }
}
