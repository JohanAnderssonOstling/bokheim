//! Browser SQLite VFS bootstrap. All higher-level behavior remains in the
//! regular backend; this module only installs durable browser file semantics.

use sqlite_wasm_vfs::sahpool::{OpfsSAHError, OpfsSAHPoolCfgBuilder, OpfsSAHPoolUtil, install};
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{self, Error, Seek, SeekFrom, Write};
use std::rc::Rc;
use std::time::Duration;

use client_platform_runtime::executor::{on_storage_thread, run_blocking, storage_call, storage_cleanup};
use content_address::ContentHash;
use operation_failure::{Classified, Failure};
use rusqlite::OptionalExtension;
use sync_common::BookId;
use wasm_bindgen::prelude::*;

pub use operation_failure::OperationError as StorageError;

fn storage_js_error(error: JsValue) -> StorageError {
    error.into()
}
#[wasm_bindgen(module = "/src/staged_assets.js")]
extern "C" {
    #[wasm_bindgen(catch, js_name = removeAsset)]
    async fn remove_asset(name: &str) -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_name = removeLibrary)]
    async fn remove_library(library: &str) -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_name = listAssets)]
    async fn list_assets() -> Result<js_sys::Array, JsValue>;
    #[wasm_bindgen(catch, js_name = createAsset)]
    async fn create_asset(name: &str) -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_name = writeStaged)]
    fn write_staged(name: &str, offset: f64, bytes: &[u8]) -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_name = flushStaged)]
    fn flush_staged(name: &str) -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_name = stagedBlob)]
    async fn staged_blob(name: &str) -> Result<web_sys::Blob, JsValue>;
    #[wasm_bindgen(catch, js_name = openStaged)]
    async fn open_staged(name: &str) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_name = closeStaged)]
    fn close_staged(name: &str);
    #[wasm_bindgen(catch, js_name = readStaged)]
    fn read_staged(name: &str, offset: f64, bytes: &mut [u8]) -> Result<usize, JsValue>;
    #[wasm_bindgen(js_name = retireStaged)]
    fn retire_staged(name: &str);
}
fn js_error(error: JsValue) -> StorageError {
    error.into()
}
const VFS_NAME: &str = "bokheim-opfs-sahpool-v1";
const VFS_DIRECTORY: &str = ".bokheim-opfs-sahpool-v1";
const TRANSIENT_FILE_CAPACITY: u32 = 8;

#[derive(Default)]
struct ReaderState {
    count: usize,
    delete_on_close: bool,
}

thread_local! {
    static READERS: RefCell<HashMap<String, ReaderState>> = RefCell::new(HashMap::new());
    static ASSET_INDEX: RefCell<Option<rusqlite::Connection>> = const { RefCell::new(None) };
    static STORAGE: RefCell<Option<Rc<OpfsSAHPoolUtil>>> = const { RefCell::new(None) };
}

/// Why browser storage could not be opened.
#[derive(Debug)]
pub enum StorageInitError {
    /// Another tab holds the storage's file handles.
    Busy(String),
    Failed(StorageError),
}

impl From<StorageError> for StorageInitError {
    fn from(error: StorageError) -> Self {
        Self::Failed(error)
    }
}

impl std::fmt::Display for StorageInitError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Busy(error) => formatter.write_str(error),
            Self::Failed(error) => std::fmt::Display::fmt(error, formatter),
        }
    }
}

