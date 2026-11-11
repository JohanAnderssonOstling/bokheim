//! Opens the embedded browser from the catalog.
use crate::library::services::LibraryContext;
use crate::services::notify_error;
use gpui::prelude::*;
use gpui::{AnyElement, App, Context, Entity, Window};
use gpui_component::IconName;
use library_backend::LibraryClient;
use library_backend::executor::BackendExecutor;
use ui_components::{self as components, user_facing_error};
use ui_text::text;

pub(super) struct BrowserActions {
    library: LibraryClient,
    executor: BackendExecutor,
    browser_available: bool,
}

impl BrowserActions {
    pub(super) fn new(ui: LibraryContext, cx: &mut Context<Self>) -> Self {
        let executor = ui.management().executor().clone();
        let probe = executor.clone();
        cx.spawn(async move |actions, cx| {
            let available = crate::services::browser::available(&probe).await;
            let _ = actions.update(cx, |actions, cx| {
                actions.browser_available = available;
                cx.notify();
            });
        })
        .detach();
        Self { library: ui.backend().clone(), executor, browser_available: false }
    }

    fn open_browser(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let library = self.library.clone();
        let executor = self.executor.clone();
        let preferences = crate::stores::preferences::current_browsing_preferences(cx);
        cx.spawn_in(window, async move |actions, cx| {
            if let Err(error) = crate::services::browser::open(&executor, library, preferences).await {
                let _ = actions.update_in(cx, |_, window, cx| notify_error("browser-open-error", format!(text!(browse::could_not_open_the_browser), user_facing_error(&error, "")), window, cx));
            }
        })
        .detach();
    }

    /// Opens the browser directly where a browser window is available.
    pub(crate) fn render_browser_trigger(actions: Entity<Self>, cx: &App) -> Option<AnyElement> {
        actions.read(cx).browser_available.then(|| {
            components::outlined_icon_button("catalog-open-browser", text!(browse::open_browser), IconName::Globe).on_click(move |_, window, cx| actions.update(cx, |actions, cx| actions.open_browser(window, cx))).into_any_element()
        })
    }
}
