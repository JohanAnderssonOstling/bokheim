use content_address::{BookId, ContentHash};
use server_asset_store::{AssetError, AssetKind, FsAssetStore};
use server_postgres::{AssetApplication, PostgresDatabase, PostgresSyncRepository};
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use std::{io::Cursor, sync::Arc};
use sync_common::{
    LibraryId,
    api::{assets::BlobManifestEntry, libraries::LibraryNameRequest},
};
use tokio::io::AsyncReadExt;

struct Fixture {
    database: PostgresDatabase,
    assets: AssetApplication,
    admin: PgPool,
    schema: String,
    root: tempfile::TempDir,
}
impl Fixture {
    async fn new() -> Self {
        let url = std::env::var("SYNC_E2E_DATABASE_URL").unwrap();
        let admin = PgPool::connect(&url).await.unwrap();
        let schema = format!("stable_content_{}", LibraryId::new_v4().simple());
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        let search_path = format!("SET search_path TO {schema}");
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .after_connect(move |connection, _| {
                let statement = search_path.clone();
                Box::pin(async move { sqlx::query(&statement).execute(connection).await.map(|_| ()) })
            })
            .connect(&url)
            .await
            .unwrap();
        let database = PostgresDatabase::from_pool(pool);
        server_postgres::migrate(&database).await.unwrap();
        sqlx::query("INSERT INTO users(id) VALUES ('alice'),('bob')").execute(database.pool()).await.unwrap();
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(FsAssetStore::new(root.path().join("books"), root.path().join("thumbnails")));
        store.initialize().await.unwrap();
        let assets = AssetApplication::new(database.clone(), store);
        Self { database, assets, admin, schema, root }
    }
    async fn library(&self, user: &str) -> LibraryId {
        let library = LibraryId::new_v4();
        PostgresSyncRepository::new(self.database.clone()).create_library(user, &LibraryNameRequest { library_id: library, library_name: "Books".into(), storage: Default::default() }).await.unwrap();
        library
    }
    async fn put(&self, user: &str, library: LibraryId, book: BookId, bytes: Vec<u8>) {
        let checksum = ContentHash::from(blake3::hash(&bytes));
        self.assets.put_book_with_checksum(user, &library, &book, checksum, bytes.len() as u64, Box::new(Cursor::new(bytes))).await.unwrap();
    }
    async fn cleanup(self) {
        self.database.pool().close().await;
        sqlx::query(&format!("DROP SCHEMA {} CASCADE", self.schema)).execute(&self.admin).await.unwrap();
    }
}

#[tokio::test]
#[ignore = "requires SYNC_E2E_DATABASE_URL"]
async fn first_uploaded_revision_is_immutable_independently_of_book_identity() {
    let f = Fixture::new().await;
    let library = f.library("alice").await;
    let initial = ContentHash::from(blake3::hash(b"initial PDF"));
    let book = BookId::for_local_content(library.as_bytes(), initial);
    let accepted = b"PDF annotated before upload".to_vec();
    let checksum = ContentHash::from(blake3::hash(&accepted));
    f.put("alice", library, book, accepted.clone()).await;
    let replacement = vec![b'x'; accepted.len()];
    let changed = ContentHash::from(blake3::hash(&replacement));
    assert!(matches!(f.assets.put_book_with_checksum("alice", &library, &book, changed, replacement.len() as u64, Box::new(Cursor::new(replacement))).await, Err(AssetError::HashMismatch)));
    assert_eq!(f.assets.book_content_hash("alice", &library, &book).await.unwrap(), Some(checksum));
    assert_eq!(f.assets.storage_usage("alice").await.unwrap().used_bytes, accepted.len() as u64);
    let mut stored = f.assets.open_from("alice", &library, AssetKind::Book, &book, 0).await.unwrap().unwrap();
    let mut bytes = Vec::new();
    stored.reader.read_to_end(&mut bytes).await.unwrap();
    assert_eq!(bytes, accepted);
    // A different logical book can establish its own first annotated revision.
    let other_library = f.library("bob").await;
    let other = BookId::for_local_content(other_library.as_bytes(), initial);
    f.put("bob", other_library, other, b"independently annotated PDF".to_vec()).await;
    assert_ne!(f.assets.book_content_hash("bob", &other_library, &other).await.unwrap(), Some(checksum));
    f.cleanup().await;
}