pub async fn initialize() -> Result<(), StorageInitError> {
    let config = OpfsSAHPoolCfgBuilder::new().vfs_name(VFS_NAME).directory(VFS_DIRECTORY).clear_on_init(false).build();
    let storage = install::<rusqlite::ffi::WasmOsCallback>(&config, true).await.map_err(|error| {
        // A sync access handle cannot be created while another tab holds it.
        let busy = matches!(error, OpfsSAHError::CreateSyncAccessHandle(_));
        let error = storage_error(error);
        if busy && error.failure().is_none() { StorageInitError::Busy(error.to_string()) } else { StorageInitError::Failed(error) }
    })?;
    super::reference_storage::recover(&storage)?;
    // SQLite can synchronously create short-lived journal files. Keep eight
    // unused handles available without permanently growing the pool on every
    // launch, and clean up surplus handles left by older builds.
    let required_capacity = storage.count().saturating_add(TRANSIENT_FILE_CAPACITY + 1);
    storage.reserve_minimum_capacity(required_capacity).await.map_err(storage_error)?;
    STORAGE.with(|slot| *slot.borrow_mut() = Some(Rc::new(storage)));
    let index = rusqlite::Connection::open("__asset_index_v1.sqlite3").map_err(StorageError::from).map_err(StorageInitError::from)?;
    super::asset_index::initialize(&index).map_err(StorageError::from).map_err(StorageInitError::from)?;
    ASSET_INDEX.with(|slot| *slot.borrow_mut() = Some(index));
    // Initialization must not wait for a library-wide object sweep.
    client_platform_runtime::executor::spawn_detached(async {
        if let Err(error) = client_platform_runtime::executor::sleep(Duration::ZERO).await {
            log::warn!("Object reclamation could not be scheduled: {error}");
            return;
        }
        if let Err(error) = reclaim_unreferenced_objects().await {
            log::warn!("Object reclamation will retry on next startup: {error}");
        }
    });
    let storage = self::storage()?;
    let surplus = storage.get_capacity().saturating_sub(storage.count().saturating_add(TRANSIENT_FILE_CAPACITY));
    if surplus != 0 {
        storage.reduce_capacity(surplus).await.map_err(storage_error)?;
    }
    Ok(())
}

async fn reclaim_unreferenced_objects() -> Result<(), StorageError> {
    for value in list_assets().await.map_err(js_error)?.iter() {
        let name = value.as_string().ok_or("Invalid book object name")?;
        // Recheck current ownership after enumeration: imports and readers may
        // have started since the sweep began. Generations are never reused.
        let pinned = READERS.with(|readers| readers.borrow().contains_key(&name));
        let published = with_index(|index| index.query_row(include_str!("sql/web_storage/asset_lifecycle_contract_select.sql"), [&name], |row| row.get::<_, bool>(0)))?;
        if !pinned && !published {
            remove_asset(&name).await.map_err(js_error)?;
        }
        client_platform_runtime::executor::sleep(Duration::ZERO).await.map_err(|error| StorageError::from_error(&error))?;
    }
    Ok(())
}

pub fn database_exists(name: &str) -> Result<bool, StorageError> {
    storage()?.exists(name).map_err(storage_error)
}

/// Bulk-install a closed, disposable reference cache, never a user database.
/// A pending pool entry lets the next startup discard an interrupted import.
pub fn import_reference_database(name: &str, image: &[u8]) -> Result<(), StorageError> {
    let storage = storage()?;
    super::reference_storage::import(&storage, name, image)
}

pub fn reference_database(name: &str) -> Result<Option<String>, StorageError> {
    with_index(|index| index.query_row("SELECT filename FROM reference_selection WHERE name=?1", [name], |row| row.get(0)).optional())
}

/// Publish a completed immutable file for the next startup. Open readers keep
/// their original file; publication never replaces an active reference.
pub fn publish_reference_database(name: &str, filename: &str) -> Result<(), StorageError> {
    with_index(|index| index.execute("INSERT INTO reference_selection(name,filename) VALUES(?1,?2) ON CONFLICT(name) DO UPDATE SET filename=excluded.filename WHERE filename<>excluded.filename", [name, filename]).map(|_| ()))
}

/// Call only during startup, before any library opens an attached reference.
pub fn prune_reference_databases(prefix: &str, selected: &str) -> Result<(), StorageError> {
    let storage = storage()?;
    super::reference_storage::prune(&storage, prefix, selected)
}

pub async fn reserve_database_capacity(additional_databases: usize) -> Result<(), StorageError> {
    if !on_storage_thread() {
        return storage_call(move || async move { Box::pin(reserve_database_capacity(additional_databases)).await }).await.map_err(|error| StorageError::from_error(&error))?;
    }
    let storage = storage()?;
    let additional_databases = u32::try_from(additional_databases).unwrap_or(u32::MAX);
    let required_capacity = storage.count().saturating_add(additional_databases).saturating_add(TRANSIENT_FILE_CAPACITY);
    storage.reserve_minimum_capacity(required_capacity).await.map_err(storage_error)
}

