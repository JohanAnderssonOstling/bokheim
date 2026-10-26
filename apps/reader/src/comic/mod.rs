//! Image-only fixed-page reader. Archive access and rendering remain separate.

mod controls;
mod image_decode;

use book_content::comic::ComicReader;
use fixed_page::{DISPLAY_CACHE_BUDGET, PageCache, PageImage, PageLayout, PageSwipe, PageZoom as Zoom};
use gpui::{App, Bounds, Context, FocusHandle, Focusable, MouseButton, Pixels, Point, Render, ScrollDelta, Task, Window, div, img, prelude::*, px};
use gpui_component::ElementExt;
use library_backend::LibraryClient;
use library_model::BookLocator;
use std::{collections::HashMap, rc::Rc, sync::Arc};
use ui_components as components;

use crate::invalidation::Component;
use crate::shell::{
    chrome,
    page::ReaderSurface,
    persistence::{ReadingPositionWriter, StoredPosition},
    settings_navigation::SettingsNavigation,
    state_panel::{SidebarState, SidebarTab},
    toolbar::{self, SidebarContent, SidebarTarget},
};

pub(crate) struct ComicReaderView {
    reader: ComicReader,
    title: String,
    current: usize,
    columns: usize,
    right_to_left: bool,
    zoom: Zoom,
    offset: f32,
    pan_x: f32,
    viewport: Bounds<Pixels>,
    drag: Option<(Point<Pixels>, f32, f32)>,
    dragged: bool,
    swipe: PageSwipe,
    pages: PageCache<usize, PageImage>,
    errors: HashMap<usize, String>,
    load_task: Option<Task<()>>,
    positions: ReadingPositionWriter,
    navigation_revision: u64,
    sidebar: SidebarState,
    chrome: chrome::ChromeState,
    focus: FocusHandle,
    sidebar_focus: FocusHandle,
    rail_focus: FocusHandle,
    settings_navigation: SettingsNavigation,
    close_reader: crate::CloseReader,
    #[cfg(feature = "kobo")]
    brightness: gpui::Entity<crate::shell::brightness::BrightnessControls>,
}

impl Focusable for ComicReaderView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl ComicReaderView {
    pub(crate) fn new(
        locator: BookLocator, title: String, reader: ComicReader, initial_page: usize, offset: f32, _app: app::AppClient, library: Rc<LibraryClient>, close_reader: crate::CloseReader, window: &mut Window, cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let current = initial_page.min(reader.pages.len().saturating_sub(1));
        let mut sidebar = SidebarState::from_preferences(cx);
        if sidebar.is_visible() && !matches!(sidebar.effective_or_default(true), SidebarTab::Contents | SidebarTab::Settings) {
            sidebar.show(SidebarTab::Contents);
        }
        let mut view = Self {
            right_to_left: reader.right_to_left,
            reader,
            title,
            current,
            columns: 1,
            zoom: if offset > 0.0 { Zoom::FitWidth } else { Zoom::FitPage },
            offset,
            pan_x: 0.0,
            viewport: Bounds::default(),
            drag: None,
            dragged: false,
            swipe: PageSwipe::default(),
            pages: PageCache::new(DISPLAY_CACHE_BUDGET),
            errors: HashMap::new(),
            load_task: None,
            positions: ReadingPositionWriter::new(cx, locator.content_hash(), library),
            navigation_revision: 0,
            sidebar,
            chrome: chrome::ChromeState::from_preferences(Component::ComicShell, cx),
            focus,
            sidebar_focus: cx.focus_handle().tab_index(0).tab_stop(true),
            rail_focus: cx.focus_handle().tab_stop(true),
            settings_navigation: SettingsNavigation::new(window, cx),
            close_reader,
            #[cfg(feature = "kobo")]
            brightness: cx.new(|cx| crate::shell::brightness::BrightnessControls::new(_app, window, cx)),
        };
        view.request_pages(cx);
        view
    }

