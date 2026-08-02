use bokheim_ui_components as components;
use gpui::prelude::*;
use gpui::{
    App, Bounds, ClipboardItem, Context, Corners, CursorStyle, DispatchPhase, Element, ElementId, Entity, EventEmitter, FocusHandle, GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId, IntoElement, KeyContext, KeyDownEvent,
    LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Render, RenderImage, ScrollHandle, Style, Window, fill, point, px, rgba, size,
};
use image::{Frame, RgbaImage};
use pdf_reader_core::{NormalizedRect, PageRenderKey, PdfDocumentInfo, PdfDocumentSession, PdfPageTextLayout, PdfSearchHit, PdfTextSelection, ReaderInteractionStyle};
use smallvec::SmallVec;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const DISPLAY_CACHE_BUDGET: usize = 128 * 1024 * 1024;
const DEFAULT_RENDER_WIDTH: u16 = 1400;
const DEFAULT_DISPLAY_WIDTH: f32 = 900.0;

#[derive(Clone, Debug)]
pub enum PdfViewEvent {
    PageChanged { current: usize, total: usize },
    SearchChanged { current: usize, total: usize },
    LoadFailed(String),
}

pub struct PdfView {
    session: Option<Arc<Mutex<PdfDocumentSession>>>,
    info: Option<PdfDocumentInfo>,
    page: Option<Arc<DisplayPage>>,
    display_cache: DisplayPageCache,
    current_page: usize,
    target_width: u16,
    display_scale: f32,
    load_generation: u64,
    render_generation: u64,
    pending_render: Option<(u64, PageRenderKey)>,
    search_generation: u64,
    search_in_flight: bool,
    pending_search: Option<(u64, String)>,
    loading: bool,
    rendering: bool,
    error: Option<String>,
    selection_anchor: Option<(f32, f32)>,
    selection_active: Option<(f32, f32)>,
    selection: Option<PdfTextSelection>,
    search_hits: Vec<PdfSearchHit>,
    active_search_hit: Option<usize>,
    pending_search_scroll: Option<NormalizedRect>,
    interaction_style: ReaderInteractionStyle,
    scroll_handle: ScrollHandle,
    focus: FocusHandle,
}

struct DisplayPage {
    image: Arc<RenderImage>,
    text_layout: Arc<PdfPageTextLayout>,
    byte_len: usize,
}

struct DisplayPageCache {
    byte_budget: usize,
    byte_len: usize,
    pages: HashMap<PageRenderKey, Arc<DisplayPage>>,
    recency: VecDeque<PageRenderKey>,
}

struct PdfPageElement {
    view: Entity<PdfView>,
    focus: FocusHandle,
    page: Arc<DisplayPage>,
    selection: Vec<NormalizedRect>,
    search_hits: Vec<(NormalizedRect, bool)>,
    interaction_style: ReaderInteractionStyle,
    display_scale: f32,
}

struct PdfPagePrepaint {
    hitbox: Hitbox,
    page_bounds: Bounds<Pixels>,
}

impl EventEmitter<PdfViewEvent> for PdfView {}

impl PdfView {
    pub fn from_bytes(display_path: PathBuf, bytes: Vec<u8>, initial_page: usize, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let mut view = Self {
            session: None,
            info: None,
            page: None,
            display_cache: DisplayPageCache::new(DISPLAY_CACHE_BUDGET),
            current_page: initial_page,
            target_width: DEFAULT_RENDER_WIDTH,
            display_scale: 1.0,
            load_generation: 1,
            render_generation: 0,
            pending_render: None,
            search_generation: 0,
            search_in_flight: false,
            pending_search: None,
            loading: true,
            rendering: false,
            error: None,
            selection_anchor: None,
            selection_active: None,
            selection: None,
            search_hits: Vec::new(),
            active_search_hit: None,
            pending_search_scroll: None,
            interaction_style: ReaderInteractionStyle::default(),
            scroll_handle: ScrollHandle::new(),
            focus,
        };
        view.start_load(display_path, bytes, cx);
        view
    }

    pub fn info(&self) -> Option<&PdfDocumentInfo> {
        self.info.as_ref()
    }

    pub fn current_page(&self) -> usize {
        self.current_page
    }

    pub fn page_count(&self) -> usize {
        self.info.as_ref().map(PdfDocumentInfo::page_count).unwrap_or(0)
    }

    pub fn next_page(&mut self, cx: &mut Context<Self>) {
        let page_count = self.page_count();
        if self.current_page + 1 >= page_count {
            return;
        }
        self.current_page += 1;
        self.reset_page_scroll();
        self.clear_selection();
        self.request_current_page(cx);
    }

