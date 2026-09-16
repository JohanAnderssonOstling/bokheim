use super::*;

#[tokio::test]
#[ignore = "requires the PostgreSQL sync server started by run_sync_e2e.sh"]
async fn production_http_postgres_syncs_a_backend_outbox_to_a_second_replica() {
    let server_url = ServerUrl::parse(&std::env::var("SYNC_E2E_SERVER_URL").expect("SYNC_E2E_SERVER_URL")).expect("valid E2E server URL");
    let password = std::env::var("SYNC_E2E_ADMIN_PASSWORD").expect("SYNC_E2E_ADMIN_PASSWORD");
    let session = account_client::login(&server_url, "admin@bokheim.local", &password).await.expect("log in to E2E server");
    let library_id = fixture_library_id("backend-e2e");
    account_client::create_library(&session, library_id.clone(), "Backend E2E".to_owned()).await.expect("create E2E library");

    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
    let mut manager = test_manager(temp.path());
    manager.server_url = server_url;
    manager.library_id = library_id.clone();
    manager.credentials = Arc::new(std::sync::RwLock::new(Some(session_credentials(&session))));

    let conn = manager.database.get().unwrap();
    let transaction = conn.unchecked_transaction().unwrap();
    client_library::placements::upsert_dir(&transaction, &fixture_dir_id("backend-e2e-dir"), &sync_common::ROOT_DIR_ID, &"Backend E2E".to_owned()).unwrap();
    transaction.commit().unwrap();

    manager.sync_once().await.expect("publish Backend outbox through real server");

    let observed =
        sync_transport::exchange_changes(&reqwest::Client::new(), &session_credentials(&session), library_id, fixture_replica_id("backend-e2e-observer"), Vec::new(), SyncCursor::default()).await.expect("pull through real server");
    assert!(observed.pull.mutations.iter().any(|change| change.mutation.kind == "directory_name"));
}

#[tokio::test]
#[ignore = "requires the PostgreSQL sync server started by run_sync_e2e.sh"]
async fn production_http_postgres_preserves_offline_outbox_and_reconciles_a_second_backend() {
    let server_url = ServerUrl::parse(&std::env::var("SYNC_E2E_SERVER_URL").expect("SYNC_E2E_SERVER_URL")).expect("valid E2E server URL");
    let password = std::env::var("SYNC_E2E_ADMIN_PASSWORD").expect("SYNC_E2E_ADMIN_PASSWORD");
    let session = account_client::login(&server_url, "admin@bokheim.local", &password).await.expect("log in to E2E server");
    let library_id = fixture_library_id("backend-e2e-offline");
    account_client::create_library(&session, library_id.clone(), "Backend offline E2E".to_owned()).await.expect("create E2E library");

    let writer_root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(writer_root.path().join(crate::APP_HIDDEN_DIR)).unwrap();
    let mut writer = test_manager_with_replica(writer_root.path(), "backend-e2e-offline-writer");
    writer.library_id = library_id.clone();
    let offline_session = account_client::Session::try_new(ServerUrl::parse("http://127.0.0.1:9").unwrap(), session.user_id().to_owned(), session.email().to_owned(), session.token().to_owned()).unwrap();
    writer.credentials = Arc::new(std::sync::RwLock::new(Some(session_credentials(&offline_session))));

    // The writer has no network route while it makes its local changes.
    // A failed exchange must leave the durable outbox intact for a later
    // reconnection, rather than losing either local register.
    let first_dir = fixture_dir_id("backend-e2e-offline-a");
    let second_dir = fixture_dir_id("backend-e2e-offline-b");
    {
        let conn = writer.database.get().unwrap();
        let transaction = conn.unchecked_transaction().unwrap();
        client_library::placements::upsert_dir(&transaction, &first_dir, &sync_common::ROOT_DIR_ID, &"Offline A".to_owned()).unwrap();
        client_library::placements::upsert_dir(&transaction, &second_dir, &sync_common::ROOT_DIR_ID, &"Offline B".to_owned()).unwrap();
        transaction.commit().unwrap();
    }
    assert!(writer.sync_once().await.is_err(), "the unavailable server must fail the sync attempt");
    assert!(writer.sync_once().await.is_err(), "a repeated offline attempt must not consume the durable outbox");
    {
        let conn = writer.database.get().unwrap();
        assert_eq!(client_library::sync_state::get_pending_mutations(&conn).unwrap().len(), 6, "all three registers for each directory remain queued while offline");
    }

    writer.server_url = server_url.clone();
    writer.credentials = Arc::new(std::sync::RwLock::new(Some(session_credentials(&session))));
    writer.sync_once().await.expect("reconnect and publish offline outbox");
    {
        let conn = writer.database.get().unwrap();
        assert!(client_library::sync_state::get_pending_mutations(&conn).unwrap().is_empty(), "server acknowledgement clears only the published outbox");
    }

    let observer_root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(observer_root.path().join(crate::APP_HIDDEN_DIR)).unwrap();
    let mut observer = test_manager_with_replica(observer_root.path(), "backend-e2e-offline-observer");
    observer.server_url = server_url;
    observer.library_id = library_id;
    observer.credentials = Arc::new(std::sync::RwLock::new(Some(session_credentials(&session))));
    observer.sync_once().await.expect("second backend pulls the writer's offline history");

    let conn = observer.database.get().unwrap();
    for (id, expected_name) in [(&first_dir, "Offline A"), (&second_dir, "Offline B")] {
        let actual_name: String = conn.query_row(include_str!("../../sql/library_sync/production_http_postgres_preserves_offline_outbox_and_reconciles_a_second_backend_select.sql"), [id.to_string()], |row| row.get(0)).unwrap();
        assert_eq!(actual_name, expected_name);
    }
}

