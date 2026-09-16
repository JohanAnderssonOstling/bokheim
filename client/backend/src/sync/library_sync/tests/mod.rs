use super::*;
use library_model::{AnnotationAnchor, AnnotationState, AnnotationStyle, BookFormat, BookLifecycleState, ReadingPositionState};
use library_replica::{MutationBody, StateMutation};
use std::io::Write;
use std::path::Path;
use sync_common::{LibraryRevision, MutationRejection, MutationRejectionReason, ReplicaSeq, ServerMutation, SyncCursor};
use test_support::{fixture_content_hash, fixture_dir_id, fixture_library_id, fixture_replica_id};

const TEST_CONTENT_HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const TEST_AUTH_TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
use crate::BlobKind;
use crate::sync::transfer::TransferJob;
use asset_transfer::should_download_thumbnail;
use client_library::sync_state::CURSOR_RECOVERY_PENDING_KEY;
use library_replica::coalesce_mutations_for_push;
use library_replica::prepare_push;
use sync_common::SyncExchangeResponse;
use sync_common::api::assets::BlobManifestEntry;
use sync_common::{PullStateResponse, PushMutationsResponse};
use sync_transport::{PushResponseError, validate_pull_batch, validate_push_response};

mod accounts;
mod assets;
mod cancellation;
mod metadata;
mod postgres;
mod protocol;
mod recovery;

fn test_credentials(server_url: &ServerUrl) -> sync_transport::SyncCredentials {
    sync_transport::SyncCredentials::new(server_url.clone(), TEST_AUTH_TOKEN)
}

fn session_credentials(session: &account_client::Session) -> sync_transport::SyncCredentials {
    sync_transport::SyncCredentials::new(session.server_url().clone(), session.token())
}

fn test_content_hash() -> ContentHash {
    ContentHash::new(&TEST_CONTENT_HASH)
}

