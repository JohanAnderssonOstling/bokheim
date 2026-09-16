//! Runtime contract probes, compiled only into the test example.
use crate::storage::files::AssetStore;
use crate::{BlobKind, ContentHash};
use wasm_bindgen::JsValue;

#[wasm_bindgen::prelude::wasm_bindgen(inline_js = r#"
let active = 0, opened = 0, bookActive = 0, bookOpened = 0, gate;
let releaseSweep;
export function blockObjectSweep() {
    const open = FileSystemDirectoryHandle.prototype.getDirectoryHandle;
    let started, release;
    const entered = new Promise(resolve => started = resolve);
    const waiting = new Promise(resolve => release = resolve);
    FileSystemDirectoryHandle.prototype.getDirectoryHandle = async function(name, options) {
        if (name === '__book_objects' && !options?.create) {
            started();
            await waiting;
        }
        return open.call(this, name, options);
    };
    releaseSweep = async () => {
        await entered;
        FileSystemDirectoryHandle.prototype.getDirectoryHandle = open;
        release();
    };
}
export async function releaseObjectSweep() { await releaseSweep(); }
export function observeStagedHandles() {
    const open = FileSystemFileHandle.prototype.createSyncAccessHandle;
    FileSystemFileHandle.prototype.createSyncAccessHandle = async function(...args) {
        if (this.name !== '0') {
            const handle = await open.apply(this, args);
            if (/^([0-9a-f-]{36}|[0-9a-f]{64})$/.test(this.name)) {
                bookActive++; bookOpened++;
                const close = handle.close.bind(handle);
                let closed = false;
                handle.close = () => { close(); if (!closed) { bookActive--; closed = true; } };
            }
            return handle;
        }
        const waiting = gate;
        if (waiting) { waiting.started.resolve(); await waiting.released.promise; }
        const handle = await open.apply(this, args);
        active++; opened++;
        const close = handle.close.bind(handle);
        let closed = false;
        handle.close = () => { close(); if (!closed) { active--; closed = true; } };
        waiting?.completed.resolve();
        return handle;
    };
}
export function bookHandleCount() { return bookActive; }
export function bookOpenCount() { return bookOpened; }
export function stagedHandleCount() { return active; }
export function stagedOpenCount() { return opened; }
export async function createLazyFixture() {
    const root = await navigator.storage.getDirectory();
    const imports = await root.getDirectoryHandle('__import_objects', {create:true});
    const job = await imports.getDirectoryHandle('12345678-1234-1234-1234-123456789012', {create:true});
    const handle = await (await job.getFileHandle('0', {create:true})).createSyncAccessHandle();
    try { handle.truncate(0); handle.write(new Uint8Array([1,2,3])); handle.flush(); } finally { handle.close(); }
}
export function blockStagedOpen() {
    const deferred = () => { let resolve; const promise = new Promise(done => resolve = done); return {promise,resolve}; };
    gate = {started:deferred(), released:deferred(), completed:deferred()};
}
export function stagedOpenStarted() { return gate.started.promise; }
export function releaseStagedOpen() { gate.released.resolve(); return gate.completed.promise.finally(() => gate = undefined); }
"#)]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = blockObjectSweep)]
    fn block_object_sweep();
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = releaseObjectSweep)]
    async fn release_object_sweep();
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = bookHandleCount)]
    fn book_handle_count() -> u32;
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = bookOpenCount)]
    fn book_open_count() -> u32;
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = observeStagedHandles)]
    fn observe_staged_handles();
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = stagedHandleCount)]
    fn staged_handle_count() -> u32;
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = stagedOpenCount)]
    fn staged_open_count() -> u32;
    #[wasm_bindgen::prelude::wasm_bindgen(catch, js_name = createLazyFixture)]
    async fn create_lazy_fixture() -> Result<(), wasm_bindgen::JsValue>;
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = blockStagedOpen)]
    fn block_staged_open();
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = stagedOpenStarted)]
    async fn staged_open_started();
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = releaseStagedOpen)]
    async fn release_staged_open();
}

