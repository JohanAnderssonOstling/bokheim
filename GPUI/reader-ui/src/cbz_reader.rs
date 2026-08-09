use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use bokheim_ui_components as components;
use epub_svelte_lib::{BookLocator, LibraryBackend};
use gpui::prelude::*;
use gpui::{
    App, Bounds, ContentMask, Context, Corners, DispatchPhase, Element, ElementId, Entity, FocusHandle, Focusable, GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId, IntoElement, KeyBinding, KeyContext, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Render, RenderImage, ScrollDelta, ScrollWheelEvent, Style, Window, point, px, relative, size,
};
use image::Frame;
use smallvec::SmallVec;

use crate::epub::ReaderWindowId;
use crate::invalidation::Component;
use crate::settings::ReaderServices;
use crate::shell::persistence::PositionWriter;
use crate::{DecreaseScale, IncreaseScale, NextPage, PreviousPage};

const CACHE_BUDGET: usize = 192 * 1024 * 1024;
const PAGE_GAP: f32 = 12.0;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum CbzFitMode {
    #[default]
    Page,
    Width,
    ActualSize,
}

struct CachedPage {
    image: Arc<RenderImage>,
    byte_len: usize,
}

pub(crate) struct PendingCbzReader {
    pub(crate) id: ReaderWindowId,
    pub(crate) locator: BookLocator,
    pub(crate) title: String,
    pub(crate) library: LibraryBackend,
    pub(crate) publication: Arc<crate::publication::NativePublication>,
    pub(crate) initial_page: usize,
}

pub(crate) struct CbzReaderView {
    pub(crate) id: ReaderWindowId,
    pub(crate) locator: BookLocator,
    pub(crate) unavailable: bool,
    title: String,
    publication: Arc<crate::publication::NativePublication>,
    page_count: usize,
    current_page: usize,
    rtl: bool,
    spread: bool,
    fit_mode: CbzFitMode,
    zoom: f32,
    pan: (f32, f32),
    drag: Option<(Point<Pixels>, (f32, f32))>,
    viewport: Option<Bounds<Pixels>>,
    cache: HashMap<usize, CachedPage>,
    recency: VecDeque<usize>,
    cache_bytes: usize,
    loading: HashSet<usize>,
    error: Option<String>,
    positions: PositionWriter<usize>,
    focus: FocusHandle,
    _bounds_subscription: gpui::Subscription,
}

impl CbzReaderView {
    pub(crate) fn new(pending: PendingCbzReader, window: &mut Window, cx: &mut Context<Self>) -> Self {
        #[cfg(debug_assertions)]
        crate::invalidation::initialize_performance_measurements();
        let (page_count, rtl) = match &pending.publication.layout {
            html_publication::PublicationLayout::PagedImages { page_count, reading_direction, .. } => (*page_count, *reading_direction == html_publication::ReadingDirection::RightToLeft),
            html_publication::PublicationLayout::Reflowable => (pending.publication.document_uris.len(), false),
        };
        let bounds_subscription = crate::shell::geometry::observe(window, cx);
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let book_id = pending.locator.book_id();
        let positions = PositionWriter::new(
            cx,
            "persist CBZ page",
            move |library, page: &usize| {
                let progress = if page_count <= 1 { 0.0 } else { *page as f32 / (page_count - 1) as f32 };
                library.update_reading_position(book_id, &cbz_page_cfi(*page), progress).map_err(|error| error.to_string())
            },
            pending.library.clone(),
        );
        Self {
            id: pending.id,
            locator: pending.locator,
            unavailable: false,
            title: pending.title,
            publication: pending.publication,
            page_count,
            current_page: pending.initial_page.min(page_count.saturating_sub(1)),
            rtl,
            spread: false,
            fit_mode: CbzFitMode::Page,
            zoom: 1.0,
            pan: (0.0, 0.0),
            drag: None,
            viewport: None,
            cache: HashMap::new(),
            recency: VecDeque::new(),
            cache_bytes: 0,
            loading: HashSet::new(),
            error: None,
            positions,
            focus,
            _bounds_subscription: bounds_subscription,
        }
    }

    pub(crate) fn start(&mut self, cx: &mut Context<Self>) {
        self.ensure_pages(cx);
    }

    fn visible_pages(&self) -> Vec<usize> {
        if !self.spread || self.current_page == 0 {
            return (self.current_page < self.page_count).then_some(self.current_page).into_iter().collect();
        }
        let first = if self.current_page.is_multiple_of(2) { self.current_page - 1 } else { self.current_page };
        let mut pages = vec![first];
        if first + 1 < self.page_count {
            pages.push(first + 1);
        }
        if self.rtl {
            pages.reverse();
        }
        pages
    }

