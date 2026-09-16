use bokheim_backend::AppClient;
#[cfg(feature = "kobo")]
use gpui::App;
use std::rc::Rc;

#[cfg(feature = "kobo")]
pub type SaveKoboAutoRotate = Rc<dyn Fn(bool, &mut App)>;

/// Data the backend resolves once, before the first window is built.
#[derive(Clone)]
pub struct AppStartup {
    pub libraries: Vec<bokheim_backend::LibraryEntry>,
    pub browsing_preferences: app_preferences::BrowsingPreferences,
    pub account_status: bokheim_backend::AccountStatus,
}

#[derive(Clone)]
pub struct AppServices {
    pub(crate) browsing_preferences: Rc<std::cell::RefCell<app_preferences::BrowsingPreferences>>,
    pub(crate) backend: Rc<AppClient>,
    pub(crate) startup: Rc<AppStartup>,
    #[cfg(feature = "kobo")]
    pub(crate) save_kobo_auto_rotate: SaveKoboAutoRotate,
}

impl AppServices {
    pub async fn for_new_window(&self) -> Result<Self, String> {
        let mut startup = (*self.startup).clone();
        startup.browsing_preferences = self.browsing_preferences.borrow().clone();
        startup.libraries = self.backend.libraries().await?;
        let mut services = self.clone();
        services.startup = Rc::new(startup);
        Ok(services)
    }

    pub fn new(backend: AppClient, startup: AppStartup, #[cfg(feature = "kobo")] save_kobo_auto_rotate: SaveKoboAutoRotate) -> Self {
        Self {
            browsing_preferences: Rc::new(std::cell::RefCell::new(startup.browsing_preferences.clone())),
            backend: Rc::new(backend),
            startup: Rc::new(startup),
            #[cfg(feature = "kobo")]
            save_kobo_auto_rotate,
        }
    }

    #[cfg(feature = "kobo")]
    pub fn save_kobo_auto_rotate(&self, enabled: bool, cx: &mut App) {
        (self.save_kobo_auto_rotate)(enabled, cx);
    }
}
