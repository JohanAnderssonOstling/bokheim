//! Selected-file capabilities. Only the import service opens and buffers them.
#[cfg(feature = "store")]
use crate::DictionaryFile;
use crate::OperationError;
use crate::{MAX_FILE_BYTES, MAX_IMPORT_BYTES, Result};
#[cfg(not(target_arch = "wasm32"))]
use std::{future::Future, io::Read, pin::Pin};

#[cfg(not(target_arch = "wasm32"))]
pub type ImportReader = Box<dyn Read + Send>;
#[cfg(not(target_arch = "wasm32"))]
pub type ImportReaderFuture = Pin<Box<dyn Future<Output = Result<ImportReader>> + Send>>;

pub struct DictionarySource {
    name: String,
    size_bytes: Option<u64>,
    #[cfg(not(target_arch = "wasm32"))]
    open: Box<dyn FnOnce() -> ImportReaderFuture + Send>,
    #[cfg(target_arch = "wasm32")]
    file: Option<Box<dyn FnOnce() -> wasm_bindgen::JsValue + Send>>,
}

impl DictionarySource {
    /// Native providers open on the dictionary executor, without a UI dependency.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(name: String, size_bytes: Option<u64>, open: impl FnOnce() -> ImportReaderFuture + Send + 'static) -> Self {
        Self { name, size_bytes, open: Box::new(open) }
    }

    /// Retain the selected browser File capability without loading its contents.
    #[cfg(target_arch = "wasm32")]
    pub fn from_browser(name: String, size_bytes: Option<u64>, file: Option<Box<dyn FnOnce() -> wasm_bindgen::JsValue + Send>>) -> Self {
        Self { name, size_bytes, file }
    }

    #[cfg(target_arch = "wasm32")]
    fn into_browser_file(self) -> Result<(String, web_sys::File)> {
        use wasm_bindgen::JsCast;
        let file = self.file.ok_or("Selected dictionary file is unavailable")?().dyn_into().map_err(|_| "Selected source is not a browser file")?;
        Ok((self.name, file))
    }
}

/// One dictionary selection, validated before the first file is opened. The
/// provider's size is advisory; reads enforce both per-file and total bounds.
pub struct DictionaryImport {
    sources: Vec<DictionarySource>,
}

impl DictionaryImport {
    pub fn new(sources: Vec<DictionarySource>) -> Result<Self> {
        if sources.len() > 8 {
            return Err("Select the files for one dictionary at a time".into());
        }
        let mut total = 0u64;
        for source in &sources {
            if let Some(size) = source.size_bytes {
                if size > MAX_FILE_BYTES {
                    return Err("Dictionary files must be smaller than 256 MiB each".into());
                }
                total = total.saturating_add(size);
                if total > MAX_IMPORT_BYTES {
                    return Err("Dictionary import exceeds the size limit".into());
                }
            }
        }
        Ok(Self { sources })
    }

    #[cfg(feature = "store")]
    pub(crate) async fn read(self) -> Result<Vec<DictionaryFile>> {
        let mut files = Vec::with_capacity(self.sources.len());
        let mut total = 0;
        for source in self.sources {
            let limit = MAX_FILE_BYTES.min(MAX_IMPORT_BYTES - total);
            #[cfg(not(target_arch = "wasm32"))]
            let (name, bytes) = {
                let reader = (source.open)().await?;
                let bytes = client_platform_runtime::executor::run_blocking(move || crate::bounded_read(reader, limit)).await.map_err(OperationError::operation)??;
                (source.name, bytes)
            };
            #[cfg(target_arch = "wasm32")]
            let (name, bytes) = {
                use js_sys::Uint8Array;
                use wasm_bindgen_futures::JsFuture;
                let (name, file) = source.into_browser_file()?;
                let length = file.size() as u64;
                if length > limit {
                    return Err("Dictionary import exceeds the size limit".into());
                }
                // File snapshots have a fixed size. Small slices bound each
                // asynchronous allocation while keeping the whole read here.
                let mut bytes = Vec::new();
                while (bytes.len() as u64) < length {
                    let end = length.min(bytes.len() as u64 + 64 * 1024);
                    let chunk = file.slice_with_f64_and_f64(bytes.len() as f64, end as f64).map_err(OperationError::from)?;
                    let chunk = JsFuture::from(chunk.array_buffer()).await.map_err(OperationError::from)?;
                    let chunk = Uint8Array::new(&chunk).to_vec();
                    if chunk.len() as u64 != end - bytes.len() as u64 {
                        return Err("Dictionary source ended before its declared size".into());
                    }
                    bytes.extend_from_slice(&chunk);
                }
                (name, bytes)
            };
            total += bytes.len() as u64;
            files.push(DictionaryFile { name, bytes });
        }
        Ok(files)
    }
}