#[tokio::test]
#[ignore = "requires the PostgreSQL sync server started by run_sync_e2e.sh"]
async fn production_http_postgres_drains_all_sync_pages_for_a_fresh_backend() {
    let server_url = ServerUrl::parse(&std::env::var("SYNC_E2E_SERVER_URL").expect("SYNC_E2E_SERVER_URL")).unwrap();
    let password = std::env::var("SYNC_E2E_ADMIN_PASSWORD").expect("SYNC_E2E_ADMIN_PASSWORD");
    let session = account_client::login(&server_url, "admin@bokheim.local", &password).await.unwrap();
    let library_id = fixture_library_id("backend-e2e-pagination");
    account_client::create_library(&session, library_id.clone(), "Backend pagination E2E".to_owned()).await.unwrap();

    // Reading registers avoid filesystem materialization, leaving this as
    // a focused protocol test: 1,001 states must cross the 1,000-item
    // pull boundary and be drained by a fresh persistent client.
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
    let changes = (1..=sync_common::MAX_PULL_CHANGES as u64 + 1)
        .map(|index| StateMutation {
            mutation_id: sync_common::MutationId::new(),
            body: MutationBody::ReadingPosition { content_hash: fixture_content_hash(index), value: ReadingPositionState { location: library_model::ReadingPosition::parse("epubcfi(/6/2)").unwrap(), progress: 0.5 } },
            changed_at: now,
            replica_seq: ReplicaSeq::new(index).unwrap(),
        })
        .collect::<Vec<_>>();
    for page in changes.chunks(sync_common::MAX_PUSH_MUTATIONS) {
        let wire_page = page.iter().map(StateMutation::to_wire).collect::<Result<Vec<_>, _>>().unwrap();
        let response = sync_transport::exchange_changes(&reqwest::Client::new(), &session_credentials(&session), library_id.clone(), fixture_replica_id("backend-e2e-pagination-writer"), wire_page, SyncCursor::default())
            .await
            .expect("publish a page of reading registers");
        assert_eq!(response.push.accepted.len(), page.len());
        assert!(response.push.rejected.is_empty());
    }

    let observer_root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(observer_root.path().join(crate::APP_HIDDEN_DIR)).unwrap();
    let mut observer = test_manager_with_replica(observer_root.path(), "backend-e2e-pagination-observer");
    observer.server_url = server_url;
    observer.library_id = library_id;
    observer.credentials = Arc::new(std::sync::RwLock::new(Some(session_credentials(&session))));
    observer.sync_once().await.expect("drain every remote synchronization page");
    let conn = observer.database.get().unwrap();
    let positions: i64 = conn.query_row(include_str!("../../sql/library_sync/production_http_postgres_drains_all_sync_pages_for_a_fresh_backend_select.sql"), [], |row| row.get(0)).unwrap();
    assert_eq!(positions, sync_common::MAX_PULL_CHANGES as i64 + 1);
    let cursor = client_library::sync_state::get_pull_cursor(&conn).unwrap();
    assert!(cursor.reading_revision.is_some(), "the final reading cursor is persisted with the final page");
}

