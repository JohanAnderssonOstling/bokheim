use app::AppClient;
use gpui::App;
use std::rc::Rc;

#[cfg(feature = "kobo")]
pub type SaveKoboAutoRotate = Rc<dyn Fn(bool, &mut App)>;

/// Data the backend resolves once, before the first window is built.
#[derive(Clone)]
pub struct AppStartup {
    pub default_library_root: String,
    pub dictionary_storage_root: String,
    pub preference_storage_root: String,
    pub libraries: Vec<app::LibraryEntry>,
    pub browsing_preferences: app_preferences::BrowsingPreferences,
    pub account_status: app::AccountStatus,
}

#[derive(Clone)]
pub struct AppServices {
    pub(crate) management: library_management::management::ManagementClient,
    pub(crate) preferences: app_preferences::Client,
    pub(crate) dictionaries: dictionary::LocalClient,
    pub(crate) updates: Option<gpui::Entity<super::UpdateControls>>,
    pub(crate) backend: Rc<AppClient>,
    pub(crate) startup: Rc<AppStartup>,
    #[cfg(target_arch = "wasm32")]
    pub(crate) sync_events: Option<async_channel::Sender<crate::universal::sync::SyncEvent>>,
    #[cfg(feature = "kobo")]
    pub(crate) save_kobo_auto_rotate: SaveKoboAutoRotate,
}

struct LocalPreferences(app_preferences::Client);
impl gpui::Global for LocalPreferences {}
pub(crate) fn preference_client(cx: &App) -> app_preferences::Client {
    cx.global::<LocalPreferences>().0.clone()
}

impl AppServices {
    /// Installs the device-local services shared by browser and reader windows.
    pub fn configure(&self, cx: &mut App) {
        cx.set_global(LocalPreferences(self.preferences.clone()));
        dictionary_settings::configure(Rc::new(self.dictionaries.clone()), cx);
    }

    pub fn with_updates(mut self, updates: gpui::Entity<super::UpdateControls>) -> Self {
        self.updates = Some(updates);
        self
    }

    pub async fn for_new_window(&self) -> Result<Self, app::BackendError> {
        let mut startup = (*self.startup).clone();
        startup.libraries = self.management.libraries().await?;
        let mut services = self.clone();
        services.startup = Rc::new(startup);
        Ok(services)
    }

    pub fn new(backend: AppClient, startup: AppStartup, #[cfg(feature = "kobo")] save_kobo_auto_rotate: SaveKoboAutoRotate) -> Self {
        Self {
            management: backend.management.clone(),
            preferences: backend.preferences.clone(),
            dictionaries: dictionary::LocalClient::new(startup.dictionary_storage_root.clone(), backend.management.executor().clone()),
            updates: None,
            backend: Rc::new(backend),
            startup: Rc::new(startup),
            #[cfg(target_arch = "wasm32")]
            sync_events: None,
            #[cfg(feature = "kobo")]
            save_kobo_auto_rotate,
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub fn with_sync_events(mut self, events: async_channel::Sender<crate::SyncEvent>) -> Self {
        self.sync_events = Some(events);
        self
    }

    #[cfg(feature = "kobo")]
    pub fn save_kobo_auto_rotate(&self, enabled: bool, cx: &mut App) {
        (self.save_kobo_auto_rotate)(enabled, cx);
    }
}
