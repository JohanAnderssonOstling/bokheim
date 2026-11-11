//! Expendable prepared text, published only after a complete staged write.

use super::{AssetStore, AssetStoreError};
use book_access::BookReader;
#[cfg(not(target_arch = "wasm32"))]
use client_platform_runtime::executor::run_blocking;
#[cfg(target_arch = "wasm32")]
use client_platform_runtime::executor::storage_call;
use content_address::ContentHash;
use std::io::{self, Seek, Write};

pub trait TextCacheWriter: Write + Seek {}
impl<W: Write + Seek> TextCacheWriter for W {}

impl AssetStore {
    /// Decode through one open file on the storage executor, without collecting
    /// the serialized cache in memory. An absent file does not call the decoder.
    pub async fn read_text_cache<T, F>(&self, checksum: ContentHash, read: F) -> Result<Option<T>, AssetStoreError>
    where
        T: Send + 'static,
        F: FnOnce(&mut dyn BookReader) -> io::Result<T> + Send + 'static,
    {
        let name = format!("text-cache-v1/{checksum}");
        #[cfg(not(target_arch = "wasm32"))]
        {
            use std::{fs::File, io::ErrorKind};
            let store = self.clone();
            let root = self.lock_root_shared().await;
            run_blocking(move || {
                let _root = root;
                if !store.root_available() {
                    return Ok(None);
                }
                let mut reader = match File::open(store.0.assets_root.join(name)) {
                    Ok(reader) => reader,
                    Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
                    Err(error) => return Err(error.into()),
                };
                read(&mut reader).map(Some).map_err(Into::into)
            })
            .await
            .map_err(AssetStoreError::operation)?
        }
        #[cfg(target_arch = "wasm32")]
        {
            let name = super::imp::name(&self.0, name);
            storage_call(move || async move {
                let Some(mut reader) = client_platform_web::web_storage::open_reader(&name).await.map_err(AssetStoreError::operation)? else { return Ok(None) };
                read(&mut reader).map(Some).map_err(AssetStoreError::from)
            })
            .await
            .map_err(AssetStoreError::operation)?
        }
    }

    pub async fn write_text_cache<F>(&self, checksum: ContentHash, write: F) -> Result<(), AssetStoreError>
    where
        F: FnOnce(&mut dyn TextCacheWriter) -> io::Result<bool> + Send + 'static,
    {
        let name = format!("text-cache-v1/{checksum}");
        #[cfg(not(target_arch = "wasm32"))]
        {
            use std::{fs, io::ErrorKind};
            let store = self.clone();
            let root = self.lock_root_shared().await;
            run_blocking(move || {
                let _root = root;
                if !store.root_available() {
                    return Err(AssetStoreError::operation("library folder is unavailable"));
                }
                let destination = store.0.assets_root.join(name);
                let directory = destination.parent().expect("text cache directory");
                match fs::create_dir(directory) {
                    Ok(()) => super::imp::sync_parent(directory)?,
                    Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error.into()),
                }
                // The OS releases this lease if a process is killed. Only a
                // writer holding it can create or remove this book's staging
                // files, so cleanup cannot remove another active writer.
                let lease_name = format!("text-cache-{checksum}");
                let Some(_lease) = super::imp::lease(&store.0, &lease_name, true)? else { return Ok(()) };
                let prefix = format!(".bokheim-text-{checksum}.part-");
                for entry in fs::read_dir(directory)? {
                    let entry = entry?;
                    if entry.file_name().to_string_lossy().starts_with(&prefix) && entry.file_type()?.is_file() {
                        match fs::remove_file(entry.path()) {
                            Ok(()) => {}
                            Err(error) if error.kind() == ErrorKind::NotFound => {}
                            Err(error) => log::warn!("could not remove interrupted text cache write: {error}"),
                        }
                    }
                }
                let mut staged = tempfile::Builder::new().prefix(&prefix).tempfile_in(directory)?;
                if !write(staged.as_file_mut())? {
                    return Ok(());
                }
                staged.as_file().sync_all()?;
                if !store.root_available() {
                    return Err(AssetStoreError::operation("library folder changed while caching text"));
                }
                staged.persist(&destination).map_err(|error| error.error)?;
                super::imp::sync_parent(&destination)
            })
            .await
            .map_err(AssetStoreError::operation)?
        }
        #[cfg(target_arch = "wasm32")]
        {
            // OPFS stages a fresh physical file, flushes it, then atomically
            // publishes its name in the durable asset index.
            let name = super::imp::name(&self.0, name);
            client_platform_web::web_storage::write_file_with(&name, move |writer| write(writer)).await.map_err(AssetStoreError::operation)
        }
    }
}