pub(super) fn storage_error(error: OpfsSAHError) -> StorageError {
    let value = match &error {
        OpfsSAHError::GetDirHandle(value)
        | OpfsSAHError::GetFileHandle(value)
        | OpfsSAHError::CreateSyncAccessHandle(value)
        | OpfsSAHError::IterHandle(value)
        | OpfsSAHError::GetPath(value)
        | OpfsSAHError::RemoveEntity(value)
        | OpfsSAHError::GetSize(value)
        | OpfsSAHError::Read(value)
        | OpfsSAHError::Write(value)
        | OpfsSAHError::Flush(value)
        | OpfsSAHError::Truncate(value)
        | OpfsSAHError::Reflect(value) => Some(value.clone()),
        _ => None,
    };
    match value {
        Some(value) => StorageError::from(value).context(error),
        None => StorageError::new(error),
    }
}


fn storage() -> Result<Rc<OpfsSAHPoolUtil>, StorageError> {
    STORAGE.with(|slot| slot.borrow().clone().ok_or_else(|| StorageError::from("browser storage has not been initialized")))
}

fn with_index<T>(operation: impl FnOnce(&rusqlite::Connection) -> rusqlite::Result<T>) -> Result<T, StorageError> {
    ASSET_INDEX.with(|slot| {
        let index = slot.borrow();
        operation(index.as_ref().ok_or("browser asset index has not been initialized")?).map_err(Into::into)
    })
}

fn physical_name(name: &str) -> Result<Option<String>, StorageError> {
    use rusqlite::OptionalExtension;
    with_index(|index| index.query_row(include_str!("sql/web_storage/physical_name_select.sql"), [name], |row| row.get(0)).optional())
}

pub async fn read_file(name: &str) -> Result<Option<Vec<u8>>, StorageError> {
    if !on_storage_thread() {
        let name = name.to_owned();
        return storage_call(move || async move { Box::pin(read_file(&name)).await }).await.map_err(|error| StorageError::from_error(&error))?;
    }
    use std::io::Read;
    let Some(mut reader) = open_reader(name).await? else { return Ok(None) };
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).map_err(|error| StorageError::from_error(&error))?;
    Ok(Some(bytes))
}

/// A private generation. Only the SQLite index commit makes it visible.
/// Replaced generations survive until their last reader closes; bootstrap
/// discards abandoned generations left by interrupted writes.
pub struct FileWriter {
    name: String,
    physical: String,
    length: u64,
    committed: bool,
    _source: FileReader,
}

pub async fn begin_write(name: &str) -> Result<FileWriter, StorageError> {
    if !on_storage_thread() {
        let name = name.to_owned();
        return storage_call(move || async move { Box::pin(begin_write(&name)).await }).await.map_err(|error| StorageError::from_error(&error))?;
    }
    create_asset_writer(new_asset_writer(name)?).await
}

fn new_asset_writer(name: &str) -> Result<FileWriter, StorageError> {
    let mut parts = name.split('/');
    let (Some("assets"), Some(library), Some(kind), Some(_asset), None) = (parts.next(), parts.next(), parts.next(), parts.next(), parts.next()) else {
        return Err("Invalid library asset name".into());
    };
    let library = uuid::Uuid::parse_str(library).map_err(|_| "Invalid library asset owner")?;
    if !matches!(kind, "book" | "thumbnail" | "text-cache-v1") {
        return Err("Invalid library asset kind".into());
    }
    let physical = format!("__libraries/{library}/{kind}/{}", uuid::Uuid::new_v4());
    let source = FileReader::new(physical.clone(), 0);
    Ok(FileWriter { name: name.to_owned(), physical, length: 0, committed: false, _source: source })
}

async fn create_asset_writer(writer: FileWriter) -> Result<FileWriter, StorageError> {
    // The task retains ownership across cancellation and closes/removes any late handle.
    let (sender, receiver) = tokio::sync::oneshot::channel();
    wasm_bindgen_futures::spawn_local(async move {
        let result = create_asset(&writer.physical).await.map_err(storage_js_error).map(|()| writer);
        let _ = sender.send(result);
    });
    receiver.await.map_err(|_| StorageError::from("book creation stopped"))?
}

