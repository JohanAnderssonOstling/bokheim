use axum::body::{to_bytes, Body};
use axum::http::{header, Request, StatusCode};
use server_account::{AccountTokenKey, PostgresAccountService};
use server_asset_store::{AssetError, AssetKind, CatalogStorage, FsAssetStore};
use server_postgres::{AssetApplication, PostgresDatabase, PostgresSyncRepository};
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::{io::Cursor, path::PathBuf, sync::Arc, time::Duration};
use sync_common::api::assets::{BlobManifestEntry, BlobManifestRejectionReason, BlobManifestRequest, BlobManifestResponse};
use sync_common::api::libraries::LibraryNameRequest;
use sync_common::{BookId, LibraryId};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;
use tower::ServiceExt;

fn hash(bytes: &[u8]) -> BookId {
    BookId::from_bytes(*blake3::hash(bytes).as_bytes())
}

struct Fixture {
    database: PostgresDatabase,
    admin: PgPool,
    schema: String,
    root: TempDir,
    catalog: CatalogStorage,
    assets: Arc<AssetApplication>,
    sync: PostgresSyncRepository,
    alice: LibraryId,
    bob: LibraryId,
}

impl Fixture {
    async fn new() -> Self {
        let url = std::env::var("SYNC_E2E_DATABASE_URL").expect("use an isolated local PostgreSQL test database");
        let admin = PgPool::connect(&url).await.unwrap();
        let schema = format!("catalog_assets_{}", LibraryId::new_v4().simple());
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        let search_path = format!("SET search_path TO {schema}");
        let pool = PgPoolOptions::new()
            .max_connections(8)
            .after_connect(move |connection, _| {
                let statement = search_path.clone();
                Box::pin(async move { sqlx::query(&statement).execute(connection).await.map(|_| ()) })
            })
            .connect(&url)
            .await
            .unwrap();
        let database = PostgresDatabase::from_pool(pool);
        server_postgres::migrate(&database).await.unwrap();
        sqlx::query("INSERT INTO users(id) VALUES('alice'), ('bob'), ('charlie')").execute(database.pool()).await.unwrap();
        let root = TempDir::new().unwrap();
        let catalog = CatalogStorage { books: root.path().join("public/books"), pins: root.path().join("private-pins") };
        std::fs::create_dir_all(&catalog.books).unwrap();
        let store = Arc::new(FsAssetStore::new(root.path().join("sync/books"), root.path().join("sync/thumbnails")).with_catalog(catalog.clone()));
        store.initialize().await.unwrap();
        let assets = Arc::new(AssetApplication::new(database.clone(), store.clone()));
        let sync = PostgresSyncRepository::new(database.clone());
        let alice = LibraryId::new_v4();
        let bob = LibraryId::new_v4();
        for (user, library) in [("alice", alice), ("bob", bob)] {
            sync.create_library(user, &LibraryNameRequest { library_id: library, library_name: "Books".into(), storage: Default::default(), kind: Default::default() }).await.unwrap();
        }
        Self { database, admin, schema, root, catalog, assets, sync, alice, bob }
    }

    fn publish(&self, bytes: &[u8]) -> BookId {
        let hash = hash(bytes);
        std::fs::write(self.catalog.books.join(format!("{hash}.epub")), bytes).unwrap();
        hash
    }

    fn private_copy(&self, hash: BookId) -> PathBuf {
        self.root.path().join("sync/books").join(&hash.to_hex()[..2]).join(hash.to_hex())
    }

    async fn claim(&self, user: &str, library: LibraryId, hash: BookId, size_bytes: u64) -> BlobManifestResponse {
        self.assets.negotiate_books(user, &library, vec![BlobManifestEntry { byte_checksum: None, book_id: hash, size_bytes }]).await.unwrap()
    }

    async fn read(&self, user: &str, library: LibraryId, hash: BookId) -> Option<Vec<u8>> {
        let Some(mut asset) = self.assets.open_from(user, &library, AssetKind::Book, &hash, 0).await.unwrap() else { return None };
        let mut bytes = Vec::new();
        asset.reader.read_to_end(&mut bytes).await.unwrap();
        Some(bytes)
    }

    async fn finish(self) {
        self.database.pool().close().await;
        sqlx::query(&format!("DROP SCHEMA {} CASCADE", self.schema)).execute(&self.admin).await.unwrap();
        self.admin.close().await;
    }
}

