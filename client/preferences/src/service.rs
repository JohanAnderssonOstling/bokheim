//! Ordered preference changes and their device-wide effects.
use crate::{BrowsingPreferencePatch, BrowsingPreferences, PreferenceStore, ReaderPreferences};
use client_platform_runtime::executor::BackendExecutor;
use client_platform_runtime::text_search;
#[cfg(any(target_arch = "wasm32", not(feature = "kobo")))]
use client_platform_runtime::transfers;
use std::{io, path::Path, sync::Arc};
use tokio::sync::Mutex;

/// Libraries own index cleanup, including libraries whose folders cannot open.
pub trait TextSearchIndexCleanup: Send + Sync {
    fn schedule_cleanup(&self);
}

pub struct Service {
    store: Arc<PreferenceStore>,
    updates: Mutex<()>,
    indexes: Arc<dyn TextSearchIndexCleanup>,
}

impl Service {
    pub fn start(store: Arc<PreferenceStore>, data_dir: &Path, executor: &BackendExecutor, indexes: Arc<dyn TextSearchIndexCleanup>) -> io::Result<Self> {
        let browsing = store.browsing();
        text_search::set_enabled(browsing.text_search());
        #[cfg(all(not(target_arch = "wasm32"), not(feature = "kobo")))]
        transfers::initialize(data_dir, browsing.allow_metered_transfers(), executor)?;
        #[cfg(target_arch = "wasm32")]
        transfers::POLICY.set_allow_metered(browsing.allow_metered_transfers());
        #[cfg(any(target_arch = "wasm32", feature = "kobo"))]
        let _ = (data_dir, executor);
        Ok(Self { store, updates: Mutex::new(()), indexes })
    }

    pub fn storage_root(&self) -> &Path {
        self.store.storage_root()
    }

    pub fn reader(&self) -> ReaderPreferences {
        self.store.reader()
    }

    pub fn browsing(&self) -> BrowsingPreferences {
        self.store.browsing()
    }

    pub async fn update_browsing(&self, patch: BrowsingPreferencePatch) -> io::Result<BrowsingPreferences> {
        let _ordered = self.updates.lock().await;
        let clear_text_index = matches!(patch, BrowsingPreferencePatch::TextSearch(false));
        #[cfg(all(not(target_arch = "wasm32"), not(feature = "kobo")))]
        let previous_metered = self.store.browsing().allow_metered_transfers();
        #[cfg(all(not(target_arch = "wasm32"), not(feature = "kobo")))]
        let transfer_setting = matches!(patch, BrowsingPreferencePatch::AllowMeteredTransfers(_));
        #[cfg(all(not(target_arch = "wasm32"), not(feature = "kobo")))]
        if let BrowsingPreferencePatch::AllowMeteredTransfers(allow) = &patch {
            transfers::save_allow_metered(*allow)?;
        }
        let preferences = match self.store.update_browsing(patch).await {
            Ok(preferences) => preferences,
            Err(error) => {
                #[cfg(all(not(target_arch = "wasm32"), not(feature = "kobo")))]
                if transfer_setting {
                    let _ = transfers::save_allow_metered(previous_metered);
                }
                return Err(error);
            }
        };
        #[cfg(target_arch = "wasm32")]
        transfers::POLICY.set_allow_metered(preferences.allow_metered_transfers());
        text_search::set_enabled(preferences.text_search());
        if clear_text_index {
            self.indexes.schedule_cleanup();
        }
        Ok(preferences)
    }
}