async fn lazy_reader_contract(phase: &str) -> Result<(), String> {
    use crate::platform::web_storage;
    use std::io::Read;
    let name = "assets/lazy-contract/book/one";
    if phase == "prepare" {
        create_lazy_fixture().await.map_err(|error| format!("{error:?}"))?;
        web_storage::adopt_staged("__import_objects/12345678-1234-1234-1234-123456789012/0".into(), 3).await?.publish_as(name.into())?;
    }
    assert_eq!(staged_handle_count(), 0, "publishing a book must release its staging handle");
    let before = staged_open_count();
    assert!(web_storage::file_exists(name)?);
    assert_eq!(web_storage::total_file_bytes("assets/lazy-contract/book/").await?, 3);
    assert_eq!(staged_open_count(), before, "availability and accounting must not open access handles");
    let (first, second) = futures_util::join!(web_storage::open_reader(name), web_storage::open_reader(name));
    let first = first?.unwrap();
    let mut second = second?.unwrap();
    assert_eq!(staged_handle_count(), 1, "concurrent readers share one physical handle");
    assert_eq!(staged_open_count(), before + 1);
    drop(first);
    assert_eq!(staged_handle_count(), 1);
    let mut bytes = Vec::new();
    second.read_to_end(&mut bytes).map_err(|error| error.to_string())?;
    assert_eq!(bytes, [1, 2, 3]);
    drop(second);
    assert_eq!(staged_handle_count(), 0, "the last reader closes its handle");

    block_staged_open();
    let mut opening = Box::pin(web_storage::open_reader(name));
    assert!(futures_util::poll!(opening.as_mut()).is_pending());
    staged_open_started().await;
    drop(opening);
    release_staged_open().await;
    crate::executor::sleep(std::time::Duration::ZERO).await;
    assert_eq!(staged_handle_count(), 0, "cancelled preparation must close a late handle");

    if phase == "verify" {
        block_staged_open();
        let mut opening = Box::pin(web_storage::open_reader(name));
        assert!(futures_util::poll!(opening.as_mut()).is_pending());
        staged_open_started().await;
        let mut second_opening = Box::pin(web_storage::open_reader(name));
        assert!(futures_util::poll!(second_opening.as_mut()).is_pending());
        web_storage::remove_file(name)?;
        assert!(!web_storage::file_exists(name)?);
        release_staged_open().await;
        let mut reader = opening.await?.unwrap();
        assert_eq!(staged_handle_count(), 1, "deletion must preserve a reader whose handle is still opening");
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).map_err(|error| error.to_string())?;
        assert_eq!(bytes, [1, 2, 3]);
        let mut second = second_opening.await?.unwrap();
        drop(reader);
        assert_eq!(staged_handle_count(), 1, "retirement must wait for every reader");
        let mut remaining = Vec::new();
        second.read_to_end(&mut remaining).map_err(|error| error.to_string())?;
        assert_eq!(remaining, [1, 2, 3]);
        drop(second);
        assert_eq!(staged_handle_count(), 0);
    }
    Ok(())
}

fn hash(bytes: &[u8]) -> ContentHash {
    ContentHash::new(blake3::hash(bytes).to_hex().as_str())
}