#[tokio::test]
#[ignore = "requires SYNC_E2E_DATABASE_URL"]
async fn canonical_bytes_are_charged_once_and_remain_until_the_last_reference() {
    let f = Fixture::new().await;
    let first_library = f.library("alice").await;
    let second_library = f.library("alice").await;
    let third_library = f.library("alice").await;
    let initial = ContentHash::from(blake3::hash(b"original publication"));
    let first = BookId::for_local_content(first_library.as_bytes(), initial);
    let second = BookId::for_local_content(second_library.as_bytes(), initial);
    let third = BookId::for_local_content(third_library.as_bytes(), initial);
    let bytes = b"identical canonical bytes".to_vec();
    let checksum = ContentHash::from(blake3::hash(&bytes));
    let length = bytes.len() as u64;
    server_postgres::configure_user_storage_quota(&f.database, "alice", length as i64).await.unwrap();
    let preview = f.assets.negotiate_books("alice", &first_library, [first, second, third].into_iter().map(|content_hash| BlobManifestEntry { content_hash, size_bytes: length, byte_checksum: Some(checksum) }).collect()).await.unwrap();
    assert_eq!(preview.upload.len(), 3);
    assert!(preview.owned.is_empty() && preview.rejected.is_empty());
    f.put("alice", first_library, first, bytes.clone()).await;
    // Even at a full quota, another logical identity for already-owned bytes
    // can be uploaded/verified or adopted through manifest negotiation.
    f.put("alice", second_library, second, bytes.clone()).await;
    let response = f.assets.negotiate_books("alice", &third_library, vec![BlobManifestEntry { content_hash: third, size_bytes: length, byte_checksum: Some(checksum) }]).await.unwrap();
    assert_eq!(response.owned, vec![third]);
    assert!(response.upload.is_empty() && response.rejected.is_empty());
    let usage = f.assets.storage_usage("alice").await.unwrap();
    assert_eq!((usage.used_bytes, usage.reserved_bytes), (length, 0));
    assert_eq!(f.assets.book_storage_usage("alice", 100).await.unwrap().len(), 1);
    assert_eq!(f.assets.library_storage_usage("alice").await.unwrap().iter().map(|entry| entry.used_bytes).sum::<u64>(), length);
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let inode = |book: BookId| std::fs::metadata(f.root.path().join("books").join(&book.to_hex().as_str()[..2]).join(book.to_hex())).unwrap().ino();
        assert_eq!(inode(first), inode(second));
        assert_eq!(inode(first), inode(third));
    }
    // Another account still has to prove possession, then pays once for its
    // own ownership while sharing the same immutable physical bytes.
    let bob_library = f.library("bob").await;
    let bob = BookId::for_local_content(bob_library.as_bytes(), initial);
    let response = f.assets.negotiate_books("bob", &bob_library, vec![BlobManifestEntry { content_hash: bob, size_bytes: length, byte_checksum: Some(checksum) }]).await.unwrap();
    assert_eq!(response.upload, vec![bob]);
    assert!(response.owned.is_empty());
    f.put("bob", bob_library, bob, bytes.clone()).await;
    assert_eq!(f.assets.storage_usage("bob").await.unwrap().used_bytes, length);

    sqlx::query("DELETE FROM library_blob_claim WHERE user_id='alice' AND content_hash=$1").bind(first.as_bytes()).execute(f.database.pool()).await.unwrap();
    sqlx::query(include_str!("../src/sql/cloud_storage/release_unreferenced.sql")).bind("alice").execute(f.database.pool()).await.unwrap();
    assert_eq!(f.assets.storage_usage("alice").await.unwrap().used_bytes, length);
    // Exercise lifecycle reference release separately from claim release.
    sqlx::query("INSERT INTO user_blob_reference(user_id,content_hash,reference_count) VALUES('alice',$1,1)").bind(second.as_bytes()).execute(f.database.pool()).await.unwrap();
    sqlx::query("DELETE FROM library_blob_claim WHERE user_id='alice' AND content_hash=$1").bind(second.as_bytes()).execute(f.database.pool()).await.unwrap();
    let release = sqlx::query(include_str!("../src/sql/assets/apply_blob_reference_deltas.sql")).bind("alice").bind(vec![second.as_bytes().to_vec()]).bind(vec![-1_i64]).fetch_one(f.database.pool()).await.unwrap();
    assert!(release.get::<bool, _>("valid"));
    assert_eq!(release.get::<i64, _>("released"), 0);
    assert_eq!(f.assets.storage_usage("alice").await.unwrap().used_bytes, length);
    sqlx::query("DELETE FROM library_blob_claim WHERE user_id='alice' AND content_hash=$1").bind(third.as_bytes()).execute(f.database.pool()).await.unwrap();
    sqlx::query(include_str!("../src/sql/cloud_storage/release_unreferenced.sql")).bind("alice").execute(f.database.pool()).await.unwrap();
    assert_eq!(f.assets.storage_usage("alice").await.unwrap().used_bytes, 0);
    f.assets.collect_unused_assets().await.unwrap();
    let mut stored = f.assets.open_from("bob", &bob_library, AssetKind::Book, &bob, 0).await.unwrap().unwrap();
    let mut remaining = Vec::new();
    stored.reader.read_to_end(&mut remaining).await.unwrap();
    assert_eq!(remaining, bytes);
    assert_eq!(f.assets.storage_usage("bob").await.unwrap().used_bytes, length);
    f.cleanup().await;
}