const LOCAL_FILE_LIMIT: u64 = 8 * 1024 * 1024;

fn worker_checksum(result: &JsValue, expected_size: f64) -> Result<ContentHash, StorageError> {
    let size = js_sys::Reflect::get(result, &"size".into()).map_err(storage_js_error)?.as_f64();
    if size != Some(expected_size) {
        return Err("processed book has an unexpected size".into());
    }
    js_sys::Reflect::get(result, &"checksum".into()).map_err(storage_js_error)?.as_string().ok_or("import worker did not return a checksum")?.parse().map_err(|_| StorageError::from("import worker returned an invalid checksum"))
}

/// Stage a selected file in one pass. Small files stay with the storage owner
/// to avoid handing off their handle; larger files use the import worker.
pub async fn stage_selected_file(name: &str, file: &web_sys::File) -> Result<(FileWriter, ContentHash), StorageError> {
    stage_file(name, file, None, None).await
}

async fn stage_file(name: &str, file: &web_sys::Blob, verified_checksum: Option<ContentHash>, source: Option<FileReader>) -> Result<(FileWriter, ContentHash), StorageError> {
    if !on_storage_thread() {
        return Err("selected file staging requires the storage executor".into());
    }
    if file.size() <= LOCAL_FILE_LIMIT as f64 {
        if file.size() <= 0.0 {
            return Err("cannot import an empty book".into());
        }
        let mut writer = begin_write(name).await?;
        let buffer = wasm_bindgen_futures::JsFuture::from(file.array_buffer()).await.map_err(storage_js_error)?;
        let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
        if bytes.len() as f64 != file.size() {
            return Err("imported book has an unexpected size".into());
        }
        writer.write_all(&bytes).await?;
        let checksum = verified_checksum.unwrap_or_else(|| blake3::hash(&bytes).into());
        return Ok((writer, checksum));
    }
    // Pin the private generation until the worker has flushed and closed it.
    let mut writer = new_asset_writer(name)?;
    let file = file.clone();
    let (mut sender, receiver) = tokio::sync::oneshot::channel();
    wasm_bindgen_futures::spawn_local(async move {
        let result = async {
            let abort = web_sys::AbortController::new().map_err(storage_js_error)?;
            let result = {
                let signal = abort.signal();
                let checksum = verified_checksum.map(|checksum| checksum.to_hex());
                let copying = super::import_worker::copy_file(&file, &writer.physical, &signal, checksum.as_deref());
                tokio::pin!(copying);
                tokio::select! {
                    biased;
                    _ = sender.closed() => {
                        abort.abort();
                        // Cancellation is acknowledged after closing the worker's
                        // file handle; only then may the generation be retired.
                        let _ = copying.await;
                        return Err(StorageError::from("book import was cancelled"));
                    }
                    result = &mut copying => result.map_err(storage_js_error)?,
                }
            };
            let size = file.size();
            let checksum = worker_checksum(&result, size)?;
            if verified_checksum.is_some_and(|expected| expected != checksum) {
                return Err(StorageError::from("copied book checksum changed"));
            }
            let stored_size = open_staged(&writer.physical).await.map_err(storage_js_error)?.as_f64().ok_or("invalid staged file size")?;
            if stored_size != size {
                return Err(StorageError::from("staged book has an unexpected size"));
            }
            writer.length = size as u64;
            Ok((writer, checksum))
        }
        .await;
        drop(source);
        let _ = sender.send(result);
    });
    receiver.await.map_err(|_| StorageError::from("book import stopped"))?
}

/// A reader pins a generation name, so replacing a logical file cannot change
/// the bytes observed by an already-open reader. Access stays in the owner worker.
pub struct FileReader {
    physical: String,
    position: u64,
    length: u64,
}