fn test_epub(payload: &[u8]) -> Vec<u8> {
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let compressed = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    archive.start_file("mimetype", stored).unwrap();
    archive.write_all(b"application/epub+zip").unwrap();
    archive.start_file("META-INF/container.xml", compressed).unwrap();
    archive
        .write_all(br#"<?xml version="1.0"?><container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#)
        .unwrap();
    archive.start_file("OEBPS/content.opf", compressed).unwrap();
    archive.write_all(br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata/><manifest><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="chapter"/></spine></package>"#).unwrap();
    archive.start_file("OEBPS/chapter.xhtml", compressed).unwrap();
    archive.write_all(br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><pre>"#).unwrap();
    archive.write_all(payload).unwrap();
    archive.write_all(b"</pre></body></html>").unwrap();
    archive.finish().unwrap().into_inner()
}

fn local_seq(value: u64) -> ReplicaSeq {
    ReplicaSeq::new(value).unwrap()
}

fn millis(value: u64) -> sync_common::UnixMillis {
    value
}

fn pulled_change(server_sequence: u64, body: MutationBody) -> ServerMutation {
    ServerMutation {
        mutation: StateMutation { mutation_id: sync_common::MutationId::new(), body, changed_at: millis(10), replica_seq: local_seq(server_sequence) }.to_wire().unwrap(),
        replica_id: test_support::fixture_replica_id("remote-device"),
        revision: LibraryRevision::new(server_sequence).unwrap(),
    }
}

fn deleted(content_hash: u64) -> MutationBody {
    MutationBody::BookLifecycle { content_hash: fixture_content_hash(content_hash), value: BookLifecycleState::Deleted { origin_folder_id: None } }
}

fn added(content_hash: u64, title: &str, added_at: u64) -> MutationBody {
    MutationBody::BookLifecycle {
        content_hash: fixture_content_hash(content_hash),
        value: BookLifecycleState::Present { metadata: library_model::SyncBookMetadata { title: title.to_owned(), contributors: Vec::new(), publication: Default::default() }, added_at: millis(added_at), format: BookFormat::Epub },
    }
}

fn cursor(revision: Option<LibraryRevision>) -> SyncCursor {
    SyncCursor { state_revision: revision, reading_revision: revision }
}

fn pull_response(mutations: Vec<ServerMutation>, next_revision: Option<LibraryRevision>, has_more: bool) -> PullStateResponse {
    let next_cursor = if has_more {
        let mut cursor = SyncCursor::default();
        for mutation in &mutations {
            if mutation.mutation.kind == sync_common::mutation_kind::READING_POSITION {
                cursor.reading_revision = Some(mutation.revision);
            } else {
                cursor.state_revision = Some(mutation.revision);
            }
        }
        cursor
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

fn queue_outbox_reading_change(manager: &LibrarySync, mutation: &StateMutation, sequence: u64) {
    let conn = manager.database.get().unwrap();
    conn.execute(
        include_str!("../../sql/library_sync/exchange_page_insert.sql"),
        rusqlite::params![mutation.mutation_id.to_string(), fixture_content_hash(sequence).as_str(), sync_common::wire::encode(&mutation.body).unwrap(), i64::try_from(mutation.changed_at).unwrap()],
    )
    .unwrap();
}

/// A deferred mutation means "ask again later", not "this exchange
/// failed". Failing the whole synchronization strands everything the
/// exchange did accomplish -- notably asset transfers and the completion
/// checkpoint -- behind one mutation the server will not take yet, which
/// for a device whose clock is ahead never resolves on its own.

fn reading_change(sequence: u64, content_hash: u64, cfi: &str, changed_at: u64) -> StateMutation {
    StateMutation {
        mutation_id: sync_common::MutationId::new(),
        body: MutationBody::ReadingPosition { content_hash: fixture_content_hash(content_hash), value: ReadingPositionState { location: library_model::ReadingPosition::parse(cfi).unwrap(), progress: 0.0 } },
        changed_at: millis(changed_at),
        replica_seq: local_seq(sequence),
    }
}

fn transmittable(changes: Vec<StateMutation>) -> sync_common::PushBatcher {
    let queued = prepare_push(changes).unwrap();
    assert!(queued.untransmittable().is_empty(), "this fixture holds nothing larger than a batch");
    queued
}

fn annotation_change(sequence: u64, text: String) -> StateMutation {
    StateMutation {
        mutation_id: sync_common::MutationId::new(),
        body: MutationBody::Annotation {
            annotation_id: uuid::Uuid::new_v4().to_string(),
            value: AnnotationState {
                content_hash: fixture_content_hash(sequence),
                anchor: AnnotationAnchor::EpubCfi { cfi: "epubcfi(/6/2)".to_owned() },
                exact_text: text.clone(),
                style: AnnotationStyle::Highlight,
                color: "yellow".to_owned(),
                note: text,
                created_at: millis(1),
                modified_at: millis(1),
                deleted: false,
            },
        },
        changed_at: millis(sequence),
        replica_seq: local_seq(sequence),
    }
}

fn test_manager(root: &Path) -> LibrarySync {
    test_manager_with_replica(root, "client")
}

fn test_manager_with_replica(root: &Path, replica_id: &str) -> LibrarySync {
    let database = Database::open(root.join(crate::APP_HIDDEN_LIBRARY_DB_PATH), client_library::schema::configure_connection).unwrap();
    {
        let conn = database.get().unwrap();
        client_library::schema::init_db(&conn).unwrap();
    }
    LibrarySync {
        asset_storage_enabled: Arc::new(AtomicBool::new(true)),
        notification_interest: Default::default(),
        assets: crate::storage::files::open(root.to_str().unwrap()).unwrap().with_database_path(database.path().to_path_buf()),
        database,
        replica_id: fixture_replica_id(replica_id),
        server_url: ServerUrl::parse("http://127.0.0.1:9").unwrap(),
        http_client: reqwest::Client::new(),
        library_id: fixture_library_id("library"),
        event_tx: None,
        credentials: Arc::new(std::sync::RwLock::new(None)),
        account: None,
        transfer_queue: Arc::new(TransferQueue::default()),
        sync_gate: tokio::sync::Mutex::new(()),
        asset_gate: tokio::sync::Mutex::new(()),
        registration_gate: tokio::sync::Mutex::new(()),
        inventory_checked: AtomicBool::new(false),
        metadata_activity: AtomicUsize::new(0),
        asset_activity: AtomicUsize::new(0),
    }
}
