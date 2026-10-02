use std::sync::RwLock;
use super::*;
use crate::sync::error::SyncResult;
use book_model::{AnnotationAnchor, AnnotationState, AnnotationStyle};
use library_replica::{BookLifecycleState, ReadingPositionState};
use library_replica::{LibraryStateEntry, StatePublication};
use std::path::Path;
use sync_common::{LibraryRevision, MutationRejection, MutationRejectionReason, ReplicaSeq, StateDelivery, SyncCursor};
use test_support::{fixture_content_hash, fixture_dir_id, fixture_library_id, fixture_replica_id};

const TEST_AUTH_TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
use library_database::CURSOR_RECOVERY_PENDING_KEY;
use library_replica::coalesce_mutations_for_push;
use library_replica::prepare_push;
use sync_common::SyncExchangeResponse;
use sync_common::{PullStateResponse, PushMutationsResponse};
use sync_transport::{validate_pull_batch, validate_push_response, PushResponseError};

mod accounts;
mod assets;
mod cancellation;
mod metadata;
mod protocol;
mod recovery;

fn test_credentials(server_url: &ServerUrl) -> sync_transport::SyncCredentials {
    sync_transport::SyncCredentials::new(server_url.clone(), TEST_AUTH_TOKEN)
}

fn test_shared_credentials(server_url: &ServerUrl) -> library_runtime::account::SharedSyncCredentials {
    shared_credentials(test_credentials(server_url))
}

fn shared_credentials(credentials: sync_transport::SyncCredentials) -> library_runtime::account::SharedSyncCredentials {
    std::sync::Arc::new(RwLock::new(Some(credentials)))
}

fn local_seq(value: u64) -> ReplicaSeq {
    ReplicaSeq::new(value).unwrap()
}

fn millis(value: u64) -> sync_common::UnixMillis {
    value
}

fn pulled_change(server_sequence: u64, body: LibraryStateEntry) -> StateDelivery {
    sync_common::fixture_delivery(
        sync_common::transport::representation::<_, sync_common::OpaquePublication>(&((body).publication(sync_common::ReplicaId::nil(), millis(10), local_seq(server_sequence)))).unwrap(),
        test_support::fixture_replica_id("remote-device"),
        LibraryRevision::new(server_sequence).unwrap(),
    )
}

fn deleted(content_hash: u64) -> LibraryStateEntry {
    LibraryStateEntry::BookLifecycle { content_hash: fixture_content_hash(content_hash), value: BookLifecycleState::Deleted { origin_folder_id: None } }
}

fn added(content_hash: u64, added_at: u64) -> LibraryStateEntry {
    LibraryStateEntry::BookLifecycle { content_hash: fixture_content_hash(content_hash), value: BookLifecycleState::Present }
}

fn cursor(revision: Option<LibraryRevision>) -> SyncCursor {
    SyncCursor { lifecycle_revision: revision, field_revision: revision }
}

fn pull_response(mutations: Vec<StateDelivery>, next_revision: Option<LibraryRevision>, has_more: bool) -> PullStateResponse {
    let mutations = crate::sync::fixture_lifecycles_first(mutations);
    let next_cursor = if has_more {
        let fields = mutations.iter().filter(|mutation| mutation.state.value.address.kind != "book_lifecycle");
        SyncCursor { lifecycle_revision: mutations.iter().map(|mutation| mutation.revision).max(), field_revision: fields.map(|mutation| mutation.revision).last() }
    } else {
        cursor(next_revision)
    };
    PullStateResponse { mutations, next_cursor, has_more }
}

fn binary_sync_response<T: serde::Serialize>(value: &T) -> axum::response::Response {
    use axum::response::IntoResponse;

    let encoded = sync_common::transport::encode(value).unwrap();
    let mut response = encoded.into_response();
    response.headers_mut().insert(axum::http::header::CONTENT_TYPE, axum::http::HeaderValue::from_static(sync_common::transport::MEDIA_TYPE));
    response
}

fn decode_sync_exchange(headers: &axum::http::HeaderMap, body: &[u8]) -> sync_common::SyncExchangeRequest {
    assert!(headers.get(axum::http::header::CONTENT_ENCODING).is_none());
    sync_common::transport::decode(body, sync_common::transport::MAX_DECODED_REQUEST_BYTES).unwrap()
}

/// A deferred mutation means "ask again later", not "this exchange
/// failed". Failing the whole synchronization strands everything the
/// exchange did accomplish -- notably asset transfers and the completion
/// checkpoint -- behind one mutation the server will not take yet, which
/// for a device whose clock is ahead never resolves on its own.
fn reading_change(sequence: u64, content_hash: u64, cfi: &str, changed_at: u64) -> StatePublication {
    (LibraryStateEntry::ReadingPosition { content_hash: fixture_content_hash(content_hash), value: ReadingPositionState { location: book_model::ReadingPosition::parse(cfi).unwrap(), progress: 0.0 } }).publication(
        sync_common::ReplicaId::nil(),
        millis(changed_at),
        local_seq(sequence),
    )
}

