//! Explicit durability scope for Copy imports. Metadata must not be committed
//! until the caller has successfully flushed this scope. Source files remain
//! available throughout; Move and provider uploads use individual durability.
use super::{AssetStore, AssetStoreError};
use client_platform_runtime::executor::run_blocking;
use std::collections::BTreeSet;
use std::fs::File;
#[cfg(target_os = "linux")]
use std::mem;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

#[cfg(target_os = "linux")]
mod writeback;

#[derive(Default)]
pub(crate) struct PendingDurability {
    paths: Mutex<BTreeSet<PathBuf>>,
    #[cfg(target_os = "linux")]
    writeback: Mutex<writeback::Writeback>,
}

impl PendingDurability {
    pub(crate) fn record(&self, path: &Path) {
        self.paths.lock().unwrap_or_else(PoisonError::into_inner).insert(path.to_owned());
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn queue_writeback(&self, file: &File, size: u64) {
        self.writeback.lock().unwrap_or_else(PoisonError::into_inner).queue(file, size);
    }

    #[cfg(target_os = "linux")]
    fn finish_writeback(&self) -> std::io::Result<()> {
        let writeback = mem::take(&mut *self.writeback.lock().unwrap_or_else(PoisonError::into_inner));
        // Wait on the blocking executor, outside the state mutex. Dropping an
        // abandoned scope merely closes the queue and lets its worker exit.
        writeback.finish()
    }
}

impl AssetStore {
    /// An isolated store handle for a single, sequential Copy batch. Other
    /// handles keep their normal durability policy. Publish only ordinary book
    /// files through this handle, then flush before committing their metadata.
    pub fn for_import_batch(&self) -> Self {
        let mut store = self.clone();
        store.0.import_batch = Some(Arc::new(PendingDurability::default()));
        store
    }

    /// Flush all published book bytes, proofs and directory entries. No writes
    /// may run concurrently through this handle. A failure leaves all source
    /// books untouched and must prevent the metadata commit; retry is safe.
    pub async fn flush_import_batch(&self) -> Result<(), AssetStoreError> {
        let store = self.clone();
        let root = self.lock_root_shared().await;
        run_blocking(move || {
            let _root = root;
            store.flush_import_batch_blocking()
        })
        .await
        .map_err(AssetStoreError::operation)?
    }

    fn flush_import_batch_blocking(&self) -> Result<(), AssetStoreError> {
        if !self.root_available() {
            return Err(AssetStoreError::operation("library folder is unavailable"));
        }
        let Some(batch) = &self.0.import_batch else { return Ok(()) };
        #[cfg(target_os = "linux")]
        batch.finish_writeback()?;
        let mut paths = batch.paths.lock().unwrap_or_else(PoisonError::into_inner);
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::MetadataExt;
            let mut devices = BTreeSet::new();
            // A library may contain mount points: flush each affected device.
            // Keep paths on failure so the complete boundary can be retried.
            for path in paths.iter() {
                let file = File::open(path)?;
                if devices.insert(file.metadata()?.dev()) {
                    client_platform_native::filesystem::sync_filesystem(&file)?;
                }
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let mut directories = BTreeSet::new();
            for path in paths.iter() {
                File::options().read(true).write(true).open(path)?.sync_all()?;
                // Include ancestors: a newly created proof directory's name
                // must survive along with the files inside it.
                directories.extend(path.ancestors().skip(1).map(Path::to_path_buf));
            }
            for directory in directories.iter().rev() {
                client_platform_native::filesystem::sync_directory(directory)?;
            }
        }
        if !self.root_available() {
            return Err(AssetStoreError::operation("library folder changed during import"));
        }
        paths.clear();
        Ok(())
    }
}
