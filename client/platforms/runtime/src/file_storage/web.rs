use super::DirectoryEntry;
use std::path::Path;

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

fn error(value: JsValue) -> String {
    format!("File storage: {value:?}")
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

    pub async fn read_at(&self, offset: u64, length: usize) -> Result<Vec<u8>, String> {
        if offset.checked_add(length as u64).is_none_or(|end| end > self.size) {
            return Err("Invalid file read range".into());
        }
        let bytes = JsFuture::from(read_file(&self.file, offset as f64, length as f64)).await.map_err(error)?;
        let bytes = Uint8Array::new(&bytes).to_vec();
        if bytes.len() != length {
            return Err("Invalid file read size".into());
        }
        Ok(bytes)
    }
}

pub async fn read(path: &Path) -> Result<Option<Vec<u8>>, String> {
    let bytes = JsFuture::from(read_all(&path.to_string_lossy())).await.map_err(error)?;
    Ok((!bytes.is_null()).then(|| Uint8Array::new(&bytes).to_vec()))
}

pub async fn write_atomic(path: &Path, bytes: Vec<u8>) -> Result<(), String> {
    JsFuture::from(write_file(&path.to_string_lossy(), &Uint8Array::from(bytes.as_slice()))).await.map_err(error)?;
    Ok(())
}

pub async fn open(path: &Path) -> Result<Source, String> {
    let file = JsFuture::from(open_file(&path.to_string_lossy())).await.map_err(error)?;
    let size = source_size(&file).map_err(error)? as u64;
    Ok(Source { file, size })
}

pub async fn remove_directory(path: &Path) -> Result<(), String> {
    JsFuture::from(remove_directory_js(&path.to_string_lossy())).await.map_err(error)?;
    Ok(())
}

pub async fn file_size(path: &Path) -> Result<Option<u64>, String> {
    let size = JsFuture::from(path_size(&path.to_string_lossy())).await.map_err(error)?;
    if size.is_null() {
        return Ok(None);
    }
    size.as_f64().map(|size| Some(size as u64)).ok_or_else(|| "Invalid file size".into())
}

pub async fn list_directory(path: &Path) -> Result<Vec<DirectoryEntry>, String> {
    let entries = JsFuture::from(list_directory_js(&path.to_string_lossy())).await.map_err(error)?;
    js_sys::Array::from(&entries)
        .iter()
        .map(|entry| {
            let property = |name: &str| js_sys::Reflect::get(&entry, &name.into()).map_err(error);
            Ok(DirectoryEntry {
                name: property("name")?.as_string().ok_or("Invalid file name")?,
                size: property("size")?.as_f64().ok_or("Invalid file size")? as u64,
                modified_ms: property("modified")?.as_f64().ok_or("Invalid file timestamp")?.max(0.0) as u64,
            })
        })
        .collect()
}

pub async fn remove_file(path: &Path) -> Result<(), String> {
    JsFuture::from(remove_file_js(&path.to_string_lossy())).await.map_err(error)?;
    Ok(())
}

/// Holds an exclusive browser lock until dropped, including cancellation while waiting.
pub struct StorageLock(JsValue);
impl StorageLock {
    pub async fn acquire(path: &Path) -> Result<Self, String> {
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