#[tokio::test]
#[ignore = "requires SYNC_E2E_DATABASE_URL"]
async fn schema_thirteen_upgrade_preserves_keys_bytes_claims_and_quota() {
    let f = Fixture::new().await;
    let library = f.library("alice").await;
    let bytes = b"legacy uploaded book".to_vec();
    let checksum = ContentHash::from(blake3::hash(&bytes));
    let book = BookId::from(checksum);
    f.put("alice", library, book, bytes.clone()).await;
    let before = f.assets.storage_usage("alice").await.unwrap();
    sqlx::raw_sql("DROP TABLE user_canonical_charge; ALTER TABLE blob_object DROP COLUMN byte_checksum; DROP TABLE canonical_blob_object; ALTER TABLE bokheim_schema_version DROP CONSTRAINT bokheim_schema_version_version_check; UPDATE bokheim_schema_version SET version=13; ALTER TABLE bokheim_schema_version ADD CONSTRAINT bokheim_schema_version_version_check CHECK(version=13)").execute(f.database.pool()).await.unwrap();
    server_postgres::upgrade_existing(&f.database).await.unwrap();
    server_postgres::verify_schema(&f.database).await.unwrap();
    assert_eq!(f.assets.book_content_hash("alice", &library, &book).await.unwrap(), Some(checksum));
    assert_eq!(f.assets.storage_usage("alice").await.unwrap(), before);
    assert_eq!(f.assets.library_storage_usage("alice").await.unwrap().iter().map(|entry| entry.used_bytes).sum::<u64>(), bytes.len() as u64);
    let mut stored = f.assets.open_from("alice", &library, AssetKind::Book, &book, 0).await.unwrap().unwrap();
    let mut actual = Vec::new();
    stored.reader.read_to_end(&mut actual).await.unwrap();
    assert_eq!(actual, bytes);
    f.cleanup().await;
}