    fn request_pages(&mut self, cx: &mut Context<Self>) {
        let end = self.current + self.visible_count();
        let mut requested: Vec<_> = (self.current..end).collect();
        if end < self.reader.pages.len() {
            requested.push(end);
        }
        if self.current > 0 {
            requested.push(self.current - 1);
        }
        requested.retain(|index| !self.pages.contains(*index) && !self.errors.contains_key(index));
        let reader = self.reader.clone();
        self.load_task = Some(cx.spawn(async move |this, cx| {
            for index in requested {
                let reader = reader.clone();
                let result = cx
                    .background_executor()
                    .spawn(async move {
                        let bytes = reader.read_page(index).await.map_err(|error| error.to_string())?;
                        image_decode::decode(bytes).map(|page| PageImage::from_bgra(page.pixels).with_source_size(page.width, page.height))
                    })
                    .await;
                if this
                    .update(cx, |view, cx| {
                        match result {
                            Ok(page) => {
                                gpui_image_eviction::track_image(&page.image, cx);
                                let visible = view.current..view.current + view.visible_count();
                                let bytes = page.byte_len;
                                view.pages.insert_protected(index, Arc::new(page), bytes, |key| visible.contains(&key));
                            }
                            Err(error) => {
                                view.errors.insert(index, error);
                            }
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    return;
                }
            }
        }));
    }

    fn evict_pages(&mut self) {
        let visible = self.current..self.current + self.visible_count();
        self.pages.trim(|key| visible.contains(&key));
    }

    fn layout(&self, index: usize) -> Option<PageLayout> {
        let page = self.pages.peek(index)?;
        Some(PageLayout::new((page.width, page.height), (f32::from(self.viewport.size.width) / self.visible_count() as f32, f32::from(self.viewport.size.height)), self.zoom, self.offset, self.pan_x))
    }

    fn visible_count(&self) -> usize {
        let remaining = self.reader.pages.len().saturating_sub(self.current);
        if self.reader.pages[self.current].double_page || self.reader.pages.get(self.current + 1).is_some_and(|page| page.double_page) { 1 } else { self.columns.min(remaining).max(1) }
    }

    fn record_position(&self) {
        self.positions.record(StoredPosition::comic_page(self.current, self.offset, self.reader.pages.len(), self.visible_count()), self.navigation_revision);
    }

    fn clamp_position(&mut self) {
        if let Some(layout) = self.layout(self.current) {
            self.offset = self.offset.clamp(0.0, layout.max_offset);
            self.pan_x = layout.pan_x;
        }
    }

    fn go_to(&mut self, page: usize, cx: &mut Context<Self>) {
        let page = page.min(self.reader.pages.len().saturating_sub(1));
        if page == self.current {
            return;
        }
        self.current = page;
        self.offset = 0.0;
        self.pan_x = 0.0;
        self.navigation_revision += 1;
        self.record_position();
        self.evict_pages();
        self.request_pages(cx);
        cx.notify();
    }

    fn turn(&mut self, direction: i32, cx: &mut Context<Self>) {
        let previous = if self.columns == 2 && self.current >= 2 && !self.reader.pages[self.current].double_page && !self.reader.pages[self.current - 1].double_page && !self.reader.pages[self.current - 2].double_page { 2 } else { 1 };
        let page = if direction < 0 {
            self.current.saturating_sub(previous)
        } else if self.current + self.visible_count() < self.reader.pages.len() {
            self.current + self.visible_count()
        } else {
            self.current
        };
        self.go_to(page, cx);
    }

    fn scroll(&mut self, dx: f32, dy: f32, turn_at_edge: bool, cx: &mut Context<Self>) {
        let Some(layout) = self.layout(self.current) else { return };
        let offset = (self.offset - dy / layout.height).clamp(0.0, layout.max_offset);
        let page = self.pages.peek(self.current).expect("layout requires a loaded page");
        let pan = PageLayout::new((page.width, page.height), (f32::from(self.viewport.size.width) / self.visible_count() as f32, f32::from(self.viewport.size.height)), self.zoom, offset, self.pan_x - dx).pan_x;
        if offset == self.offset && pan == self.pan_x {
            if turn_at_edge && dy.abs() >= 1.0 {
                self.turn(if dy < 0.0 { 1 } else { -1 }, cx);
            }
            return;
        }
        self.offset = offset;
        self.pan_x = pan;
        self.record_position();
        cx.notify();
    }

    fn adjust_zoom(&mut self, factor: f32, cx: &mut Context<Self>) {
        if let Some(layout) = self.layout(self.current) {
            self.zoom = Zoom::Fixed((layout.scale * factor).clamp(0.01, 16.0));
            self.clamp_position();
            self.record_position();
            cx.notify();
        }
    }

    fn finish_swipe(&mut self, window: &Window, cx: &mut Context<Self>) {
        if let Some((dx, dy)) = self.swipe.finish() {
            if let Some(forward) = components::swipe_turns_forward_with_direction(gpui::point(dx, dy), window, self.right_to_left) {
                self.turn(if forward { 1 } else { -1 }, cx);
            }
        }
    }

    pub(crate) fn native_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sidebar.is_visible() && self.chrome.is_visible() {
            self.hide_sidebar(window, cx);
        } else {
            self.close_reader(window, cx);
        }
    }
}

impl Render for ComicReaderView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        components::sync_reader_rem_size(window, cx);
        crate::invalidation::record_render(Component::ComicShell);
        let theme = components::browser_theme(cx);
        let overlay = chrome::sidebar_overlays_document(window);
        let active_tab = if overlay { self.sidebar.effective(true) } else { Some(self.sidebar_tab()) };
        let active_tab = active_tab.map(|tab| if matches!(tab, SidebarTab::Contents | SidebarTab::Settings) { tab } else { SidebarTab::Contents });
        let visible = self.chrome.is_visible();
        let active_tab = active_tab.filter(|_| visible);
        self.chrome.sync_system_bars(visible, cx);
        let entity = cx.entity();
        let mut document = div().id("comic-document").size_full().relative().overflow_hidden().bg(gpui::rgb(0x242424)).track_focus(&self.focus).key_context("ComicReaderDocument");
        let visible_count = self.visible_count();
        let end = self.current + visible_count;
        for index in self.current..end {
            let slot = if self.right_to_left { visible_count - 1 - (index - self.current) } else { index - self.current };
            let cell_width = f32::from(self.viewport.size.width) / visible_count as f32;
            if let Some(layout) = self.layout(index) {
                if let Some(page) = self.pages.get(index) {
                    document = document.child(
                        div()
                            .absolute()
                            .left(px(slot as f32 * cell_width))
                            .top_0()
                            .w(px(cell_width))
                            .h_full()
                            .overflow_hidden()
                            .child(img(page.image.clone()).absolute().left(px(layout.left)).top(px(layout.top)).w(px(layout.width)).h(px(layout.height)).bg(gpui::rgb(0xffffff))),
                    );
                }
            } else {
                let message = self.errors.get(&index).map(|error| format!("Could not display page {}: {error}", index + 1)).unwrap_or_else(|| format!("Loading page {}…", index + 1));
                document = document.child(div().absolute().left(px(slot as f32 * cell_width)).top_0().w(px(cell_width)).h_full().flex().items_center().justify_center().text_color(gpui::rgb(0xffffff)).child(message).when(
                    self.errors.contains_key(&index),
                    |body| {
                        body.child(components::outlined_text_button(("comic-retry", index), "Retry").on_click(cx.listener(move |view, _, _, cx| {
                            view.errors.remove(&index);
                            view.request_pages(cx);
                        })))
                    },
                ));
            }
        }
        let measure_target = entity.clone();
        let document = document
            .on_prepaint(move |bounds, _, cx| {
                measure_target.update(cx, |view, cx| {
                    if view.viewport != bounds {
                        let previous = (view.offset, view.pan_x);
                        view.viewport = bounds;
                        view.clamp_position();
                        if previous != (view.offset, view.pan_x) {
                            view.record_position();
                        }
                        cx.notify();
                    }
                });
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, event: &gpui::MouseDownEvent, window, cx| {
                    view.focus.focus(window, cx);
                    view.drag = Some((event.position, view.offset, view.pan_x));
                    view.dragged = false;
                }),
            )
            .on_mouse_move(cx.listener(|view, event: &gpui::MouseMoveEvent, window, cx| {
                if !components::uses_mobile_reader_controls(window) && !view.focus.is_focused(window) {
                    view.focus.focus(window, cx);
                }
                let Some((start, offset, pan)) = view.drag else { return };
                if !event.dragging() {
                    return;
                }
                let (dx, dy) = (f32::from(event.position.x - start.x), f32::from(event.position.y - start.y));
                if dx.abs() + dy.abs() < 6.0 && !view.dragged {
                    return;
                }
                view.dragged = true;
                view.offset = offset;
                view.pan_x = pan;
                view.scroll(dx, dy, false, cx);
                cx.stop_propagation();
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|view, _, _, _| {
                    view.drag = None;
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|view, _, _, _| {
                    view.drag = None;
                }),
            )
            .on_click(cx.listener(|view, event: &gpui::ClickEvent, window, cx| {
                if view.dragged {
                    view.dragged = false;
                    return;
                }
                let fraction = f32::from(event.position().x - view.viewport.origin.x) / f32::from(view.viewport.size.width).max(1.0);
                if fraction < 0.25 || fraction > 0.75 {
                    let direction = if (fraction < 0.25) != view.right_to_left { -1 } else { 1 };
                    view.turn(direction, cx);
                    view.chrome.hide_for_motion(window, cx);
                } else {
                    if components::uses_mobile_reader_controls(window) && !view.chrome.is_visible() {
                        view.sidebar.close(cx);
                        view.chrome.show_from_touch(cx);
                    } else {
                        view.chrome.toggle(cx);
                    }
                }
            }))
            .on_scroll_wheel(cx.listener(|view, event: &gpui::ScrollWheelEvent, window, cx| {
                let (dx, dy) = match event.delta {
                    ScrollDelta::Pixels(delta) => (f32::from(delta.x), f32::from(delta.y)),
                    ScrollDelta::Lines(delta) => (delta.x * 32.0, delta.y * 32.0),
                    ScrollDelta::Pages(delta) => (delta.x * f32::from(view.viewport.size.width), delta.y * f32::from(view.viewport.size.height)),
                };
                if matches!(event.delta, ScrollDelta::Pages(_)) {
                    view.swipe.begin(false);
                    view.swipe.push(dx, dy);
                    view.finish_swipe(window, cx);
                } else if window.is_page_scroll_swipe() {
                    if matches!(event.touch_phase, gpui::TouchPhase::Started) {
                        view.swipe.begin(components::in_bottom_gesture_edge(event.position, window));
                    }
                    if matches!(event.touch_phase, gpui::TouchPhase::Cancelled) {
                        view.swipe.cancel();
                    }
                    if view.swipe.push(dx, dy).is_some() {
                        let previous = (view.offset, view.pan_x);
                        view.scroll(dx, dy, false, cx);
                        if previous != (view.offset, view.pan_x) {
                            view.swipe.mark_panned();
                        }
                    }
                    if matches!(event.touch_phase, gpui::TouchPhase::Ended) {
                        view.finish_swipe(window, cx);
                    }
                } else {
                    view.scroll(dx, dy, true, cx);
                }
                view.chrome.hide_for_motion(window, cx);
                cx.stop_propagation();
            }));
        let count = self.reader.pages.len();
        let percent = book_model::pdf_page_progress(self.current + visible_count - 1, count);
        let progress_label = format!("{percent:.0}%");
        let sidebar = active_tab.map(|tab| {
            let body = self.render_panel(tab, window, cx);
            let content = SidebarContent::new(percent / 100.0, progress_label.clone(), format!("{}/{count}", self.current + 1), Some(self.focus.clone()), body);
            toolbar::reader_sidebar(self, &entity, content, theme, window, cx)
        });
        let surface = ReaderSurface {
            document: components::reader_document_body().child(document).into_any_element(),
            dock: None,
            sidebar,
            active_tab,
            chrome_visible: visible,
            document_ready: true,
            loading_focus: self.focus.clone(),
            title: self.title.clone(),
            chapter: format!("Page {} of {count}", self.current + 1),
            progress_label,
            bottom_bar_id: "comic-reader-bottom-bar",
        };
        let touch = cx.supports_touch_input() || components::uses_mobile_reader_controls(window);
        let interactions = chrome::surface_interactions("comic", visible, active_tab.is_some(), touch, cx, |view: &mut Self, action, _, cx| match action {
            chrome::SurfaceAction::PreviousPage => view.turn(-1, cx),
            chrome::SurfaceAction::NextPage => view.turn(1, cx),
            chrome::SurfaceAction::ToggleChrome => view.chrome.toggle(cx),
            chrome::SurfaceAction::ShowChrome => {
                view.chrome.show_from_pointer(cx);
            }
        });
        crate::shell::page::reader_surface(self, surface, theme, window, cx).children(interactions)
    }
}