fn transmittable(changes: Vec<StatePublication>) -> sync_common::PushBatcher {
    let queued = prepare_push(changes).unwrap();
    assert!(queued.untransmittable().is_empty(), "this fixture holds nothing larger than a batch");
    queued
}

fn annotation_change(sequence: u64, text: String) -> StatePublication {
    (LibraryStateEntry::Annotation {
        annotation_id: uuid::Uuid::new_v4().to_string(),
        value: AnnotationState {
            content_hash: fixture_content_hash(sequence),
            detail: book_model::AnnotationDetail {
                anchor: AnnotationAnchor::EpubCfi { cfi: "epubcfi(/6/2)".to_owned() },
                exact_text: text.clone(),
                style: AnnotationStyle::Highlight,
                color: "yellow".to_owned(),
                note: text,
                created_at: millis(1),
                toc_ordinal: None,
                progress: None,
            },
            modified_at: millis(1),
            deleted: false,
        },
    })
    .publication(sync_common::ReplicaId::nil(), millis(sequence), local_seq(sequence))
}

/// Test handle pairing the session-owned database with its sync worker.
/// Method calls dereference to `LibrarySync`; storage travels explicitly.
struct TestManager {
    sync: LibrarySync,
    database: Database,
}

impl TestManager {
    /// Session hands the worker its library file, mirroring production.
    fn transfer_worker(&self) -> asset_transfer::transfer::TransferWorker {
        self.sync.transfer_worker()
    }

    fn request_download(&self, content_hash: ContentHash) -> SyncResult<()> {
        self.sync.request_download(content_hash)
    }

    fn request_downloads(&self, content_hashes: &[ContentHash]) -> SyncResult<()> {
        self.sync.request_downloads(content_hashes)
    }

    fn prepare_requested_download(&self, hash: &ContentHash) -> SyncResult<Option<asset_transfer::TransferJob>> {
        self.sync.prepare_requested_download(hash)
    }

    async fn synchronize_state(&self) -> SyncResult<sync_engine::SyncOutcome> {
        self.sync.synchronize_state_mode(false).await
    }

    async fn sync_once(&self) -> SyncResult<()> {
        self.sync.sync_once().await
    }

    async fn reconcile_assets(&self, targeted: Vec<ContentHash>) -> SyncResult<()> {
        self.sync.reconcile_assets(targeted).await
    }

    fn queue_outbox_reading_change(&self, mutation: &StatePublication, sequence: u64) {
        // These fixtures exercise edits of books already present on the device.
        library_database::configure_fixture_connection(&self.database, |conn| {
            let tx = conn.unchecked_transaction()?;
            tx.execute("INSERT OR IGNORE INTO book(content_hash) VALUES(?1)", [fixture_content_hash(sequence).as_str()])?;
            tx.commit()
        })
        .unwrap();
        self.database
            .enqueue_outbox_reading_change(fixture_content_hash(sequence).as_str(), &sync_common::wire::encode(&mutation.state.value).unwrap(), i64::try_from(mutation.state.version.changed_at).unwrap())
            .unwrap();
    }
}

impl std::ops::Deref for TestManager {
    type Target = LibrarySync;
    fn deref(&self) -> &Self::Target {
        &self.sync
    }
}

impl std::ops::DerefMut for TestManager {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.sync
    }
}

fn test_manager(root: &Path) -> TestManager {
    test_manager_with_replica(root, "client")
}

fn test_manager_with_replica(root: &Path, replica_id: &str) -> TestManager {
    let db_path = root.join(crate::APP_HIDDEN_LIBRARY_DB_PATH);
    let database = Database::open(&db_path).unwrap();
    database.initialize_library().unwrap();
    let sync = LibrarySync::new(LibrarySyncConfig {
        database_path: db_path.clone(),
        assets: crate::asset_store::open(root.to_str().unwrap()).unwrap(),
        replica_id: fixture_replica_id(replica_id),
        server_url: ServerUrl::parse("http://127.0.0.1:9").unwrap(),
        library_id: fixture_library_id("library"),
        event_tx: None,
        transfer_queue: Arc::new(TransferQueue::default()),
        credentials: Arc::new(RwLock::new(None)),
        account: None,
        notification_interest: Default::default(),
        cpu: std::sync::Arc::new(client_platform_native::cpu_host::NativeHost::default()),
    });
    TestManager { sync, database }
}
