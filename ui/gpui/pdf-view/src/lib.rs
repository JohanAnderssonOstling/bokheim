use ui_components as components;
use gpui::prelude::*;
use gpui::{
    AnyElement, App, Bounds, ClipboardItem, ContentMask, Context, Corners, CursorStyle, DispatchPhase, Element, ElementId, Entity, EventEmitter, FocusHandle, GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId, IntoElement,
    KeyContext, KeyDownEvent, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Render, RenderImage, ScrollDelta, ScrollHandle, Style, Window, div, fill, point, px, rgba, size,
};
use image::{Frame, RgbaImage};
use pdf_reader_core::{
    FitWidthColumn, FitWidthPage, FitWidthPlan, FitWidthViewport, NormalizedRect, PageRenderKey, PdfDocumentInfo, PdfDocumentSession, PdfPageTextLayout, PdfSearchHit, PdfTextSelection, PixelCrop, ReaderInteractionStyle,
    SelectionGranularity, SelectionOptions, crop_normalized_rect,
};
use smallvec::SmallVec;
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Seek};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const DISPLAY_CACHE_BUDGET: usize = 128 * 1024 * 1024;
const DEFAULT_RENDER_WIDTH: u16 = 1400;
const AUTO_SPREAD_MIN_WIDTH: f32 = 1080.0;
const FIT_WIDTH_SURFACE_PADDING: f32 = 0.0;
const FIT_WIDTH_COLUMN_GAP: f32 = 1.0;
const DEFAULT_AVAILABLE_WIDTH: f32 = 948.0;
const DEFAULT_AVAILABLE_HEIGHT: f32 = 800.0;
pub const MIN_VISIBLE_PAGE_COUNT: usize = 1;
pub const MAX_VISIBLE_PAGE_COUNT: usize = 6;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PdfPageLayout {
    Single,
    Spread,
    #[default]
    Automatic,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PdfFitMode {
    #[default]
    FitWidth,
    FitPage,
    Custom,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct FitWidthCursor {
    page_index: usize,
    page_column_index: u32,
}

#[derive(Clone, Debug)]
pub enum PdfViewEvent {
    PageChanged { current: usize, total: usize },
    SearchChanged { current: usize, total: usize },
    LoadFailed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PdfAnnotationStyle {
    Highlight,
    Underline,
    Squiggly,
    Strikethrough,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PdfAnnotationOverlay {
    pub id: String,
    pub page_index: usize,
    /// Untrimmed, normalized PDF page coordinates.
    pub rects: Vec<NormalizedRect>,
    pub style: PdfAnnotationStyle,
    /// Packed as `0xRRGGBBAA`.
    pub color: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PdfSelectionSnapshot {
    pub page_index: usize,
    pub text: String,
    /// Untrimmed, normalized PDF page coordinates.
    pub rects: Vec<NormalizedRect>,
}

pub struct PdfView {
    session: Option<Arc<Mutex<PdfDocumentSession>>>,
    info: Option<PdfDocumentInfo>,
    pages: Vec<(usize, Arc<DisplayPage>)>,
    display_cache: DisplayPageCache,
    effective_page_sizes: HashMap<(usize, bool), (f32, f32)>,
    page_break_avoidance: HashMap<(usize, bool), Arc<[(f32, f32)]>>,
    fit_width_plan: FitWidthPlan,
    current_page: usize,
    current_page_column: u32,
    target_width: u16,
    display_scale: f32,
    fit_mode: PdfFitMode,
    available_width: f32,
    available_height: f32,
    page_layout: PdfPageLayout,
    trim_margins: bool,
    visible_page_count: usize,
    load_generation: u64,
    render_generation: u64,
    pending_render: Option<(u64, Vec<PageRenderKey>)>,
    search_generation: u64,
    search_in_flight: bool,
    pending_search: Option<(u64, String)>,
    rendering: bool,
    error: Option<String>,
    selection_anchor: Option<(f32, f32)>,
    selection_active: Option<(f32, f32)>,
    selection_page: Option<usize>,
    selection_page_column: Option<u32>,
    selection_granularity: SelectionGranularity,
    selection: Option<PdfTextSelection>,
    annotations: Vec<PdfAnnotationOverlay>,
    search_hits: Vec<PdfSearchHit>,
    active_search_hit: Option<usize>,
    pending_search_scroll: Option<(usize, NormalizedRect)>,
    pending_full_page_position: Option<f32>,
    interaction_style: ReaderInteractionStyle,
    scroll_handle: ScrollHandle,
    focus: FocusHandle,
}

struct DisplayPage {
    image: Arc<RenderImage>,
    text_layout: Arc<PdfPageTextLayout>,
    crop: PixelCrop,
    byte_len: usize,
    break_avoidance: Arc<[(f32, f32)]>,
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
    column: FitWidthColumn,
    page: Arc<DisplayPage>,
    selection: Vec<NormalizedRect>,
    annotations: Vec<(NormalizedRect, PdfAnnotationStyle, u32)>,
    search_hits: Vec<(NormalizedRect, bool)>,
    interaction_style: ReaderInteractionStyle,
}

struct PdfPagePrepaint {
    hitbox: Hitbox,
    column_bounds: Bounds<Pixels>,
    image_bounds: Bounds<Pixels>,
}

impl EventEmitter<PdfViewEvent> for PdfView {}

impl PdfView {
    pub fn from_bytes(display_path: PathBuf, bytes: Vec<u8>, initial_page: usize, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut view = Self::empty(initial_page, window, cx);
        view.start_load(move || PdfDocumentSession::from_bytes(display_path, bytes), cx);
        view
    }

    pub fn from_reader<R>(display_path: PathBuf, reader: R, initial_page: usize, window: &mut Window, cx: &mut Context<Self>) -> Self
    where
        R: Read + Seek + Send + 'static,
    {
        let mut view = Self::empty(initial_page, window, cx);
        view.start_load(move || PdfDocumentSession::from_reader(display_path, reader), cx);
        view
    }

    fn empty(initial_page: usize, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        Self {
            session: None,
            info: None,
            pages: Vec::new(),
            display_cache: DisplayPageCache::new(DISPLAY_CACHE_BUDGET),
            effective_page_sizes: HashMap::new(),
            page_break_avoidance: HashMap::new(),
            fit_width_plan: FitWidthPlan::default(),
            current_page: initial_page,
            current_page_column: 0,
            target_width: DEFAULT_RENDER_WIDTH,
            display_scale: 1.0,
            fit_mode: PdfFitMode::FitWidth,
            available_width: DEFAULT_AVAILABLE_WIDTH,
            available_height: DEFAULT_AVAILABLE_HEIGHT,
            page_layout: PdfPageLayout::Automatic,
            trim_margins: true,
            visible_page_count: 1,
            load_generation: 1,
            render_generation: 0,
            pending_render: None,
            search_generation: 0,
            search_in_flight: false,
            pending_search: None,
            rendering: false,
            error: None,
            selection_anchor: None,
            selection_active: None,
            selection_page: None,
            selection_page_column: None,
            selection_granularity: SelectionGranularity::Character,
            selection: None,
            annotations: Vec::new(),
            search_hits: Vec::new(),
            active_search_hit: None,
            pending_search_scroll: None,
            pending_full_page_position: None,
            interaction_style: ReaderInteractionStyle::default(),
            scroll_handle: ScrollHandle::new(),
            focus,
        }
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
        let count = resolve_page_count(page_layout, self.available_width);
        self.apply_visible_page_count(count, cx);
    }

    pub fn visible_page_count(&self) -> usize {
        self.visible_page_count
    }

    pub fn set_visible_page_count(&mut self, count: usize, cx: &mut Context<Self>) {
        self.page_layout = match count {
            1 => PdfPageLayout::Single,
            2 => PdfPageLayout::Spread,
            _ => PdfPageLayout::Automatic,
        };
        self.apply_visible_page_count(count.clamp(MIN_VISIBLE_PAGE_COUNT, MAX_VISIBLE_PAGE_COUNT), cx);
    }

    pub fn set_available_width(&mut self, available_width: f32, cx: &mut Context<Self>) {
        if available_width.is_finite() && available_width > 0.0 && (self.available_width - available_width).abs() >= 1.0 {
            let page_position = self.current_page_position();
            self.available_width = available_width;
            self.rebuild_fit_width_plan();
            self.restore_page_position(page_position);
            self.reset_page_scroll();
            self.request_current_pages(cx);
            cx.notify();
        }
        if self.page_layout == PdfPageLayout::Automatic && self.visible_page_count <= 2 {
            let count = resolve_page_count(self.page_layout, available_width);
            self.apply_visible_page_count(count, cx);
        }
    }

    fn set_available_height(&mut self, available_height: f32, cx: &mut Context<Self>) {
        if !available_height.is_finite() || available_height <= 0.0 || (self.available_height - available_height).abs() < 1.0 {
            return;
        }
        let page_position = self.current_page_position();
        self.available_height = available_height;
        self.rebuild_fit_width_plan();
        self.restore_page_position(page_position);
        self.reset_page_scroll();
        self.request_current_pages(cx);
        cx.notify();
    }

    pub fn trim_margins(&self) -> bool {
        self.trim_margins
    }

    pub fn set_trim_margins(&mut self, trim_margins: bool, cx: &mut Context<Self>) {
        if self.trim_margins == trim_margins {
            return;
        }
        self.pending_full_page_position = Some(self.current_page_position());
        self.trim_margins = trim_margins;
        self.clear_selection();
        self.request_current_pages(cx);
        cx.notify();
    }

    pub fn can_previous_page(&self) -> bool {
        self.previous_group_cursor().is_some()
    }

    pub fn can_next_page(&self) -> bool {
        self.next_group_cursor().is_some()
    }

    pub fn visible_page_range(&self) -> Option<(usize, usize)> {
        let columns = self.visible_columns();
        Some((columns.first()?.page_index(), columns.last()?.page_index()))
    }

    pub fn next_page(&mut self, cx: &mut Context<Self>) {
        let Some(next) = self.next_group_cursor() else {
            return;
        };
        self.set_cursor(next);
        self.reset_page_scroll();
        self.clear_selection();
        self.request_current_pages(cx);
    }

    pub fn previous_page(&mut self, cx: &mut Context<Self>) {
        let Some(previous) = self.previous_group_cursor() else {
            return;
        };
        self.set_cursor(previous);
        self.reset_page_scroll();
        self.clear_selection();
        self.request_current_pages(cx);
    }

    pub fn set_page(&mut self, page_index: usize, cx: &mut Context<Self>) {
        if page_index >= self.page_count() || (page_index == self.current_page && self.current_page_column == 0) {
            return;
        }
        self.current_page = page_index;
        self.current_page_column = 0;
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

    pub fn fit_mode(&self) -> PdfFitMode {
        self.fit_mode
    }

    pub fn set_fit_mode(&mut self, fit_mode: PdfFitMode, cx: &mut Context<Self>) {
        if self.fit_mode == fit_mode && (self.display_scale - 1.0).abs() < f32::EPSILON {
            return;
        }
        let page_position = self.current_page_position();
        self.fit_mode = fit_mode;
        self.display_scale = 1.0;
        self.target_width = DEFAULT_RENDER_WIDTH;
        self.rebuild_fit_width_plan();
        self.restore_page_position(page_position);
        self.reset_page_scroll();
        self.request_current_pages(cx);
        cx.notify();
    }

    pub fn set_display_scale(&mut self, scale: f32, cx: &mut Context<Self>) {
        let scale = scale.clamp(0.5, 2.5);
        if (self.display_scale - scale).abs() < f32::EPSILON {
            return;
        }
        let page_position = self.current_page_position();
        self.fit_mode = PdfFitMode::Custom;
        self.display_scale = scale;
        self.rebuild_fit_width_plan();
        self.restore_page_position(page_position);
        let render_width = (DEFAULT_RENDER_WIDTH as f32 * scale).round() as u16;
        let previous_render_width = self.target_width;
        self.set_render_width(render_width, cx);
        if self.target_width == previous_render_width {
            self.request_current_pages(cx);
        }
        cx.notify();
    }

    pub fn change_display_scale(&mut self, delta: f32, cx: &mut Context<Self>) {
        self.set_display_scale(self.display_scale + delta, cx);
    }

    fn apply_visible_page_count(&mut self, count: usize, cx: &mut Context<Self>) {
        let count = count.clamp(MIN_VISIBLE_PAGE_COUNT, MAX_VISIBLE_PAGE_COUNT);
        if self.visible_page_count == count {
            return;
        }
        let page_position = self.current_page_position();
        self.visible_page_count = count;
        self.rebuild_fit_width_plan();
        self.restore_page_position(page_position);
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
                                this.current_page = hit.page_index();
                                this.current_page_column = 0;
                                this.reset_page_scroll();
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
        self.current_page = hit.page_index();
        self.current_page_column = 0;
        self.reset_page_scroll();
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

    pub fn selection_snapshot(&self) -> Option<PdfSelectionSnapshot> {
        let page_index = self.selection_page?;
        let selection = self.selection.as_ref()?;
        let (_, page) = self.pages.iter().find(|(candidate, _)| *candidate == page_index)?;
        let rects = selection.rects().iter().map(|rect| page.crop.display_rect_to_full(*rect)).collect::<Vec<_>>();
        (!selection.text().is_empty() && !rects.is_empty()).then(|| PdfSelectionSnapshot { page_index, text: selection.text().to_owned(), rects })
    }

    pub fn set_annotations(&mut self, annotations: Vec<PdfAnnotationOverlay>, cx: &mut Context<Self>) {
        if self.annotations == annotations {
            return;
        }
        self.annotations = annotations;
        cx.notify();
    }

    pub fn reveal_annotation(&mut self, page_index: usize, bounds: NormalizedRect, cx: &mut Context<Self>) {
        if page_index >= self.page_count() {
            return;
        }
        self.pending_search_scroll = Some((page_index, bounds));
        self.current_page = page_index;
        self.current_page_column = 0;
        self.reset_page_scroll();
        self.clear_selection();
        self.request_current_pages(cx);
    }

    pub fn focus_handle(&self) -> FocusHandle {
        self.focus.clone()
    }

    fn start_load(&mut self, load: impl FnOnce() -> pdf_reader_core::PdfResult<PdfDocumentSession> + Send + 'static, cx: &mut Context<Self>) {
        let generation = self.load_generation;
        let work = cx.background_executor().spawn(async move { load() });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                if this.load_generation != generation {
                    return;
                }
                match result {
                    Ok(session) => {
                        this.info = Some(session.info().clone());
                        this.session = Some(Arc::new(Mutex::new(session)));
                        this.current_page = this.current_page.min(this.page_count().saturating_sub(1));
                        this.rebuild_fit_width_plan();
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
            let pending_position = self.pending_full_page_position.take();
            self.pages = pages;
            for (page_index, page) in &self.pages {
                self.effective_page_sizes.insert((*page_index, self.trim_margins), page.dimensions());
                self.page_break_avoidance.insert((*page_index, self.trim_margins), page.break_avoidance.clone());
            }
            self.rebuild_fit_width_plan();
            if let Some(position) = pending_position {
                self.restore_page_position(position);
                self.reset_page_scroll();
            }
            self.pending_render = None;
            if self.reveal_pending_search() {
                self.request_current_pages(cx);
                return;
            }
            self.emit_page_changed(cx);
            cx.notify();
            return;
        }
        let missing_keys = keys.into_iter().filter(|key| !self.display_cache.contains(*key)).collect::<Vec<_>>();
        self.error = None;
        if self.rendering {
            self.pending_render = Some((generation, missing_keys));
            return;
        }
        self.start_render(generation, missing_keys, cx);
    }

    fn visible_render_keys(&self) -> Vec<PageRenderKey> {
        let mut page_indices = Vec::new();
        for column in self.render_window_columns() {
            if !page_indices.contains(&column.page_index()) {
                page_indices.push(column.page_index());
            }
        }
        page_indices.into_iter().map(|page_index| PageRenderKey::with_margin_trim(page_index, self.target_width, self.trim_margins)).collect()
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
                let rendered = session.render_page_with_margin_trim(key.page_index(), Some(key.target_width()), key.trim_margins())?;
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
                            let mut converted_pages = Vec::with_capacity(rendered_pages.len());
                            for (key, rendered) in rendered_pages {
                                match DisplayPage::from_rendered(rendered) {
                                    Ok(page) => {
                                        converted_pages.push((key, Arc::new(page)));
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
                                let pending_position = this.pending_full_page_position.take();
                                let page_position = pending_position.unwrap_or_else(|| this.current_page_position());
                                for (key, page) in &converted_pages {
                                    this.effective_page_sizes.insert((key.page_index(), key.trim_margins()), page.dimensions());
                                    this.page_break_avoidance.insert((key.page_index(), key.trim_margins()), page.break_avoidance.clone());
                                    if let Some((_, existing)) = this.pages.iter_mut().find(|(page_index, _)| *page_index == key.page_index()) {
                                        *existing = page.clone();
                                    } else {
                                        this.pages.push((key.page_index(), page.clone()));
                                    }
                                }
                                this.rebuild_fit_width_plan();
                                this.restore_page_position(page_position);
                                if pending_position.is_some() {
                                    this.reset_page_scroll();
                                }
                                let keys = this.visible_render_keys();
                                let mut visible_pages = Vec::with_capacity(keys.len());
                                for key in &keys {
                                    let page = converted_pages.iter().find(|(rendered_key, _)| rendered_key == key).map(|(_, page)| page.clone()).or_else(|| this.display_cache.get(*key));
                                    if let Some(page) = page {
                                        visible_pages.push((key.page_index(), page));
                                    }
                                }
                                for (key, page) in converted_pages {
                                    this.display_cache.insert(key, page);
                                }
                                if visible_pages.len() == keys.len() {
                                    this.pages = visible_pages;
                                    if this.reveal_pending_search() {
                                        this.request_current_pages(cx);
                                    } else {
                                        this.emit_page_changed(cx);
                                    }
                                } else {
                                    this.pages = visible_pages;
                                    this.request_current_pages(cx);
                                }
                            }
                        }
                        Err(error) => {
                            this.error = Some(error.to_string());
                        }
                    }
                }
                let next_render = this.pending_render.take().filter(|(next_generation, _)| *next_generation == this.render_generation);
                if let Some((next_generation, next_keys)) = next_render {
                    this.start_render(next_generation, next_keys, cx);
                }
                if this.render_generation == generation {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn emit_page_changed(&self, cx: &mut Context<Self>) {
        cx.emit(PdfViewEvent::PageChanged { current: self.current_page + 1, total: self.page_count() });
    }

    fn clear_selection(&mut self) {
        self.selection_anchor = None;
        self.selection_active = None;
        self.selection_page = None;
        self.selection_page_column = None;
        self.selection_granularity = SelectionGranularity::Character;
        self.selection = None;
    }

    fn reset_page_scroll(&self) {
        let display_index = self.current_cursor().and_then(|cursor| self.column_for_cursor(cursor)).map(|column| column.display_column_index()).unwrap_or(0);
        let row = display_index / self.visible_page_count.max(1);
        self.scroll_handle.set_offset(point(Pixels::ZERO, px(-(row as f32 * self.page_display_height()))));
    }

    fn sync_vertical_scroll_target(&mut self, delta_y: f32, cx: &mut Context<Self>) {
        let row_height = self.page_display_height();
        let Some(display_columns) = self.display_column_count() else { return };
        let row_count = display_columns.div_ceil(self.visible_page_count.max(1));
        let current_offset = -f32::from(self.scroll_handle.offset().y);
        let target_offset = (current_offset - delta_y).clamp(0.0, (row_count.saturating_sub(1) as f32 * row_height).max(0.0));
        let row = (target_offset / row_height.max(1.0)).floor() as usize;
        let display_index = (row * self.visible_page_count.max(1)).min(display_columns.saturating_sub(1));
        let Some(cursor) = self.cursor_at_display_column(display_index) else { return };
        if self.current_cursor() != Some(cursor) {
            self.set_cursor(cursor);
            self.clear_selection();
            self.request_current_pages(cx);
        }
    }

    fn reveal_pending_search(&mut self) -> bool {
        let Some((page_index, hit)) = self.pending_search_scroll.take() else {
            return false;
        };
        let Some((_, page)) = self.pages.iter().find(|(visible_page, _)| *visible_page == page_index) else {
            self.pending_search_scroll = Some((page_index, hit));
            return false;
        };
        let Some(hit) = page.map_rect(hit) else {
            return false;
        };
        let previous_column = self.current_page_column;
        let target_y = (hit.top() + hit.height() * 0.5).clamp(0.0, 1.0);
        let page_columns = self.fit_width_plan.columns().iter().filter(|column| column.page_index() == page_index).collect::<Vec<_>>();
        if !page_columns.is_empty() {
            self.current_page_column = page_columns.iter().position(|column| column.page_point_to_column((hit.left(), target_y)).is_some()).unwrap_or_else(|| page_columns.len().saturating_sub(1)) as u32;
        }
        self.reset_page_scroll();
        self.current_page_column != previous_column
    }

    fn begin_selection(&mut self, page_index: usize, page_column_index: u32, position: (f32, f32), granularity: SelectionGranularity, cx: &mut Context<Self>) {
        if self.current_page != page_index || self.current_page_column != page_column_index {
            self.current_page = page_index;
            self.current_page_column = page_column_index;
            self.emit_page_changed(cx);
        }
        self.selection_page = Some(page_index);
        self.selection_page_column = Some(page_column_index);
        self.selection_anchor = Some(position);
        self.selection_active = Some(position);
        self.selection_granularity = granularity;
        self.update_selection();
        cx.notify();
    }

    fn update_selection_to(&mut self, page_index: usize, page_column_index: u32, position: (f32, f32), cx: &mut Context<Self>) -> bool {
        if self.selection_anchor.is_none() || self.selection_page != Some(page_index) || self.selection_page_column != Some(page_column_index) {
            return false;
        }
        self.selection_active = Some(position);
        self.update_selection();
        cx.notify();
        true
    }

    fn update_selection(&mut self) {
        self.selection = self
            .pages
            .iter()
            .find(|(page_index, _)| Some(*page_index) == self.selection_page)
            .and_then(|(_, page)| Some((page, self.selection_anchor?, self.selection_active?)))
            .and_then(|(page, anchor, active)| page.text_layout.select_with_options(anchor, active, SelectionOptions::default().with_granularity(self.selection_granularity)));
    }

    fn page_display_width(&self) -> f32 {
        let column_count = self.visible_page_count as f32;
        let gaps = (column_count - 1.0) * FIT_WIDTH_COLUMN_GAP;
        let usable_width = (self.available_width - FIT_WIDTH_SURFACE_PADDING * 2.0 - gaps).max(1.0);
        (usable_width / column_count * self.display_scale).max(1.0)
    }

    fn page_display_height(&self) -> f32 {
        (self.available_height - FIT_WIDTH_SURFACE_PADDING * 2.0).max(1.0)
    }

    fn rebuild_fit_width_plan(&mut self) {
        let Some(info) = &self.info else {
            self.fit_width_plan = FitWidthPlan::default();
            return;
        };
        let Some(viewport) = FitWidthViewport::new(self.page_display_width(), self.page_display_height()) else {
            self.fit_width_plan = FitWidthPlan::default();
            return;
        };
        let pages = info
            .pages()
            .iter()
            .enumerate()
            .filter_map(|(page_index, page)| {
                let (width, height) = self.effective_page_sizes.get(&(page_index, self.trim_margins)).copied().unwrap_or((page.width_points(), page.height_points()));
                FitWidthPage::new(page_index, width, height)
            })
            .collect::<Vec<_>>();
        self.fit_width_plan = match self.fit_mode {
            PdfFitMode::FitPage => FitWidthPlan::fit_page(pages, viewport),
            PdfFitMode::FitWidth | PdfFitMode::Custom => {
                let pages = pages.iter().copied().map(|page| (page, self.page_break_avoidance.get(&(page.page_index(), self.trim_margins)).map(AsRef::as_ref).unwrap_or(&[])));
                FitWidthPlan::new_avoiding_breaks(pages, viewport).unwrap_or_default()
            }
        };
    }

    fn page_column_count(&self, page_index: usize) -> usize {
        self.fit_width_plan.columns().iter().filter(|column| column.page_index() == page_index).count()
    }

    fn normalized_cursor(&self, cursor: FitWidthCursor) -> Option<FitWidthCursor> {
        if cursor.page_index >= self.page_count() {
            return None;
        }
        let column_count = self.page_column_count(cursor.page_index);
        if column_count == 0 {
            return None;
        }
        Some(FitWidthCursor { page_index: cursor.page_index, page_column_index: cursor.page_column_index.min(column_count.saturating_sub(1) as u32) })
    }

    fn current_cursor(&self) -> Option<FitWidthCursor> {
        self.normalized_cursor(FitWidthCursor { page_index: self.current_page, page_column_index: self.current_page_column })
    }

    fn set_cursor(&mut self, cursor: FitWidthCursor) {
        if let Some(cursor) = self.normalized_cursor(cursor) {
            self.current_page = cursor.page_index;
            self.current_page_column = cursor.page_column_index;
        }
    }

    fn column_for_cursor(&self, cursor: FitWidthCursor) -> Option<FitWidthColumn> {
        let cursor = self.normalized_cursor(cursor)?;
        self.fit_width_plan.columns().iter().find(|column| column.page_index() == cursor.page_index && column.page_column_index() == cursor.page_column_index).copied()
    }

    fn next_group_cursor(&self) -> Option<FitWidthCursor> {
        let display_index = self.column_for_cursor(self.current_cursor()?)?.display_column_index();
        let total = self.display_column_count()?;
        next_flow_index(display_index, total, self.visible_page_count).and_then(|display_index| self.cursor_at_display_column(display_index))
    }

    fn previous_group_cursor(&self) -> Option<FitWidthCursor> {
        let display_index = self.column_for_cursor(self.current_cursor()?)?.display_column_index();
        let total = self.display_column_count()?;
        previous_flow_index(display_index, total, self.visible_page_count).and_then(|display_index| self.cursor_at_display_column(display_index))
    }

    fn visible_columns(&self) -> Vec<FitWidthColumn> {
        let Some(cursor) = self.current_cursor() else {
            return Vec::new();
        };
        let Some(column) = self.column_for_cursor(cursor) else {
            return Vec::new();
        };
        let Some(total) = self.display_column_count() else {
            return Vec::new();
        };
        let visible = visible_flow_indices(column.display_column_index(), total, self.visible_page_count);
        self.fit_width_plan.columns().iter().copied().filter(|column| visible.contains(&column.display_column_index())).collect()
    }

    fn render_window_columns(&self) -> Vec<FitWidthColumn> {
        let Some(cursor) = self.current_cursor() else { return Vec::new() };
        let Some(current) = self.column_for_cursor(cursor) else { return Vec::new() };
        let Some(total) = self.display_column_count() else { return Vec::new() };
        let page_count = self.visible_page_count.max(1);
        let current_row = current.display_column_index() / page_count;
        let first_row = current_row.saturating_sub(1);
        let last_row = (current_row + 1).min(total.saturating_sub(1) / page_count);
        self.fit_width_plan
            .columns()
            .iter()
            .copied()
            .filter(|column| {
                let row = column.display_column_index() / page_count;
                first_row <= row && row <= last_row
            })
            .collect()
    }

    fn display_column_count(&self) -> Option<usize> {
        self.fit_width_plan.columns().last().map(|column| column.display_column_index() + 1)
    }

    fn cursor_at_display_column(&self, display_column_index: usize) -> Option<FitWidthCursor> {
        let column = self.fit_width_plan.columns().iter().find(|column| column.display_column_index() == display_column_index)?;
        Some(FitWidthCursor { page_index: column.page_index(), page_column_index: column.page_column_index() })
    }

    fn current_page_position(&self) -> f32 {
        let display_y = self.current_cursor().and_then(|cursor| self.column_for_cursor(cursor)).map(|column| column.page_rect().top()).unwrap_or(0.0);
        self.pages.iter().find(|(page_index, _)| *page_index == self.current_page).map(|(_, page)| page.crop.display_point_to_full((0.5, display_y)).1).unwrap_or(display_y)
    }

    fn restore_page_position(&mut self, full_page_position: f32) {
        let columns = self.fit_width_plan.columns().iter().filter(|column| column.page_index() == self.current_page).collect::<Vec<_>>();
        if columns.is_empty() {
            self.current_page_column = 0;
            return;
        }
        let display_y = self.pages.iter().find(|(page_index, _)| *page_index == self.current_page).map(|(_, page)| page.crop.full_point_to_display((0.5, full_page_position)).1).unwrap_or(full_page_position);
        self.current_page_column = columns.iter().position(|column| column.page_point_to_column((0.5, display_y)).is_some()).unwrap_or_else(|| columns.len().saturating_sub(1)) as u32;
    }
}

fn resolve_page_count(page_layout: PdfPageLayout, available_width: f32) -> usize {
    match page_layout {
        PdfPageLayout::Single => 1,
        PdfPageLayout::Spread => 2,
        PdfPageLayout::Automatic if available_width >= AUTO_SPREAD_MIN_WIDTH => 2,
        PdfPageLayout::Automatic => 1,
    }
}

fn visible_flow_indices(current: usize, total: usize, page_count: usize) -> Vec<usize> {
    if total == 0 {
        return Vec::new();
    }
    let page_count = page_count.max(1);
    let current = current.min(total - 1);
    let first = current - current % page_count;
    (first..(first + page_count).min(total)).collect()
}

fn next_flow_index(current: usize, total: usize, page_count: usize) -> Option<usize> {
    let visible = visible_flow_indices(current, total, page_count);
    visible.last()?.checked_add(1).filter(|next| *next < total)
}

fn previous_flow_index(current: usize, total: usize, page_count: usize) -> Option<usize> {
    let first = *visible_flow_indices(current, total, page_count).first()?;
    if first == 0 {
        return None;
    }
    Some(first.saturating_sub(page_count.max(1)))
}

impl DisplayPage {
    fn from_rendered(page: pdf_reader_core::RenderedPdfPage) -> Result<Self, String> {
        let (rgba, width, height, crop, text_layout, break_avoidance) = page.into_parts();
        let byte_len = rgba.len();
        let image = RgbaImage::from_raw(width, height, rgba).ok_or_else(|| "PDF renderer returned an invalid RGBA buffer".to_owned())?;
        Ok(Self { image: Arc::new(RenderImage::new(SmallVec::from_vec(vec![Frame::new(image)]))), text_layout, crop, byte_len, break_avoidance })
    }

    fn map_rect(&self, rect: NormalizedRect) -> Option<NormalizedRect> {
        crop_normalized_rect(self.crop, rect)
    }

    fn dimensions(&self) -> (f32, f32) {
        let image_size = self.image.size(0);
        (i32::from(image_size.width) as f32, i32::from(image_size.height) as f32)
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

    fn contains(&self, key: PageRenderKey) -> bool {
        self.pages.contains_key(&key)
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
        Some(format!("gpui-pdf-column-{}-{}", self.page_index, self.column.page_column_index()).into())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = px(self.column.display_width()).into();
        style.size.height = px(self.column.display_height()).into();
        style.flex_shrink = 0.0;
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _: &mut Self::RequestLayoutState, window: &mut Window, cx: &mut App) -> Self::PrepaintState {
        window.set_focus_handle(&self.focus, cx);
        let image_size = self.page.image.size(0);
        let image_width = i32::from(image_size.width) as f32;
        let image_height = i32::from(image_size.height) as f32;
        let full_page_height = f32::from(bounds.size.width) * image_height / image_width.max(1.0);
        let image_bounds = Bounds { origin: point(bounds.origin.x, bounds.origin.y - px(self.column.page_rect().top() * full_page_height)), size: size(bounds.size.width, px(full_page_height)) };
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);

        let viewport_height = f32::from(self.view.read(cx).scroll_handle.bounds().size.height);
        if viewport_height > 0.0 {
            self.view.update(cx, |view, cx| view.set_available_height(viewport_height, cx));
        }
        PdfPagePrepaint { hitbox, column_bounds: bounds, image_bounds }
    }

    fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, _: Bounds<Pixels>, _: &mut Self::RequestLayoutState, prepaint: &mut Self::PrepaintState, window: &mut Window, _cx: &mut App) {
        window.with_content_mask(Some(ContentMask { bounds: prepaint.column_bounds }), |window| {
            let _ = window.paint_image(prepaint.image_bounds, Corners::default(), self.page.image.clone(), 0, false);
            for (rect, active) in &self.search_hits {
                let color = if *active { self.interaction_style.active_search_match } else { self.interaction_style.search_match };
                window.paint_quad(fill(normalized_bounds(*rect, prepaint.column_bounds), rgba(color.packed_rgba())));
            }
            for (rect, style, color) in &self.annotations {
                for rect in annotation_paint_rects(*rect, *style) {
                    window.paint_quad(fill(normalized_bounds(rect, prepaint.column_bounds), rgba(*color)));
                }
            }
            for rect in &self.selection {
                let color = self.interaction_style.selection;
                window.paint_quad(fill(normalized_bounds(*rect, prepaint.column_bounds), rgba(color.packed_rgba())));
            }
        });
        window.set_cursor_style(CursorStyle::IBeam, &prepaint.hitbox);

        let mut key_context = KeyContext::default();
        key_context.add("PdfReaderDocument");
        window.set_key_context(key_context);

        let hitbox = prepaint.hitbox.clone();
        let column_bounds = prepaint.column_bounds;
        let column = self.column;
        let page_index = self.page_index;
        let mouse_down_target = self.view.clone();
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || event.button != MouseButton::Left || !hitbox.is_hovered(window) {
                return;
            }
            mouse_down_target.update(cx, |view, cx| {
                view.focus.focus(window, cx);
                let granularity = match event.click_count {
                    1 => SelectionGranularity::Character,
                    2 => SelectionGranularity::Word,
                    _ => SelectionGranularity::Paragraph,
                };
                let page_point = column.column_point_to_page(normalized_point(event.position, column_bounds));
                view.begin_selection(page_index, column.page_column_index(), page_point, granularity, cx);
                cx.stop_propagation();
            });
        });

        let column_bounds = prepaint.column_bounds;
        let column = self.column;
        let page_index = self.page_index;
        let mouse_move_target = self.view.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
            if phase != DispatchPhase::Bubble || !event.dragging() {
                return;
            }
            mouse_move_target.update(cx, |view, cx| {
                let page_point = column.column_point_to_page(normalized_point(event.position, column_bounds));
                if view.update_selection_to(page_index, column.page_column_index(), page_point, cx) {
                    cx.stop_propagation();
                }
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
            return match self.error.clone() {
                Some(message) => components::reader_document_message(message).into_any_element(),
                None => components::empty_document().into_any_element(),
            };
        }
        let mut packed_columns: Vec<(usize, f32, Vec<(usize, AnyElement)>)> = Vec::new();
        for column in self.fit_width_plan.columns().iter().copied() {
            let page_index = column.page_index();
            let element = self
                .pages
                .iter()
                .find(|(candidate, _)| *candidate == page_index)
                .map(|(_, page)| {
                    let page = page.clone();
                    let selection = if self.selection_page == Some(page_index) { self.selection.as_ref().into_iter().flat_map(PdfTextSelection::rects).filter_map(|rect| column.page_rect_to_column(*rect)).collect() } else { Vec::new() };
                    let search_hits = self
                        .search_hits
                        .iter()
                        .enumerate()
                        .filter(|(_, hit)| hit.page_index() == page_index)
                        .filter_map(|(index, hit)| {
                            let page_bounds = page.map_rect(hit.bounds())?;
                            column.page_rect_to_column(page_bounds).map(|bounds| (bounds, self.active_search_hit == Some(index)))
                        })
                        .collect();
                    let annotations = self
                        .annotations
                        .iter()
                        .filter(|annotation| annotation.page_index == page_index)
                        .flat_map(|annotation| {
                            annotation.rects.iter().filter_map(|rect| {
                                let page_bounds = page.map_rect(*rect)?;
                                column.page_rect_to_column(page_bounds).map(|bounds| (bounds, annotation.style, annotation.color))
                            })
                        })
                        .collect();
                    PdfPageElement { view: cx.entity(), focus: self.focus.clone(), page_index, column, page, selection, annotations, search_hits, interaction_style: self.interaction_style }.into_any_element()
                })
                .unwrap_or_else(|| div().w(px(column.display_width())).h(px(column.display_height())).flex_shrink_0().into_any_element());
            let display_column_index = column.display_column_index();
            if let Some((_, _, packed_pages)) = packed_columns.last_mut().filter(|(index, _, _)| *index == display_column_index) {
                packed_pages.push((page_index, element));
            } else {
                packed_columns.push((display_column_index, column.display_width(), vec![(page_index, element)]));
            }
        }
        let mut rows: Vec<(usize, Vec<AnyElement>)> = Vec::new();
        for (display_column_index, width, pages) in packed_columns {
            let mut stacked_pages = Vec::new();
            let mut previous_page = None;
            for (page_index, page) in pages {
                if previous_page.is_some_and(|previous| previous != page_index) {
                    stacked_pages.push(div().w_full().h(px(1.0)).flex_shrink_0().bg(gpui::rgb(0x000000)).into_any_element());
                }
                previous_page = Some(page_index);
                stacked_pages.push(page);
            }
            let row_index = display_column_index / self.visible_page_count.max(1);
            if rows.last().is_none_or(|(index, _)| *index != row_index) {
                rows.push((row_index, Vec::new()));
            }
            let row_columns = &mut rows.last_mut().expect("row was just inserted").1;
            if !row_columns.is_empty() {
                row_columns.push(div().w(px(1.0)).h_full().flex_shrink_0().bg(gpui::rgb(0x000000)).into_any_element());
            }
            row_columns.push(div().id(("pdf-display-column", display_column_index)).w(px(width)).flex().flex_col().flex_shrink_0().children(stacked_pages).into_any_element());
        }
        let row_height = self.page_display_height();
        let document = div().w_full().flex().flex_col().flex_shrink_0().children(rows.into_iter().map(|(row_index, columns)| {
            div().id(("pdf-display-row", row_index)).w_full().h(px(row_height)).flex().flex_shrink_0().items_start().justify_center().when(row_index > 0, |row| row.border_t_1().border_color(gpui::rgb(0x000000))).children(columns)
        }));
        let wheel_target = cx.entity();
        components::reader_pdf_scroll_surface(&self.scroll_handle)
            .on_scroll_wheel(move |event, _, cx| {
                let delta_y = match event.delta {
                    ScrollDelta::Pixels(delta) => f32::from(delta.y),
                    ScrollDelta::Lines(delta) => delta.y * 16.0,
                };
                if delta_y != 0.0 {
                    wheel_target.update(cx, |view, cx| view.sync_vertical_scroll_target(delta_y, cx));
                }
            })
            .child(document)
            .into_any_element()
    }
}

fn normalized_bounds(rect: NormalizedRect, page: Bounds<Pixels>) -> Bounds<Pixels> {
    Bounds { origin: point(page.origin.x + page.size.width * rect.left(), page.origin.y + page.size.height * rect.top()), size: size(page.size.width * rect.width(), page.size.height * rect.height()) }
}

fn annotation_paint_rects(rect: NormalizedRect, style: PdfAnnotationStyle) -> SmallVec<[NormalizedRect; 16]> {
    let mut painted = SmallVec::new();
    match style {
        PdfAnnotationStyle::Highlight => painted.push(rect),
        PdfAnnotationStyle::Underline => {
            let height = (rect.height() * 0.12).max(0.0025).min(rect.height());
            painted.push(NormalizedRect::new(rect.left(), rect.bottom() - height, rect.width(), height));
        }
        PdfAnnotationStyle::Squiggly => {
            let height = (rect.height() * 0.12).max(0.0025).min(rect.height());
            let segment_count = ((rect.width() / (rect.height() * 0.35).max(0.008)).round() as usize).clamp(4, 24);
            let segment_width = rect.width() / segment_count as f32;
            for index in 0..segment_count {
                let offset = if index % 2 == 0 { height } else { 0.0 };
                painted.push(NormalizedRect::new(rect.left() + segment_width * index as f32, rect.bottom() - height - offset, segment_width, height));
            }
        }
        PdfAnnotationStyle::Strikethrough => {
            let height = (rect.height() * 0.1).max(0.0025).min(rect.height());
            painted.push(NormalizedRect::new(rect.left(), rect.top() + (rect.height() - height) * 0.52, rect.width(), height));
        }
    }
    painted
}

fn normalized_point(position: Point<Pixels>, page: Bounds<Pixels>) -> (f32, f32) {
    ((f32::from(position.x - page.origin.x) / f32::from(page.size.width).max(1.0)).clamp(0.0, 1.0), (f32::from(position.y - page.origin.y) / f32::from(page.size.height).max(1.0)).clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_count_groups_consecutive_fit_width_columns() {
        assert_eq!(visible_flow_indices(0, 8, 3), vec![0, 1, 2]);
        assert_eq!(visible_flow_indices(2, 8, 3), vec![0, 1, 2]);
        assert_eq!(visible_flow_indices(3, 8, 3), vec![3, 4, 5]);
        assert_eq!(visible_flow_indices(6, 8, 3), vec![6, 7]);
    }

    #[test]
    fn navigation_advances_by_configured_page_count() {
        assert_eq!(next_flow_index(0, 8, 3), Some(3));
        assert_eq!(next_flow_index(2, 8, 3), Some(3));
        assert_eq!(next_flow_index(3, 8, 3), Some(6));
        assert_eq!(previous_flow_index(5, 8, 3), Some(0));
        assert_eq!(previous_flow_index(6, 8, 3), Some(3));
        assert_eq!(previous_flow_index(2, 8, 3), None);
    }

    #[test]
    fn single_navigation_reaches_each_slice_before_the_next_page() {
        let logical_columns = [(0, 0), (0, 1), (1, 0), (2, 0), (2, 1), (2, 2)];
        let mut flow_index = 0;
        let mut visited = vec![logical_columns[flow_index]];
        while let Some(next) = next_flow_index(flow_index, logical_columns.len(), 1) {
            flow_index = next;
            visited.push(logical_columns[flow_index]);
        }
        assert_eq!(visited, logical_columns);
        assert_eq!(previous_flow_index(2, logical_columns.len(), 1), Some(1));
    }

    #[test]
    fn automatic_layout_uses_the_available_reader_width() {
        assert_eq!(resolve_page_count(PdfPageLayout::Automatic, AUTO_SPREAD_MIN_WIDTH - 1.0), 1);
        assert_eq!(resolve_page_count(PdfPageLayout::Automatic, AUTO_SPREAD_MIN_WIDTH), 2);
        assert_eq!(resolve_page_count(PdfPageLayout::Single, AUTO_SPREAD_MIN_WIDTH * 2.0), 1);
        assert_eq!(resolve_page_count(PdfPageLayout::Spread, 200.0), 2);
    }

    #[test]
    fn display_cache_evicts_oldest_page_with_a_byte_budget() {
        fn page(byte_len: usize) -> Arc<DisplayPage> {
            let image = RgbaImage::new(1, 1);
            Arc::new(DisplayPage { image: Arc::new(RenderImage::new(SmallVec::from_vec(vec![Frame::new(image)]))), text_layout: Arc::new(PdfPageTextLayout::default()), crop: PixelCrop::full(1, 1), byte_len, break_avoidance: Arc::from([]) })
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
