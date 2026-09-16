mod document;
mod input;
mod overlays;
mod search;
mod sidebar;

use crate::epub::{LoadState, ReaderView};
use crate::invalidation::Component;
use crate::shell::chrome;
use crate::shell::input::{self as shell_input, CommandTarget};
use crate::shell::state_panel::SidebarPanel;
use crate::{DecreaseColumnWidth, DecreaseFont, IncreaseColumnWidth, IncreaseFont, NextPage, NextSection, PreviousPage, PreviousSection, ToggleFullscreen};
use gpui::prelude::*;
use gpui::{App, Context, FocusHandle, Focusable, IntoElement, KeyBinding, Render, Window};
use ui_components as components;

impl Focusable for ReaderView {
    /// Focusing the reader means focusing the document.
    ///
    /// Navigation keys are bound in the `ReaderDocument` context, which only
    /// the HTML view establishes, so anything that focuses this view has to
    /// land on the document or the keyboard goes dead. Answering with the
    /// document handle keeps that true for every caller instead of leaving a
    /// list of call sites to remember. Before the publication is ready there
    /// is no document, so the page container stands in.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.document_focus(cx).unwrap_or_else(|| self.focus.clone())
    }
}

impl Render for ReaderView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::invalidation::record_render(Component::ReaderShell);
        let theme = components::browser_theme(cx);
        let entity = cx.entity();
        let touch_input = cx.supports_touch_input();
        let sidebar_visible = self.sidebar.is_visible();
        let split_sidebar_width = chrome::sidebar_width(sidebar_visible, window, cx);
        let available_width = (f32::from(window.viewport_size().width) - split_sidebar_width).max(1.0);
        if let LoadState::Ready(renderer) = &self.load_state {
            renderer.update(cx, |renderer, cx| renderer.set_available_width(f64::from(available_width), cx));
        }
        let document = self.render_document(entity.clone(), cx);

        let mut document_body = components::reader_document_body().child(document);
        if let Some(annotation) = self.render_annotation_popup(&entity, sidebar_visible, window, cx) {
            document_body = document_body.child(annotation);
        }
        if let Some(footnote) = self.render_footnote(cx) {
            document_body = document_body.child(footnote);
        }
        if let Some(image) = self.render_image_preview(&entity, split_sidebar_width, window, cx) {
            document_body = document_body.child(image);
        }

        let content = if sidebar_visible {
            let sidebar = match self.sidebar.active() {
                Some(SidebarPanel::Annotations) => self.render_reader_state(cx),
                Some(SidebarPanel::Contents) => self.render_toc(entity.clone(), theme, cx),
                None => unreachable!("a visible sidebar has an active panel"),
            };
            chrome::with_sidebar(sidebar, document_body.into_any_element(), self.chrome.is_visible() || self.search.query.is_some(), self.search.query.is_some(), window, cx)
        } else {
            document_body.into_any_element()
        };

        // The page container is only focusable while the publication is
        // loading, when it is the one thing that can hold the keyboard. A
        // focusable container steals focus from the document on any click that
        // reaches it — a margin, the toolbar's background, the sidebar — and
        // navigation is bound in the document's key context, so the paging keys
        // would go dead until the text was clicked again. Dropping the handle
        // once the document exists leaves clicks on the chrome harmless; the
        // container still sits on the dispatch path, so `Reader` bindings keep
        // firing.
        let document_ready = self.document_focus(cx).is_some();
        let search = self.render_search(window, theme, cx);
        let chrome_visible = self.chrome.is_visible() || search.is_some();
        self.chrome.sync_system_bars(chrome_visible, cx);
        let chrome_height = components::READER_TOOLBAR_HEIGHT + if search.is_some() { crate::shell::search::bar_height(window) } else { 0.0 };
        let toolbar = chrome_visible.then(|| self.render_toolbar(window, cx));
        let page = crate::shell::page::reader_page(window, cx, theme, document_ready, &self.focus, toolbar, search, content)
            .on_scroll_wheel(cx.listener(Self::handle_mouse_wheel))
            .on_action(cx.listener(shell_input::dispatch::<Self, PreviousSection>))
            .on_action(cx.listener(shell_input::dispatch::<Self, NextSection>))
            .on_action(cx.listener(shell_input::dispatch::<Self, IncreaseFont>))
            .on_action(cx.listener(shell_input::dispatch::<Self, DecreaseFont>))
            .on_action(cx.listener(shell_input::dispatch::<Self, IncreaseColumnWidth>))
            .on_action(cx.listener(shell_input::dispatch::<Self, DecreaseColumnWidth>))
            .on_action(cx.listener(shell_input::dispatch::<Self, ToggleFullscreen>));
        let interactions = chrome::surface_interactions("epub", chrome_visible, sidebar_visible, chrome_height, touch_input, cx, |reader: &mut Self, action, window, cx| match action {
            chrome::SurfaceAction::PreviousPage => {
                reader.execute(&PreviousPage, window, cx);
            }
            chrome::SurfaceAction::NextPage => {
                reader.execute(&NextPage, window, cx);
            }
            chrome::SurfaceAction::ToggleChrome => reader.chrome.toggle(cx),
            chrome::SurfaceAction::ShowChrome => reader.chrome.show(cx),
        });
        page.children(interactions)
    }
}