    fn wanted_pages(&self) -> Vec<usize> {
        let mut pages = self.visible_pages();
        let anchor = self.current_page;
        for distance in 1..=2 {
            if let Some(page) = anchor.checked_sub(distance) {
                pages.push(page);
            }
            if anchor + distance < self.page_count {
                pages.push(anchor + distance);
            }
        }
        pages.sort_unstable();
        pages.dedup();
        pages
    }

    fn ensure_pages(&mut self, cx: &mut Context<Self>) {
        let worker = ReaderServices::get(cx).blocking.clone();
        for page in self.wanted_pages() {
            if self.cache.contains_key(&page) || !self.loading.insert(page) {
                continue;
            }
            let Some(uri) = self.publication.document_uris.get(page).and_then(|document| document.rsplit_once('/').map(|_| format!("cbz/images/{page}"))) else {
                self.loading.remove(&page);
                continue;
            };
            let provider = self.publication.provider.clone();
            let result = worker.dispatch(move || {
                let bytes = provider.read_bytes(&uri).map_err(|error| error.to_string())?;
                let rgba = image::load_from_memory(&bytes).map_err(|error| error.to_string())?.to_rgba8();
                let byte_len = rgba.len();
                let image = Arc::new(RenderImage::new(SmallVec::from_vec(vec![Frame::new(rgba)])));
                Ok::<_, String>((page, image, byte_len))
            });
            cx.spawn(async move |this, cx| {
                let loaded = result.recv().await;
                let _ = this.update(cx, |this, cx| {
                    this.loading.remove(&page);
                    let should_notify = match loaded {
                        Ok(Ok((page, image, byte_len))) => {
                            let visible = this.visible_pages().contains(&page);
                            this.insert_page(page, image, byte_len);
                            visible
                        }
                        Ok(Err(error)) => {
                            this.error = Some(format!("Could not decode page {}: {error}", page + 1));
                            true
                        }
                        Err(_) => {
                            this.error = Some("CBZ image worker stopped".to_owned());
                            true
                        }
                    };
                    if should_notify {
                        crate::invalidation::notify(cx, Component::CbzShell, "visible_page_ready");
                    }
                });
            })
            .detach();
        }
    }

    fn insert_page(&mut self, page: usize, image: Arc<RenderImage>, byte_len: usize) {
        if let Some(old) = self.cache.remove(&page) {
            self.cache_bytes = self.cache_bytes.saturating_sub(old.byte_len);
        }
        self.cache.insert(page, CachedPage { image, byte_len });
        self.cache_bytes = self.cache_bytes.saturating_add(byte_len);
        self.recency.retain(|candidate| *candidate != page);
        self.recency.push_back(page);
        let protected = self.wanted_pages().into_iter().collect::<HashSet<_>>();
        while self.cache_bytes > CACHE_BUDGET {
            let Some(candidate) = self.recency.pop_front() else { break };
            if protected.contains(&candidate) {
                self.recency.push_back(candidate);
                if self.recency.iter().all(|page| protected.contains(page)) {
                    break;
                }
                continue;
            }
            if let Some(removed) = self.cache.remove(&candidate) {
                self.cache_bytes = self.cache_bytes.saturating_sub(removed.byte_len);
            }
        }
    }

    fn set_page(&mut self, page: usize, cx: &mut Context<Self>) {
        let page = page.min(self.page_count.saturating_sub(1));
        if page == self.current_page {
            return;
        }
        self.current_page = page;
        self.pan = (0.0, 0.0);
        self.error = None;
        self.positions.record(page, "CBZ page");
        self.ensure_pages(cx);
        crate::invalidation::notify(cx, Component::CbzShell, "state_changed");
    }

    fn previous_page(&mut self, cx: &mut Context<Self>) {
        let step = if self.spread && self.current_page > 0 { 2 } else { 1 };
        self.set_page(self.current_page.saturating_sub(step), cx);
    }

    fn next_page(&mut self, cx: &mut Context<Self>) {
        let step = if self.spread && self.current_page > 0 { 2 } else { 1 };
        self.set_page(self.current_page.saturating_add(step), cx);
    }