pub async fn run(phase: &str) -> Result<(), String> {
    if phase == "browser-resources" {
        return client_platform_web::transport::resources::contract().await.map_err(|error| format!("browser resource contract: {error:?}"));
    }
    observe_staged_handles();
    block_object_sweep();
    crate::platform::web_storage::initialize().await?;
    if let Some(phase) = phase.strip_prefix("history-") {
        release_object_sweep().await;
        return crate::storage::transfer_history::contract(phase);
    }
    // Initialization must return even while object enumeration is blocked.
    release_object_sweep().await;
    assert_eq!(book_handle_count(), 0, "initialization must not open book handles");
    if phase == "verify" {
        assert_eq!(book_open_count(), 0, "subsequent startup must not open any book objects");
    }
    assert_eq!(staged_open_count(), 0, "startup must not open imported book files");
    crate::platform::web_storage::asset_lifecycle_contract(phase, |locator| crate::storage::database::has_pending_outbox(locator).map_err(|error| error.to_string())).await?;
    crate::library::browser_publication::contract().await?;
    let store = crate::storage::files::open("storage-contract").map_err(|error| error.to_string())?;
    if let Some(server) = phase.strip_prefix("upload:") {
        return crate::sync::transfer::web_upload::contract(server).await;
    }
    let key = hash(b"publication");
    match phase {
        "publication-identity" => {
            // Small valid catalog fixture: the original identity intentionally
            // differs from the checksum of this later PDF revision.
            let identity = hash(b"original PDF revision");
            let mut bytes = b"%PDF-1.7\n".to_vec();
            let catalog = bytes.len();
            bytes.extend_from_slice(format!("1 0 obj\n<< /Type /Catalog /Pages 2 0 R /BokheimContentHash ({identity}) >>\nendobj\n").as_bytes());
            let pages = bytes.len();
            bytes.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\n");
            let xref = bytes.len();
            bytes.extend_from_slice(format!("xref\n0 3\n0000000000 65535 f \n{catalog:010} 00000 n \n{pages:010} 00000 n \ntrailer\n<< /Root 1 0 R /Size 3 >>\nstartxref\n{xref}\n%%EOF\n").as_bytes());
            let checksum = hash(&bytes);
            assert_ne!(identity, checksum);
            let staged = store.stage_reader(Box::new(std::io::Cursor::new(bytes.clone()))).await.map_err(|error| error.to_string())?;
            assert_eq!(staged.content_hash(), identity, "browser imports must reuse embedded identity");
            staged.commit().await.map_err(|error| error.to_string())?;
            let verified = store.verified_book_reader(identity).await.map_err(|error| error.to_string())?.unwrap();
            assert_eq!(verified.checksum, checksum);
            drop(verified);
            let mut writer = store.begin_stream_write(BlobKind::Book, &identity).await.map_err(|error| error.to_string())?;
            writer.set_transfer_checksum(checksum);
            writer.write_all(&bytes).await.map_err(|error| error.to_string())?;
            writer.commit().await.map_err(|error| error.to_string())?;
            assert_eq!(read_book(&store, &identity).await?, bytes);
            let wrong_identity = hash(b"unrelated publication");
            let mut wrong = store.begin_stream_write(BlobKind::Book, &wrong_identity).await.map_err(|error| error.to_string())?;
            wrong.set_transfer_checksum(checksum);
            wrong.write_all(&bytes).await.map_err(|error| error.to_string())?;
            assert!(wrong.commit().await.is_err(), "valid checksum cannot authorize the wrong book identity");
            assert!(!store.exists(BlobKind::Book, &wrong_identity).map_err(|error| error.to_string())?);
            let mut corrupt = store.begin_stream_write(BlobKind::Book, &identity).await.map_err(|error| error.to_string())?;
            corrupt.set_transfer_checksum(checksum);
            bytes.extend_from_slice(b"corruption");
            corrupt.write_all(&bytes).await.map_err(|error| error.to_string())?;
            assert!(corrupt.commit().await.is_err(), "matching embedded identity cannot bypass byte verification");
            return Ok(());
        }
        "prepare" => {
            let mut writer = store.begin_stream_write(BlobKind::Book, &key).await.map_err(|e| e.to_string())?;
            writer.write_all(b"publi").await.map_err(|e| e.to_string())?;
            assert!(!store.exists(BlobKind::Book, &key).map_err(|e| e.to_string())?);
            writer.write_all(b"cation").await.map_err(|e| e.to_string())?;
            writer.commit().await.map_err(|e| e.to_string())?;
            store.write(BlobKind::Thumbnail, &key, b"previous thumbnail").await.map_err(|e| e.to_string())?;
            let thumbnail_hash = hash(b"streamed thumbnail");
            let mut thumbnail = store.begin_stream_write(BlobKind::Thumbnail, &thumbnail_hash).await.map_err(|e| e.to_string())?;
            thumbnail.write_all(b"streamed ").await.map_err(|e| e.to_string())?;
            assert!(!store.exists(BlobKind::Thumbnail, &thumbnail_hash).map_err(|e| e.to_string())?);
            thumbnail.write_all(b"thumbnail").await.map_err(|e| e.to_string())?;
            thumbnail.commit().await.map_err(|e| e.to_string())?;
            let mut rejected = store.begin_stream_write(BlobKind::Thumbnail, &thumbnail_hash).await.map_err(|e| e.to_string())?;
            rejected.write_all(b"corrupt replacement").await.map_err(|e| e.to_string())?;
            assert!(rejected.commit().await.is_err());
            assert_eq!(store.read_bytes(BlobKind::Thumbnail, &thumbnail_hash).await.map_err(|e| e.to_string())?.unwrap(), b"streamed thumbnail");
            // Simulate worker termination while a replacement has partial bytes.
            let mut abandoned = store.begin_stream_write(BlobKind::Thumbnail, &key).await.map_err(|e| e.to_string())?;
            abandoned.write_all(b"partial").await.map_err(|e| e.to_string())?;
            std::mem::forget(abandoned);
            let mut rejected = store.begin_stream_write(BlobKind::Book, &key).await.map_err(|e| e.to_string())?;
            rejected.write_all(b"wrong content").await.map_err(|e| e.to_string())?;
            assert!(rejected.commit().await.is_err());
            assert_eq!(read_book(&store, &key).await?, b"publication");
            assert_eq!(store.read_bytes(BlobKind::Thumbnail, &key).await.map_err(|e| e.to_string())?.unwrap(), b"previous thumbnail");
            // Cancellation runs through the actual browser scheduler.
            let (started, ready) = tokio::sync::oneshot::channel();
            let (dropped, released) = tokio::sync::oneshot::channel();
            struct Resource(Option<tokio::sync::oneshot::Sender<()>>);
            impl Drop for Resource {
                fn drop(&mut self) {
                    let _ = self.0.take().unwrap().send(());
                }
            }
            let task = crate::executor::BackendExecutor::new().unwrap().spawn(async move {
                let _resource = Resource(Some(dropped));
                let _ = started.send(());
                std::future::pending::<()>().await;
            });
            ready.await.map_err(|e| e.to_string())?;
            drop(task);
            released.await.map_err(|e| e.to_string())?;
        }
        "restart" => {
            assert_eq!(read_book(&store, &key).await?, b"publication");
            assert_eq!(store.read_bytes(BlobKind::Thumbnail, &key).await.map_err(|e| e.to_string())?.unwrap(), b"previous thumbnail");
            use std::io::Read as _;
            let mut previous = store.prepare_reader(BlobKind::Thumbnail, &key).await.map_err(|e| e.to_string())?.unwrap();
            store.write(BlobKind::Thumbnail, &key, b"replacement thumbnail").await.map_err(|e| e.to_string())?;
            let mut previous_bytes = Vec::new();
            previous.read_to_end(&mut previous_bytes).map_err(|e| e.to_string())?;
            assert_eq!(previous_bytes, b"previous thumbnail");
            drop(previous);
            let mut deleted = store.prepare_reader(BlobKind::Thumbnail, &key).await.map_err(|e| e.to_string())?.unwrap();
            store.remove(BlobKind::Thumbnail, &key).map_err(|e| e.to_string())?;
            assert!(!store.exists(BlobKind::Thumbnail, &key).map_err(|e| e.to_string())?);
            assert!(store.read_bytes(BlobKind::Thumbnail, &key).await.map_err(|e| e.to_string())?.is_none());
            let mut deleted_bytes = Vec::new();
            deleted.read_to_end(&mut deleted_bytes).map_err(|e| e.to_string())?;
            assert_eq!(deleted_bytes, b"replacement thumbnail");
            store.write(BlobKind::Thumbnail, &key, &deleted_bytes).await.map_err(|e| e.to_string())?;
            let staged = store.stage_reader(Box::new(std::io::Cursor::new(b"staged browser publication".to_vec()))).await.map_err(|e| e.to_string())?;
            let staged_hash = staged.content_hash();
            let mut reader = staged.open_reader().map_err(|e| e.to_string())?;
            let mut inspected = Vec::new();
            reader.read_to_end(&mut inspected).map_err(|e| e.to_string())?;
            assert_eq!(inspected, b"staged browser publication");
            staged.commit().await.map_err(|e| e.to_string())?;
            assert!(store.exists(BlobKind::Book, &staged_hash).map_err(|e| e.to_string())?);
            // A publication larger than the former browser limit, without a book-sized allocation.
            let chunk = vec![42; 1024 * 1024];
            let mut hasher = blake3::Hasher::new();
            for _ in 0..129 {
                hasher.update(&chunk);
            }
            let key = ContentHash::new(hasher.finalize().to_hex().as_str());
            let mut writer = store.begin_stream_write(BlobKind::Book, &key).await.map_err(|e| e.to_string())?;
            for _ in 0..129 {
                writer.write_all(&chunk).await.map_err(|e| e.to_string())?;
            }
            writer.commit().await.map_err(|e| e.to_string())?;
            assert!(store.exists(BlobKind::Book, &key).map_err(|e| e.to_string())?);
        }
        "verify" => {
            assert_eq!(store.read_bytes(BlobKind::Thumbnail, &key).await.map_err(|e| e.to_string())?.unwrap(), b"replacement thumbnail");
            let bytes = crate::storage::files::book_bytes("storage-contract", "storage-contract").await.map_err(|e| e.to_string())?;
            assert!(bytes > 128 * 1024 * 1024);
            assert_eq!(bytes, 129 * 1024 * 1024 + b"publication".len() as u64 + b"staged browser publication".len() as u64);
        }
        _ => return Err("unknown contract phase".into()),
    }
    lazy_reader_contract(phase).await?;
    index_paging_contract(phase).await?;
    cover_recovery_contract(phase).await?;
    trash_recovery_contract(phase).await?;
    Ok(())
}

