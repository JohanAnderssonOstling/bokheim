mod document;
mod input;
mod overlays;
mod search;
mod sidebar;

use crate::epub::{LoadState, ReaderView};
use crate::invalidation::Component;
use crate::shell::chrome;
use crate::shell::input::{self as shell_input, CommandTarget};
use crate::shell::state_panel;
use crate::shell::toolbar::{self as sidebar_chrome, SidebarContent};
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
    /// list of call sites to remember. Before the book is ready there
    /// is no document, so the page container stands in.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.document_focus(cx).unwrap_or_else(|| self.focus.clone())
    }
}

impl Render for ReaderView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        components::sync_reader_rem_size(window, cx);
        crate::invalidation::record_render(Component::ReaderShell);
        let theme = components::browser_theme(cx);
        if let LoadState::Ready(renderer) = &self.load_state {
            let selected = cx.try_global::<components::theme::SelectedPalette>().copied();
            if self.applied_palette != selected {
                self.applied_palette = selected;
                let appearance = crate::settings::library_theme_adaptation(cx);
                let overrides = crate::settings::ReaderSettings::preferences(cx).style_overrides(Some(appearance));
                let paint = crate::settings::library_paint_palette(cx);
                renderer.update(cx, |renderer, cx| {
                    renderer.apply(html_view_core::RendererCommand::SetReaderStyleOverrides(overrides), cx);
                    renderer.apply(html_view_core::RendererCommand::SetReaderPaintPalette(paint), cx);
                });
            }
        }
        let entity = cx.entity();
        if self.chrome.reveal_when_system_bars_return(window, cx) {
            state_panel::select_tab(&mut self.sidebar, crate::shell::state_panel::SidebarTab::Contents, cx);
            crate::invalidation::notify(cx, Component::ReaderShell, "system_bar_sidebar_shown");
        }
        let touch_input = cx.supports_touch_input();
        let search_visible = self.search.query.is_some();
        let chrome_visible = self.chrome.is_visible() || search_visible;
        let mobile = components::uses_mobile_navigation(window);
        // A persistent desktop split always shows a tab — there is no closed
        // state once the toolbar's toggle button is gone. The mobile/compact
        // overlay keeps the open-or-closed sheet behaviour instead.
        let active_tab = self.active_sidebar_tab(window);
        let sidebar_visible = chrome_visible && active_tab.is_some();
        let preferences = crate::settings::ReaderSettings::preferences(cx);
        let (split_sidebar_width, horizontal_margin, available_width) = Self::horizontal_layout(window, cx, sidebar_visible);
        let vertical_margin = preferences.vertical_margin_px.min(((f32::from(window.viewport_size().height) - 100.0) / 2.0).max(0.0));
        let max_columns = self.max_columns_for_gap(f64::from(available_width), cx);
        let fitted_column_width = self.fitted_column_width(f64::from(available_width), cx);
        let column_gap = f64::from(preferences.min_column_gap_px) / preferences.scale.max(0.1);
        if let LoadState::Ready(renderer) = &self.load_state {
            renderer.update(cx, |renderer, cx| {
                renderer.apply(html_view_core::RendererCommand::SetColumnViewportPlan {
                    available_width: f64::from(available_width),
                    column_width: fitted_column_width,
                    column_count: max_columns,
                    column_gap,
                }, cx);
            });
        }
        let document = self.render_document(entity.clone(), horizontal_margin, vertical_margin, cx);

        let mut document_body = components::reader_document_body()
            .id("epub-reader-document-surface")
            .child(document);
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
            let active = active_tab.expect("sidebar_visible implies an active tab");
            let (tabs, body) = self.render_sidebar(active, entity.clone(), theme, window, cx);
            let progress = self.progress.summary();
            let search_element = self.render_search(window, theme, cx);
            let sidebar_content = SidebarContent::new(self.spine_title(), progress.label(), progress.tooltip(), self.document_focus(cx), tabs, body);
            let sidebar = sidebar_chrome::reader_sidebar(self, &entity, sidebar_content, search_element, theme, window);
            chrome::with_sidebar(sidebar, document_body.into_any_element(), window, cx)
        } else {
            document_body.into_any_element()
        };

        // The page container is only focusable while the book is
        // loading, when it is the one thing that can hold the keyboard. A
        // focusable container steals focus from the document on any click that
        // reaches it — a margin, the sidebar's background — and navigation is
        // bound in the document's key context, so the paging keys would go
        // dead until the text was clicked again. Dropping the handle once the
        // document exists leaves clicks on the chrome harmless; the container
        // still sits on the dispatch path, so `Reader` bindings keep firing.
        let document_ready = self.document_focus(cx).is_some();
        self.chrome.sync_system_bars(chrome_visible, cx);
        let page = crate::shell::page::reader_page(window, cx, theme, document_ready, &self.focus, chrome_visible, None, content)
            // Reading is pointer-first: merely returning to this window should
            // make its page keys available again. Activate at the platform
            // level as well as restoring the document's GPUI focus, so this
            // also takes keyboard input back from another application.
            .on_mouse_move(cx.listener(|reader: &mut Self, _, window, cx| reader.reclaim_hover_focus(window, cx)))
            .on_scroll_wheel(cx.listener(Self::handle_mouse_wheel))
            .on_action(cx.listener(shell_input::dispatch::<Self, PreviousSection>))
            .on_action(cx.listener(shell_input::dispatch::<Self, NextSection>))
            .on_action(cx.listener(shell_input::dispatch::<Self, IncreaseFont>))
            .on_action(cx.listener(shell_input::dispatch::<Self, DecreaseFont>))
            .on_action(cx.listener(shell_input::dispatch::<Self, IncreaseColumnWidth>))
            .on_action(cx.listener(shell_input::dispatch::<Self, DecreaseColumnWidth>))
            .on_action(cx.listener(shell_input::dispatch::<Self, ToggleFullscreen>));
        let interactions = chrome::surface_interactions("epub", chrome_visible, sidebar_visible, touch_input, cx, |reader: &mut Self, action, window, cx| match action {
            chrome::SurfaceAction::PreviousPage => {
                reader.execute(&PreviousPage, window, cx);
            }
            chrome::SurfaceAction::NextPage => {
                reader.execute(&NextPage, window, cx);
            }
            chrome::SurfaceAction::ToggleChrome => reader.chrome.toggle(cx),
            chrome::SurfaceAction::ShowChrome => {
                if !reader.chrome.show_from_pointer(cx) {
                    return;
                }
                state_panel::select_tab(&mut reader.sidebar, crate::shell::state_panel::SidebarTab::Contents, cx);
                crate::invalidation::notify(cx, Component::ReaderShell, "sidebar_shown");
            }
        });
        page.children(interactions)
    }
}

/// Quiet period that ends a wheel gesture, matching the browse paginator so the
/// two surfaces respond to the same flick in the same way.
const WHEEL_SCROLL_END_DELAY: std::time::Duration = std::time::Duration::from_millis(150);

impl ReaderView {
    /// Unconditionally claim both OS-window and document focus on pointer hover.
    fn reclaim_hover_focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        window.activate_window();
        self.focus_document(window, cx);
    }

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