    fn change_zoom(&mut self, delta: f32, anchor: Option<Point<Pixels>>, cx: &mut Context<Self>) {
        let old = self.zoom;
        let zoom = (self.zoom * (1.0 + delta)).clamp(0.25, 8.0);
        if zoom == old {
            return;
        }
        self.zoom = zoom;
        if let (Some(anchor), Some(viewport)) = (anchor, self.viewport) {
            let center = point(viewport.origin.x + viewport.size.width / 2.0, viewport.origin.y + viewport.size.height / 2.0);
            let ratio = self.zoom / old;
            self.pan.0 = f32::from(anchor.x - center.x) * (1.0 - ratio) + self.pan.0 * ratio;
            self.pan.1 = f32::from(anchor.y - center.y) * (1.0 - ratio) + self.pan.1 * ratio;
        }
        self.clamp_pan();
        crate::invalidation::notify(cx, Component::CbzShell, "state_changed");
    }

    fn reset_view(&mut self, cx: &mut Context<Self>) {
        if self.zoom == 1.0 && self.pan == (0.0, 0.0) {
            return;
        }
        self.zoom = 1.0;
        self.pan = (0.0, 0.0);
        crate::invalidation::notify(cx, Component::CbzShell, "state_changed");
    }

    fn set_fit_mode(&mut self, fit_mode: CbzFitMode, cx: &mut Context<Self>) {
        if self.fit_mode == fit_mode {
            return;
        }
        self.fit_mode = fit_mode;
        self.zoom = 1.0;
        self.pan = (0.0, 0.0);
        crate::invalidation::notify(cx, Component::CbzShell, "fit_mode_changed");
    }

    fn set_spread(&mut self, spread: bool, cx: &mut Context<Self>) {
        if self.spread == spread {
            return;
        }
        self.spread = spread;
        self.zoom = 1.0;
        self.pan = (0.0, 0.0);
        self.ensure_pages(cx);
        crate::invalidation::notify(cx, Component::CbzShell, "spread_changed");
    }

    fn clamp_pan(&mut self) {
        let Some(viewport) = self.viewport else { return };
        let Some((width, height)) = self.content_size(viewport) else { return };
        let max_x = ((width - f32::from(viewport.size.width)) / 2.0).max(0.0);
        let max_y = ((height - f32::from(viewport.size.height)) / 2.0).max(0.0);
        self.pan.0 = self.pan.0.clamp(-max_x, max_x);
        self.pan.1 = self.pan.1.clamp(-max_y, max_y);
    }

    fn natural_size(&self) -> Option<(f32, f32)> {
        let pages = self.visible_pages();
        let mut width = 0.0;
        let mut height: f32 = 0.0;
        for (position, page) in pages.iter().enumerate() {
            let image = &self.cache.get(page)?.image;
            let image_size = image.size(0);
            width += i32::from(image_size.width) as f32;
            height = height.max(i32::from(image_size.height) as f32);
            if position > 0 {
                width += PAGE_GAP
            }
        }
        Some((width.max(1.0), height.max(1.0)))
    }

    fn content_size(&self, viewport: Bounds<Pixels>) -> Option<(f32, f32)> {
        let (width, height) = self.natural_size()?;
        let available_width = f32::from(viewport.size.width).max(1.0);
        let available_height = f32::from(viewport.size.height).max(1.0);
        let fitted = match self.fit_mode {
            CbzFitMode::Page => (available_width / width).min(available_height / height),
            CbzFitMode::Width => available_width / width,
            CbzFitMode::ActualSize => 1.0,
        };
        let scale = fitted * self.zoom;
        Some((width * scale, height * scale))
    }

    fn paint_pages(&self, viewport: Bounds<Pixels>) -> Vec<(Arc<RenderImage>, Bounds<Pixels>)> {
        let pages = self.visible_pages();
        let Some((natural_width, natural_height)) = self.natural_size() else { return Vec::new() };
        let available_width = f32::from(viewport.size.width).max(1.0);
        let available_height = f32::from(viewport.size.height).max(1.0);
        let fitted = match self.fit_mode {
            CbzFitMode::Page => (available_width / natural_width).min(available_height / natural_height),
            CbzFitMode::Width => available_width / natural_width,
            CbzFitMode::ActualSize => 1.0,
        };
        let scale = fitted * self.zoom;
        let total_width = natural_width * scale;
        let mut x = viewport.origin.x + px((available_width - total_width) / 2.0 + self.pan.0);
        let center_y = viewport.origin.y + viewport.size.height / 2.0 + px(self.pan.1);
        let mut result = Vec::new();
        for page in pages {
            let Some(cached) = self.cache.get(&page) else { return Vec::new() };
            let image_size = cached.image.size(0);
            let width = i32::from(image_size.width) as f32 * scale;
            let height = i32::from(image_size.height) as f32 * scale;
            let bounds = Bounds { origin: point(x, center_y - px(height / 2.0)), size: size(px(width), px(height)) };
            result.push((cached.image.clone(), bounds));
            x += px(width + PAGE_GAP * scale);
        }
        result
    }