#[tokio::test]
#[ignore = "requires SYNC_E2E_DATABASE_URL"]
async fn account_book_storage_ranks_unique_owned_books_and_excludes_other_accounts() {
    let f = Fixture::new().await;
    let accounts = Arc::new(PostgresAccountService::new(f.database.clone(), AccountTokenKey::from_base64("WlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlo=").unwrap()));
    let user = accounts.ensure_configured_user("storage@example.com", "storage-test-password").await.unwrap();
    let session = accounts.login("storage@example.com", "storage-test-password").await.unwrap();
    let owner = &user.user_id;
    let first = LibraryId::new_v4();
    let second = LibraryId::new_v4();
    for library in [first, second] {
        f.sync.create_library(owner, &LibraryNameRequest { library_id: library, library_name: "Storage".into(), storage: Default::default(), kind: Default::default() }).await.unwrap();
    }
    let small = hash(b"abc");
    let large = hash(b"123456789");
    let other = hash(b"a different account's larger book");
    f.assets.put_book(owner, &first, &small, 3, Box::new(Cursor::new(b"abc"))).await.unwrap();
    f.assets.put_book(owner, &first, &large, 9, Box::new(Cursor::new(b"123456789"))).await.unwrap();
    f.claim(owner, second, large, 9).await;
    f.assets.put_book("bob", &f.bob, &other, 33, Box::new(Cursor::new(b"a different account's larger book"))).await.unwrap();

    let ranked = f.assets.book_storage_usage(owner, 100).await.unwrap();
    assert_eq!(ranked.iter().map(|book| (book.book_id, book.used_bytes)).collect::<Vec<_>>(), [(large, 9), (small, 3)]);
    assert_eq!(f.assets.book_storage_usage(owner, 1).await.unwrap(), ranked[..1]);
    assert!(f.assets.book_storage_usage("charlie", 100).await.unwrap().is_empty());
    assert_eq!(f.assets.book_storage_usage("bob", 100).await.unwrap()[0].book_id, other);

    let app = sync_server::app(f.database.clone(), accounts, sync_server::AppConfig { books: f.root.path().join("sync/books"), thumbnails: f.root.path().join("sync/thumbnails"), catalog: Some(f.catalog.clone()), ..Default::default() })
        .await
        .unwrap();
    assert_eq!(app.clone().oneshot(Request::get("/api/account/storage/books").body(Body::empty()).unwrap()).await.unwrap().status(), StatusCode::UNAUTHORIZED);
    let response = app.oneshot(Request::get("/api/account/storage/books").header(header::AUTHORIZATION, format!("Bearer {}", session.token)).header(header::ACCEPT, wire::MEDIA_TYPE).body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), wire::MAX_DECODED_RESPONSE_BYTES).await.unwrap();
    let books: Vec<account_contract::BookStorageUsage> = wire::decode(&bytes, wire::MAX_DECODED_RESPONSE_BYTES).unwrap();
    assert_eq!(books.iter().map(|book| book.book_id).collect::<Vec<_>>(), [large, small]);
    f.finish().await;
}

