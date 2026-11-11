use super::DirectoryEntry;
use std::{io, path::Path};

use operation_failure::OperationError;
use js_sys::{Promise, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen(module = "/src/file_storage/web.js")]
extern "C" {
    #[wasm_bindgen(catch, js_name = storageBeginLock)]
    fn begin_lock(path: &str) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_name = storageLockReady)]
    fn lock_ready(ticket: &JsValue) -> Promise;
    #[wasm_bindgen(js_name = storageCloseLock)]
    fn close_lock(ticket: &JsValue);
    #[wasm_bindgen(js_name = storageReadAll)]
    fn read_all(path: &str) -> Promise;
    #[wasm_bindgen(js_name = storageWriteAtomic)]
    fn write_file(path: &str, bytes: &Uint8Array) -> Promise;
    #[wasm_bindgen(js_name = storageOpen)]
    fn open_file(path: &str) -> Promise;
    #[wasm_bindgen(js_name = storageRemoveDirectory)]
    fn remove_directory_js(path: &str) -> Promise;
    #[wasm_bindgen(js_name = storageRead)]
    fn read_file(handle: &JsValue, offset: f64, length: f64) -> Promise;
    #[wasm_bindgen(catch, js_name = storageSize)]
    fn source_size(handle: &JsValue) -> Result<f64, JsValue>;
    #[wasm_bindgen(js_name = storageFileSize)]
    fn path_size(path: &str) -> Promise;
    #[wasm_bindgen(js_name = storageListDirectory)]
    fn list_directory_js(path: &str) -> Promise;
    #[wasm_bindgen(js_name = storageRemoveFile)]
    fn remove_file_js(path: &str) -> Promise;
}

fn error(value: JsValue) -> io::Error {
    let name = js_sys::Reflect::get(&value, &"name".into()).ok().and_then(|name| name.as_string());
    let kind = match name.as_deref() {
        Some("QuotaExceededError") => io::ErrorKind::StorageFull,
        Some("NotAllowedError" | "SecurityError") => io::ErrorKind::PermissionDenied,
        Some("NoModificationAllowedError") => io::ErrorKind::WouldBlock,
        Some("NotFoundError") => io::ErrorKind::NotFound,
        _ => io::ErrorKind::Other,
    };
    io::Error::new(kind, OperationError::from(value))
}

/// A File snapshot supports asynchronous range reads in windows and workers.
pub struct Source {
    file: JsValue,
    size: u64,
}

impl Source {
    pub fn len(&self) -> u64 {
        self.size
    }

    pub async fn read_at(&self, offset: u64, length: usize) -> io::Result<Vec<u8>> {
        if offset.checked_add(length as u64).is_none_or(|end| end > self.size) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid file read range"));
        }
        let bytes = JsFuture::from(read_file(&self.file, offset as f64, length as f64)).await.map_err(error)?;
        let bytes = Uint8Array::new(&bytes).to_vec();
        if bytes.len() != length {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid file read size"));
        }
        Ok(bytes)
    }
}

pub async fn read(path: &Path) -> io::Result<Option<Vec<u8>>> {
    let bytes = JsFuture::from(read_all(&path.to_string_lossy())).await.map_err(error)?;
    Ok((!bytes.is_null()).then(|| Uint8Array::new(&bytes).to_vec()))
}

pub async fn write_atomic(path: &Path, bytes: Vec<u8>) -> io::Result<()> {
    JsFuture::from(write_file(&path.to_string_lossy(), &Uint8Array::from(bytes.as_slice()))).await.map_err(error)?;
    Ok(())
}

pub async fn open(path: &Path) -> io::Result<Source> {
    let file = JsFuture::from(open_file(&path.to_string_lossy())).await.map_err(error)?;
    let size = source_size(&file).map_err(error)? as u64;
    Ok(Source { file, size })
}

pub async fn remove_directory(path: &Path) -> io::Result<()> {
    JsFuture::from(remove_directory_js(&path.to_string_lossy())).await.map_err(error)?;
    Ok(())
}

pub async fn file_size(path: &Path) -> io::Result<Option<u64>> {
    let size = JsFuture::from(path_size(&path.to_string_lossy())).await.map_err(error)?;
    if size.is_null() {
        return Ok(None);
    }
    size.as_f64().map(|size| Some(size as u64)).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Invalid file size"))
}

pub async fn list_directory(path: &Path) -> io::Result<Vec<DirectoryEntry>> {
    let entries = JsFuture::from(list_directory_js(&path.to_string_lossy())).await.map_err(error)?;
    js_sys::Array::from(&entries)
        .iter()
        .map(|entry| {
            let property = |name: &str| js_sys::Reflect::get(&entry, &name.into()).map_err(error);
            Ok(DirectoryEntry {
                name: property("name")?.as_string().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Invalid file name"))?,
                size: property("size")?.as_f64().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Invalid file size"))? as u64,
                modified_ms: property("modified")?.as_f64().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Invalid file timestamp"))?.max(0.0) as u64,
            })
        })
        .collect()
}

pub async fn remove_file(path: &Path) -> io::Result<()> {
    JsFuture::from(remove_file_js(&path.to_string_lossy())).await.map_err(error)?;
    Ok(())
}

/// Holds an exclusive browser lock until dropped, including cancellation while waiting.
pub struct StorageLock(JsValue);
impl StorageLock {
    pub async fn acquire(path: &Path) -> io::Result<Self> {
        let guard = Self(begin_lock(&path.to_string_lossy()).map_err(error)?);
        JsFuture::from(lock_ready(&guard.0)).await.map_err(error)?;
        Ok(guard)
    }
}
impl Drop for StorageLock {
    fn drop(&mut self) {
        close_lock(&self.0);
    }
}
