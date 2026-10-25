use super::*;
use book_model::{BookDetails, BookFormat, BookRecord};
use cloud_storage::{
    connections::{CloudConnections, ConnectionPersistence, GoogleConnection},
    oauth::{GoogleAccount, GoogleOAuth, GoogleOAuthConfig, GoogleTokens},
};
use library_database::Database;
use library_files::AssetStore;
use library_placement::LibraryPlacement;
use library_replica::LibraryStateEntry;
use library_runtime::{events::LibraryEventRouter, scheduler::SyncScheduler};
use serde_json::{json, Value};
use std::{
    io::Cursor,
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};
use sync_common::{
    storage::{CloudProvider, LibraryStorage, ProviderScope},
    ROOT_DIR_ID,
};
use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};
use url::Url;

fn root() -> CloudLibraryRoot {
    CloudLibraryRoot { scope: ProviderScope { provider: CloudProvider::GoogleDrive, account_id: ProviderId::new("account").unwrap(), container_id: ProviderId::new("my_drive").unwrap() }, folder_id: ProviderId::new("folder").unwrap() }
}

fn importer(path: &Path, library: Uuid) -> LibraryImport {
    let database = Arc::new(Database::open(path.join("library.sqlite").to_str().unwrap()).unwrap());
    database.initialize_library().unwrap();
    database.set_storage_policy(&LibraryStorage::External(root()), false).unwrap();
    let download = path.join("downloads");
    std::fs::create_dir_all(&download).unwrap();
    let assets = AssetStore::open(download.to_str().unwrap()).unwrap();
    let (sync, _) = SyncScheduler::new();
    let (assets_work, _) = SyncScheduler::new();
    let (files, _) = SyncScheduler::new();
    let (enrichment, _) = SyncScheduler::new();
    let (events, _) = LibraryEventRouter::new(sync, assets_work, files, enrichment);
    let placement = Arc::new(LibraryPlacement { database: database.clone(), assets: assets.clone(), events: events.clone() });
    LibraryImport {
        library_id: library,
        cloud: Default::default(),
        provider_upload_lock: Default::default(),
        database,
        assets,
        cpu: Arc::new(client_platform_native::cpu_host::NativeHost),
        events,
        placement,
        root: download.to_str().unwrap().into(),
    }
}

async fn queued(importer: &LibraryImport, bytes: &[u8], parent: DirId) -> ProviderUploadDraft {
    let copied = importer.assets.stage_book(library_model::RelativeBookPath::parse("Book.m4b").unwrap(), Box::new(Cursor::new(bytes.to_vec()))).await.unwrap();
    let checksum = BookId::from(content_address::hash_reader(&mut Cursor::new(bytes)).unwrap());
    let draft = ProviderUploadDraft {
        id: ProviderUploadDraft::identity(parent, "Book.m4b", checksum),
        library_id: importer.library_id,
        root: root(),
        parent,
        file_name: "Book.m4b".into(),
        format: BookFormat::M4b,
        checksum,
        size_bytes: bytes.len() as u64,
        inspection: InspectedBook { pdf: None, metadata: BookRecord { metadata: BookDetails { title: "Book".into(), ..Default::default() }, description: String::new() }, audiobook: None, toc: Vec::new() },
    };
    copied.into_staged(checksum, bytes.len() as u64).retain_provider_upload(importer.assets.clone(), draft.id, serde_json::to_vec(&draft).unwrap()).await.unwrap();
    importer.database.queue_provider_upload(&draft).unwrap();
    draft
}

struct Tokens;
impl ConnectionPersistence for Tokens {
    fn load_google(&self, _: &ProviderId) -> Result<Option<GoogleConnection>, CloudError> {
        Ok(Some(GoogleConnection {
            account: GoogleAccount { id: root().scope.account_id, email: "reader@example.com".into(), name: None },
            tokens: GoogleTokens { access_token: "local-test-token".into(), refresh_token: None, expires_at: u64::MAX },
        }))
    }
    fn save_google(&self, _: &GoogleConnection, _: Option<&str>) -> Result<bool, CloudError> {
        panic!("fresh test token must not be refreshed")
    }
}

#[derive(Clone, Copy)]
enum Failure {
    FirstChunk,
    FinalResponse,
    ExpiredFinalSession,
}

#[derive(Clone)]
struct Request {
    method: String,
    path: String,
    range: Option<String>,
    body: Vec<u8>,
}

