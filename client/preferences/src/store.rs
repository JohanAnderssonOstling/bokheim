//! Device-local preference documents using the shared native/OPFS file layer.
use crate::{BrowsingPreferencePatch, BrowsingPreferences, ReaderPreferencePatch, ReaderPreferences};
use client_platform_runtime::{file_storage as files, text_search, transfers};
use serde::{de::DeserializeOwned, Serialize};
use std::collections::HashMap;
use std::io::{self, Error, ErrorKind};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use tokio::sync::{watch, Mutex as AsyncMutex};

const READER: &str = "reader.json";
const BROWSING: &str = "browsing.json";
const EXPORT_FOLDER: &str = "annotation-export-folder.json";

static STORES: std::sync::LazyLock<Mutex<HashMap<PathBuf, Weak<PreferenceStore>>>> = std::sync::LazyLock::new(Default::default);

#[derive(Debug)]
struct Snapshot {
    reader: ReaderPreferences,
    export_folder: Option<String>,
}

/// Windows and browser workers share one store for each storage root.
#[derive(Debug)]
pub struct PreferenceStore {
    root: PathBuf,
    snapshot: Mutex<Snapshot>,
    browsing: watch::Sender<BrowsingPreferences>,
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
        let store = Arc::new(Self::new(root.clone(), Snapshot { reader, export_folder }, browsing));
        stores.insert(root, Arc::downgrade(&store));
        Ok(store)
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn open(root: PathBuf) -> io::Result<Arc<Self>> {
        if let Some(store) = STORES.lock().unwrap_or_else(PoisonError::into_inner).get(&root).and_then(Weak::upgrade) {
            return Ok(store);
        }
        let reader = decode(files::read(&root.join(READER)).await?)?;
        let browsing = recoverable(files::read(&root.join(BROWSING)).await?, "browsing")?;
        let export_folder = recoverable(files::read(&root.join(EXPORT_FOLDER)).await?, "annotation export")?;
        let mut stores = STORES.lock().unwrap_or_else(PoisonError::into_inner);
        stores.retain(|_, store| store.strong_count() > 0);
        if let Some(store) = stores.get(&root).and_then(Weak::upgrade) {
            return Ok(store);
        }
        let store = Arc::new(Self::new(root.clone(), Snapshot { reader, export_folder }, browsing));
        stores.insert(root, Arc::downgrade(&store));
        Ok(store)
    }

    fn new(root: PathBuf, snapshot: Snapshot, browsing: BrowsingPreferences) -> Self {
        Self { root, snapshot: Mutex::new(snapshot), browsing: watch::channel(browsing).0, writes: AsyncMutex::new(()) }
    }

    pub fn reader(&self) -> ReaderPreferences {
        self.snapshot.lock().unwrap_or_else(PoisonError::into_inner).reader.clone()
    }

    pub fn browsing(&self) -> BrowsingPreferences {
        self.browsing.borrow().clone()
    }

    /// Observe browsing preferences after they have been successfully saved.
    pub fn subscribe_browsing(&self) -> watch::Receiver<BrowsingPreferences> {
        self.browsing.subscribe()
    }

    pub fn annotation_export_folder(&self) -> Option<String> {
        self.snapshot.lock().unwrap_or_else(PoisonError::into_inner).export_folder.clone()
    }

    async fn write(&self, name: &str, value: &impl Serialize) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(value).map_err(|error| Error::new(ErrorKind::InvalidData, error))?;
        files::write_atomic(&self.root.join(name), bytes).await
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

    pub(crate) async fn update_browsing(&self, patch: BrowsingPreferencePatch) -> io::Result<BrowsingPreferences> {
        let _write = self.writes.lock().await;
        let mut preferences = self.browsing();
        let text_search_changed = matches!(patch, BrowsingPreferencePatch::TextSearch(_));
        let metered_transfers_changed = matches!(patch, BrowsingPreferencePatch::AllowMeteredTransfers(_));
        patch.apply(&mut preferences).map_err(|error| Error::new(ErrorKind::InvalidInput, error))?;
        self.write(BROWSING, &preferences).await?;
        if text_search_changed {
            text_search::set_enabled(preferences.text_search());
        }
        if metered_transfers_changed {
            transfers::POLICY.set_allow_metered(preferences.allow_metered_transfers());
        }
        self.browsing.send_replace(preferences.clone());
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
