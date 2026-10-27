//! Feature-gated runtime checks executed in a real browser worker with OPFS.

use super::*;
use crate::integration::LibraryAccountSession;
use crate::library::{LibraryOpenConfig, LibraryRuntimeSpec};
use cloud_storage::{
    connections::{CloudConnections, ConnectionPersistence, GoogleConnection},
    oauth::{GoogleAccount, GoogleOAuth, GoogleOAuthConfig, GoogleTokens},
};
use std::{
    io::{Read, Seek, SeekFrom},
    sync::Arc,
};
use sync_common::storage::{CloudProvider, LibraryStorage, ProviderScope};

const EPUB: &[u8] = include_bytes!("../../../tests/fixtures/cloud-cover.epub");

struct BrowserLog;
impl log::Log for BrowserLog {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Info
    }
    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            web_sys::console::log_1(&format!("{}: {}", record.level(), record.args()).into());
        }
    }
    fn flush(&self) {}
}
static LOGGER: BrowserLog = BrowserLog;

#[derive(Clone, Debug)]
struct NoBokheimAccount;
impl account_client::SessionPersistence for NoBokheimAccount {
    fn load_account_session(&self) -> std::io::Result<Option<account_client::Session>> {
        Ok(None)
    }
    fn save_account_session(&self, _: &account_client::Session) -> std::io::Result<()> {
        panic!("provider storage cannot sign in to Bokheim")
    }
    fn clear_account_session(&self) -> std::io::Result<()> {
        Ok(())
    }
}

struct Tokens;
impl ConnectionPersistence for Tokens {
    fn load_google(&self, _: &ProviderId) -> Result<Option<GoogleConnection>, CloudError> {
        Ok(Some(GoogleConnection {
            account: GoogleAccount { id: ProviderId::new("fixture-account").unwrap(), email: "reader@example.com".into(), name: None },
            tokens: GoogleTokens { access_token: "fixture-token".into(), refresh_token: None, expires_at: u64::MAX },
        }))
    }
    fn save_google(&self, _: &GoogleConnection, _: Option<&str>) -> Result<bool, CloudError> {
        panic!("fixture token must not refresh")
    }
}

pub async fn install_taxonomy(endpoint: &str) -> Result<(), BackendError> {
    log::set_logger(&LOGGER).map_err(BackendError::operation)?;
    log::set_max_level(log::LevelFilter::Info);
    std::panic::set_hook(Box::new(|info| web_sys::console::error_1(&info.to_string().into())));
    client_platform_web::web_storage::initialize().await.map_err(|error| BackendError::message(error.to_string()))?;
    let snapshot = reqwest::get(format!("{endpoint}/taxonomy.sqlite3")).await.map_err(BackendError::operation)?.error_for_status().map_err(BackendError::operation)?.bytes().await.map_err(BackendError::operation)?;
    let mut db = rusqlite::Connection::open_in_memory()?;
    db.deserialize_read_exact("main", snapshot.as_ref(), snapshot.len(), false)?;
    let release: String = db.query_row("SELECT value FROM taxonomy_meta WHERE key='release_id'", [], |row| row.get(0))?;
    let image = subject_projection::installed::prepare_image(&snapshot, (release.parse().map_err(BackendError::operation)?, 1), "browser cloud fixture").map_err(BackendError::message)?;
    let path = std::path::Path::new("taxonomy-cloud-fixture.sqlite3");
    client_platform_web::web_storage::import_reference_database(path.to_str().unwrap(), &image).map_err(BackendError::message)?;
    subject_projection::installed::activate(path).map_err(BackendError::message)
}