async fn request(socket: &mut TcpStream) -> Request {
    let mut bytes = Vec::new();
    let end = loop {
        let mut chunk = [0; 8192];
        let n = socket.read(&mut chunk).await.unwrap();
        assert_ne!(n, 0);
        bytes.extend_from_slice(&chunk[..n]);
        if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            break end + 4;
        }
    };
    let headers = String::from_utf8(bytes[..end].to_vec()).unwrap();
    let mut line = headers.lines().next().unwrap().split_whitespace();
    let method = line.next().unwrap().to_owned();
    let path = line.next().unwrap().to_owned();
    let value = |key: &str| headers.lines().filter_map(|line| line.split_once(':')).find(|(name, _)| name.eq_ignore_ascii_case(key)).map(|(_, value)| value.trim().to_owned());
    assert_eq!(value("Authorization").as_deref(), Some("Bearer local-test-token"));
    let length = value("Content-Length").map(|value| value.parse::<usize>().unwrap()).unwrap_or(0);
    let range = value("Content-Range").or_else(|| value("Range"));
    while bytes.len() < end + length {
        let mut chunk = vec![0; (end + length - bytes.len()).min(64 * 1024)];
        let n = socket.read(&mut chunk).await.unwrap();
        assert_ne!(n, 0);
        bytes.extend_from_slice(&chunk[..n]);
    }
    Request { method, path, range, body: bytes[end..end + length].to_vec() }
}

async fn reply(socket: &mut TcpStream, status: u16, headers: &[(&str, String)], body: &[u8]) {
    let mut message = format!("HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n", body.len());
    for (key, value) in headers {
        message.push_str(&format!("{key}: {value}\r\n"));
    }
    message.push_str("\r\n");
    socket.write_all(message.as_bytes()).await.unwrap();
    socket.write_all(body).await.unwrap();
}

fn metadata(size: usize, parent: &str, revision: u64) -> Vec<u8> {
    serde_json::to_vec(&json!({"id":"reserved", "name":"Book.m4b", "mimeType":"audio/mp4", "parents":[parent], "size":size.to_string(), "version":revision.to_string()})).unwrap()
}

async fn drive(size: usize, failure: Failure) -> (ConnectedDrive, Arc<Mutex<Vec<Request>>>, JoinHandle<()>, Arc<AtomicU64>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
    let oauth = GoogleOAuth::new(GoogleOAuthConfig { client_id: "test-client".into(), client_secret: None }).unwrap();
    let connected = CloudConnections::new(oauth, Arc::new(Tokens)).test_google_drive(root().scope, url.clone()).unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let capture = requests.clone();
    let revision = Arc::new(AtomicU64::new(1));
    let current_revision = revision.clone();
    let task = tokio::spawn(async move {
        let mut uploaded = Vec::new();
        let mut failed = false;
        let mut nested: Option<Value> = None;
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let request = request(&mut socket).await;
            let path = Url::parse(&format!("http://mock{}", request.path)).unwrap();
            let parent = if nested.is_some() { "nested-folder" } else { "folder" };
            let revision = current_revision.load(Ordering::SeqCst);
            let (status, headers, body) = match (request.method.as_str(), path.path()) {
                ("GET", "/drive/v3/files") => (200, Vec::new(), serde_json::to_vec(&json!({"files": nested.iter().collect::<Vec<_>>()})).unwrap()),
                ("GET", "/drive/v3/files/nested-folder") => (200, Vec::new(), serde_json::to_vec(nested.as_ref().unwrap()).unwrap()),
                ("POST", "/drive/v3/files") => {
                    let mut created: Value = serde_json::from_slice(&request.body).unwrap();
                    assert_eq!(created["parents"], json!(["folder"]));
                    assert!(created["appProperties"]["bokheimDirectory"].as_str().unwrap().parse::<DirId>().is_ok());
                    created["id"] = json!("nested-folder");
                    nested = Some(created.clone());
                    (200, Vec::new(), serde_json::to_vec(&created).unwrap())
                }
                ("GET", "/drive/v3/files/generateIds") => (200, Vec::new(), serde_json::to_vec(&json!({"ids":["reserved"]})).unwrap()),
                ("GET", "/drive/v3/files/reserved") if uploaded.len() != size => (404, Vec::new(), Vec::new()),
                ("GET", "/drive/v3/files/reserved") => {
                    if path.query_pairs().any(|(key, value)| key == "alt" && value == "media") {
                        let (start, end) = request.range.as_deref().unwrap().strip_prefix("bytes=").unwrap().split_once('-').unwrap();
                        let start: usize = start.parse().unwrap();
                        let end: usize = end.parse().unwrap();
                        (206, vec![("Content-Range", format!("bytes {start}-{end}/{size}"))], uploaded[start..=end].to_vec())
                    } else {
                        (200, Vec::new(), metadata(size, parent, revision))
                    }
                }
                ("POST", "/upload/drive/v3/files") => {
                    let metadata: Value = serde_json::from_slice(&request.body).unwrap();
                    assert_eq!(metadata["id"], "reserved");
                    assert_eq!(metadata["parents"], json!([parent]));
                    (200, vec![("Location", url.join("upload/drive/v3/session").unwrap().to_string())], Vec::new())
                }
                ("PUT", "/upload/drive/v3/session") => {
                    let range = request.range.as_deref().unwrap();
                    if range.starts_with("bytes */") {
                        if uploaded.len() == size && matches!(failure, Failure::ExpiredFinalSession) {
                            (404, Vec::new(), Vec::new())
                        } else if uploaded.len() == size {
                            (200, Vec::new(), metadata(size, parent, revision))
                        } else {
                            (308, vec![("Range", format!("bytes=0-{}", uploaded.len() - 1))], Vec::new())
                        }
                    } else {
                        let (span, _) = range.strip_prefix("bytes ").unwrap().split_once('/').unwrap();
                        let (start, _) = span.split_once('-').unwrap();
                        assert_eq!(start.parse::<usize>().unwrap(), uploaded.len(), "confirmed bytes must never be uploaded twice");
                        uploaded.extend_from_slice(&request.body);
                        let should_fail = !failed
                            && match failure {
                                Failure::FirstChunk => uploaded.len() < size,
                                Failure::FinalResponse | Failure::ExpiredFinalSession => uploaded.len() == size,
                            };
                        if should_fail {
                            failed = true;
                            (503, Vec::new(), Vec::new())
                        } else if uploaded.len() == size {
                            (200, Vec::new(), metadata(size, parent, revision))
                        } else {
                            (308, vec![("Range", format!("bytes=0-{}", uploaded.len() - 1))], Vec::new())
                        }
                    }
                }
                _ => panic!("unexpected mock Drive request {} {}", request.method, request.path),
            };
            capture.lock().unwrap().push(request);
            reply(&mut socket, status, &headers, &body).await;
        }
    });
    (connected, requests, task, revision)
}

