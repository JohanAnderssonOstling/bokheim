use server_asset_store::{AssetError, AssetKind, FsAssetStore};
use server_postgres::{AssetApplication, PostgresDatabase, PostgresSyncRepository};
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::{io::Cursor, sync::Arc};
use sync_common::api::{assets::BlobManifestEntry, libraries::LibraryNameRequest};
use sync_common::storage::{CloudLibraryRoot, CloudProvider, LibraryStorage, ProviderId, ProviderScope};
use sync_common::{BookId, ConflictRank, LibraryId, OpaquePublication, OpaqueStateEntry, ReplicaId, ReplicaSeq, StateAddress, SyncCursor, SyncExchangeRequest, VersionKey, Versioned};
use tokio::io::AsyncReadExt;

#[tokio::test]
#[ignore = "requires SYNC_E2E_DATABASE_URL"]
async fn another_accounts_book_requires_upload_even_with_a_forged_lifecycle() {
    let url = std::env::var("SYNC_E2E_DATABASE_URL").unwrap();
    let admin = PgPool::connect(&url).await.unwrap();
    let schema = format!("asset_authorization_{}", LibraryId::new_v4().simple());
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
    sqlx::query("INSERT INTO users(id) VALUES ('alice'), ('bob')").execute(database.pool()).await.unwrap();
    let sync = PostgresSyncRepository::new(database.clone());
    let alice_library = LibraryId::new_v4();
    let bob_library = LibraryId::new_v4();
    for (user, library) in [("alice", alice_library), ("bob", bob_library)] {
        sync.create_library(user, &LibraryNameRequest { library_id: library, library_name: "Private".into(), storage: Default::default() }).await.unwrap();
    }
    let root = std::env::temp_dir().join(&schema);
    let store = Arc::new(FsAssetStore::new(root.join("books"), root.join("thumbnails")));
    store.initialize().await.unwrap();
    let assets = AssetApplication::new(database.clone(), store);
    // The BLAKE3 digest of the complete file b"abc".
    let hash: BookId = "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85".parse().unwrap();
    assets.put_book("alice", &alice_library, &hash, 3, Box::new(Cursor::new(b"abc"))).await.unwrap();
    assert!(assets.open_from("bob", &bob_library, AssetKind::Book, &hash, 0).await.unwrap().is_none());
    let manifest = assets.negotiate_books("bob", &bob_library, vec![BlobManifestEntry { content_hash: hash, size_bytes: 3 }]).await.unwrap();
    assert!(manifest.owned.is_empty());
    assert_eq!(manifest.upload, vec![hash]);
    assert!(assets.open_from("bob", &bob_library, AssetKind::Book, &hash, 0).await.unwrap().is_none());

    // A wrong length must not disclose whether another account stores a hash.
    let manifest = assets.negotiate_books("bob", &bob_library, vec![BlobManifestEntry { content_hash: hash, size_bytes: 4 }]).await.unwrap();
    assert!(manifest.owned.is_empty());
    assert!(manifest.rejected.is_empty());
    assert_eq!(manifest.upload, vec![hash]);

    // Sync metadata is client-authored, so a lifecycle naming a victim's hash
    // cannot establish possession or authorize any book/playback download.
    let replica = ReplicaId::new_v4();
    let sequence = ReplicaSeq::new(1).unwrap();
    let result = sync
        .exchange(
            "bob",
            &SyncExchangeRequest {
                library_id: bob_library,
                replica_id: replica,
                cursor: SyncCursor::default(),
                mutations: vec![OpaquePublication {
                    publisher_seq: sequence,
                    state: Versioned {
                        value: OpaqueStateEntry { address: StateAddress { kind: "book_lifecycle".into(), entity_key: hash.into(), entity_subkey: String::new() }, book: Some(hash), bytes: vec![0] },
                        version: VersionKey::new(1, ConflictRank::Update, replica, sequence),
                    },
                }],
            },
        )
        .await
        .unwrap();
    assert!(result.push.rejected.is_empty());
    assert!(assets.open_from("bob", &bob_library, AssetKind::Book, &hash, 0).await.unwrap().is_none());
    assert!(assets.audiobook_track("bob", &bob_library, &hash, 0).await.unwrap().is_none());
    let manifest = assets.negotiate_books("bob", &bob_library, vec![BlobManifestEntry { content_hash: hash, size_bytes: 3 }]).await.unwrap();
    assert!(manifest.owned.is_empty());
    assert_eq!(manifest.upload, vec![hash]);

    // A valid upload still works and physical content remains deduplicated.
    assets.put_book("bob", &bob_library, &hash, 3, Box::new(Cursor::new(b"abc"))).await.unwrap();
    let mut asset = assets.open_from("bob", &bob_library, AssetKind::Book, &hash, 0).await.unwrap().unwrap();
    let mut bytes = Vec::new();
    asset.reader.read_to_end(&mut bytes).await.unwrap();
    assert_eq!(bytes, b"abc");
    assert_eq!(assets.storage_usage("bob").await.unwrap().used_bytes, 3);

    // Selecting external storage denies every Bokheim file/cover operation,
    // including when this user already owns the requested bytes elsewhere.
    let external_library = LibraryId::new_v4();
    let storage = LibraryStorage::External(CloudLibraryRoot {
        scope: ProviderScope { provider: CloudProvider::GoogleDrive, account_id: ProviderId::new("google-account").unwrap(), container_id: ProviderId::new("my_drive").unwrap() },
        folder_id: ProviderId::new("library-folder").unwrap(),
    });
    sync.create_library("alice", &LibraryNameRequest { library_id: external_library, library_name: "Google Drive".into(), storage }).await.unwrap();
    let before = assets.storage_usage("alice").await.unwrap();
    assert!(matches!(assets.open_from("alice", &external_library, AssetKind::Book, &hash, 0).await, Err(AssetError::Forbidden)));
    assert!(matches!(assets.audiobook_track("alice", &external_library, &hash, 0).await, Err(AssetError::Forbidden)));
    assert!(matches!(assets.negotiate_books("alice", &external_library, vec![BlobManifestEntry { content_hash: hash, size_bytes: 3 }]).await, Err(AssetError::Forbidden)));
    assert!(matches!(assets.put_book("alice", &external_library, &hash, 3, Box::new(Cursor::new(b"abc"))).await, Err(AssetError::Forbidden)));
    assert!(matches!(assets.thumbnail_presence("alice", &external_library, &[hash]).await, Err(AssetError::Forbidden)));
    assert!(matches!(assets.thumbnail_pair("alice", &external_library, &hash).await, Err(AssetError::Forbidden)));
    assert!(matches!(assets.put_thumbnail_pair("alice", &external_library, &hash, b"cover", b"browse").await, Err(AssetError::Forbidden)));
    assert!(matches!(assets.remove_thumbnail("alice", &external_library, &hash).await, Err(AssetError::Forbidden)));
    for kind in [AssetKind::Thumbnail, AssetKind::ThumbnailBrowse] {
        assert!(matches!(assets.open_from("alice", &external_library, kind, &hash, 0).await, Err(AssetError::Forbidden)));
        assert!(matches!(assets.put("alice", &external_library, kind, &hash, Some(5), Box::new(Cursor::new(b"cover"))).await, Err(AssetError::Forbidden)));
    }
    assert_eq!(assets.storage_usage("alice").await.unwrap(), before);
    assert!(!assets.library_storage_usage("alice").await.unwrap().iter().any(|usage| usage.library_id == external_library && usage.used_bytes != 0));

    database.pool().close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE")).execute(&admin).await.unwrap();
    admin.close().await;
    std::fs::remove_dir_all(root).unwrap();
}