    fn render_settings(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        components::reader_settings_dropdown()
            .child(components::reader_settings_title())
            .child(
                components::reader_setting_row("Fit")
                    .child(components::reader_choice_button("cbz-fit-page", "Page", self.fit_mode == CbzFitMode::Page).on_click(cx.listener(|this, _, _, cx| this.set_fit_mode(CbzFitMode::Page, cx))))
                    .child(components::reader_choice_button("cbz-fit-width", "Width", self.fit_mode == CbzFitMode::Width).on_click(cx.listener(|this, _, _, cx| this.set_fit_mode(CbzFitMode::Width, cx))))
                    .child(components::reader_choice_button("cbz-fit-actual", "100%", self.fit_mode == CbzFitMode::ActualSize).on_click(cx.listener(|this, _, _, cx| this.set_fit_mode(CbzFitMode::ActualSize, cx)))),
            )
            .child(
                components::reader_setting_row("Pages")
                    .child(components::reader_choice_button("cbz-single", "Single", !self.spread).on_click(cx.listener(|this, _, _, cx| this.set_spread(false, cx))))
                    .child(components::reader_choice_button("cbz-spread", "Spread", self.spread).on_click(cx.listener(|this, _, _, cx| this.set_spread(true, cx)))),
            )
            .child(
                components::reader_setting_row("Zoom")
                    .child(components::reader_square_button("cbz-zoom-out", "−").on_click(cx.listener(|this, _, _, cx| this.change_zoom(-0.1, None, cx))))
                    .child(components::reader_setting_value(format!("{:.0}%", self.zoom * 100.0)))
                    .child(components::reader_square_button("cbz-zoom-in", "+").on_click(cx.listener(|this, _, _, cx| this.change_zoom(0.1, None, cx))))
                    .child(components::reader_choice_button("cbz-reset", "Reset", false).on_click(cx.listener(|this, _, _, cx| this.reset_view(cx)))),
            )
            .into_any_element()
    }

    fn on_previous_action(&mut self, _: &PreviousPage, _: &mut Window, cx: &mut Context<Self>) {
        if self.rtl { self.next_page(cx) } else { self.previous_page(cx) }
    }

    fn on_next_action(&mut self, _: &NextPage, _: &mut Window, cx: &mut Context<Self>) {
        if self.rtl { self.previous_page(cx) } else { self.next_page(cx) }
    }

    fn on_increase_scale(&mut self, _: &IncreaseScale, _: &mut Window, cx: &mut Context<Self>) {
        self.change_zoom(0.1, None, cx)
    }
    fn on_decrease_scale(&mut self, _: &DecreaseScale, _: &mut Window, cx: &mut Context<Self>) {
        self.change_zoom(-0.1, None, cx)
    }
}

impl Focusable for CbzReaderView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for CbzReaderView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::invalidation::record_render(Component::CbzShell);
        window.set_window_title(&self.title);
        let entity = cx.entity();
        let can_previous = self.current_page > 0;
        let can_next = self.current_page + 1 < self.page_count;
        let previous_arrow = if self.rtl { "→" } else { "←" };
        let next_arrow = if self.rtl { "←" } else { "→" };
        let settings = self.render_settings(cx);
        let toolbar = components::reader_toolbar()
            .child(components::reader_control_button_state("cbz-previous-page", previous_arrow, can_previous).tooltip("Previous page").on_click(cx.listener(|this, _, _, cx| this.previous_page(cx))))
            .child(components::reader_match_count(format!("{}/{}", self.current_page.saturating_add(1), self.page_count)))
            .child(components::reader_control_button_state("cbz-next-page", next_arrow, can_next).tooltip("Next page").on_click(cx.listener(|this, _, _, cx| this.next_page(cx))))
            .child(components::reader_title(self.title.clone()))
            .child(components::reader_settings_popover().child(settings));
        components::page()
            .key_context("CbzReaderDocument")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::on_previous_action))
            .on_action(cx.listener(Self::on_next_action))
            .on_action(cx.listener(Self::on_increase_scale))
            .on_action(cx.listener(Self::on_decrease_scale))
            .child(toolbar)
            .child(components::reader_content().child(CbzCanvasElement { view: entity }))
    }
}

struct CbzCanvasElement {
    view: Entity<CbzReaderView>,
}
struct CbzCanvasPrepaint {
    hitbox: Hitbox,
    pages: Vec<(Arc<RenderImage>, Bounds<Pixels>)>,
}