impl FileReader {
    /// Copy an already verified, immutable generation without hashing it again.
    /// Retain its pin until the copy worker finishes, including on cancellation.
    pub async fn copy_verified_to(&self, name: &str, checksum: ContentHash) -> Result<(FileWriter, ContentHash), StorageError> {
        let physical = self.physical.clone();
        let length = self.length;
        let name = name.to_owned();
        storage_call(move || async move {
            let source = FileReader::new(physical, length);
            let file = staged_blob(&source.physical).await.map_err(storage_js_error)?;
            if file.size() != length as f64 {
                return Err(StorageError::from("stored book has an unexpected size"));
            }
            stage_file(&name, &file, Some(checksum), Some(source)).await
        })
        .await
        .map_err(|error| StorageError::from_error(&error))?
    }

    pub async fn book_id(&self) -> Result<Option<BookId>, StorageError> {
        let physical = self.physical.clone();
        run_blocking(move || with_index(|index| index.query_row(include_str!("sql/web_storage/content_hash_select.sql"), [&physical], |row| row.get(0)).optional())).await.map_err(|error| StorageError::from_error(&error))?
    }

    pub async fn read_chunk(&mut self, length: usize) -> Result<Vec<u8>, StorageError> {
        let physical = self.physical.clone();
        let position = self.position;
        let bytes = run_blocking(move || {
            let mut bytes = vec![0; length];
            let count = read_staged(&physical, position as f64, &mut bytes).map_err(js_error)?;
            bytes.truncate(count);
            Ok::<_, StorageError>(bytes)
        })
        .await
        .map_err(|error| StorageError::from_error(&error))??;
        self.position += bytes.len() as u64;
        Ok(bytes)
    }

    pub async fn upload_file(&self) -> Result<web_sys::Blob, StorageError> {
        staged_blob(&self.physical).await.map_err(js_error)
    }

    /// Hash the pinned generation without moving the reader's position.
    pub async fn checksum(&self) -> Result<(ContentHash, u64), StorageError> {
        let physical = self.physical.clone();
        let length = self.length;
        storage_call(move || async move {
            let source = FileReader::new(physical, length);
            let (mut sender, receiver) = tokio::sync::oneshot::channel();
            wasm_bindgen_futures::spawn_local(async move {
                let result = async {
                    if length <= LOCAL_FILE_LIMIT {
                        // Detect growth as well as truncation since opening.
                        let mut bytes = vec![0; length as usize + 1];
                        let count = read_staged(&source.physical, 0.0, &mut bytes).map_err(storage_js_error)?;
                        if count as u64 != length {
                            return Err(StorageError::from("stored book has an unexpected size"));
                        }
                        return Ok((blake3::hash(&bytes[..count]).into(), length));
                    }
                    let file = staged_blob(&source.physical).await.map_err(storage_js_error)?;
                    if file.size() != length as f64 {
                        return Err(StorageError::from("stored book has an unexpected size"));
                    }
                    let abort = web_sys::AbortController::new().map_err(storage_js_error)?;
                    let signal = abort.signal();
                    let hashing = super::import_worker::hash_file(&file, &signal);
                    tokio::pin!(hashing);
                    let result = tokio::select! {
                        biased;
                        _ = sender.closed() => {
                            abort.abort();
                            // Retain the generation until the worker has stopped
                            // reading its File snapshot, including on cancellation.
                            let _ = hashing.await;
                            return Err(StorageError::from("book verification was cancelled"));
                        }
                        result = &mut hashing => result.map_err(storage_js_error)?,
                    };
                    Ok((worker_checksum(&result, length as f64)?, length))
                }
                .await;
                drop(source);
                let _ = sender.send(result);
            });
            receiver.await.map_err(|_| StorageError::from("book verification stopped"))?
        })
        .await
        .map_err(|error| StorageError::from_error(&error))?
    }

    fn new(physical: String, length: u64) -> Self {
        READERS.with(|readers| readers.borrow_mut().entry(physical.clone()).or_default().count += 1);
        Self { physical, position: 0, length }
    }
}

fn retire(physical: String) {
    let deferred = READERS.with(|readers| {
        if let Some(state) = readers.borrow_mut().get_mut(&physical) {
            state.delete_on_close = true;
            true
        } else {
            false
        }
    });
    if !deferred {
        // Book already succeeded; bootstrap retries failed cleanup.
        retire_staged(&physical);
    }
}