#[tokio::test]
#[ignore = "requires the PostgreSQL sync server started by run_sync_e2e.sh"]
async fn production_http_postgres_serves_asset_endpoints() {
    let server_url = ServerUrl::parse(&std::env::var("SYNC_E2E_SERVER_URL").expect("SYNC_E2E_SERVER_URL")).expect("valid E2E server URL");
    let password = std::env::var("SYNC_E2E_ADMIN_PASSWORD").expect("SYNC_E2E_ADMIN_PASSWORD");
    let observer_session = account_client::login(&server_url, "admin@bokheim.local", &password).await.expect("log in observer");
    let publisher_session = account_client::login(&server_url, "admin@bokheim.local", &password).await.expect("log in publisher");
    assert_eq!(account_client::current_account(&observer_session).await.unwrap().email, observer_session.email());

    let library_id = fixture_library_id("backend-e2e-assets");
    account_client::create_library(&publisher_session, library_id.clone(), "Backend asset E2E".to_owned()).await.expect("create E2E library");
    assert!(account_client::account_snapshot(&observer_session).await.unwrap().libraries.iter().any(|library| library.library_id == library_id));

    use asset_transfer::TransferJob;
    fn replica(root: &Path, library: LibraryId, credentials: sync_transport::SyncCredentials) -> LibrarySync {
        std::fs::create_dir_all(root.join(crate::APP_HIDDEN_DIR)).unwrap();
        let mut manager = test_manager(root);
        manager.library_id = library;
        manager.credentials = Arc::new(std::sync::RwLock::new(Some(credentials)));
        manager
    }
    fn placement(manager: &LibrarySync, hash: ContentHash, name: &str) -> crate::BookPlacement {
        let conn = manager.database.get().unwrap();
        conn.execute(include_str!("../../sql/library_sync/placement_insert.sql"), rusqlite::params![hash.as_str(), name.rsplit_once('.').unwrap().1]).unwrap();
        conn.execute(include_str!("../../sql/library_sync/placement_insert_2.sql"), rusqlite::params![crate::ROOT_DIR_ID.to_string(), hash.as_str(), name]).unwrap();
        conn.execute(include_str!("../../sql/library_sync/placement_insert_3.sql"), rusqlite::params![hash.as_str(), crate::ROOT_DIR_ID.to_string(), format!("/{name}")]).unwrap();
        crate::BookPlacement::new(hash, crate::RelativeBookPath::parse(&format!("/{name}")).unwrap())
    }
    fn local_version(manager: &LibrarySync, identity: ContentHash, bytes: &[u8]) {
        let checksum = ContentHash::new(blake3::hash(bytes).to_hex().as_str());
        let mut conn = manager.database.get().unwrap();
        let tx = conn.transaction().unwrap();
        client_library::assets::observe_local_placement_publication(&tx, &crate::ROOT_DIR_ID, &identity, &checksum, bytes.len() as u64).unwrap();
        tx.commit().unwrap();
    }
    let publisher_root = tempfile::tempdir().unwrap();
    let observer_root = tempfile::tempdir().unwrap();
    let publisher = replica(publisher_root.path(), library_id, session_credentials(&publisher_session));
    let observer = replica(observer_root.path(), library_id, session_credentials(&observer_session));
    let uploader = publisher.transfer_worker();
    let downloader = observer.transfer_worker();
    let client = reqwest::Client::new();
    let book_bytes = test_epub(b"real server asset endpoint coverage");
    let book_hash = ContentHash::new(&blake3::hash(&book_bytes).to_hex().to_string());
    let publisher_credentials = session_credentials(&publisher_session);
    let observer_credentials = session_credentials(&observer_session);
    let manifest = sync_transport::negotiate_blobs(&client, &publisher_credentials, &library_id, vec![BlobManifestEntry { content_hash: book_hash.clone(), checksum: book_hash.clone(), size_bytes: book_bytes.len() as u64 }])
        .await
        .expect("negotiate missing book blob");
    assert_eq!(manifest.upload, vec![book_hash.clone()]);
    placement(&publisher, book_hash, "book.epub");
    let book_placement = placement(&observer, book_hash, "book.epub");
    std::fs::write(publisher_root.path().join("book.epub"), &book_bytes).unwrap();
    local_version(&publisher, book_hash, &book_bytes);
    uploader.handle_job(TransferJob::upload_blob(book_hash, TransferOrigin::UserInitiated)).await.expect("upload negotiated book through the production worker");

    let book_url = publisher_session.server_url().endpoint(&format!("api/libraries/{library_id}/blobs/{}", book_hash.as_str()));
    let head = client.head(book_url.clone()).bearer_auth(publisher_session.token()).send().await.unwrap();
    assert_eq!(head.status(), reqwest::StatusCode::OK);
    assert_eq!(head.headers().get(reqwest::header::CONTENT_LENGTH).unwrap(), &book_bytes.len().to_string());
    let range = client.get(book_url).bearer_auth(publisher_session.token()).header(reqwest::header::RANGE, "bytes=4-11").send().await.unwrap();
    assert_eq!(range.status(), reqwest::StatusCode::PARTIAL_CONTENT);
    assert_eq!(range.bytes().await.unwrap().as_ref(), &book_bytes[4..12]);
    downloader.handle_job(TransferJob::download_blob(book_hash, book_placement, TransferOrigin::UserInitiated)).await.unwrap();
    assert_eq!(std::fs::read(observer_root.path().join("book.epub")).unwrap(), book_bytes);
    let owned = sync_transport::negotiate_blobs(&client, &observer_credentials, &library_id, vec![BlobManifestEntry { content_hash: book_hash.clone(), checksum: book_hash.clone(), size_bytes: book_bytes.len() as u64 }])
        .await
        .expect("negotiate uploaded book blob");
    assert_eq!(owned.owned, vec![book_hash.clone()]);

    let publication = tempfile::tempdir().unwrap();
    let audiobook = publication.path().join("mutable.m4b");
    std::fs::write(&audiobook, include_bytes!("../../../../tests/fixtures/embedded-cover.m4b")).unwrap();
    let identity = publication_identity::ensure(&audiobook).unwrap();
    let mut revision = std::fs::read(&audiobook).unwrap();
    let first_checksum = ContentHash::new(blake3::hash(&revision).to_hex().as_str());
    placement(&publisher, identity, "mutable.m4b");
    let revision_placement = placement(&observer, identity, "mutable.m4b");
    std::fs::write(publisher_root.path().join("mutable.m4b"), &revision).unwrap();
    local_version(&publisher, identity, &revision);
    uploader.handle_job(TransferJob::upload_blob(identity, TransferOrigin::UserInitiated)).await.unwrap();
    let revision_url = publisher_session.server_url().endpoint(&format!("api/libraries/{library_id}/blobs/{identity}"));
    let head = client.head(revision_url.clone()).bearer_auth(publisher_session.token()).send().await.unwrap();
    assert_eq!(head.headers().get("x-bokheim-content-checksum").unwrap(), first_checksum.as_str());
    let first_etag = head.headers().get(reqwest::header::ETAG).unwrap().clone();
    assert_eq!(first_etag, format!("\"{first_checksum}\""));
    let range = client.get(revision_url.clone()).bearer_auth(publisher_session.token()).header(reqwest::header::RANGE, "bytes=4-11").header(reqwest::header::IF_RANGE, first_etag.clone()).send().await.unwrap();
    assert_eq!(range.status(), reqwest::StatusCode::PARTIAL_CONTENT);
    assert_eq!(range.bytes().await.unwrap().as_ref(), &revision[4..12]);
    revision.extend_from_slice(b"\0\0\0\x0cfreedata");
    std::fs::write(publisher_root.path().join("mutable.m4b"), &revision).unwrap();
    local_version(&publisher, identity, &revision);
    uploader.handle_job(TransferJob::upload_blob(identity, TransferOrigin::UserInitiated)).await.unwrap();
    let stale_range = client.get(revision_url).bearer_auth(publisher_session.token()).header(reqwest::header::RANGE, "bytes=4-11").header(reqwest::header::IF_RANGE, first_etag).send().await.unwrap();
    assert_eq!(stale_range.status(), reqwest::StatusCode::OK, "a stale range must restart with the complete current revision");
    assert_eq!(stale_range.bytes().await.unwrap().as_ref(), revision);
    downloader.handle_job(TransferJob::download_blob(identity, revision_placement, TransferOrigin::UserInitiated)).await.unwrap();
    assert_eq!(std::fs::read(observer_root.path().join("mutable.m4b")).unwrap(), revision);

    let mut jpeg = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(2, 3, image::Rgb([24, 96, 192]))).write_to(&mut jpeg, image::ImageOutputFormat::Jpeg(85)).unwrap();
    let thumbnail = jpeg.into_inner();
    let thumbnail_url = publisher_session.server_url().endpoint(&format!("api/libraries/{library_id}/thumbnails/{}", book_hash.as_str()));
    assert_eq!(client.put(thumbnail_url.clone()).bearer_auth(publisher_session.token()).body(thumbnail.clone()).send().await.unwrap().status(), reqwest::StatusCode::OK);
    let thumbnail_head = client.head(thumbnail_url.clone()).bearer_auth(observer_session.token()).send().await.unwrap();
    assert_eq!(thumbnail_head.status(), reqwest::StatusCode::OK);
    assert_eq!(thumbnail_head.headers().get(reqwest::header::CONTENT_LENGTH).unwrap(), &thumbnail.len().to_string());
    downloader.clone().run(vec![TransferJob::download_thumbnail(book_hash, TransferOrigin::UserInitiated)]).await.unwrap();
    assert_eq!(observer.assets.read_bytes(BlobKind::Thumbnail, &book_hash).await.unwrap().unwrap(), thumbnail);
    assert!(observer.assets.thumbnail_set_exists(&book_hash).unwrap(), "download must generate the browse variant");

    // Asset endpoints must reject malformed, unauthorized, and quota
    // exceeding requests without disturbing an already-owned blob.
    assert_eq!(client.head(publisher_session.server_url().endpoint(&format!("api/libraries/{library_id}/blobs/{}", book_hash.as_str()))).send().await.unwrap().status(), reqwest::StatusCode::UNAUTHORIZED);
    let invalid_range = client
        .get(publisher_session.server_url().endpoint(&format!("api/libraries/{library_id}/blobs/{}", book_hash.as_str())))
        .bearer_auth(publisher_session.token())
        .header(reqwest::header::RANGE, format!("bytes={}-", book_bytes.len()))
        .send()
        .await
        .unwrap();
    assert_eq!(invalid_range.status(), reqwest::StatusCode::RANGE_NOT_SATISFIABLE);
    let wrong_hash = ContentHash::new(&"f".repeat(64));
    let missing_length = client
        .put(publisher_session.server_url().endpoint(&format!("api/libraries/{library_id}/blobs/{}", wrong_hash.as_str())))
        .bearer_auth(publisher_session.token())
        .body(reqwest::Body::wrap_stream(futures_util::stream::iter(vec![Ok::<_, std::io::Error>(axum::body::Bytes::from(book_bytes.clone()))])))
        .send()
        .await
        .unwrap();
    assert_eq!(missing_length.status(), reqwest::StatusCode::LENGTH_REQUIRED);
    let wrong_hash_upload = client.put(publisher_session.server_url().endpoint(&format!("api/libraries/{library_id}/blobs/{}", wrong_hash.as_str()))).bearer_auth(publisher_session.token()).body(book_bytes.clone()).send().await.unwrap();
    assert_eq!(wrong_hash_upload.status(), reqwest::StatusCode::BAD_REQUEST);

    let database_url = std::env::var("SYNC_E2E_DATABASE_URL").expect("SYNC_E2E_DATABASE_URL");
    let database = sqlx::PgPool::connect(&database_url).await.unwrap();
    sqlx::query(include_str!("../../sql/library_sync/local_version_update.sql")).bind(publisher_session.user_id()).execute(&database).await.unwrap();
    let quota_hash = ContentHash::new(&"e".repeat(64));
    let quota = sync_transport::negotiate_blobs(&client, &publisher_credentials, &library_id, vec![BlobManifestEntry { content_hash: quota_hash.clone(), checksum: quota_hash.clone(), size_bytes: 1 }])
        .await
        .expect("quota rejection is a valid manifest response");
    assert_eq!(quota.rejected.len(), 1);
    assert_eq!(quota.rejected[0].content_hash, quota_hash);
    assert_eq!(quota.rejected[0].reason, sync_common::api::assets::BlobManifestRejectionReason::QuotaExceeded);
}