async fn interruption(failure: Failure, nested: bool) {
    let root = tempfile::tempdir().unwrap();
    let library = Uuid::new_v4();
    let importer = importer(root.path(), library);
    let bytes = (0..MAX_RANGE_BYTES + 17).map(|n| (n % 251) as u8).collect::<Vec<_>>();
    let parent = if nested {
        let id = DirId::new_v4();
        importer.database.register_created_directory(&id, &ROOT_DIR_ID, "Audio", "Audio").unwrap();
        id
    } else {
        ROOT_DIR_ID
    };
    let draft = queued(&importer, &bytes, parent).await;
    let (connection, requests, server, _) = drive(bytes.len(), failure).await;
    let upload = importer.database.provider_upload(draft.id).unwrap().unwrap();
    assert!(importer.run_provider_upload_connected(upload, &connection).await.is_err());
    assert!(importer.database.provider_remote_books().unwrap().is_empty());
    assert!(!importer.database.sync_publishable_mutations().unwrap().iter().any(|publication| publication.state.value.book().is_some()));
    assert!(importer.database.provider_upload(draft.id).unwrap().unwrap().session.is_some());
    drop(importer);
    let importer = self::importer(root.path(), library);
    let upload = importer.database.provider_upload(draft.id).unwrap().unwrap();
    let id = tokio::time::timeout(std::time::Duration::from_secs(10), importer.run_provider_upload_connected(upload, &connection)).await.unwrap().unwrap();
    assert!(id.is_external());
    assert_ne!(id, draft.checksum);
    assert!(importer.database.is_book_downloaded(&id).unwrap());
    assert!(importer.assets.book_exists(&id, &importer.database.book_paths(&id).unwrap()).unwrap());
    assert!(importer.database.provider_upload(draft.id).unwrap().is_none());
    assert!(importer.assets.retained_provider_uploads().unwrap().is_empty());
    assert!(importer.database.pending_book_uploads(0, 100).unwrap().is_empty());
    assert!(importer
        .database
        .sync_publishable_mutations()
        .unwrap()
        .iter()
        .any(|publication| matches!(&publication.state.value, LibraryStateEntry::ExternalBook { content_hash, value } if *content_hash == id && value.file.content_hash == draft.checksum.content_hash())));
    let capture = requests.lock().unwrap();
    assert_eq!(capture.iter().filter(|request| request.method == "POST" && request.path.starts_with("/upload/")).count(), 1, "resuming must retain the original provider object/session");
    if nested {
        assert_eq!(capture.iter().filter(|request| request.method == "POST" && request.path.starts_with("/drive/")).count(), 1);
        let created: Value = capture.iter().find(|request| request.method == "POST" && request.path.starts_with("/drive/")).map(|request| serde_json::from_slice(&request.body).unwrap()).unwrap();
        assert_eq!(created["appProperties"]["bokheimLibrary"], library.to_string());
        assert_eq!(created["appProperties"]["bokheimDirectory"], parent.to_string());
        assert_eq!(importer.database.provider_directory(parent).unwrap().unwrap().as_str(), "nested-folder");
    }
    assert_eq!(capture.iter().filter(|request| request.path.contains("generateIds")).count(), 1);
    let chunks = capture.iter().filter(|request| request.range.as_ref().is_some_and(|range| range.starts_with("bytes ") && !range.starts_with("bytes */"))).map(|request| request.body.clone()).flatten().collect::<Vec<_>>();
    assert_eq!(chunks, bytes);
    server.abort();
}

