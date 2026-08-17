use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use epub_svelte_lib::BookLocator;
use epub_svelte_lib::FrontendContract;
use epub_svelte_lib::app::AppBackend;
use gpui::{AnyWindowHandle, App, Window};

use crate::model::{BrowserSession, BrowserWindowGeometry};

pub type OpenReader = Rc<dyn Fn(BookLocator, String, Option<String>, &mut App)>;
pub type SaveBrowserState = Rc<dyn Fn(BrowserSession, Option<BrowserWindowGeometry>, &mut App)>;
#[cfg(feature = "kobo")]
pub type SaveKoboAutoRotate = Rc<dyn Fn(bool, &mut App)>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DesktopUpdate {
    pub version: String,
    pub release_name: String,
    pub download_url: String,
    pub checksum_url: String,
}

pub type CheckDesktopUpdate = Rc<dyn Fn() -> async_channel::Receiver<Result<Option<DesktopUpdate>, String>>>;
pub type InstallDesktopUpdate = Rc<dyn Fn(DesktopUpdate) -> async_channel::Receiver<Result<(), String>>>;
pub type RestartDesktopUpdate = Rc<dyn Fn() -> Result<(), String>>;

#[derive(Clone)]
pub struct DesktopUpdater {
    current_version: String,
    check: CheckDesktopUpdate,
    install: InstallDesktopUpdate,
    restart: RestartDesktopUpdate,
    available_update: Rc<RefCell<Option<DesktopUpdate>>>,
}

impl DesktopUpdater {
    pub fn new(current_version: impl Into<String>, check: CheckDesktopUpdate, install: InstallDesktopUpdate, restart: RestartDesktopUpdate) -> Self {
        Self { current_version: current_version.into(), check, install, restart, available_update: Rc::new(RefCell::new(None)) }
    }

    pub fn current_version(&self) -> &str {
        &self.current_version
    }

    pub fn check(&self) -> async_channel::Receiver<Result<Option<DesktopUpdate>, String>> {
        (self.check)()
    }

    pub fn install(&self, update: DesktopUpdate) -> async_channel::Receiver<Result<(), String>> {
        (self.install)(update)
    }

    pub fn restart(&self) -> Result<(), String> {
        (self.restart)()
    }

    pub fn available_update(&self) -> Option<DesktopUpdate> {
        self.available_update.borrow().clone()
    }

    pub fn set_available_update(&self, update: Option<DesktopUpdate>) {
        *self.available_update.borrow_mut() = update;
    }
}

#[derive(Clone)]
pub struct AppServices {
    pub(crate) backend: AppBackend,
    pub(crate) contracts: Arc<FrontendContract>,
    pub(crate) open_reader: OpenReader,
    pub(crate) save_state: SaveBrowserState,
    pub(crate) desktop_updater: Option<DesktopUpdater>,
    #[cfg(feature = "kobo")]
    pub(crate) save_kobo_auto_rotate: SaveKoboAutoRotate,
    window: AnyWindowHandle,
}

impl AppServices {
    #[allow(clippy::too_many_arguments)]
    pub fn new(backend: AppBackend, open_reader: OpenReader, save_state: SaveBrowserState, desktop_updater: Option<DesktopUpdater>, #[cfg(feature = "kobo")] save_kobo_auto_rotate: SaveKoboAutoRotate, window: &Window) -> Self {
        Self {
            backend,
            contracts: Arc::new(FrontendContract::default()),
            open_reader,
            save_state,
            desktop_updater,
            #[cfg(feature = "kobo")]
            save_kobo_auto_rotate,
            window: window.window_handle(),
        }
    }

    pub(crate) fn with_window<R>(&self, cx: &mut App, body: impl FnOnce(&mut Window, &mut App) -> R) -> Option<R> {
        self.window.update(cx, |_, window, cx| body(window, cx)).ok()
    }
}