#[tokio::test]
#[ignore = "requires SYNC_E2E_DATABASE_URL"]
async fn catalog_claims_share_one_inode_charge_each_account_once_and_survive_unpublishing() {
    let f = Fixture::new().await;
    let hash = f.publish(b"abc");
    assert!(f.read("alice", f.alice, hash).await.is_none(), "public availability alone is not account ownership");
    for (user, library) in [("alice", f.alice), ("alice", f.alice), ("bob", f.bob)] {
        let response = f.claim(user, library, hash, 3).await;
        assert_eq!(response.owned, vec![hash]);
        assert!(response.upload.is_empty());
        assert!(response.rejected.is_empty());
        let usage = f.assets.storage_usage(user).await.unwrap();
        assert_eq!((usage.used_bytes, usage.reserved_bytes), (3, 0));
        assert_eq!(f.read(user, library, hash).await.unwrap(), b"abc");
    }
    assert!(!f.private_copy(hash).exists());
    let pin = f.catalog.pins.join(hash.to_hex());
    let public = f.catalog.books.join(format!("{hash}.epub"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(std::fs::metadata(&pin).unwrap().ino(), std::fs::metadata(&public).unwrap().ino());
    }
    assert_eq!(std::fs::read_dir(&f.catalog.pins).unwrap().count(), 1);
    let second = LibraryId::new_v4();
    f.sync.create_library("alice", &LibraryNameRequest { library_id: second, library_name: "Second".into(), storage: Default::default(), kind: Default::default() }).await.unwrap();
    assert_eq!(f.claim("alice", second, hash, 3).await.owned, vec![hash]);
    assert_eq!(f.assets.storage_usage("alice").await.unwrap().used_bytes, 3);
    let breakdown = f.assets.library_storage_usage("alice").await.unwrap();
    assert_eq!(breakdown.iter().map(|library| library.used_bytes).sum::<u64>(), 3);

    std::fs::remove_file(public).unwrap();
    assert_eq!(f.assets.collect_unused_assets().await.unwrap().books, 0);
    assert_eq!(f.read("alice", second, hash).await.unwrap(), b"abc");
    // A retained link must not make a removed catalog entry available by hash
    // to a new account, just as another user's private upload is not available.
    let third = LibraryId::new_v4();
    f.sync.create_library("charlie", &LibraryNameRequest { library_id: third, library_name: "Third".into(), storage: Default::default(), kind: Default::default() }).await.unwrap();
    assert_eq!(f.claim("charlie", third, hash, 3).await.upload, vec![hash]);
    assert!(f.read("charlie", third, hash).await.is_none());

    f.sync.delete_library("alice", &f.alice).await.unwrap();
    f.assets.collect_unused_assets().await.unwrap();
    assert_eq!(f.assets.storage_usage("alice").await.unwrap().used_bytes, 3);
    f.sync.delete_library("alice", &second).await.unwrap();
    f.assets.collect_unused_assets().await.unwrap();
    assert_eq!(f.assets.storage_usage("alice").await.unwrap().used_bytes, 0);
    assert_eq!(f.read("bob", f.bob, hash).await.unwrap(), b"abc");
    f.sync.delete_library("bob", &f.bob).await.unwrap();
    assert_eq!(f.assets.collect_unused_assets().await.unwrap().books, 1);
    assert!(!pin.exists());
    f.finish().await;
}

#[tokio::test]
#[ignore = "requires SYNC_E2E_DATABASE_URL"]
async fn public_claims_reject_wrong_sizes_quota_overruns_and_unowned_libraries() {
    let f = Fixture::new().await;
    let hash = f.publish(b"abc");
    let wrong = f.claim("alice", f.alice, hash, 4).await;
    assert_eq!(wrong.rejected[0].reason, BlobManifestRejectionReason::SizeMismatch);
    server_postgres::configure_user_storage_quota(&f.database, "alice", 2).await.unwrap();
    assert_eq!(f.claim("alice", f.alice, hash, 3).await.rejected[0].reason, BlobManifestRejectionReason::QuotaExceeded);
    assert!(matches!(f.assets.negotiate_books("bob", &f.alice, vec![BlobManifestEntry { byte_checksum: None, book_id: hash, size_bytes: 3 }]).await, Err(AssetError::Forbidden)));
    let usage = f.assets.storage_usage("alice").await.unwrap();
    assert_eq!((usage.used_bytes, usage.reserved_bytes), (0, 0));
    assert_eq!(std::fs::read_dir(&f.catalog.pins).unwrap().count(), 0);
    f.sync.set_cloud_storage("bob", &f.bob, uuid::Uuid::new_v4(), false).await.unwrap();
    assert!(matches!(f.assets.negotiate_books("bob", &f.bob, vec![BlobManifestEntry { byte_checksum: None, book_id: hash, size_bytes: 3 }]).await, Err(AssetError::Forbidden)));
    assert_eq!(std::fs::read_dir(&f.catalog.pins).unwrap().count(), 0);
    f.finish().await;
}

#[tokio::test]
#[ignore = "requires SYNC_E2E_DATABASE_URL"]
async fn concurrent_catalog_claims_do_not_double_charge_or_exceed_quota() {
    let f = Fixture::new().await;
    let a = f.publish(b"abc");
    let b = f.publish(b"def");
    server_postgres::configure_user_storage_quota(&f.database, "alice", 5).await.unwrap();
    let (first, repeated) = timeout(Duration::from_secs(5), async { tokio::join!(f.claim("alice", f.alice, a, 3), f.claim("alice", f.alice, a, 3)) }).await.unwrap();
    assert_eq!(first.owned, vec![a]);
    assert_eq!(repeated.owned, vec![a]);
    assert_eq!(f.assets.storage_usage("alice").await.unwrap().used_bytes, 3);
    server_postgres::configure_user_storage_quota(&f.database, "bob", 5).await.unwrap();
    let (first, second) = timeout(Duration::from_secs(5), async { tokio::join!(f.claim("bob", f.bob, a, 3), f.claim("bob", f.bob, b, 3)) }).await.unwrap();
    assert_eq!(first.owned.len() + second.owned.len(), 1);
    let rejected = first.rejected.iter().chain(&second.rejected).collect::<Vec<_>>();
    assert_eq!(rejected.len(), 1);
    assert_eq!(rejected[0].reason, BlobManifestRejectionReason::QuotaExceeded);
    assert_eq!(f.assets.storage_usage("bob").await.unwrap().used_bytes, 3);
    f.finish().await;
}

#[tokio::test]
#[ignore = "requires SYNC_E2E_DATABASE_URL"]
async fn private_uploads_still_require_possession_and_background_cleanup_consolidates_old_copies() {
    let f = Fixture::new().await;
    let hash = hash(b"abc");
    f.assets.put_book("alice", &f.alice, &hash, 3, Box::new(Cursor::new(b"abc"))).await.unwrap();
    assert_eq!(f.claim("bob", f.bob, hash, 3).await.upload, vec![hash]);
    assert!(f.read("bob", f.bob, hash).await.is_none());
    assert!(f.private_copy(hash).exists());
    f.publish(b"abc");
    let result = f.assets.collect_unused_assets().await.unwrap();
    assert_eq!(result.deduplicated_books, 1);
    assert!(!f.private_copy(hash).exists());
    assert_eq!(f.assets.storage_usage("alice").await.unwrap().used_bytes, 3);
    assert_eq!(f.read("alice", f.alice, hash).await.unwrap(), b"abc");
    // An atomic re-publication may replace the public inode without changing
    // its hash. Consolidation must repoint the retained link too.
    let replacement = f.catalog.books.join("replacement.epub");
    std::fs::write(&replacement, b"abc").unwrap();
    std::fs::rename(replacement, f.catalog.books.join(format!("{hash}.epub"))).unwrap();
    assert_eq!(f.assets.collect_unused_assets().await.unwrap().deduplicated_books, 1);
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(std::fs::metadata(f.catalog.pins.join(hash.to_hex())).unwrap().ino(), std::fs::metadata(f.catalog.books.join(format!("{hash}.epub"))).unwrap().ino());
    }
    assert_eq!(f.claim("bob", f.bob, hash, 3).await.owned, vec![hash]);
    f.sync.delete_library("alice", &f.alice).await.unwrap();
    f.sync.delete_library("bob", &f.bob).await.unwrap();
    assert_eq!(f.assets.collect_unused_assets().await.unwrap().books, 1);
    assert!(f.catalog.books.join(format!("{hash}.epub")).exists(), "collecting sync ownership must never remove the public file");
    assert_eq!(std::fs::read_dir(&f.catalog.pins).unwrap().count(), 0);
    f.finish().await;
}