    pub fn previous_page(&mut self, cx: &mut Context<Self>) {
        if self.current_page == 0 {
            return;
        }
        self.current_page -= 1;
        self.reset_page_scroll();
        self.clear_selection();
        self.request_current_page(cx);
    }

    pub fn set_page(&mut self, page_index: usize, cx: &mut Context<Self>) {
        if page_index >= self.page_count() || page_index == self.current_page {
            return;
        }
        self.current_page = page_index;
        self.reset_page_scroll();
        self.clear_selection();
        self.request_current_page(cx);
    }

    pub fn set_render_width(&mut self, target_width: u16, cx: &mut Context<Self>) {
        let target_width = PageRenderKey::new(0, target_width).target_width();
        if self.target_width == target_width {
            return;
        }
        self.target_width = target_width;
        self.request_current_page(cx);
    }

    pub fn display_scale(&self) -> f32 {
        self.display_scale
    }

    pub fn set_display_scale(&mut self, scale: f32, cx: &mut Context<Self>) {
        let scale = scale.clamp(0.5, 2.5);
        if (self.display_scale - scale).abs() < f32::EPSILON {
            return;
        }
        self.display_scale = scale;
        let render_width = (DEFAULT_RENDER_WIDTH as f32 * scale).round() as u16;
        self.set_render_width(render_width, cx);
        cx.notify();
    }

    pub fn change_display_scale(&mut self, delta: f32, cx: &mut Context<Self>) {
        self.set_display_scale(self.display_scale + delta, cx);
    }

    pub fn set_interaction_style(&mut self, style: ReaderInteractionStyle, cx: &mut Context<Self>) {
        if self.interaction_style == style {
            return;
        }
        self.interaction_style = style;
        cx.notify();
    }

    pub fn search(&mut self, query: String, cx: &mut Context<Self>) {
        self.search_generation = self.search_generation.wrapping_add(1);
        let generation = self.search_generation;
        if query.trim().is_empty() {
            self.pending_search = None;
            self.search_hits.clear();
            self.active_search_hit = None;
            self.pending_search_scroll = None;
            cx.emit(PdfViewEvent::SearchChanged { current: 0, total: 0 });
            cx.notify();
            return;
        }
        if self.search_in_flight || self.session.is_none() {
            self.pending_search = Some((generation, query));
            return;
        }
        self.start_search(generation, query, cx);
    }