// A real library with SQLite metadata and OPFS bytes, without an account or
// networking. Each phase executes in a freshly started browser worker.
async fn cover_recovery_contract(phase: &str) -> Result<(), String> {
    let database = crate::storage::database::Database::open_library("cover-recovery-contract", client_library::schema::configure_connection).map_err(|e| e.to_string())?;
    client_library::schema::init_db(&*database.get().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let assets = crate::storage::files::open("cover-recovery-contract").map_err(|e| e.to_string())?;
    let (events, receiver) = crate::library::events::library_event_channel();
    let metadata = metadata_client::MetadataClient::new(crate::METADATA_SERVER_ORIGIN).map_err(|error| error.to_string())?;
    let library = crate::library::LibrarySession::from_parts(crate::LibraryId::new_v4(), database, assets, metadata, events, receiver, crate::library::discovery::LibraryScanner::Disabled);
    let bytes = include_bytes!("../../tests/fixtures/storage-contract.epub");
    let key = hash(bytes);
    match phase {
        "prepare" => {
            assert_eq!(library.import_book_reader(&crate::ROOT_DIR_ID, Box::new(std::io::Cursor::new(bytes.to_vec())), "contract.epub".into()).await.map_err(|e| e.to_string())?, key);
            let placements: i64 = library.db.get().map_err(|e| e.to_string())?.query_row(include_str!("sql/storage_contract/cover_recovery_contract_select.sql"), [], |row| row.get(0)).map_err(|e| e.to_string())?;
            assert_eq!(placements, 0, "browser imports must keep flat hash storage");
            library.run_local_jobs().await.map_err(|e| e.to_string())?;
            assert!(library.assets.thumbnail_set_exists(&key).map_err(|e| e.to_string())?);
            // Model termination after the durable request and first cover write.
            client_library::thumbnail_jobs::request_thumbnail(&*library.db.get().map_err(|e| e.to_string())?, &key).map_err(|e| e.to_string())?;
            library.assets.write(BlobKind::Thumbnail, &key, b"interrupted cover generation").await.map_err(|e| e.to_string())?;
        }
        "restart" => {
            assert_eq!(client_library::thumbnail_jobs::thumbnail_work_state(&*library.db.get().map_err(|e| e.to_string())?, &key).map_err(|e| e.to_string())?.unwrap().0, "pending");
            library.run_local_jobs().await.map_err(|e| e.to_string())?;
            assert!(library.assets.thumbnail_set_exists(&key).map_err(|e| e.to_string())?);
            assert_ne!(library.assets.read_bytes(BlobKind::Thumbnail, &key).await.map_err(|e| e.to_string())?.unwrap(), b"interrupted cover generation");
        }
        "verify" => {
            assert_eq!(client_library::thumbnail_jobs::thumbnail_work_state(&*library.db.get().map_err(|e| e.to_string())?, &key).map_err(|e| e.to_string())?.unwrap().0, "ready");
            assert!(library.assets.thumbnail_set_exists(&key).map_err(|e| e.to_string())?);
        }
        _ => unreachable!(),
    }
    Ok(())
}

async fn trash_recovery_contract(phase: &str) -> Result<(), String> {
    let database = crate::storage::database::Database::open_library("trash-contract", client_library::schema::configure_connection).map_err(|e| e.to_string())?;
    client_library::schema::init_db(&*database.get().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let assets = crate::storage::files::open("trash-contract").map_err(|e| e.to_string())?;
    let (events, receiver) = crate::library::events::library_event_channel();
    let metadata = metadata_client::MetadataClient::new(crate::METADATA_SERVER_ORIGIN).map_err(|error| error.to_string())?;
    let library = crate::library::LibrarySession::from_parts(crate::LibraryId::new_v4(), database, assets, metadata, events, receiver, crate::library::discovery::LibraryScanner::Disabled);
    let bytes = include_bytes!("../../tests/fixtures/storage-contract.epub");
    let hash = hash(bytes);
    match phase {
        "prepare" => {
            library.import_book_reader(&crate::ROOT_DIR_ID, Box::new(std::io::Cursor::new(bytes.to_vec())), "book.epub".into()).await.map_err(|e| e.to_string())?;
            let shelf = library.create_directory(&crate::ROOT_DIR_ID, "Shelf").map_err(|e| e.to_string())?;
            library.move_book_to_directory(&hash, &crate::ROOT_DIR_ID, &shelf.id).map_err(|e| e.to_string())?;
            let mut conn = library.db.get().map_err(|e| e.to_string())?;
            let transaction = conn.transaction().map_err(|e| e.to_string())?;
            client_library::placements::trash_book_record(&transaction, &hash).map_err(|e| e.to_string())?;
            transaction.commit().map_err(|e| e.to_string())?;
        }
        "restart" => {
            library.run_local_jobs().await.map_err(|e| e.to_string())?;
            assert_eq!(client_library::placements::library_trash(&*library.db.get().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?.books.len(), 1);
            assert!(library.assets.exists(BlobKind::Book, &hash).map_err(|e| e.to_string())?);
            library.restore_book(&hash).map_err(|e| e.to_string())?;
        }
        "verify" => {
            assert!(client_library::placements::library_trash(&*library.db.get().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?.books.is_empty());
            assert_eq!(read_book(&library.assets, &hash).await?, bytes);
            let pending: i64 = library.db.get().map_err(|e| e.to_string())?.query_row(include_str!("sql/storage_contract/trash_recovery_contract_select.sql"), [], |r| r.get(0)).map_err(|e| e.to_string())?;
            assert_eq!(pending, 0, "virtual restores must not queue physical work");
            let conn = library.db.get().map_err(|e| e.to_string())?;
            assert!(client_library::assets::is_book_downloaded(&conn, hash).map_err(|e| e.to_string())?);
            assert!(client_library::assets::get_asset_requests(&conn, BlobKind::Book).map_err(|e| e.to_string())?.is_empty());
            drop(conn);

            library.run_local_jobs().await.map_err(|e| e.to_string())?;
            let pending: i64 = library.db.get().map_err(|e| e.to_string())?.query_row(include_str!("sql/storage_contract/trash_recovery_contract_select.sql"), [], |r| r.get(0)).map_err(|e| e.to_string())?;
            assert_eq!(pending, 0);
            assert_eq!(read_book(&library.assets, &hash).await?, bytes);
            let shelf: String = library.db.get().map_err(|e| e.to_string())?.query_row(include_str!("sql/storage_contract/trash_recovery_contract_select_3.sql"), [], |row| row.get(0)).map_err(|e| e.to_string())?;
            let shelf = crate::DirId::parse_str(&shelf).map_err(|e| e.to_string())?;
            library.move_directory_to_trash(&shelf).map_err(|e| e.to_string())?;
            library.restore_directory(&shelf, None).map_err(|e| e.to_string())?;
            assert!(client_library::assets::is_book_downloaded(&*library.db.get().map_err(|e| e.to_string())?, hash).map_err(|e| e.to_string())?);
            library.move_book_to_trash(&hash).map_err(|e| e.to_string())?;
            library.assets.remove(BlobKind::Book, &hash).map_err(|e| e.to_string())?;
            library.restore_book(&hash).map_err(|e| e.to_string())?;
            assert!(!client_library::assets::is_book_downloaded(&*library.db.get().map_err(|e| e.to_string())?, hash).map_err(|e| e.to_string())?);
            assert!(client_library::assets::is_book_download_requested(&*library.db.get().map_err(|e| e.to_string())?, hash).map_err(|e| e.to_string())?);
            library.assets.write(BlobKind::Book, &hash, bytes).await.map_err(|e| e.to_string())?;
            library.move_book_to_trash(&hash).map_err(|e| e.to_string())?;
            library.purge_book(&hash).map_err(|e| e.to_string())?;
            library.run_local_jobs().await.map_err(|e| e.to_string())?;
            assert!(!library.assets.exists(BlobKind::Book, &hash).map_err(|e| e.to_string())?);
            assert!(!library.assets.thumbnail_set_exists(&hash).map_err(|e| e.to_string())?);
            assert_eq!(library.db.get().map_err(|e| e.to_string())?.query_row(include_str!("sql/storage_contract/trash_recovery_contract_select.sql"), [], |row| row.get::<_, i64>(0)).map_err(|e| e.to_string())?, 0);
            client_library::file_work::queue_directory_work(&*library.db.get().map_err(|e| e.to_string())?, &crate::ROOT_DIR_ID).map_err(|e| e.to_string())?;
            assert!(!library.pending_local_jobs().map_err(|e| e.to_string())?);
        }
        _ => unreachable!(),
    }
    Ok(())
}

async fn index_paging_contract(phase: &str) -> Result<(), String> {
    use crate::platform::web_storage;
    let prefix = "assets/index-contract/book/";
    if phase == "prepare" {
        for index in 0..70 {
            web_storage::write_file(&format!("{prefix}{index:03}"), &[7; 3]).await?;
        }
        web_storage::remove_file(&format!("{prefix}000"))?;
    }
    let mut after = String::new();
    let mut names = Vec::new();
    let total = web_storage::total_file_bytes(prefix).await?;
    loop {
        let page = web_storage::list_files_page(prefix, &after)?;
        assert!(page.len() <= 32);
        let Some(next) = page.last().cloned() else { break };
        names.extend(page);
        after = next;
    }
    assert_eq!(names.len(), 69);
    assert_eq!(names[0], format!("{prefix}001"));
    assert_eq!(total, 69 * 3);
    assert!(!web_storage::file_exists(&format!("{prefix}000"))?);
    Ok(())
}

async fn read_book(store: &crate::storage::files::AssetStore, hash: &crate::ContentHash) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut reader = store.prepare_reader(crate::BlobKind::Book, hash).await.map_err(|e| e.to_string())?.ok_or("Missing book")?;
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes)
}