/// Quiet period that ends a wheel gesture, matching the browse paginator so the
/// two surfaces respond to the same flick in the same way.
const WHEEL_SCROLL_END_DELAY: std::time::Duration = std::time::Duration::from_millis(150);

impl ReaderView {
    /// Turns the page on a wheel gesture rather than scrolling the document.
    ///
    /// The whole burst is treated as one gesture: each event postpones the turn,
    /// and when input goes quiet the net direction advances exactly one page —
    /// otherwise a single trackpad flick would fly through a chapter. Scrolling
    /// down goes forward, the same direction the browse grid pages.
    fn handle_mouse_wheel(&mut self, event: &gpui::ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        let delta = match event.delta {
            gpui::ScrollDelta::Pixels(delta) => f32::from(delta.y),
            gpui::ScrollDelta::Lines(delta) => delta.y,
        };
        if !delta.is_finite() || delta == 0.0 {
            return;
        }
        self.wheel_delta += delta;
        self.wheel_debounce.take();
        let timer = cx.background_executor().timer(WHEEL_SCROLL_END_DELAY);
        self.wheel_debounce = Some(cx.spawn_in(window, async move |reader, cx| {
            timer.await;
            let _ = reader.update_in(cx, |reader, window, cx| {
                reader.wheel_debounce.take();
                let delta = std::mem::take(&mut reader.wheel_delta);
                if delta < 0.0 {
                    reader.execute(&NextPage, window, cx);
                } else if delta > 0.0 {
                    reader.execute(&PreviousPage, window, cx);
                }
            });
        }));
        cx.stop_propagation();
    }
}

/// The key context the HTML document element establishes while it holds focus.
///
/// Navigation binds here rather than to `Reader` so that page keys — `space`
/// above all — never fire while the search input has focus.
const DOCUMENT_CONTEXT: &str = "ReaderDocument";

pub(crate) fn bind_keys(cx: &mut App) {
    let primary = if cfg!(target_os = "macos") { "cmd" } else { "ctrl" };
    shell_input::bind_document_keys(cx, DOCUMENT_CONTEXT);
    cx.bind_keys([
        KeyBinding::new(&format!("{primary}-left"), PreviousSection, Some(DOCUMENT_CONTEXT)),
        KeyBinding::new(&format!("{primary}-right"), NextSection, Some(DOCUMENT_CONTEXT)),
        KeyBinding::new("f11", ToggleFullscreen, Some("Reader")),
        KeyBinding::new("ctrl-alt-=", IncreaseColumnWidth, Some(DOCUMENT_CONTEXT)),
        KeyBinding::new("ctrl-alt-+", IncreaseColumnWidth, Some(DOCUMENT_CONTEXT)),
        KeyBinding::new("ctrl-alt--", DecreaseColumnWidth, Some(DOCUMENT_CONTEXT)),
        KeyBinding::new("alt-=", IncreaseFont, Some(DOCUMENT_CONTEXT)),
        KeyBinding::new("alt-+", IncreaseFont, Some(DOCUMENT_CONTEXT)),
        KeyBinding::new("alt--", DecreaseFont, Some(DOCUMENT_CONTEXT)),
    ]);
}
