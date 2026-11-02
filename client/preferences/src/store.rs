//! Device-local preference documents using the shared native/OPFS file layer.
use crate::{BrowsingPreferencePatch, BrowsingPreferences, ReaderPreferencePatch, ReaderPreferences};
use client_platform_runtime::file_storage as files;
use serde::{Serialize, de::DeserializeOwned};
use std::collections::HashMap;
use std::io::{self, Error, ErrorKind};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use tokio::sync::Mutex as AsyncMutex;

const READER: &str = "reader.json";
const BROWSING: &str = "browsing.json";
const EXPORT_FOLDER: &str = "annotation-export-folder.json";

#[cfg(not(target_arch = "wasm32"))]
static STORES: std::sync::LazyLock<Mutex<HashMap<PathBuf, Weak<PreferenceStore>>>> = std::sync::LazyLock::new(Default::default);
#[cfg(target_arch = "wasm32")]
thread_local! {
    static STORES: std::cell::RefCell<HashMap<PathBuf, Weak<PreferenceStore>>> = Default::default();
}

#[derive(Debug)]
struct Snapshot {
    reader: ReaderPreferences,
    browsing: BrowsingPreferences,
    export_folder: Option<String>,
}

/// Windows in one process share a store. Browser realms retain independent snapshots.
#[derive(Debug)]
pub struct PreferenceStore {
    root: PathBuf,
    snapshot: Mutex<Snapshot>,
    writes: AsyncMutex<()>,
}

fn decode<T: DeserializeOwned + Default>(bytes: Option<Vec<u8>>) -> io::Result<T> {
    bytes.map(|bytes| serde_json::from_slice(&bytes).map_err(|error| Error::new(ErrorKind::InvalidData, error))).transpose().map(|value| value.unwrap_or_default())
}

fn recoverable<T: DeserializeOwned + Default>(bytes: Option<Vec<u8>>, name: &str) -> io::Result<T> {
    match decode(bytes) {
        Err(error) if error.kind() == ErrorKind::InvalidData => {
            log::warn!("discarding unreadable {name} preferences: {error}");
            Ok(T::default())
        }
        result => result,
    }
}

impl PreferenceStore {
    pub fn storage_root(&self) -> &Path {
        &self.root
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn open(root: PathBuf) -> io::Result<Arc<Self>> {
        let mut stores = STORES.lock().unwrap_or_else(PoisonError::into_inner);
        stores.retain(|_, store| store.strong_count() > 0);
        if let Some(store) = stores.get(&root).and_then(Weak::upgrade) {
            return Ok(store);
        }
        let reader = decode(files::read_sync(&root.join(READER))?)?;
        let browsing = recoverable(files::read_sync(&root.join(BROWSING))?, "browsing")?;
        let export_folder = recoverable(files::read_sync(&root.join(EXPORT_FOLDER))?, "annotation export")?;
        let store = Arc::new(Self::new(root.clone(), Snapshot { reader, browsing, export_folder }));
        stores.insert(root, Arc::downgrade(&store));
        Ok(store)
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn open(root: PathBuf) -> io::Result<Arc<Self>> {
        if let Some(store) = STORES.with_borrow(|stores| stores.get(&root).and_then(Weak::upgrade)) {
            return Ok(store);
        }
        let reader = decode(files::read(&root.join(READER)).await.map_err(Error::other)?)?;
        let browsing = recoverable(files::read(&root.join(BROWSING)).await.map_err(Error::other)?, "browsing")?;
        let export_folder = recoverable(files::read(&root.join(EXPORT_FOLDER)).await.map_err(Error::other)?, "annotation export")?;
        Ok(STORES.with_borrow_mut(|stores| {
            stores.retain(|_, store| store.strong_count() > 0);
            if let Some(store) = stores.get(&root).and_then(Weak::upgrade) {
                return store;
            }
            let store = Arc::new(Self::new(root.clone(), Snapshot { reader, browsing, export_folder }));
            stores.insert(root, Arc::downgrade(&store));
            store
        }))
    }

    fn new(root: PathBuf, snapshot: Snapshot) -> Self {
        Self { root, snapshot: Mutex::new(snapshot), writes: AsyncMutex::new(()) }
    }

    pub fn reader(&self) -> ReaderPreferences {
        self.snapshot.lock().unwrap_or_else(PoisonError::into_inner).reader.clone()
    }

    pub fn browsing(&self) -> BrowsingPreferences {
        self.snapshot.lock().unwrap_or_else(PoisonError::into_inner).browsing.clone()
    }

    pub fn annotation_export_folder(&self) -> Option<String> {
        self.snapshot.lock().unwrap_or_else(PoisonError::into_inner).export_folder.clone()
    }

    async fn write(&self, name: &str, value: &impl Serialize) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(value).map_err(|error| Error::new(ErrorKind::InvalidData, error))?;
        files::write_atomic(&self.root.join(name), bytes).await.map_err(Error::other)
    }

    /// Serialize the whole read–modify–write operation, publishing only persisted values.
    pub async fn update_reader(&self, patches: Vec<ReaderPreferencePatch>) -> io::Result<ReaderPreferences> {
        let _write = self.writes.lock().await;
        let mut preferences = self.reader();
        for patch in patches {
            patch.apply(&mut preferences).map_err(|error| Error::new(ErrorKind::InvalidInput, error))?;
        }
        self.write(READER, &preferences).await?;
        self.snapshot.lock().unwrap_or_else(PoisonError::into_inner).reader = preferences.clone();
        Ok(preferences)
    }

    pub async fn update_browsing(&self, patch: BrowsingPreferencePatch) -> io::Result<BrowsingPreferences> {
        let _write = self.writes.lock().await;
        // The UI and app worker both patch this document. Read it under the
        // shared browser lock so changing backend policy preserves UI choices.
        #[cfg(target_arch = "wasm32")]
        let _lock = files::StorageLock::acquire(&self.root.join(BROWSING)).await.map_err(Error::other)?;
        #[cfg(target_arch = "wasm32")]
        let mut preferences = recoverable(files::read(&self.root.join(BROWSING)).await.map_err(Error::other)?, "browsing")?;
        #[cfg(not(target_arch = "wasm32"))]
        let mut preferences = self.browsing();
        patch.apply(&mut preferences).map_err(|error| Error::new(ErrorKind::InvalidInput, error))?;
        self.write(BROWSING, &preferences).await?;
        self.snapshot.lock().unwrap_or_else(PoisonError::into_inner).browsing = preferences.clone();
        Ok(preferences)
    }

    pub async fn set_annotation_export_folder(&self, folder: String) -> io::Result<String> {
        let path = Path::new(&folder);
        if folder.trim().is_empty() || !path.is_absolute() || path.components().any(|component| matches!(component, Component::ParentDir)) {
            return Err(Error::new(ErrorKind::InvalidInput, "choose an absolute annotation export folder"));
        }
        let _write = self.writes.lock().await;
        self.write(EXPORT_FOLDER, &folder).await?;
        self.snapshot.lock().unwrap_or_else(PoisonError::into_inner).export_folder = Some(folder.clone());
        Ok(folder)
    }
}
