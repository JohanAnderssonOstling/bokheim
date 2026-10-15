use crate::invalidation::notify;
use crate::settings::{ReaderSettings, SidebarTab};
use std::time::Duration;
mod document;
mod input;
mod overlays;
mod search;
mod sidebar;
pub(super) mod touch_selection;

use crate::epub::{LoadState, ReaderView};
use crate::invalidation::Component;
use crate::shell::chrome;
use crate::shell::input::{self as shell_input, CommandTarget};
use crate::shell::page::ReaderSurface;
use crate::shell::toolbar::{self as sidebar_chrome, SidebarContent};
use crate::{DecreaseColumnWidth, DecreaseFont, IncreaseColumnWidth, IncreaseFont, NextPage, NextSection, PreviousPage, PreviousSection, ToggleFullscreen};
use gpui::prelude::*;
use gpui::{App, Context, FocusHandle, Focusable, IntoElement, KeyBinding, Render, ScrollDelta, Window};
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
        #[cfg(all(target_os = "android", feature = "native-tts", not(feature = "kobo")))]
        self.tts.sync_volume_buttons(cx);
        components::sync_reader_rem_size(window, cx);
        crate::invalidation::record_render(Component::ReaderShell);
        let theme = components::browser_theme(cx);
        if let LoadState::Ready(renderer) = &self.load_state {
            let selected = cx.try_global::<components::theme::SelectedPalette>().copied();
            if self.applied_palette != selected {
                self.applied_palette = selected;
                let appearance = crate::settings::library_theme_adaptation(cx);
                let overrides = ReaderSettings::preferences(cx).style_overrides(Some(appearance), crate::settings::library_link_foreground(cx));
                let paint = crate::settings::library_paint_palette(cx);
                renderer.update(cx, |renderer, cx| {
                    renderer.apply(html_view_core::RendererCommand::SetReaderStyleOverrides(overrides), cx);
                    renderer.apply(html_view_core::RendererCommand::SetReaderPaintPalette(paint), cx);
                });
            }
        }
        let entity = cx.entity();
        let mobile = components::uses_mobile_reader_controls(window);
        if mobile {
            self.settings_navigation.keyboard_visibility().set(false);
        }
        let touch_input = cx.supports_touch_input() || mobile;
        let chrome_visible = self.chrome.is_visible();
        // A persistent desktop split always shows a tab — there is no closed
        // state once the toolbar's toggle button is gone. The mobile/compact
        // overlay keeps the open-or-closed sheet behaviour instead.
        let active_tab = self.active_sidebar_tab(window);
        let sidebar_visible = chrome_visible && active_tab.is_some();
        // A saved Search panel starts searching when first opened; hiding
        // it afterwards preserves the query, matches, and highlights.
        if sidebar_visible && active_tab == Some(SidebarTab::Search) {
            self.open_search(cx);
        }
        let preferences = ReaderSettings::preferences(cx);
        let (split_sidebar_width, horizontal_margin, available_width) = Self::horizontal_layout(window, cx, sidebar_visible);
        let vertical_margin = preferences.vertical_margin_px.min(((f32::from(window.viewport_size().height) - 100.0) / 2.0).max(0.0));
        let max_columns = self.max_columns_for_gap(f64::from(available_width), cx);
        let fitted_column_width = self.fitted_column_width(f64::from(available_width), cx);
        let column_gap = f64::from(preferences.min_column_gap_px) / preferences.scale.max(0.1);
        if let LoadState::Ready(renderer) = &self.load_state {
            let plan = (renderer.entity_id(), f64::from(available_width), fitted_column_width, max_columns, column_gap);
            if self.touch_swipe.is_none() || self.last_column_viewport_plan != Some(plan) {
                renderer.update(cx, |renderer, cx| {
                    renderer.apply(html_view_core::RendererCommand::SetColumnViewportPlan { available_width: plan.1, column_width: plan.2, column_count: plan.3, column_gap: plan.4 }, cx);
                });
                self.last_column_viewport_plan = Some(plan);
            }
        }
        let drag_offset = self.swipe_preview.offset(window, cx);
        let document = gpui::div().size_full().transform(gpui::ElementTransform::default().translate(drag_offset)).child(self.render_document(entity.clone(), horizontal_margin, vertical_margin, mobile, cx));

        let mut document_body = components::reader_document_body().id("epub-reader-document-surface").child(document).on_scroll_wheel(cx.listener(Self::handle_mouse_wheel));
        document_body = document_body.children(self.render_touch_selection(window, cx));
        if let Some(annotation) = self.render_annotation_popup(&entity, sidebar_visible, window, cx) {
            document_body = document_body.child(annotation);
        }
        if let Some(footnote) = self.render_footnote(cx) {
            document_body = document_body.child(footnote);
        }
        if let Some(image) = self.render_image_preview(&entity, split_sidebar_width, window, cx) {
            document_body = document_body.child(image);
        }

        #[cfg(all(feature = "native-tts", not(feature = "kobo"), not(target_arch = "wasm32")))]
        let dock = self.tts.dock_open.then(|| crate::shell::tts::dock(&self.tts, mobile, theme, cx));
        #[cfg(not(all(feature = "native-tts", not(feature = "kobo"), not(target_arch = "wasm32"))))]
        let dock: Option<gpui::AnyElement> = None;
        let sidebar = active_tab.filter(|_| sidebar_visible).map(|active| {
            let body = self.render_sidebar(active, entity.clone(), theme, cx);
            let progress = self.progress.summary();
            let sidebar_content = SidebarContent::new(f32::from(progress.percent) / 100.0, progress.label(), progress.tooltip(), self.document_focus(cx), body);
            sidebar_chrome::reader_sidebar(self, &entity, sidebar_content, theme, window, cx)
        });

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
        let surface = ReaderSurface {
            document: document_body.into_any_element(),
            dock,
            sidebar,
            active_tab,
            chrome_visible,
            document_ready,
            loading_focus: self.focus.clone(),
            title: self.book_title.clone(),
            chapter: self.spine_title(),
            progress_label: self.progress.summary().label(),
            bottom_bar_id: "epub-reader-bottom-bar",
        };
        let page = crate::shell::page::reader_surface(self, surface, theme, window, cx)
            .id("epub-reader-page")
            // Reading is pointer-first: merely returning to this window should
            // make its page keys available again. Activate at the platform
            // level as well as restoring the document's GPUI focus, so this
            // also takes keyboard input back from another application.
            .on_mouse_move(cx.listener(|reader: &mut Self, _, window, cx| reader.reclaim_hover_focus(window, cx)))
            .capture_any_touch(cx.listener(Self::handle_selection_touch))
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
                reader.sidebar.reopen(cx);
                notify(cx, Component::ReaderShell, "sidebar_shown");
            }
        });
        let mut page = page.children(interactions);
        if let Some(actions) = &self.context_sheet {
            let sheet = actions.sheet(theme, window.insets().safe_area.bottom, &entity);
            page = page.child(sheet);
        }
        page
    }
}

