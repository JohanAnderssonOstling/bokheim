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
const SPREAD_DISPLAY_WIDTH: f32 = 500.0;
const AUTO_SPREAD_MIN_WIDTH: f32 = 1064.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PdfPageLayout {
    Single,
    Spread,
    #[default]
    Automatic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResolvedPageLayout {
    Single,
    Spread,
}

#[derive(Clone, Debug)]
pub enum PdfViewEvent {
    PageChanged { current: usize, total: usize },
    SearchChanged { current: usize, total: usize },
    LoadFailed(String),
}

pub struct PdfView {
    session: Option<Arc<Mutex<PdfDocumentSession>>>,
    info: Option<PdfDocumentInfo>,
    pages: Vec<(usize, Arc<DisplayPage>)>,
    display_cache: DisplayPageCache,
    current_page: usize,
    target_width: u16,
    display_scale: f32,
    page_layout: PdfPageLayout,
    resolved_page_layout: ResolvedPageLayout,
    load_generation: u64,
    render_generation: u64,
    pending_render: Option<(u64, Vec<PageRenderKey>)>,
    search_generation: u64,
    search_in_flight: bool,
    pending_search: Option<(u64, String)>,
    loading: bool,
    rendering: bool,
    error: Option<String>,
    selection_anchor: Option<(f32, f32)>,
    selection_active: Option<(f32, f32)>,
    selection_page: Option<usize>,
    selection: Option<PdfTextSelection>,
    search_hits: Vec<PdfSearchHit>,
    active_search_hit: Option<usize>,
    pending_search_scroll: Option<(usize, NormalizedRect)>,
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
    page_index: usize,
    page: Arc<DisplayPage>,
    selection: Vec<NormalizedRect>,
    search_hits: Vec<(NormalizedRect, bool)>,
    interaction_style: ReaderInteractionStyle,
    display_width: f32,
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
            pages: Vec::new(),
            display_cache: DisplayPageCache::new(DISPLAY_CACHE_BUDGET),
            current_page: initial_page,
            target_width: DEFAULT_RENDER_WIDTH,
            display_scale: 1.0,
            page_layout: PdfPageLayout::Automatic,
            resolved_page_layout: ResolvedPageLayout::Single,
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
            selection_page: None,
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

    pub fn page_layout(&self) -> PdfPageLayout {
        self.page_layout
    }

    pub fn set_page_layout(&mut self, page_layout: PdfPageLayout, cx: &mut Context<Self>) {
        if self.page_layout == page_layout {
            return;
        }
        self.page_layout = page_layout;
        let resolved = match page_layout {
            PdfPageLayout::Single => ResolvedPageLayout::Single,
            PdfPageLayout::Spread => ResolvedPageLayout::Spread,
            PdfPageLayout::Automatic => self.resolved_page_layout,
        };
        self.apply_resolved_page_layout(resolved, cx);
    }

    pub fn set_available_width(&mut self, available_width: f32, cx: &mut Context<Self>) {
        let resolved = resolve_page_layout(self.page_layout, available_width);
        self.apply_resolved_page_layout(resolved, cx);
    }

    pub fn can_previous_page(&self) -> bool {
        previous_page_index(self.current_page, self.page_count(), self.resolved_page_layout).is_some()
    }

    pub fn can_next_page(&self) -> bool {
        next_page_index(self.current_page, self.page_count(), self.resolved_page_layout).is_some()
    }

    pub fn visible_page_range(&self) -> Option<(usize, usize)> {
        let indices = visible_page_indices(self.current_page, self.page_count(), self.resolved_page_layout);
        Some((*indices.first()?, *indices.last()?))
    }

    pub fn next_page(&mut self, cx: &mut Context<Self>) {
        let Some(next_page) = next_page_index(self.current_page, self.page_count(), self.resolved_page_layout) else {
            return;
        };
        self.current_page = next_page;
        self.reset_page_scroll();
        self.clear_selection();
        self.request_current_pages(cx);
    }

    pub fn previous_page(&mut self, cx: &mut Context<Self>) {
        let Some(previous_page) = previous_page_index(self.current_page, self.page_count(), self.resolved_page_layout) else {
            return;
        };
        self.current_page = previous_page;
        self.reset_page_scroll();
        self.clear_selection();
        self.request_current_pages(cx);
    }

    pub fn set_page(&mut self, page_index: usize, cx: &mut Context<Self>) {
        if page_index >= self.page_count() || page_index == self.current_page {
            return;
        }
        self.current_page = page_index;
        self.reset_page_scroll();
        self.clear_selection();
        self.request_current_pages(cx);
    }

    pub fn set_render_width(&mut self, target_width: u16, cx: &mut Context<Self>) {
        let target_width = PageRenderKey::new(0, target_width).target_width();
        if self.target_width == target_width {
            return;
        }
        self.target_width = target_width;
        self.request_current_pages(cx);
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

    fn apply_resolved_page_layout(&mut self, resolved: ResolvedPageLayout, cx: &mut Context<Self>) {
        if self.resolved_page_layout == resolved {
            return;
        }
        self.resolved_page_layout = resolved;
        self.reset_page_scroll();
        self.clear_selection();
        self.request_current_pages(cx);
        cx.notify();
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
                                this.pending_search_scroll = Some((hit.page_index(), hit.bounds()));
                                if this.current_page != hit.page_index() {
                                    this.current_page = hit.page_index();
                                    this.reset_page_scroll();
                                }
                                this.clear_selection();
                                this.request_current_pages(cx);
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
        self.pending_search_scroll = Some((hit.page_index(), hit.bounds()));
        if self.current_page != hit.page_index() {
            self.current_page = hit.page_index();
            self.reset_page_scroll();
        }
        self.clear_selection();
        self.request_current_pages(cx);
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
                        this.request_current_pages(cx);
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

    fn request_current_pages(&mut self, cx: &mut Context<Self>) {
        self.render_generation = self.render_generation.wrapping_add(1);
        let generation = self.render_generation;
        let keys = self.visible_render_keys();
        if keys.is_empty() {
            self.pages.clear();
            cx.notify();
            return;
        }
        if let Some(pages) = self.cached_pages(&keys) {
            self.pages = pages;
            self.pending_render = None;
            self.reveal_pending_search();
            self.emit_page_changed(cx);
            cx.notify();
            return;
        }
        self.pages.clear();
        self.error = None;
        if self.rendering {
            self.pending_render = Some((generation, keys));
            cx.notify();
            return;
        }
        self.start_render(generation, keys, cx);
    }

    fn visible_render_keys(&self) -> Vec<PageRenderKey> {
        visible_page_indices(self.current_page, self.page_count(), self.resolved_page_layout)
            .into_iter()
            .map(|page_index| PageRenderKey::new(page_index, self.target_width))
            .collect()
    }

    fn cached_pages(&mut self, keys: &[PageRenderKey]) -> Option<Vec<(usize, Arc<DisplayPage>)>> {
        let mut pages = Vec::with_capacity(keys.len());
        for key in keys {
            pages.push((key.page_index(), self.display_cache.get(*key)?));
        }
        Some(pages)
    }

    fn start_render(&mut self, generation: u64, keys: Vec<PageRenderKey>, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        self.rendering = true;
        let work = cx.background_executor().spawn(async move {
            let session = session.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut rendered_pages = Vec::with_capacity(keys.len());
            for key in keys {
                let rendered = session.render_page(key.page_index(), Some(key.target_width()))?;
                rendered_pages.push((key, rendered));
            }
            Ok::<_, pdf_reader_core::PdfError>(rendered_pages)
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.rendering = false;
                if this.render_generation == generation {
                    match result {
                        Ok(rendered_pages) => {
                            let mut conversion_error = None;
                            for (key, rendered) in rendered_pages {
                                match DisplayPage::from_rendered(rendered) {
                                    Ok(page) => {
                                        this.display_cache.insert(key, Arc::new(page));
                                    }
                                    Err(error) => {
                                        conversion_error = Some(error);
                                        break;
                                    }
                                }
                            }
                            if let Some(error) = conversion_error {
                                this.error = Some(error);
                            } else {
                                let keys = this.visible_render_keys();
                                this.pages = this.cached_pages(&keys).unwrap_or_default();
                                this.reveal_pending_search();
                                this.emit_page_changed(cx);
                            }
                        }
                        Err(error) => {
                            this.error = Some(error.to_string());
                        }
                    }
                }
                if let Some((next_generation, next_keys)) = this.pending_render.take().filter(|(next_generation, _)| *next_generation == this.render_generation) {
                    this.start_render(next_generation, next_keys, cx);
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
        self.selection_page = None;
        self.selection = None;
    }

    fn reset_page_scroll(&self) {
        self.scroll_handle.set_offset(point(Pixels::ZERO, Pixels::ZERO));
    }

    fn reveal_pending_search(&mut self) {
        let Some((page_index, hit)) = self.pending_search_scroll.take() else {
            return;
        };
        let Some((_, page)) = self.pages.iter().find(|(visible_page, _)| *visible_page == page_index) else {
            self.pending_search_scroll = Some((page_index, hit));
            return;
        };
        let image_size = page.image.size(0);
        let aspect = i32::from(image_size.height) as f32 / (i32::from(image_size.width) as f32).max(1.0);
        let page_height = self.page_display_width() * aspect;
        let viewport_height = f32::from(self.scroll_handle.bounds().size.height);
        let target = (24.0 + hit.top() * page_height - viewport_height * 0.3).max(0.0);
        let maximum = f32::from(self.scroll_handle.max_offset().y);
        let target = if maximum > 0.0 { target.min(maximum) } else { target };
        let current = self.scroll_handle.offset();
        self.scroll_handle.set_offset(point(current.x, px(-target)));
    }

    fn begin_selection(&mut self, page_index: usize, position: (f32, f32), cx: &mut Context<Self>) {
        if self.current_page != page_index {
            self.current_page = page_index;
            self.emit_page_changed(cx);
        }
        self.selection_page = Some(page_index);
        self.selection_anchor = Some(position);
        self.selection_active = Some(position);
        self.update_selection();
        cx.notify();
    }

    fn update_selection_to(&mut self, page_index: usize, position: (f32, f32), cx: &mut Context<Self>) {
        if self.selection_anchor.is_none() || self.selection_page != Some(page_index) {
            return;
        }
        self.selection_active = Some(position);
        self.update_selection();
        cx.notify();
    }

    fn update_selection(&mut self) {
        self.selection = self
            .pages
            .iter()
            .find(|(page_index, _)| Some(*page_index) == self.selection_page)
            .and_then(|(_, page)| Some((page, self.selection_anchor?, self.selection_active?)))
            .and_then(|(page, anchor, active)| page.text_layout.select(anchor, active));
    }

    fn page_display_width(&self) -> f32 {
        let base_width = match self.resolved_page_layout {
            ResolvedPageLayout::Single => DEFAULT_DISPLAY_WIDTH,
            ResolvedPageLayout::Spread => SPREAD_DISPLAY_WIDTH,
        };
        base_width * self.display_scale
    }
}

fn resolve_page_layout(page_layout: PdfPageLayout, available_width: f32) -> ResolvedPageLayout {
    match page_layout {
        PdfPageLayout::Single => ResolvedPageLayout::Single,
        PdfPageLayout::Spread => ResolvedPageLayout::Spread,
        PdfPageLayout::Automatic if available_width >= AUTO_SPREAD_MIN_WIDTH => ResolvedPageLayout::Spread,
        PdfPageLayout::Automatic => ResolvedPageLayout::Single,
    }
}

fn spread_start(page_index: usize) -> usize {
    if page_index == 0 {
        0
    } else if page_index % 2 == 1 {
        page_index
    } else {
        page_index - 1
    }
}

fn visible_page_indices(current_page: usize, page_count: usize, layout: ResolvedPageLayout) -> Vec<usize> {
    if page_count == 0 {
        return Vec::new();
    }
    let current_page = current_page.min(page_count - 1);
    match layout {
        ResolvedPageLayout::Single => vec![current_page],
        ResolvedPageLayout::Spread if current_page == 0 => vec![0],
        ResolvedPageLayout::Spread => {
            let first = spread_start(current_page);
            (first..=(first + 1).min(page_count - 1)).collect()
        }
    }
}

fn next_page_index(current_page: usize, page_count: usize, layout: ResolvedPageLayout) -> Option<usize> {
    if page_count == 0 {
        return None;
    }
    match layout {
        ResolvedPageLayout::Single => (current_page + 1 < page_count).then_some(current_page + 1),
        ResolvedPageLayout::Spread if current_page == 0 => (page_count > 1).then_some(1),
        ResolvedPageLayout::Spread => {
            let next = spread_start(current_page) + 2;
            (next < page_count).then_some(next)
        }
    }
}

fn previous_page_index(current_page: usize, page_count: usize, layout: ResolvedPageLayout) -> Option<usize> {
    if page_count == 0 || current_page == 0 {
        return None;
    }
    match layout {
        ResolvedPageLayout::Single => Some(current_page - 1),
        ResolvedPageLayout::Spread => {
            let first = spread_start(current_page);
            Some(if first <= 1 { 0 } else { first - 2 })
        }
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
        Some(("gpui-pdf-page", self.page_index).into())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        let image_size = self.page.image.size(0);
        let aspect = i32::from(image_size.height) as f32 / (i32::from(image_size.width) as f32).max(1.0);
        style.size.width = px(self.display_width).into();
        style.size.height = px(self.display_width * aspect).into();
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
        let page_index = self.page_index;
        let mouse_down_target = self.view.clone();
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || event.button != MouseButton::Left || !hitbox.is_hovered(window) {
                return;
            }
            mouse_down_target.update(cx, |view, cx| {
                view.focus.focus(window, cx);
                view.begin_selection(page_index, normalized_point(event.position, page_bounds), cx);
                cx.stop_propagation();
            });
        });

        let page_bounds = prepaint.page_bounds;
        let page_index = self.page_index;
        let mouse_move_target = self.view.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
            if phase != DispatchPhase::Bubble || !event.dragging() {
                return;
            }
            mouse_move_target.update(cx, |view, cx| {
                view.update_selection_to(page_index, normalized_point(event.position, page_bounds), cx);
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
        if self.pages.is_empty() {
            let message = self.error.clone().unwrap_or_else(|| if self.loading { "Opening PDF…".to_owned() } else { "Rendering page…".to_owned() });
            return components::reader_document_message(message).into_any_element();
        }
        let display_width = self.page_display_width();
        let pages = self
            .pages
            .iter()
            .map(|(page_index, page)| {
                let selection = if self.selection_page == Some(*page_index) {
                    self.selection.as_ref().map(|selection| selection.rects().to_vec()).unwrap_or_default()
                } else {
                    Vec::new()
                };
                let search_hits = self
                    .search_hits
                    .iter()
                    .enumerate()
                    .filter(|(_, hit)| hit.page_index() == *page_index)
                    .map(|(index, hit)| (hit.bounds(), self.active_search_hit == Some(index)))
                    .collect();
                PdfPageElement {
                    view: cx.entity(),
                    focus: self.focus.clone(),
                    page_index: *page_index,
                    page: page.clone(),
                    selection,
                    search_hits,
                    interaction_style: self.interaction_style,
                    display_width,
                }
            })
            .collect::<Vec<_>>();
        components::reader_pdf_scroll_surface(&self.scroll_handle)
            .children(pages)
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
    fn spread_keeps_the_cover_alone_and_pairs_following_pages() {
        assert_eq!(visible_page_indices(0, 6, ResolvedPageLayout::Spread), vec![0]);
        assert_eq!(visible_page_indices(1, 6, ResolvedPageLayout::Spread), vec![1, 2]);
        assert_eq!(visible_page_indices(2, 6, ResolvedPageLayout::Spread), vec![1, 2]);
        assert_eq!(visible_page_indices(5, 6, ResolvedPageLayout::Spread), vec![5]);
    }

    #[test]
    fn spread_navigation_advances_by_visible_spreads() {
        assert_eq!(next_page_index(0, 7, ResolvedPageLayout::Spread), Some(1));
        assert_eq!(next_page_index(1, 7, ResolvedPageLayout::Spread), Some(3));
        assert_eq!(next_page_index(2, 7, ResolvedPageLayout::Spread), Some(3));
        assert_eq!(previous_page_index(3, 7, ResolvedPageLayout::Spread), Some(1));
        assert_eq!(previous_page_index(2, 7, ResolvedPageLayout::Spread), Some(0));
        assert_eq!(previous_page_index(1, 7, ResolvedPageLayout::Spread), Some(0));
    }

    #[test]
    fn automatic_layout_uses_the_available_reader_width() {
        assert_eq!(resolve_page_layout(PdfPageLayout::Automatic, AUTO_SPREAD_MIN_WIDTH - 1.0), ResolvedPageLayout::Single);
        assert_eq!(resolve_page_layout(PdfPageLayout::Automatic, AUTO_SPREAD_MIN_WIDTH), ResolvedPageLayout::Spread);
        assert_eq!(resolve_page_layout(PdfPageLayout::Single, AUTO_SPREAD_MIN_WIDTH * 2.0), ResolvedPageLayout::Single);
        assert_eq!(resolve_page_layout(PdfPageLayout::Spread, 200.0), ResolvedPageLayout::Spread);
    }

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