pub async fn run(endpoint: &str, writable: bool) -> Result<String, BackendError> {
    let id = uuid::Uuid::new_v4();
    let scope = ProviderScope { provider: CloudProvider::GoogleDrive, account_id: ProviderId::new("fixture-account").unwrap(), container_id: ProviderId::new("my_drive").unwrap() };
    let root = CloudLibraryRoot { scope: scope.clone(), folder_id: ProviderId::new("folder").unwrap() };
    let oauth = GoogleOAuth::new(GoogleOAuthConfig { client_id: "fixture-client".into(), client_secret: None }).map_err(BackendError::operation)?;
    let cloud = CloudConnections::new(oauth, Arc::new(Tokens)).with_test_google_drive_endpoint(scope.clone(), endpoint.parse().map_err(BackendError::operation)?).map_err(BackendError::operation)?;
    let account = LibraryAccountSession::load(NoBokheimAccount, "http://127.0.0.1:1".parse().unwrap()).map_err(BackendError::operation)?;
    let config = LibraryOpenConfig {
        id,
        display_name: "Drive fixture".into(),
        root: id.to_string(),
        database_locator: format!("cloud-{id}.sqlite"),
        default_sync_server_url: "http://127.0.0.1:1".parse().unwrap(),
        metadata_server_url: "http://127.0.0.1:1".into(),
        google_books_api_key: None,
        storage: LibraryStorage::External(root.clone()),
        reset_cloud_presence: false,
    };
    let session = LibrarySession::open(LibraryRuntimeSpec { config: config.clone(), account: account.library_account(), cloud: cloud.clone(), notification_interest: Default::default() })?;
    session.run_cloud_pass().await?;
    assert_eq!(session.db.book_count()?, 1);
    let (book, file) = session.db.provider_remote_books()?.pop().unwrap();
    assert!(book.is_external());
    assert_eq!(session.db.book_format(&book)?, Some(BookFormat::Epub));
    assert_eq!(session.db.provider_cover_cached(&book)?, Some(true));
    assert!(session.assets.thumbnail_set_exists(&book).map_err(BackendError::operation)?);
    let pair = session.db.provider_thumbnails(&book)?;
    assert_eq!(pair.is_some(), writable);
    if let Some(pair) = &pair {
        assert_ne!(pair.full.identity.file_id, pair.browse.identity.file_id);
    }
    assert!(session.db.pending_cover_uploads(None, 10)?.is_empty());
    let connection = session.cloud.google_drive(scope).map_err(BackendError::operation)?;
    if writable {
        session.assets.remove(library_model::BlobKind::Thumbnail, &book).map_err(BackendError::operation)?;
        session.cloud_thumbnails(&connection, &root).await?;
        assert!(session.assets.thumbnail_set_exists(&book).map_err(BackendError::operation)?);
        assert_eq!(session.db.provider_thumbnails(&book)?, pair);
    }
    session.db.sync_request_book_downloads(&[book], TransferOrigin::UserInitiated)?;
    session.download_cloud_book(&connection, book).await?;
    assert_eq!(session.persistent_download_state(book)?, DownloadState::Downloaded);
    let mut pinned = session.assets.playback_reader(&book).await.map_err(BackendError::operation)?.unwrap();
    let mut bytes = Vec::new();
    pinned.read_to_end(&mut bytes)?;
    assert_eq!(bytes, EPUB);
    assert_eq!(pinned.content_hash().map_err(BackendError::message)?, Some(book));
    let (opened, _lease) = session.prepare_book_for_reader(book).await?;
    assert!(matches!(opened.source, crate::book::BookSource::Local { .. }));

    // The browser fixture injects the actual DOMException at the OPFS flush
    // boundary. A failed replacement must retain the published generation.
    let mut full = session.assets.begin_provider_write(&file).await.map_err(BackendError::operation)?;
    full.write_all(EPUB).await.map_err(BackendError::operation)?;
    js_sys::Reflect::set(&js_sys::global(), &"__cloudFixtureStorageFault".into(), &"flush".into()).map_err(|_| BackendError::message("could not inject OPFS quota failure"))?;
    let error = match full.publish().await {
        Err(error) => BackendError::operation(error),
        Ok(_) => return Err(BackendError::message("OPFS quota injection did not fail the publication")),
    };
    assert_eq!(error.failure(), Some(client_runtime::Failure::StorageFull));
    assert_eq!(session.persistent_download_state(book)?, DownloadState::Downloaded);
    let mut retained = session.assets.playback_reader(&book).await.map_err(BackendError::operation)?.unwrap();
    let mut retained_bytes = Vec::new();
    retained.read_to_end(&mut retained_bytes)?;
    assert_eq!(retained_bytes, EPUB);
    drop(retained);

    let mut bad = file.clone();
    bad.content_hash = Some(blake3::hash(b"incorrect fixture digest").into());
    let mut writer = session.assets.begin_provider_write(&bad).await.map_err(BackendError::operation)?;
    writer.write_all(EPUB).await.map_err(BackendError::operation)?;
    assert!(writer.publish().await.is_err());
    let mut first = session.assets.begin_provider_write(&file).await.map_err(BackendError::operation)?;
    first.write_all(EPUB).await.map_err(BackendError::operation)?;
    let obsolete = first.publish().await.map_err(BackendError::operation)?;
    let mut second = session.assets.begin_provider_write(&file).await.map_err(BackendError::operation)?;
    second.write_all(EPUB).await.map_err(BackendError::operation)?;
    let _current = second.publish().await.map_err(BackendError::operation)?;
    obsolete.discard().map_err(BackendError::message)?;
    assert!(session.assets.playback_reader(&book).await.map_err(BackendError::operation)?.is_some());
    assert_eq!(session.remove_local_book_copy(book)?, DownloadState::NotDownloaded);
    assert!(session.assets.playback_reader(&book).await.map_err(BackendError::operation)?.is_none());
    pinned.seek(SeekFrom::Start(0))?;
    bytes.clear();
    pinned.read_to_end(&mut bytes)?;
    assert_eq!(bytes, EPUB, "an open reader must survive cache replacement and eviction");
    assert!(session.db.remote_asset_hashes(library_model::BlobKind::Book)?.contains(&book));
    session.db.sync_request_book_downloads(&[book], TransferOrigin::UserInitiated)?;
    session.run_cloud_pass().await?;
    assert_eq!(session.persistent_download_state(book)?, DownloadState::Downloaded);
    assert!(session.db.pending_book_uploads(0, 10)?.is_empty());
    session.remove_local_book_copy(book)?;
    session.db.update_reading_position(&book, "epubcfi(/6/2)", Some(42.0), None)?;
    let original = session.db.book_transfer_state(book)?;
    let copy_id = uuid::Uuid::new_v4();
    let copy = LibrarySession::open(LibraryRuntimeSpec {
        config: LibraryOpenConfig {
            id: copy_id,
            display_name: "Local copy".into(),
            root: copy_id.to_string(),
            database_locator: format!("cloud-copy-{copy_id}.sqlite"),
            default_sync_server_url: "http://127.0.0.1:1".parse().unwrap(),
            metadata_server_url: "http://127.0.0.1:1".into(),
            google_books_api_key: None,
            storage: LibraryStorage::Local,
            reset_cloud_presence: false,
        },
        account: account.library_account(),
        cloud: Default::default(),
        notification_interest: Default::default(),
    })?;
    js_sys::Reflect::set(&js_sys::global(), &"__cloudFixtureStorageFault".into(), &"flush".into()).map_err(|_| BackendError::message("could not inject copy quota failure"))?;
    let resource = session.export_transfer_book(book).await?;
    let error = copy.import_transfer_resource(ROOT_DIR_ID, "Book.epub".into(), resource).await.unwrap_err();
    assert_eq!(error.failure(), Some(client_runtime::Failure::StorageFull));
    assert_eq!(copy.db.book_count()?, 0);
    assert_eq!(session.db.book_count()?, 1);
    let resource = session.export_transfer_book(book).await?;
    let copied = copy.import_transfer_resource(ROOT_DIR_ID, "Book.epub".into(), resource).await?;
    assert_eq!(copied.content_hash, BookId::from(blake3::hash(EPUB)));
    assert!(!copied.content_hash.is_external());
    assert_eq!(copy.db.book_transfer_state(copied.content_hash)?.reading, original.clone().retarget(copied.content_hash)?.reading);
    assert!(session.assets.playback_reader(&book).await.map_err(BackendError::operation)?.is_none(), "copying cannot populate or change the source cache");
    assert_eq!(session.db.storage_policy()?, LibraryStorage::External(root.clone()));
    drop(pinned);
    Box::pin(check_uploads(session, &copy, copied.content_hash, original, config, cloud, &account, writable)).await?;
    Ok(serde_json::json!({"scanned":1,"provider_thumbnails":writable,"local_thumbnails":true,"downloaded":true,"local_reader":true,"quota_preserved_download":true,"checksum_rejected":true,"new_generation_preserved":true,"evicted_and_downloaded_again":true,"open_reader_preserved":true,"storage_copy_quota_retry":true,"browser_upload_retained":true,"browser_upload_recovered":writable,"browser_copy_to_drive":writable}).to_string())
}