/// Quiet period that ends a wheel gesture, matching the browse paginator so the
/// two surfaces respond to the same flick in the same way.
const WHEEL_SCROLL_END_DELAY: Duration = Duration::from_millis(150);

impl ReaderView {
    /// Reclaim document focus on hover without interrupting sidebar keyboard input.
    fn reclaim_hover_focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sidebar_focus.contains_focused(window, cx) || self.rail_focus.contains_focused(window, cx) || self.marks.selected().is_some() {
            return;
        }
        window.activate_window();
        self.focus_document(window, cx);
    }

    /// Turns the page on a wheel gesture rather than scrolling the document.
    ///
    /// Android touch swipes commit on their end event. Physical wheel and
    /// trackpad bursts use a quiet period so one flick advances only one page.
    /// Scrolling down goes forward, the same direction the browse grid pages.
    fn handle_mouse_wheel(&mut self, event: &gpui::ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.touch_selection.active {
            cx.stop_propagation();
            return;
        }
        if let ScrollDelta::Pages(delta) = event.delta {
            let direction = if delta.y != 0.0 { delta.y } else { delta.x };
            self.wheel_delta = 0.0;
            self.wheel_debounce.take();
            self.touch_swipe = None;
            self.touch_swipe_from_edge = false;
            if direction == 0.0 {
                self.swipe_preview.settle();
            } else {
                self.swipe_preview.clear();
            }
            cx.notify();
            if direction.is_finite() && direction != 0.0 {
                if direction < 0.0 {
                    self.execute(&NextPage, window, cx);
                } else {
                    self.execute(&PreviousPage, window, cx);
                }
            }
            cx.stop_propagation();
            return;
        }
        let delta = match event.delta {
            ScrollDelta::Pixels(delta) => f32::from(delta.y),
            ScrollDelta::Lines(delta) => delta.y,
            ScrollDelta::Pages(delta) => {
                if delta.y != 0.0 {
                    delta.y
                } else {
                    delta.x
                }
            }
        };

        if cfg!(target_os = "android") || window.is_page_scroll_swipe() || self.touch_swipe.is_some() {
            let travel = match event.delta {
                ScrollDelta::Pixels(delta) => gpui::point(f32::from(delta.x), f32::from(delta.y)),
                ScrollDelta::Lines(delta) | ScrollDelta::Pages(delta) => gpui::point(delta.x, delta.y),
            };
            let travel = if travel.x.is_finite() && travel.y.is_finite() { travel } else { gpui::point(0.0, 0.0) };
            match event.touch_phase {
                gpui::TouchPhase::Started => {
                    self.wheel_debounce.take();
                    self.touch_swipe = Some(travel);
                    self.touch_swipe_from_edge = components::in_bottom_gesture_edge(event.position, window);
                    if !self.touch_swipe_from_edge {
                        if self.swipe_preview.drag(travel, window) {
                            cx.notify();
                        }
                    }
                    cx.stop_propagation();
                    return;
                }
                gpui::TouchPhase::Moved if self.touch_swipe.is_some() => {
                    if let Some(swipe) = &mut self.touch_swipe {
                        *swipe = gpui::point(swipe.x + travel.x, swipe.y + travel.y);
                        if !self.touch_swipe_from_edge {
                            if self.swipe_preview.drag(*swipe, window) {
                                cx.notify();
                            }
                        }
                    }
                    cx.stop_propagation();
                    return;
                }
                gpui::TouchPhase::Ended if self.touch_swipe.is_some() => {
                    let Some(swipe) = self.touch_swipe.take() else { return };
                    // A swipe from the bottom edge is the system's.
                    let from_edge = std::mem::take(&mut self.touch_swipe_from_edge);
                    match components::swipe_turns_forward(swipe, window).filter(|_| !from_edge) {
                        Some(forward) => {
                            self.swipe_preview.clear();
                            if forward { self.execute(&NextPage, window, cx) } else { self.execute(&PreviousPage, window, cx) }
                        }
                        None => self.swipe_preview.settle(),
                    }
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                gpui::TouchPhase::Cancelled if self.touch_swipe.is_some() => {
                    self.touch_swipe = None;
                    self.touch_swipe_from_edge = false;
                    self.swipe_preview.settle();
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                _ => {}
            }
        }

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
