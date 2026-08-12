use std::rc::Rc;
use std::sync::Arc;

use epub_svelte_lib::BookLocator;
use epub_svelte_lib::FrontendContract;
use epub_svelte_lib::app::AppBackend;
use gpui::{AnyWindowHandle, App, Window};

use crate::model::{BrowserSession, BrowserWindowGeometry};

pub type OpenReader = Rc<dyn Fn(BookLocator, String, &mut App)>;
pub type SaveBrowserState = Rc<dyn Fn(BrowserSession, Option<BrowserWindowGeometry>, &mut App)>;
#[cfg(feature = "kobo")]
pub type SaveKoboAutoRotate = Rc<dyn Fn(bool, &mut App)>;

#[derive(Clone)]
pub struct AppServices {
    pub(crate) backend: AppBackend,
    pub(crate) contracts: Arc<FrontendContract>,
    pub(crate) open_reader: OpenReader,
    pub(crate) save_state: SaveBrowserState,
    #[cfg(feature = "kobo")]
    pub(crate) save_kobo_auto_rotate: SaveKoboAutoRotate,
    window: AnyWindowHandle,
}

impl AppServices {
    #[allow(clippy::too_many_arguments)]
    pub fn new(backend: AppBackend, open_reader: OpenReader, save_state: SaveBrowserState, #[cfg(feature = "kobo")] save_kobo_auto_rotate: SaveKoboAutoRotate, window: &Window) -> Self {
        Self {
            backend,
            contracts: Arc::new(FrontendContract::default()),
            open_reader,
            save_state,
            #[cfg(feature = "kobo")]
            save_kobo_auto_rotate,
            window: window.window_handle(),
        }
    }

    pub(crate) fn with_window<R>(&self, cx: &mut App, body: impl FnOnce(&mut Window, &mut App) -> R) -> Option<R> {
        self.window.update(cx, |_, window, cx| body(window, cx)).ok()
    }
}