#[tokio::test]
#[ignore = "requires SYNC_E2E_DATABASE_URL"]
async fn adoption_can_race_staged_uploads_without_recreating_a_duplicate() {
    let f = Fixture::new().await;
    let hash = hash(b"abc");
    let (mut writer, reader) = tokio::io::duplex(8);
    let assets = f.assets.clone();
    let library = f.alice;
    let upload = tokio::spawn(async move { assets.put_book("alice", &library, &hash, 3, Box::new(reader)).await });
    writer.write_all(b"a").await.unwrap();
    timeout(Duration::from_secs(5), async {
        loop {
            if f.assets.storage_usage("alice").await.unwrap().reserved_bytes == 3 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    f.publish(b"abc");
    assert_eq!(f.claim("bob", f.bob, hash, 3).await.owned, vec![hash]);
    f.assets.collect_unused_assets().await.unwrap();
    writer.write_all(b"bc").await.unwrap();
    drop(writer);
    timeout(Duration::from_secs(5), upload).await.unwrap().unwrap().unwrap();
    assert!(!f.private_copy(hash).exists());
    assert_eq!(f.read("alice", f.alice, hash).await.unwrap(), b"abc");
    assert_eq!(f.assets.storage_usage("alice").await.unwrap().used_bytes, 3);
    f.finish().await;
}

#[tokio::test]
#[ignore = "requires SYNC_E2E_DATABASE_URL"]
async fn incorrect_catalog_bytes_never_authorize_another_accounts_private_copy() {
    let f = Fixture::new().await;
    let hash = hash(b"abc");
    f.assets.put_book("alice", &f.alice, &hash, 3, Box::new(Cursor::new(b"abc"))).await.unwrap();
    std::fs::write(f.catalog.books.join(format!("{hash}.epub")), b"def").unwrap();
    assert!(matches!(f.assets.negotiate_books("bob", &f.bob, vec![BlobManifestEntry { byte_checksum: None, book_id: hash, size_bytes: 3 }]).await, Err(AssetError::HashMismatch)));
    assert_eq!(f.assets.storage_usage("bob").await.unwrap().used_bytes, 0);
    assert!(f.read("bob", f.bob, hash).await.is_none());
    assert_eq!(f.read("alice", f.alice, hash).await.unwrap(), b"abc");
    assert!(f.private_copy(hash).exists());
    assert_eq!(std::fs::read_dir(&f.catalog.pins).unwrap().count(), 0);
    f.finish().await;
}

#[tokio::test]
#[ignore = "requires SYNC_E2E_DATABASE_URL"]
async fn existing_http_manifest_protocol_adopts_and_serves_catalog_without_a_put() {
    let f = Fixture::new().await;
    let hash = f.publish(b"abc");
    let accounts = Arc::new(PostgresAccountService::new(f.database.clone(), AccountTokenKey::from_base64("WlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlo=").unwrap()));
    let user = accounts.ensure_configured_user("reader@example.com", "catalog-test-password").await.unwrap();
    let session = accounts.login("reader@example.com", "catalog-test-password").await.unwrap();
    let library = LibraryId::new_v4();
    f.sync.create_library(&user.user_id, &LibraryNameRequest { library_id: library, library_name: "Catalog".into(), storage: Default::default(), kind: Default::default() }).await.unwrap();
    let app = sync_server::app(f.database.clone(), accounts, sync_server::AppConfig { books: f.root.path().join("sync/books"), thumbnails: f.root.path().join("sync/thumbnails"), catalog: Some(f.catalog.clone()), ..Default::default() })
        .await
        .unwrap();
    let manifest = BlobManifestRequest { blobs: vec![BlobManifestEntry { byte_checksum: None, book_id: hash, size_bytes: 3 }] };
    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/api/libraries/{library}/blobs/negotiate"))
                .header(header::AUTHORIZATION, format!("Bearer {}", session.token))
                .header(header::CONTENT_TYPE, wire::MEDIA_TYPE)
                .header(header::ACCEPT, wire::MEDIA_TYPE)
                .body(Body::from(wire::encode(&manifest).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response: BlobManifestResponse = wire::decode(&to_bytes(response.into_body(), 64 * 1024).await.unwrap(), 64 * 1024).unwrap();
    assert_eq!(response.owned, vec![hash]);
    assert!(response.upload.is_empty());
    sync_common::api::assets::validate_blob_manifest_response(&manifest, &response).unwrap();
    let response =
        app.clone().oneshot(Request::get(format!("/api/libraries/{library}/blobs/{hash}")).header(header::AUTHORIZATION, format!("Bearer {}", session.token)).header(header::RANGE, "bytes=1-2").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(&to_bytes(response.into_body(), 3).await.unwrap()[..], b"bc");
    assert!(!f.private_copy(hash).exists());
    let checksum = hash.content_hash().unwrap();
    let logical = BookId::for_local_content(library.as_bytes(), checksum);
    let manifest = BlobManifestRequest { blobs: vec![BlobManifestEntry { byte_checksum: Some(checksum), book_id: logical, size_bytes: 3 }] };
    let response = app.clone().oneshot(Request::post(format!("/api/libraries/{library}/blobs/negotiate"))
        .header(header::AUTHORIZATION, format!("Bearer {}", session.token)).header(header::CONTENT_TYPE, wire::MEDIA_TYPE)
        .header(header::ACCEPT, wire::MEDIA_TYPE).body(Body::from(wire::encode(&manifest).unwrap())).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response: BlobManifestResponse = wire::decode(&to_bytes(response.into_body(), 64 * 1024).await.unwrap(), 64 * 1024).unwrap();
    assert_eq!(response.owned, vec![logical]);
    let response = app.clone().oneshot(Request::head(format!("/api/libraries/{library}/blobs/{logical}"))
        .header(header::AUTHORIZATION, format!("Bearer {}", session.token)).body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["x-bokheim-content-hash"], checksum.to_hex());
    assert_eq!(response.headers()[header::CONTENT_LENGTH], "3");
    let response = app.oneshot(Request::get(format!("/api/libraries/{library}/blobs/{logical}"))
        .header(header::AUTHORIZATION, format!("Bearer {}", session.token)).body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["x-bokheim-content-hash"], checksum.to_hex());
    assert_eq!(&to_bytes(response.into_body(), 3).await.unwrap()[..], b"abc");
    assert_eq!(f.assets.storage_usage(&user.user_id).await.unwrap().used_bytes, 3);
    f.finish().await;
}