async fn check_uploads(
    session: LibrarySession, copy: &LibrarySession, copied: BookId, original: library_database::transfer::BookTransferState, config: LibraryOpenConfig, cloud: CloudConnections, account: &LibraryAccountSession<NoBokheimAccount>,
    writable: bool,
) -> Result<(), BackendError> {
    let storage = config.storage.clone();
    // Exercise actual browser import staging, durable retry, and publication.
    js_sys::Reflect::set(&js_sys::global(), &"__cloudFixtureStorageFault".into(), &"flush".into()).unwrap();
    let error = Box::pin(session.import.import_downloaded_book(ROOT_DIR_ID, "Quota.epub", EPUB.to_vec())).await.unwrap_err();
    assert_eq!(error.failure(), Some(client_runtime::Failure::StorageFull));
    assert!(session.assets.retained_provider_uploads().map_err(BackendError::operation)?.is_empty());
    assert_eq!(session.db.pending_provider_upload_count()?, 0);
    let error = Box::pin(session.import.import_downloaded_book(ROOT_DIR_ID, "New Book.epub", EPUB.to_vec())).await.unwrap_err();
    assert_eq!(error.failure(), Some(if writable { client_runtime::Failure::CloudStorageFull } else { client_runtime::Failure::PermissionDenied }));
    assert_eq!(session.db.book_count()?, 1, "unconfirmed uploads cannot publish library metadata");
    assert_eq!(session.db.pending_provider_upload_count()?, 1);
    let retained = session.assets.retained_provider_uploads().map_err(BackendError::operation)?;
    assert_eq!(retained.len(), 1);
    let retry = session.import.run_provider_uploads().await.unwrap_err();
    assert_eq!(retry.failure(), Some(if writable { client_runtime::Failure::CloudStorageFull } else { client_runtime::Failure::PermissionDenied }));
    let pending_id = retained[0].0;
    let reserved = session.db.provider_upload(pending_id)?.unwrap().file_id.unwrap();
    if writable {
        // Same source is idempotent. The fixture commits the upload but loses
        // its final reply and subsequently expires the resumable session.
        let retry = Box::pin(session.import.import_downloaded_book(ROOT_DIR_ID, "New Book.epub", EPUB.to_vec())).await.unwrap_err();
        assert!(retry.to_string().contains("503"), "the same inspected book must reach the saved provider upload: {retry}");
        assert_eq!(session.db.pending_provider_upload_count()?, 1);
        assert_eq!(session.assets.retained_provider_uploads().map_err(BackendError::operation)?.len(), 1);
        assert_eq!(session.db.provider_upload(pending_id)?.unwrap().file_id, Some(reserved.clone()));
        assert_eq!(session.db.book_count()?, 1);
        drop(session);
        let reopened = LibrarySession::open(LibraryRuntimeSpec { config, account: account.library_account(), cloud, notification_interest: Default::default() })?;
        // Final cache promotion reuses the retained generation even when a
        // second book-sized OPFS allocation would fail.
        js_sys::Reflect::set(&js_sys::global(), &"__cloudFixtureStorageFault".into(), &"book_create".into()).unwrap();
        reopened.import.run_provider_uploads().await?;
        js_sys::Reflect::delete_property(&js_sys::global(), &"__cloudFixtureStorageFault".into()).unwrap();
        assert_eq!(reopened.db.book_count()?, 2);
        assert_eq!(reopened.db.pending_provider_upload_count()?, 0);
        assert!(reopened.assets.retained_provider_uploads().map_err(BackendError::operation)?.is_empty());
        let uploaded = reopened.db.provider_remote_books()?.into_iter().find(|(_, file)| file.identity.file_id == reserved).unwrap().0;
        assert!(uploaded.is_external());
        assert_eq!(reopened.persistent_download_state(uploaded)?, DownloadState::Downloaded);
        let mut reader = reopened.assets.playback_reader(&uploaded).await.map_err(BackendError::operation)?.unwrap();
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        assert_eq!(bytes, EPUB);
        let resource = copy.export_transfer_book(copied).await?;
        let transferred = Box::pin(reopened.import_transfer_resource(ROOT_DIR_ID, "Transferred Book".into(), resource)).await?;
        assert!(transferred.content_hash.is_external());
        assert_eq!(reopened.persistent_download_state(transferred.content_hash)?, DownloadState::Downloaded);
        assert_eq!(reopened.db.book_transfer_state(transferred.content_hash)?.reading, original.retarget(transferred.content_hash)?.reading);
        assert_eq!(copy.db.book_count()?, 1, "copying to Drive keeps the source library");
        assert!(copy.assets.playback_reader(&copied).await.map_err(BackendError::operation)?.is_some());
        assert!(reopened.assets.retained_provider_uploads().map_err(BackendError::operation)?.is_empty());
        assert_eq!(reopened.db.storage_policy()?, storage);
    }
    Ok(())
}