impl IntoElement for CbzCanvasElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for CbzCanvasElement {
    type RequestLayoutState = ();
    type PrepaintState = CbzCanvasPrepaint;
    fn id(&self) -> Option<ElementId> {
        Some("gpui-cbz-canvas".into())
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }
    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = relative(1.0).into();
        (window.request_layout(style, [], cx), ())
    }
    fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _: &mut (), window: &mut Window, cx: &mut App) -> CbzCanvasPrepaint {
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        let pages = self.view.update(cx, |view, _| {
            view.viewport = Some(bounds);
            view.clamp_pan();
            view.paint_pages(bounds)
        });
        CbzCanvasPrepaint { hitbox, pages }
    }
    fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _: &mut (), prepaint: &mut CbzCanvasPrepaint, window: &mut Window, _: &mut App) {
        window.paint_quad(gpui::fill(bounds, gpui::rgb(0x17191c)));
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for (image, page_bounds) in &prepaint.pages {
                let _ = window.paint_image(*page_bounds, Corners::default(), image.clone(), 0, false);
            }
        });
        let mut key_context = KeyContext::default();
        key_context.add("CbzReaderDocument");
        window.set_key_context(key_context);
        let hitbox = prepaint.hitbox.clone();
        let down = self.view.clone();
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || event.button != MouseButton::Left || !hitbox.is_hovered(window) {
                return;
            }
            down.update(cx, |view, cx| {
                view.focus.focus(window, cx);
                view.drag = Some((event.position, view.pan));
                cx.stop_propagation();
            });
        });
        let moved = self.view.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
            if phase != DispatchPhase::Bubble || !event.dragging() {
                return;
            }
            moved.update(cx, |view, cx| {
                let Some((anchor, start)) = view.drag else { return };
                view.pan = (start.0 + f32::from(event.position.x - anchor.x), start.1 + f32::from(event.position.y - anchor.y));
                view.clamp_pan();
                crate::invalidation::notify(cx, Component::CbzShell, "state_changed");
                cx.stop_propagation();
            });
        });
        let up = self.view.clone();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
            if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                return;
            }
            up.update(cx, |view, cx| {
                if view.drag.take().is_some() {
                    cx.stop_propagation();
                }
            });
        });
        let hitbox = prepaint.hitbox.clone();
        let scroll = self.view.clone();
        window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !hitbox.is_hovered(window) {
                return;
            }
            scroll.update(cx, |view, cx| {
                let (x, y) = match event.delta {
                    ScrollDelta::Pixels(delta) => (f32::from(delta.x), f32::from(delta.y)),
                    ScrollDelta::Lines(delta) => (delta.x * 24.0, delta.y * 24.0),
                };
                if event.modifiers.control || event.modifiers.platform {
                    view.change_zoom(if y < 0.0 { 0.1 } else { -0.1 }, Some(event.position), cx);
                } else {
                    view.pan.0 += x;
                    view.pan.1 += y;
                    view.clamp_pan();
                    crate::invalidation::notify(cx, Component::CbzShell, "state_changed");
                }
                cx.stop_propagation();
            });
        });
    }
}

pub(crate) fn cbz_page_from_cfi(value: Option<&str>, page_count: usize) -> usize {
    let Some(value) = value else { return 0 };
    let Some(start) = value.find("/6/") else { return 0 };
    let digits = value[start + 3..].chars().take_while(char::is_ascii_digit).collect::<String>();
    digits.parse::<usize>().ok().and_then(|step| step.checked_div(2)).and_then(|page| page.checked_sub(1)).unwrap_or(0).min(page_count.saturating_sub(1))
}

fn cbz_page_cfi(page: usize) -> String {
    format!("epubcfi(/6/{})", page.saturating_add(1).saturating_mul(2))
}

pub(crate) fn bind_cbz_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("left", PreviousPage, Some("CbzReaderDocument")),
        KeyBinding::new("right", NextPage, Some("CbzReaderDocument")),
        KeyBinding::new("ctrl-=", IncreaseScale, Some("CbzReaderDocument")),
        KeyBinding::new("ctrl--", DecreaseScale, Some("CbzReaderDocument")),
    ]);
}

#[cfg(test)]
mod tests {
    use super::{cbz_page_cfi, cbz_page_from_cfi};
    #[test]
    fn cbz_page_locators_are_stable_and_bounded() {
        assert_eq!(cbz_page_cfi(4), "epubcfi(/6/10)");
        assert_eq!(cbz_page_from_cfi(Some("epubcfi(/6/10)"), 20), 4);
        assert_eq!(cbz_page_from_cfi(Some("epubcfi(/6/200)"), 8), 7);
    }
}