impl Drop for FileReader {
    fn drop(&mut self) {
        let physical = self.physical.clone();
        storage_cleanup(move || release_reader(physical));
    }
}

fn release_reader(physical: String) {
    let finished = READERS.with(|readers| {
        let mut readers = readers.borrow_mut();
        let Some(state) = readers.get_mut(&physical) else {
            log::warn!("book reader cleanup found no registered generation: {}", physical);
            return None;
        };
        let Some(count) = state.count.checked_sub(1) else {
            log::warn!("book reader cleanup found an empty generation: {}", physical);
            return readers.remove(&physical);
        };
        state.count = count;
        if state.count == 0 { readers.remove(&physical) } else { None }
    });
    if let Some(state) = finished {
        if state.delete_on_close {
            retire_staged(&physical);
        } else {
            close_staged(&physical);
        }
    }
}

impl std::io::Read for FileReader {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        if !on_storage_thread() {
            return Err(Error::other("synchronous asset reads require the storage executor"));
        }
        let count = read_staged(&self.physical, self.position as f64, bytes).map_err(js_error).map_err(Error::other)?;
        self.position += count as u64;
        Ok(count)
    }
}

impl std::io::Seek for FileReader {
    fn seek(&mut self, from: SeekFrom) -> std::io::Result<u64> {
        let position = match from {
            SeekFrom::Start(position) => Some(position),
            SeekFrom::Current(offset) => self.position.checked_add_signed(offset),
            SeekFrom::End(offset) => self.length.checked_add_signed(offset),
        }
        .ok_or_else(|| Error::new(std::io::ErrorKind::InvalidInput, "invalid asset seek"))?;
        self.position = position;
        Ok(position)
    }
}

// Pin the generation before awaiting OPFS. The task owns the pin until opening
// completes, even if its caller is cancelled; a late handle cannot leak or race
// physical retirement.
async fn prepare_physical(physical: String) -> Result<FileReader, StorageError> {
    let mut reader = FileReader::new(physical, 0);
    let (sender, receiver) = tokio::sync::oneshot::channel();
    wasm_bindgen_futures::spawn_local(async move {
        let result = async {
            reader.length = open_staged(&reader.physical).await.map_err(js_error)?.as_f64().ok_or("Invalid staged file size")? as u64;
            Ok(reader)
        }
        .await;
        let _ = sender.send(result);
    });
    receiver.await.map_err(|_| "file preparation stopped".to_owned())?
}

pub async fn open_reader(name: &str) -> Result<Option<FileReader>, StorageError> {
    if !on_storage_thread() {
        let name = name.to_owned();
        return storage_call(move || async move { Box::pin(open_reader(&name)).await }).await.map_err(|error| StorageError::from_error(&error))?;
    }
    let Some(physical) = physical_name(name)? else { return Ok(None) };
    prepare_physical(physical).await.map(Some)
}

impl FileWriter {
    pub fn length(&self) -> u64 {
        self.length
    }

    pub async fn open_reader(&self) -> Result<FileReader, StorageError> {
        let physical = self.physical.clone();
        let length = self.length;
        run_blocking(move || FileReader::new(physical, length)).await.map_err(|error| StorageError::from_error(&error))
    }

    pub async fn write_all(&mut self, bytes: &[u8]) -> Result<(), StorageError> {
        let physical = self.physical.clone();
        let offset = self.length;
        let owned = bytes.to_vec();
        run_blocking(move || write_staged(&physical, offset as f64, &owned).map_err(storage_js_error)).await.map_err(|error| StorageError::from_error(&error))??;
        self.length = self.length.checked_add(bytes.len() as u64).ok_or("asset length overflow")?;
        Ok(())
    }