    fn start_search(&mut self, generation: u64, query: String, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            self.pending_search = Some((generation, query));
            return;
        };
        self.search_in_flight = true;
        let work = cx.background_executor().spawn(async move { session.lock().unwrap_or_else(std::sync::PoisonError::into_inner).search(&query) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.search_in_flight = false;
                if this.search_generation == generation {
                    match result {
                        Ok(hits) => {
                            this.search_hits = hits;
                            this.active_search_hit = (!this.search_hits.is_empty()).then_some(0);
                            if let Some(hit) = this.search_hits.first() {
                                this.pending_search_scroll = Some(hit.bounds());
                                if this.current_page != hit.page_index() {
                                    this.current_page = hit.page_index();
                                    this.reset_page_scroll();
                                }
                                this.clear_selection();
                                this.request_current_page(cx);
                            }
                            cx.emit(PdfViewEvent::SearchChanged { current: usize::from(!this.search_hits.is_empty()), total: this.search_hits.len() });
                        }
                        Err(error) => {
                            this.error = Some(error.to_string());
                        }
                    }
                }
                if let Some((next_generation, next_query)) = this.pending_search.take().filter(|(next_generation, _)| *next_generation == this.search_generation) {
                    this.start_search(next_generation, next_query, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn navigate_search(&mut self, step: i8, cx: &mut Context<Self>) {
        if self.search_hits.is_empty() {
            return;
        }
        let current = self.active_search_hit.unwrap_or(0);
        let next = if step < 0 { (current + self.search_hits.len() - 1) % self.search_hits.len() } else { (current + 1) % self.search_hits.len() };
        self.active_search_hit = Some(next);
        let hit = self.search_hits[next];
        self.pending_search_scroll = Some(hit.bounds());
        if self.current_page != hit.page_index() {
            self.current_page = hit.page_index();
            self.reset_page_scroll();
        }
        self.clear_selection();
        self.request_current_page(cx);
        cx.emit(PdfViewEvent::SearchChanged { current: next + 1, total: self.search_hits.len() });
    }

    pub fn copy_selection(&self, cx: &mut Context<Self>) -> bool {
        let Some(selection) = &self.selection else {
            return false;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(selection.text().to_owned()));
        true
    }

    pub fn focus_handle(&self) -> FocusHandle {
        self.focus.clone()
    }

    fn start_load(&mut self, display_path: PathBuf, bytes: Vec<u8>, cx: &mut Context<Self>) {
        let generation = self.load_generation;
        let work = cx.background_executor().spawn(async move { PdfDocumentSession::from_bytes(display_path, bytes) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                if this.load_generation != generation {
                    return;
                }
                this.loading = false;
                match result {
                    Ok(session) => {
                        this.info = Some(session.info().clone());
                        this.session = Some(Arc::new(Mutex::new(session)));
                        this.current_page = this.current_page.min(this.page_count().saturating_sub(1));
                        this.request_current_page(cx);
                        if let Some((search_generation, query)) = this.pending_search.take().filter(|(search_generation, _)| *search_generation == this.search_generation) {
                            this.start_search(search_generation, query, cx);
                        }
                    }
                    Err(error) => {
                        let message = error.to_string();
                        this.error = Some(message.clone());
                        cx.emit(PdfViewEvent::LoadFailed(message));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn request_current_page(&mut self, cx: &mut Context<Self>) {
        self.render_generation = self.render_generation.wrapping_add(1);
        let generation = self.render_generation;
        let key = PageRenderKey::new(self.current_page, self.target_width);
        if let Some(page) = self.display_cache.get(key) {
            self.page = Some(page);
            self.pending_render = None;
            self.reveal_pending_search();
            self.emit_page_changed(cx);
            cx.notify();
            return;
        }
        self.page = None;
        self.error = None;
        if self.rendering {
            self.pending_render = Some((generation, key));
            cx.notify();
            return;
        }
        self.start_render(generation, key, cx);
    }

    fn start_render(&mut self, generation: u64, key: PageRenderKey, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        self.rendering = true;
        let work = cx.background_executor().spawn(async move { session.lock().unwrap_or_else(std::sync::PoisonError::into_inner).render_page(key.page_index(), Some(key.target_width())) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.rendering = false;
                if this.render_generation == generation {
                    match result {
                        Ok(rendered) => match DisplayPage::from_rendered(rendered) {
                            Ok(page) => {
                                let page = Arc::new(page);
                                this.display_cache.insert(key, page.clone());
                                this.page = Some(page);
                                this.reveal_pending_search();
                                this.emit_page_changed(cx);
                            }
                            Err(error) => {
                                this.error = Some(error);
                            }
                        },
                        Err(error) => {
                            this.error = Some(error.to_string());
                        }
                    }
                }
                if let Some((next_generation, next_key)) = this.pending_render.take().filter(|(next_generation, _)| *next_generation == this.render_generation) {
                    this.start_render(next_generation, next_key, cx);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn emit_page_changed(&self, cx: &mut Context<Self>) {
        cx.emit(PdfViewEvent::PageChanged { current: self.current_page + 1, total: self.page_count() });
    }

    fn clear_selection(&mut self) {
        self.selection_anchor = None;
        self.selection_active = None;
        self.selection = None;
    }

    fn reset_page_scroll(&self) {
        self.scroll_handle.set_offset(point(Pixels::ZERO, Pixels::ZERO));
    }

    fn reveal_pending_search(&mut self) {
        let Some(hit) = self.pending_search_scroll.take() else {
            return;
        };
        let Some(page) = &self.page else {
            self.pending_search_scroll = Some(hit);
            return;
        };
        let image_size = page.image.size(0);
        let aspect = i32::from(image_size.height) as f32 / (i32::from(image_size.width) as f32).max(1.0);
        let page_height = DEFAULT_DISPLAY_WIDTH * self.display_scale * aspect;
        let viewport_height = f32::from(self.scroll_handle.bounds().size.height);
        let target = (24.0 + hit.top() * page_height - viewport_height * 0.3).max(0.0);
        let maximum = f32::from(self.scroll_handle.max_offset().y);
        let target = if maximum > 0.0 { target.min(maximum) } else { target };
        let current = self.scroll_handle.offset();
        self.scroll_handle.set_offset(point(current.x, px(-target)));
    }

    fn begin_selection(&mut self, position: (f32, f32), cx: &mut Context<Self>) {
        self.selection_anchor = Some(position);
        self.selection_active = Some(position);
        self.update_selection();
        cx.notify();
    }

    fn update_selection_to(&mut self, position: (f32, f32), cx: &mut Context<Self>) {
        if self.selection_anchor.is_none() {
            return;
        }
        self.selection_active = Some(position);
        self.update_selection();
        cx.notify();
    }

    fn update_selection(&mut self) {
        self.selection = self.page.as_ref().and_then(|page| Some((page, self.selection_anchor?, self.selection_active?))).and_then(|(page, anchor, active)| page.text_layout.select(anchor, active));
    }
}

impl DisplayPage {
    fn from_rendered(page: pdf_reader_core::RenderedPdfPage) -> Result<Self, String> {
        let (rgba, width, height, text_layout) = page.into_parts();
        let byte_len = rgba.len();
        let image = RgbaImage::from_raw(width, height, rgba).ok_or_else(|| "PDF renderer returned an invalid RGBA buffer".to_owned())?;
        Ok(Self { image: Arc::new(RenderImage::new(SmallVec::from_vec(vec![Frame::new(image)]))), text_layout, byte_len })
    }
}

impl DisplayPageCache {
    fn new(byte_budget: usize) -> Self {
        Self { byte_budget, byte_len: 0, pages: HashMap::new(), recency: VecDeque::new() }
    }

    fn get(&mut self, key: PageRenderKey) -> Option<Arc<DisplayPage>> {
        let page = self.pages.get(&key)?.clone();
        self.promote(key);
        Some(page)
    }

    fn insert(&mut self, key: PageRenderKey, page: Arc<DisplayPage>) {
        if page.byte_len > self.byte_budget {
            return;
        }
        if let Some(previous) = self.pages.remove(&key) {
            self.byte_len = self.byte_len.saturating_sub(previous.byte_len);
            self.recency.retain(|candidate| *candidate != key);
        }
        while self.byte_len + page.byte_len > self.byte_budget {
            let Some(oldest) = self.recency.pop_front() else {
                break;
            };
            if let Some(evicted) = self.pages.remove(&oldest) {
                self.byte_len = self.byte_len.saturating_sub(evicted.byte_len);
            }
        }
        self.byte_len += page.byte_len;
        self.pages.insert(key, page);
        self.recency.push_back(key);
    }

    fn promote(&mut self, key: PageRenderKey) {
        self.recency.retain(|candidate| *candidate != key);
        self.recency.push_back(key);
    }
}

impl IntoElement for PdfPageElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for PdfPageElement {
    type RequestLayoutState = ();
    type PrepaintState = PdfPagePrepaint;

    fn id(&self) -> Option<ElementId> {
        Some("gpui-pdf-page".into())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        let image_size = self.page.image.size(0);
        let aspect = i32::from(image_size.height) as f32 / (i32::from(image_size.width) as f32).max(1.0);
        style.size.width = px(DEFAULT_DISPLAY_WIDTH * self.display_scale).into();
        style.size.height = px(DEFAULT_DISPLAY_WIDTH * self.display_scale * aspect).into();
        style.flex_shrink = 0.0;
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _: &mut Self::RequestLayoutState, window: &mut Window, cx: &mut App) -> Self::PrepaintState {
        window.set_focus_handle(&self.focus, cx);
        let image_size = self.page.image.size(0);
        let available_width = f32::from(bounds.size.width).max(1.0);
        let available_height = f32::from(bounds.size.height).max(1.0);
        let image_width = i32::from(image_size.width) as f32;
        let image_height = i32::from(image_size.height) as f32;
        let scale = (available_width / image_width).min(available_height / image_height).max(0.01);
        let page_size = size(px(image_width * scale), px(image_height * scale));
        let page_bounds = Bounds { origin: point(bounds.origin.x + (bounds.size.width - page_size.width) / 2.0, bounds.origin.y + (bounds.size.height - page_size.height) / 2.0), size: page_size };
        let hitbox = window.insert_hitbox(page_bounds, HitboxBehavior::Normal);
        PdfPagePrepaint { hitbox, page_bounds }
    }

    fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, _: Bounds<Pixels>, _: &mut Self::RequestLayoutState, prepaint: &mut Self::PrepaintState, window: &mut Window, _cx: &mut App) {
        let _ = window.paint_image(prepaint.page_bounds, Corners::default(), self.page.image.clone(), 0, false);
        for (rect, active) in &self.search_hits {
            let color = if *active { self.interaction_style.active_search_match } else { self.interaction_style.search_match };
            window.paint_quad(fill(normalized_bounds(*rect, prepaint.page_bounds), rgba(color.packed_rgba())));
        }
        for rect in &self.selection {
            let color = self.interaction_style.selection;
            window.paint_quad(fill(normalized_bounds(*rect, prepaint.page_bounds), rgba(color.packed_rgba())));
        }
        window.set_cursor_style(CursorStyle::IBeam, &prepaint.hitbox);

        let mut key_context = KeyContext::default();
        key_context.add("PdfReaderDocument");
        window.set_key_context(key_context);

        let hitbox = prepaint.hitbox.clone();
        let page_bounds = prepaint.page_bounds;
        let mouse_down_target = self.view.clone();
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || event.button != MouseButton::Left || !hitbox.is_hovered(window) {
                return;
            }
            mouse_down_target.update(cx, |view, cx| {
                view.focus.focus(window, cx);
                view.begin_selection(normalized_point(event.position, page_bounds), cx);
                cx.stop_propagation();
            });
        });

        let page_bounds = prepaint.page_bounds;
        let mouse_move_target = self.view.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
            if phase != DispatchPhase::Bubble || !event.dragging() {
                return;
            }
            mouse_move_target.update(cx, |view, cx| {
                view.update_selection_to(normalized_point(event.position, page_bounds), cx);
                cx.stop_propagation();
            });
        });

        let mouse_up_target = self.view.clone();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
            if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                return;
            }
            mouse_up_target.update(cx, |view, cx| {
                if view.selection_anchor.take().is_some() {
                    view.selection_active = None;
                    cx.stop_propagation();
                }
            });
        });

        let focus = self.focus.clone();
        let key_target = self.view.clone();
        window.on_key_event(move |event: &KeyDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !focus.is_focused(window) {
                return;
            }
            let modifiers = event.keystroke.modifiers;
            if (modifiers.control || modifiers.platform) && event.keystroke.key.as_str() == "c" {
                key_target.update(cx, |view, cx| {
                    if view.copy_selection(cx) {
                        cx.stop_propagation();
                    }
                });
            }
        });
    }
}

impl Render for PdfView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(page) = self.page.clone() else {
            let message = self.error.clone().unwrap_or_else(|| if self.loading { "Opening PDF…".to_owned() } else { "Rendering page…".to_owned() });
            return components::reader_document_message(message).into_any_element();
        };
        let selection = self.selection.as_ref().map(|selection| selection.rects().to_vec()).unwrap_or_default();
        let search_hits = self.search_hits.iter().enumerate().filter(|(_, hit)| hit.page_index() == self.current_page).map(|(index, hit)| (hit.bounds(), self.active_search_hit == Some(index))).collect();
        components::reader_pdf_scroll_surface(&self.scroll_handle)
            .child(PdfPageElement { view: cx.entity(), focus: self.focus.clone(), page, selection, search_hits, interaction_style: self.interaction_style, display_scale: self.display_scale })
            .into_any_element()
    }
}

fn normalized_bounds(rect: NormalizedRect, page: Bounds<Pixels>) -> Bounds<Pixels> {
    Bounds { origin: point(page.origin.x + page.size.width * rect.left(), page.origin.y + page.size.height * rect.top()), size: size(page.size.width * rect.width(), page.size.height * rect.height()) }
}

fn normalized_point(position: Point<Pixels>, page: Bounds<Pixels>) -> (f32, f32) {
    ((f32::from(position.x - page.origin.x) / f32::from(page.size.width).max(1.0)).clamp(0.0, 1.0), (f32::from(position.y - page.origin.y) / f32::from(page.size.height).max(1.0)).clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_cache_evicts_oldest_page_with_a_byte_budget() {
        fn page(byte_len: usize) -> Arc<DisplayPage> {
            let image = RgbaImage::new(1, 1);
            Arc::new(DisplayPage { image: Arc::new(RenderImage::new(SmallVec::from_vec(vec![Frame::new(image)]))), text_layout: Arc::new(PdfPageTextLayout::default()), byte_len })
        }

        let mut cache = DisplayPageCache::new(8);
        let first = PageRenderKey::new(0, 800);
        let second = PageRenderKey::new(1, 800);
        let third = PageRenderKey::new(2, 800);
        cache.insert(first, page(4));
        cache.insert(second, page(4));
        assert!(cache.get(first).is_some());
        cache.insert(third, page(4));
        assert!(cache.get(first).is_some());
        assert!(cache.get(second).is_none());
        assert!(cache.get(third).is_some());
    }
}