#[tokio::test]
async fn interrupted_chunk_resumes_after_reopen_before_metadata_is_published() {
    interruption(Failure::FirstChunk, false).await;
}

#[tokio::test]
async fn lost_final_response_is_recovered_by_probing_the_saved_session() {
    interruption(Failure::FinalResponse, false).await;
}

#[tokio::test]
async fn expired_session_after_commit_verifies_and_reuses_the_existing_object() {
    interruption(Failure::ExpiredFinalSession, false).await;
}

#[tokio::test]
async fn nested_import_creates_a_provider_folder_and_preserves_it_on_resume() {
    interruption(Failure::FirstChunk, true).await;
}

#[tokio::test]
async fn imported_book_is_retained_without_bokheim_metadata_when_cloud_is_signed_out() {
    let root = tempfile::tempdir().unwrap();
    let library = Uuid::new_v4();
    let importer = importer(root.path(), library);
    let bytes = include_bytes!("../../../book-inspection/tests/fixtures/back-matter-evidence.epub");
    let error = importer.import_downloaded_book(ROOT_DIR_ID, "Book.epub", bytes.to_vec()).await.unwrap_err();
    assert!(error.to_string().contains("saved locally for cloud upload"));
    assert!(importer.database.provider_remote_books().unwrap().is_empty());
    assert_eq!(importer.database.provider_upload_ids().unwrap().len(), 1);
    assert_eq!(importer.assets.retained_provider_uploads().unwrap().len(), 1);
    assert!(!importer.database.sync_publishable_mutations().unwrap().iter().any(|publication| publication.state.value.book().is_some()));
}

#[tokio::test]
async fn completed_upload_retry_preserves_a_newer_scanned_provider_revision() {
    use sync_common::storage::ProviderFileIdentity;
    let root = tempfile::tempdir().unwrap();
    let importer = importer(root.path(), Uuid::new_v4());
    let bytes = vec![42; 128];
    let draft = queued(&importer, &bytes, ROOT_DIR_ID).await;
    let (connection, _, server, revision) = drive(bytes.len(), Failure::FinalResponse).await;
    let upload = importer.database.provider_upload(draft.id).unwrap().unwrap();
    assert!(importer.run_provider_upload_connected(upload, &connection).await.is_err());
    let mut file = ProviderFile {
        identity: ProviderFileIdentity { scope: draft.root.scope.clone(), file_id: ProviderId::new("reserved").unwrap() },
        revision: ProviderId::new("1").unwrap(),
        size_bytes: bytes.len() as u64,
        content_hash: draft.checksum.content_hash(),
    };
    importer.database.confirm_provider_upload(draft.id, &file).unwrap();
    revision.store(2, Ordering::SeqCst);
    file.revision = ProviderId::new("2").unwrap();
    let mut new_inspection = draft.inspection.clone();
    new_inspection.metadata.metadata.title = "New provider edition".into();
    let id = importer.database.admit_inspected_provider_book(ROOT_DIR_ID, file.clone(), draft.format, new_inspection).unwrap();
    let upload = importer.database.provider_upload(draft.id).unwrap().unwrap();
    assert_eq!(importer.run_provider_upload_connected(upload, &connection).await.unwrap(), id);
    assert_eq!(importer.database.provider_file(&id).unwrap(), Some(file));
    assert_eq!(importer.database.browse_book_detail(&id).unwrap().book.title, "New provider edition");
    assert!(!importer.database.is_book_downloaded(&id).unwrap());
    assert!(importer.database.provider_upload(draft.id).unwrap().is_none());
    assert!(importer.assets.retained_provider_uploads().unwrap().is_empty());
    server.abort();
}