    pub async fn publish_verified_as(mut self, name: String, book_id: BookId) -> Result<(), StorageError> {
        self.name = name;
        self.commit_verified(book_id).await
    }
    /// Retain the published physical generation until the owning database has
    /// accepted or rejected it. Rejection cannot remove a later replacement.
    pub async fn publish_pinned_as(self, name: String, book_id: BookId) -> Result<PublishedFile, StorageError> {
        let reader = self.open_reader().await?;
        self.publish_verified_as(name.clone(), book_id).await?;
        Ok(PublishedFile { name, reader })
    }
    pub async fn commit(self) -> Result<(), StorageError> {
        self.commit_revision(None).await
    }
    pub async fn commit_verified(self, book_id: BookId) -> Result<(), StorageError> {
        self.commit_revision(Some(book_id)).await
    }
    async fn commit_revision(self, book_id: Option<BookId>) -> Result<(), StorageError> {
        storage_call(move || async move { self.commit_revision_on_storage(book_id) }).await.map_err(|error| StorageError::from_error(&error))?
    }

    fn commit_revision_on_storage(mut self, book_id: Option<BookId>) -> Result<(), StorageError> {
        let previous = physical_name(&self.name)?;
        flush_staged(&self.physical).map_err(storage_js_error)?;
        with_index(|index| {
            let tx = index.unchecked_transaction()?;
            super::asset_index::publish(&tx, &self.name, &self.physical, self.length)?;
            if let Some(book_id) = book_id {
                tx.execute(include_str!("sql/web_storage/commit_revision_insert.sql"), rusqlite::params![self.physical, book_id])?;
            }
            tx.execute(include_str!("sql/web_storage/commit_revision_delete.sql"), [])?;
            tx.commit()
        })?;
        self.committed = true;
        if let Some(previous) = previous.filter(|previous| previous != &self.physical) {
            retire(previous);
        }
        Ok(())
    }
}

pub struct PublishedFile {
    name: String,
    reader: FileReader,
}
impl PublishedFile {
    pub async fn discard(self) -> Result<(), StorageError> {
        storage_call(move || async move { self.discard_on_storage() }).await.map_err(|error| StorageError::from_error(&error))?
    }

    fn discard_on_storage(self) -> Result<(), StorageError> {
        let removed = with_index(|index| index.execute(include_str!("sql/web_storage/discard_publication.sql"), rusqlite::params![self.name, self.reader.physical]))?;
        if removed != 0 {
            retire(self.reader.physical.clone());
        }
        Ok(())
    }
}

impl Drop for FileWriter {
    fn drop(&mut self) {
        if !self.committed {
            let physical = self.physical.clone();
            storage_cleanup(move || retire(physical));
        }
    }
}

pub async fn write_file(name: &str, bytes: &[u8]) -> Result<(), StorageError> {
    let mut writer = begin_write(name).await?;
    writer.write_all(bytes).await?;
    writer.commit().await.map_err(Into::into)
}

/// A seekable cursor over one private OPFS generation. Synchronous writes run
/// only on its storage owner; the durable index still controls publication.
pub struct FileWriterCursor {
    writer: FileWriter,
    position: u64,
}

impl Write for FileWriterCursor {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if !on_storage_thread() {
            return Err(Error::other("synchronous asset writes require the storage executor"));
        }
        if bytes.is_empty() {
            return Ok(0);
        }
        let end = self.position.checked_add(bytes.len() as u64).ok_or_else(|| Error::other("asset length overflow"))?;
        write_staged(&self.writer.physical, self.position as f64, bytes).map_err(storage_js_error).map_err(Error::other)?;
        self.position = end;
        self.writer.length = self.writer.length.max(end);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        // Sync-access writes are already handed to OPFS. The durability flush
        // happens once, immediately before committing the asset index.
        Ok(())
    }
}

impl Seek for FileWriterCursor {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        self.position = match from {
            SeekFrom::Start(position) => Some(position),
            SeekFrom::Current(offset) => self.position.checked_add_signed(offset),
            SeekFrom::End(offset) => self.writer.length.checked_add_signed(offset),
        }
        .ok_or_else(|| Error::new(io::ErrorKind::InvalidInput, "invalid asset seek"))?;
        Ok(self.position)
    }
}

