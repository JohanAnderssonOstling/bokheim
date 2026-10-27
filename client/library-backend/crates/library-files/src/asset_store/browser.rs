//! Browser OPFS stores book bytes; folder and trash changes are metadata-only.

pub(crate) use super::AssetStoreError;
use crate::hashing::HashService;
pub(crate) use book_access::BoxedBookReader;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
pub(crate) use sync_common::BookId;

#[derive(Clone, Debug)]
pub(crate) struct Handle {
    pub(crate) namespace: String,
    pub(crate) hash_service: Arc<dyn HashService>,
}

pub(crate) async fn open_verified_reader(handle: &Handle, hash: &BookId) -> Result<Option<client_platform_web::web_storage::FileReader>, AssetStoreError> {
    client_platform_web::web_storage::open_reader(&name(handle, format!("book/{hash}"))).await.map_err(AssetStoreError::operation)
}

thread_local! {
    static LEASES: RefCell<HashMap<String, (usize, bool)>> = RefCell::new(HashMap::new());
}

pub(crate) struct Lease(pub(crate) String);
impl Drop for Lease {
    fn drop(&mut self) {
        LEASES.with(|leases| {
            let mut leases = leases.borrow_mut();
            if let Some(count) = leases.get_mut(&self.0) {
                count.0 -= 1;
                if count.0 == 0 {
                    leases.remove(&self.0);
                }
            }
        });
    }
}

pub(crate) fn lease(handle: &Handle, hash: &str, exclusive: bool) -> Result<Option<Lease>, AssetStoreError> {
    let key = name(handle, format!("book/{hash}"));
    LEASES.with(|leases| {
        let mut leases = leases.borrow_mut();
        if leases.get(&key).is_some_and(|(_, held_exclusive)| exclusive || *held_exclusive) {
            return Ok(None);
        }
        let entry = leases.entry(key.clone()).or_insert((0, exclusive));
        entry.0 += 1;
        Ok(Some(Lease(key)))
    })
}

pub(crate) fn open(library_locator: &str, hash_service: Arc<dyn HashService>) -> Result<Handle, AssetStoreError> {
    Ok(Handle { namespace: namespace(library_locator)?, hash_service })
}

pub(crate) fn namespace(locator: &str) -> Result<String, AssetStoreError> {
    let id = locator.rsplit('/').next().ok_or_else(|| AssetStoreError::operation("library locator has no id"))?;
    let id = id.parse::<sync_common::LibraryId>().map_err(AssetStoreError::operation)?;
    Ok(format!("assets/{id}"))
}

pub(crate) fn name(handle: &Handle, asset_name: String) -> String {
    format!("{}/{}", handle.namespace, asset_name)
}

pub(crate) struct StagedBook {
    pub(crate) writer: client_platform_web::web_storage::FileWriter,
    pub(crate) content_hash: BookId,
    pub(crate) size_bytes: u64,
    pub(crate) destination: String,
}

pub(crate) fn finish_staged_write(handle: &Handle, writer: client_platform_web::web_storage::FileWriter, content_hash: BookId) -> Result<StagedBook, AssetStoreError> {
    let size_bytes = std::io::Seek::seek(&mut writer.open_reader(), std::io::SeekFrom::End(0))?;
    let destination = name(handle, format!("book/{content_hash}"));
    Ok(StagedBook { writer, content_hash, size_bytes, destination })
}

pub(crate) async fn stage_book(handle: &Handle, source: &mut (dyn std::io::Read + Send)) -> Result<StagedBook, AssetStoreError> {
    let mut writer = client_platform_web::web_storage::begin_write(&name(handle, "book/staged".into())).await.map_err(AssetStoreError::operation)?;
    let mut hasher = handle.hash_service.start().await.map_err(AssetStoreError::operation)?;
    // Keep chunk storage out of nested async frames on the WASM stack.
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut length = 0_u64;
    loop {
        let read = source.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        writer.write_all(&buffer[..read]).map_err(AssetStoreError::operation)?;
        hasher.update(&buffer[..read]).await.map_err(AssetStoreError::operation)?;
        length += read as u64;
    }
    if length == 0 {
        return Err(AssetStoreError::operation("book is empty"));
    }
    let content_hash = hasher.finish().await.map_err(AssetStoreError::operation)?;
    finish_staged_write(handle, writer, content_hash)
}

/// Copies a picked browser file into book storage, hashing it on the way.
pub(crate) async fn stage_browser_file(handle: &Handle, file: &web_sys::File) -> Result<StagedBook, AssetStoreError> {
    const CHUNK: u64 = 1024 * 1024;
    let mut writer = client_platform_web::web_storage::begin_write(&name(handle, "book/staged".into())).await.map_err(AssetStoreError::operation)?;
    let mut hasher = handle.hash_service.start().await.map_err(AssetStoreError::operation)?;
    let size = file.size() as u64;
    let mut copied = 0_u64;
    while copied < size {
        let end = (copied + CHUNK).min(size);
        let blob = file.slice_with_f64_and_f64(copied as f64, end as f64).map_err(|error| AssetStoreError::operation(format!("{error:?}")))?;
        let buffer = wasm_bindgen_futures::JsFuture::from(blob.array_buffer()).await.map_err(|error| AssetStoreError::operation(format!("could not read the selected file: {error:?}")))?;
        let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
        if bytes.is_empty() {
            return Err(AssetStoreError::operation("the selected file ended unexpectedly"));
        }
        writer.write_all(&bytes).map_err(AssetStoreError::operation)?;
        hasher.update(&bytes).await.map_err(AssetStoreError::operation)?;
        copied += bytes.len() as u64;
    }
    if copied == 0 {
        return Err(AssetStoreError::operation("book is empty"));
    }
    let content_hash = hasher.finish().await.map_err(AssetStoreError::operation)?;
    finish_staged_write(handle, writer, content_hash)
}

impl StagedBook {
    pub(crate) async fn commit(self) -> Result<(), AssetStoreError> {
        self.writer.publish_verified_as(self.destination, self.content_hash).map_err(AssetStoreError::operation)
    }
}

pub(crate) fn write_capacity(_handle: &Handle) -> Result<Option<u64>, AssetStoreError> {
    // OPFS enforces its dynamic quota on each chunk write. No in-memory book buffer.
    Ok(None)
}

pub(crate) async fn read(handle: &Handle, asset_name: String) -> Result<Option<Vec<u8>>, AssetStoreError> {
    client_platform_web::web_storage::read_file(&name(handle, asset_name)).await.map_err(AssetStoreError::operation)
}

pub(crate) async fn open_reader(handle: &Handle, asset_name: String) -> Result<Option<BoxedBookReader>, AssetStoreError> {
    let name = name(handle, asset_name);
    client_platform_web::web_storage::open_reader(&name).await.map(|reader| reader.map(|reader| Box::new(reader) as BoxedBookReader)).map_err(AssetStoreError::operation)
}

pub(crate) async fn write(handle: &Handle, asset_name: String, bytes: &[u8]) -> Result<(), AssetStoreError> {
    client_platform_web::web_storage::write_file(&name(handle, asset_name), bytes).await.map_err(AssetStoreError::operation)
}

pub(crate) fn remove(handle: &Handle, asset_name: String) -> Result<bool, AssetStoreError> {
    client_platform_web::web_storage::remove_file(&name(handle, asset_name)).map_err(AssetStoreError::operation)
}

pub(crate) fn exists(handle: &Handle, asset_name: String) -> Result<bool, AssetStoreError> {
    client_platform_web::web_storage::file_exists(&name(handle, asset_name)).map_err(AssetStoreError::operation)
}