/// Stream into a staged generation without collecting its bytes in memory.
/// Only a successful callback returning true publishes the completed file.
pub async fn write_file_with<F>(name: &str, write: F) -> Result<(), StorageError>
where
    F: FnOnce(&mut FileWriterCursor) -> io::Result<bool> + Send + 'static,
{
    let name = name.to_owned();
    storage_call(move || async move {
        let mut cursor = FileWriterCursor { writer: begin_write(&name).await?, position: 0 };
        if write(&mut cursor).map_err(|error| StorageError::from_error(&error))? {
            cursor.writer.commit().await?;
        }
        Ok(())
    })
    .await
    .map_err(|error| StorageError::from_error(&error))?
}

pub fn remove_file(name: &str) -> Result<bool, StorageError> {
    let previous = physical_name(name)?;
    let existed = previous.is_some();
    // Publish the deletion in the durable index.
    with_index(|index| index.execute(include_str!("sql/web_storage/remove_file_delete.sql"), [name]))?;
    if let Some(previous) = previous {
        retire(previous);
    }
    Ok(existed)
}

pub fn file_exists(name: &str) -> Result<bool, StorageError> {
    // Book flushes bytes before committing the index entry.
    Ok(physical_name(name)?.is_some())
}

pub async fn total_file_bytes(prefix: &str) -> Result<u64, StorageError> {
    if !on_storage_thread() {
        let prefix = prefix.to_owned();
        return storage_call(move || async move { Box::pin(total_file_bytes(&prefix)).await }).await.map_err(|error| StorageError::from_error(&error))?;
    }
    let prefix = prefix.to_owned();
    client_platform_runtime::storage_queue::run(move || with_index(|index| super::asset_index::total_bytes(index, &prefix))).await?
}

/// Retire one library's physical OPFS container and its global index entries.
/// The caller has already detached the library from the registry and stopped
/// its actor. A failed removal leaves the durable purge record for retry.
pub async fn purge_library(library: sync_common::LibraryId) -> Result<(), StorageError> {
    if !on_storage_thread() {
        return storage_call(move || async move { Box::pin(purge_library(library)).await }).await.map_err(|error| StorageError::from_error(&error))?;
    }
    let physical_prefix = format!("__libraries/{library}/");
    if READERS.with(|readers| readers.borrow().keys().any(|name| name.starts_with(&physical_prefix))) {
        return Err(StorageError::classified(Failure::StorageBusy, format!("library {library} still has open asset readers")));
    }
    remove_library(&library.to_string()).await.map_err(js_error)?;
    with_index(|index| {
        let tx = index.unchecked_transaction()?;
        super::asset_index::purge_library(&tx, &library.to_string())?;
        tx.commit()
    })
}

/// Rekey published names without reading, copying or hashing their bytes.
/// The library retains its migration marker until this transaction succeeds.
pub fn migrate_book_ids(namespace: &str, ids: &[(BookId, BookId)]) -> Result<(), StorageError> {
    let retired = with_index(|index| {
        let tx = index.unchecked_transaction()?;
        let mut retired = Vec::new();
        for &(old, new) in ids {
            for kind in ["book", "thumbnail"] {
                let prefix = format!("{namespace}/{kind}/{old}");
                let names = tx
                    .prepare("SELECT name,physical_name,byte_length FROM asset_file WHERE name=?1 OR (name>=?2 AND name<?3)")?
                    .query_map(rusqlite::params![prefix, format!("{prefix}."), format!("{prefix}/")], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?)))?
                    .collect::<Result<Vec<_>, _>>()?;
                for (name, physical, length) in names {
                    let destination = format!("{namespace}/{kind}/{new}{}", &name[prefix.len()..]);
                    tx.execute("INSERT OR IGNORE INTO asset_file(name,physical_name,byte_length) VALUES(?1,?2,?3)", rusqlite::params![destination, physical, length])?;
                    tx.execute("DELETE FROM asset_file WHERE name=?1", [&name])?;
                    if kind == "book" {
                        tx.execute("UPDATE asset_revision SET content_hash=?2 WHERE physical_name=?1 AND content_hash=?3", rusqlite::params![physical, new, old])?;
                    }
                    let referenced: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM asset_file WHERE physical_name=?1)", [&physical], |row| row.get(0))?;
                    if !referenced {
                        retired.push(physical);
                    }
                }
            }
        }
        tx.commit()?;
        Ok(retired)
    })?;
    for physical in retired {
        retire(physical);
    }
    Ok(())
}
