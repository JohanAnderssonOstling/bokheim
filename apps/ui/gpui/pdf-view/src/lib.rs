use gpui::prelude::*;
use gpui::{
    App, Bounds, ClipboardItem, ContentMask, Context, Corners, CursorStyle, DispatchPhase, Element, ElementId, Entity, EventEmitter, FocusHandle, GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId, IntoElement, KeyContext,
    KeyDownEvent, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Render, RenderImage, ScrollDelta, ScrollHandle, Style, Window, div, fill, point, px, rgba, size,
};
use image::{Frame, RgbaImage};
use pdf_reader_core::{
    FitWidthColumn, FitWidthPage, FitWidthPlan, FitWidthViewport, NormalizedRect, PageRenderKey, PdfDocumentInfo, PdfDocumentSession, PdfPageTextLayout, PdfResult, PdfSearchHit, PdfTextSelection, PixelCrop, ReaderInteractionStyle,
    SelectionGranularity, SelectionOptions, crop_normalized_rect,
};
use smallvec::SmallVec;
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Seek};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use ui_components as components;

const DISPLAY_CACHE_BUDGET: usize = 128 * 1024 * 1024;
const RENDER_WIDTH_BUCKET: f32 = 256.0;
const SEARCH_PAGE_CHUNK: usize = 16;
const LINE_SCROLL_HEIGHT: f32 = 16.0;
const MIN_ZOOM: f32 = 0.05;
/// US Letter, used only until the real page size is known.
const DEFAULT_PAGE_WIDTH_POINTS: f32 = 612.0;
const DEFAULT_PAGE_HEIGHT_POINTS: f32 = 792.0;
const FIT_WIDTH_SURFACE_PADDING: f32 = 0.0;
const FIT_WIDTH_COLUMN_GAP: f32 = 1.0;
const PDF_PAGE_BOUNDARY_GAP: f32 = 1.0;
const DEFAULT_AVAILABLE_WIDTH: f32 = 948.0;
const DEFAULT_AVAILABLE_HEIGHT: f32 = 800.0;
pub const MIN_VISIBLE_PAGE_COUNT: usize = 1;
pub const MAX_VISIBLE_PAGE_COUNT: usize = 6;

/// How large a page is drawn.
///
/// All three modes settle the same quantity — `zoom`, in pixels per PDF point —
/// and differ only in which end is held fixed. Because margin trimming and
/// mixed page sizes change a page's effective width, the page count these
/// derive is a property of the page being read, not of the document.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PdfZoomMode {
    /// You choose how many pages sit side by side; the zoom follows.
    FitWidth { columns: usize },
    /// You choose the zoom; the number of pages that fit follows.
    Fixed { zoom: f32 },
    /// A whole page's height fills the viewport; the zoom, and then the number
    /// of pages that fit, follow.
    FitHeight,
}

impl Default for PdfZoomMode {
    fn default() -> Self {
        Self::FitWidth { columns: 1 }
    }
}

/// A [`PdfZoomMode`] resolved against the viewport and the page being read.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ResolvedLayout {
    columns: usize,
    column_width: f32,
    zoom: f32,
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
    AnnotationActivated { id: String, position: Point<Pixels> },
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
    zoom_mode: PdfZoomMode,
    /// Device pixels per logical pixel, sampled from the window each frame so
    /// pages are rastered at the resolution they are actually painted at.
    device_pixel_ratio: f32,
    available_width: f32,
    available_height: f32,
    trim_margins: bool,
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
    active_annotation_id: Option<String>,
    search_hits: Vec<PdfSearchHit>,
    active_search_hit: Option<usize>,
    pending_search_scroll: Option<(usize, NormalizedRect)>,
    pending_full_page_position: Option<f32>,
    interaction_style: ReaderInteractionStyle,
    /// Absolute offset through the wrapped fit-width stream. One viewport
    /// height is one logical display column; the remainder is painted as a
    /// vertical translation inside every visible screen column.
    flow_offset: f32,
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
    base_byte_budget: usize,
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
    annotations: Vec<(String, NormalizedRect, PdfAnnotationStyle, u32, bool)>,
    search_hits: Vec<(NormalizedRect, bool)>,
    interaction_style: ReaderInteractionStyle,
    instance_index: usize,
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
            zoom_mode: PdfZoomMode::default(),
            device_pixel_ratio: 1.0,
            available_width: DEFAULT_AVAILABLE_WIDTH,
            available_height: DEFAULT_AVAILABLE_HEIGHT,
            trim_margins: true,
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
            active_annotation_id: None,
            search_hits: Vec::new(),
            active_search_hit: None,
            pending_search_scroll: None,
            pending_full_page_position: None,
            interaction_style: ReaderInteractionStyle::default(),
            flow_offset: 0.0,
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

    /// How many pages currently sit side by side. Chosen directly in
    /// [`PdfZoomMode::FitWidth`] and derived from the zoom otherwise.
    pub fn visible_page_count(&self) -> usize {
        self.resolve_layout().columns
    }

    pub fn set_visible_page_count(&mut self, count: usize, cx: &mut Context<Self>) {
        self.set_zoom_mode(PdfZoomMode::FitWidth { columns: count.clamp(MIN_VISIBLE_PAGE_COUNT, MAX_VISIBLE_PAGE_COUNT) }, cx);
    }

    pub fn set_available_width(&mut self, available_width: f32, cx: &mut Context<Self>) {
        if available_width.is_finite() && available_width > 0.0 && (self.available_width - available_width).abs() >= 1.0 {
            let page_position = self.current_page_position();
            self.available_width = available_width;
            self.rebuild_fit_width_plan();
            self.restore_page_position(page_position);
            self.request_current_pages(cx);
            cx.notify();
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
        self.flow_offset > 0.5
    }

    pub fn can_next_page(&self) -> bool {
        self.flow_offset + 0.5 < self.max_flow_offset()
    }

    pub fn visible_page_range(&self) -> Option<(usize, usize)> {
        let columns = self.visible_columns();
        Some((columns.first()?.page_index(), columns.last()?.page_index()))
    }

    pub fn next_page(&mut self, cx: &mut Context<Self>) {
        let target = self.keyboard_page_target(true);
        self.clear_selection();
        self.set_flow_offset(target, cx);
    }

    pub fn previous_page(&mut self, cx: &mut Context<Self>) {
        let target = self.keyboard_page_target(false);
        self.clear_selection();
        self.set_flow_offset(target, cx);
    }

    fn keyboard_page_target(&self, forward: bool) -> f32 {
        let stride = self.flow_column_height();
        let unsnapped = if forward { self.flow_offset + stride } else { self.flow_offset - stride };
        if self.zoom_mode == PdfZoomMode::FitHeight {
            return unsnapped;
        }
        let offsets = self.atomic_flow_offsets();
        if forward { forward_page_snap_offset(self.flow_offset, unsnapped, &offsets).unwrap_or(unsnapped) } else { backward_page_snap_offset(self.flow_offset, unsnapped, &offsets).unwrap_or(unsnapped) }
    }

    pub fn next_item(&mut self, cx: &mut Context<Self>) {
        let offsets = self.atomic_flow_offsets();
        let Some(target) = next_snap_offset(self.flow_offset, &offsets) else {
            return;
        };
        self.clear_selection();
        self.set_flow_offset(target, cx);
    }

    pub fn previous_item(&mut self, cx: &mut Context<Self>) {
        let offsets = self.atomic_flow_offsets();
        let Some(target) = previous_snap_offset(self.flow_offset, &offsets) else {
            return;
        };
        self.clear_selection();
        self.set_flow_offset(target, cx);
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

    /// The width every page is rastered at, in device pixels.
    ///
    /// A column is painted `page_display_width()` wide, so rastering at a fixed
    /// width both blurs wide columns and wastes most of the buffer on narrow
    /// ones — six columns of a spread are a fraction of the width of one, but
    /// used to cost the same memory and render time per page. The result is
    /// rounded up to a bucket so that dragging a window edge or stepping the
    /// zoom keeps hitting the same cached pages.
    fn render_target_width(&self) -> u16 {
        let device_width = self.page_display_width() * self.device_pixel_ratio.max(1.0);
        let bucketed = (device_width / RENDER_WIDTH_BUCKET).ceil() * RENDER_WIDTH_BUCKET;
        PageRenderKey::new(0, bucketed.clamp(0.0, u16::MAX as f32) as u16).target_width()
    }

    pub fn zoom_mode(&self) -> PdfZoomMode {
        self.zoom_mode
    }

    /// The zoom every mode resolves to, in pixels per PDF point.
    pub fn zoom(&self) -> f32 {
        self.resolve_layout().zoom
    }

    pub fn set_zoom_mode(&mut self, zoom_mode: PdfZoomMode, cx: &mut Context<Self>) {
        if self.zoom_mode == zoom_mode {
            return;
        }
        let page_position = self.current_page_position();
        self.zoom_mode = zoom_mode;
        self.rebuild_fit_width_plan();
        self.restore_page_position(page_position);
        self.clear_selection();
        self.request_current_pages(cx);
        cx.notify();
    }

    /// Switches to a fixed zoom, letting the page count fall out of it.
    pub fn set_zoom(&mut self, zoom: f32, cx: &mut Context<Self>) {
        let zoom = zoom.clamp(MIN_ZOOM, self.width_fit_zoom(MIN_VISIBLE_PAGE_COUNT));
        self.set_zoom_mode(PdfZoomMode::Fixed { zoom }, cx);
    }

    pub fn change_zoom(&mut self, delta: f32, cx: &mut Context<Self>) {
        self.set_zoom(self.zoom() + delta, cx);
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
        self.search_hits.clear();
        self.active_search_hit = None;
        self.pending_search_scroll = None;
        let page_count = self.page_count();
        cx.spawn(async move |this, cx| {
            let mut page_index = 0;
            while page_index < page_count {
                let last = (page_index + SEARCH_PAGE_CHUNK).min(page_count);
                let session = session.clone();
                let query = query.clone();
                let chunk = cx
                    .background_executor()
                    .spawn(async move {
                        let session = session.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                        (page_index..last).try_fold(Vec::new(), |mut hits, page| {
                            hits.extend(session.search_page(page, &query)?);
                            Ok::<_, pdf_reader_core::PdfError>(hits)
                        })
                    })
                    .await;
                if !this.update(cx, |this, cx| this.apply_search_chunk(generation, chunk, cx)).unwrap_or(false) {
                    break;
                }
                page_index = last;
            }
            let _ = this.update(cx, |this, cx| this.finish_search(generation, cx));
        })
        .detach();
    }

    /// Folds one chunk of hits in, reporting whether the search that produced
    /// them is still the one the reader is waiting for.
    fn apply_search_chunk(&mut self, generation: u64, chunk: PdfResult<Vec<PdfSearchHit>>, cx: &mut Context<Self>) -> bool {
        if self.search_generation != generation {
            return false;
        }
        let hits = match chunk {
            Ok(hits) => hits,
            Err(error) => {
                self.error = Some(error.to_string());
                cx.notify();
                return false;
            }
        };
        if hits.is_empty() {
            return true;
        }
        let first_chunk_with_hits = self.search_hits.is_empty();
        self.search_hits.extend(hits);
        // Jump to the first match as soon as it is known, rather than after the
        // whole document has been scanned.
        if first_chunk_with_hits && let Some(hit) = self.search_hits.first().copied() {
            self.active_search_hit = Some(0);
            self.pending_search_scroll = Some((hit.page_index(), hit.bounds()));
            self.current_page = hit.page_index();
            self.current_page_column = 0;
            self.reset_page_scroll();
            self.clear_selection();
            self.request_current_pages(cx);
        }
        cx.emit(PdfViewEvent::SearchChanged { current: usize::from(!self.search_hits.is_empty()), total: self.search_hits.len() });
        cx.notify();
        true
    }

    fn finish_search(&mut self, generation: u64, cx: &mut Context<Self>) {
        self.search_in_flight = false;
        if self.search_generation == generation {
            cx.emit(PdfViewEvent::SearchChanged { current: usize::from(!self.search_hits.is_empty()), total: self.search_hits.len() });
        }
        if let Some((next_generation, next_query)) = self.pending_search.take().filter(|(next_generation, _)| *next_generation == self.search_generation) {
            self.start_search(next_generation, next_query, cx);
        }
        cx.notify();
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

    pub fn set_active_annotation(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        if self.active_annotation_id == id {
            return;
        }
        self.active_annotation_id = id;
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
        page_indices.into_iter().map(|page_index| PageRenderKey::with_margin_trim(page_index, self.render_target_width(), self.trim_margins)).collect()
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
                                let keys = this.visible_render_keys();
                                let mut visible_pages = Vec::with_capacity(keys.len());
                                for key in &keys {
                                    let page = converted_pages.iter().find(|(rendered_key, _)| rendered_key == key).map(|(_, page)| page.clone()).or_else(|| this.display_cache.get(*key));
                                    if let Some(page) = page {
                                        visible_pages.push((key.page_index(), page));
                                    }
                                }
                                this.display_cache.reserve(visible_pages.iter().map(|(_, page)| page.byte_len).sum());
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

    fn reset_page_scroll(&mut self) {
        self.flow_offset = self.current_cursor().and_then(|cursor| self.column_for_cursor(cursor)).map(|column| self.flow_offset_for_column(column, 0.0)).unwrap_or(0.0).clamp(0.0, self.max_flow_offset());
        self.scroll_handle.set_offset(point(Pixels::ZERO, Pixels::ZERO));
    }

    fn sync_flow_scroll_target(&mut self, delta_y: f32, cx: &mut Context<Self>) {
        // Wheel/trackpad input remains continuous by design. Only discrete
        // keyboard navigation snaps to line and page-fragment boundaries.
        self.set_flow_offset(self.flow_offset - delta_y, cx);
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
        self.flow_offset = self
            .flow_offset_for_page_y(page_index, target_y)
            .unwrap_or_else(|| self.current_cursor().and_then(|cursor| self.column_for_cursor(cursor)).map(|column| self.flow_offset_for_column(column, 0.0)).unwrap_or(0.0))
            .clamp(0.0, self.max_flow_offset());
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

    /// The page the layout is measured against.
    ///
    /// Trimming changes a page's effective size, so this follows the page being
    /// read rather than the document, and the page count changes with it.
    fn reference_page_size(&self) -> (f32, f32) {
        self.effective_page_sizes
            .get(&(self.current_page, self.trim_margins))
            .copied()
            .or_else(|| self.info.as_ref()?.page(self.current_page).map(|page| (page.width_points(), page.height_points())))
            .filter(|(width, height)| width.is_finite() && height.is_finite() && *width > 0.0 && *height > 0.0)
            .unwrap_or((DEFAULT_PAGE_WIDTH_POINTS, DEFAULT_PAGE_HEIGHT_POINTS))
    }

    fn width_fit_zoom(&self, columns: usize) -> f32 {
        width_fit_zoom(columns, self.available_width, self.reference_page_size().0)
    }

    fn resolve_layout(&self) -> ResolvedLayout {
        resolve_layout(self.zoom_mode, self.available_width, self.page_display_height(), self.reference_page_size())
    }

    fn page_display_width(&self) -> f32 {
        self.resolve_layout().column_width
    }

    fn page_display_height(&self) -> f32 {
        (self.available_height - FIT_WIDTH_SURFACE_PADDING * 2.0).max(1.0)
    }

    fn flow_column_height(&self) -> f32 {
        fit_page_flow_column_height(self.zoom_mode, self.visible_page_count(), self.page_display_height(), self.fit_width_plan.columns())
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
        self.fit_width_plan = match self.zoom_mode {
            PdfZoomMode::FitHeight => FitWidthPlan::fit_page(pages, viewport),
            PdfZoomMode::FitWidth { .. } | PdfZoomMode::Fixed { .. } => self.wrapped_plan(&pages, viewport),
        };
    }

    /// Wraps every page into fit-width columns, degrading instead of leaving
    /// the reader with nothing to show.
    ///
    /// The planner rejects a whole document over one unplannable page, and an
    /// empty plan renders no columns, so the reader would sit on "Rendering PDF
    /// page…" forever. A rejected page costs break avoidance first, then
    /// wrapping altogether, and only ever the layout — never the document.
    fn wrapped_plan(&self, pages: &[FitWidthPage], viewport: FitWidthViewport) -> FitWidthPlan {
        let with_bands = pages.iter().copied().map(|page| (page, self.page_break_avoidance.get(&(page.page_index(), self.trim_margins)).map(AsRef::as_ref).unwrap_or(&[])));
        match FitWidthPlan::new_avoiding_breaks_with_page_gap(with_bands, viewport, PDF_PAGE_BOUNDARY_GAP) {
            Ok(plan) => plan,
            Err(error) => {
                log::warn!("planning PDF fit-width columns: {error}; retrying without break avoidance");
                FitWidthPlan::new(pages.iter().copied(), viewport).unwrap_or_else(|error| {
                    log::warn!("planning PDF fit-width columns: {error}; falling back to whole pages");
                    FitWidthPlan::fit_page(pages.iter().copied(), viewport)
                })
            }
        }
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

    fn visible_columns(&self) -> Vec<FitWidthColumn> {
        let Some(total) = self.display_column_count() else {
            return Vec::new();
        };
        let visible = visible_flow_indices(self.flow_base_index(), total, self.visible_page_count());
        self.fit_width_plan.columns().iter().copied().filter(|column| visible.contains(&column.display_column_index())).collect()
    }

    fn render_window_columns(&self) -> Vec<FitWidthColumn> {
        let Some(total) = self.display_column_count() else { return Vec::new() };
        let first_visible = self.flow_base_index().min(total.saturating_sub(1));
        let first = first_visible.saturating_sub(1);
        let last = (first_visible + self.visible_page_count().max(1)).min(total.saturating_sub(1));
        self.fit_width_plan.columns().iter().copied().filter(|column| first <= column.display_column_index() && column.display_column_index() <= last).collect()
    }

    fn display_column_count(&self) -> Option<usize> {
        self.fit_width_plan.columns().last().map(|column| column.display_column_index() + 1)
    }

    fn flow_base_index(&self) -> usize {
        split_flow_offset(self.flow_offset, self.flow_column_height()).0
    }

    fn flow_remainder(&self) -> f32 {
        split_flow_offset(self.flow_offset, self.flow_column_height()).1
    }

    fn max_flow_offset(&self) -> f32 {
        let total = self.display_column_count().unwrap_or(0);
        total.saturating_sub(self.visible_page_count().max(1)) as f32 * self.flow_column_height()
    }

    fn column_offset_within_display(&self, target: FitWidthColumn) -> f32 {
        if self.zoom_mode == PdfZoomMode::FitHeight {
            return ((self.flow_column_height() - target.display_height()) * 0.5).max(0.0);
        }
        self.fit_width_plan
            .columns()
            .iter()
            .take_while(|column| column.flow_index() != target.flow_index())
            .filter(|column| column.display_column_index() == target.display_column_index())
            .map(|column| column.leading_page_gap() + column.display_height())
            .sum::<f32>()
            + target.leading_page_gap()
    }

    fn flow_offset_for_column(&self, column: FitWidthColumn, column_y: f32) -> f32 {
        column.display_column_index() as f32 * self.flow_column_height() + self.column_offset_within_display(column) + column_y.clamp(0.0, 1.0) * column.display_height()
    }

    fn flow_offset_for_page_y(&self, page_index: usize, display_y: f32) -> Option<f32> {
        let column = self.fit_width_plan.columns().iter().copied().find(|column| column.page_index() == page_index && column.page_point_to_column((0.5, display_y)).is_some())?;
        let column_y = column.page_point_to_column((0.5, display_y))?.1;
        Some(self.flow_offset_for_column(column, column_y))
    }

    fn flow_anchor(&self) -> Option<(FitWidthColumn, f32)> {
        let display_index = self.flow_base_index();
        let remainder = self.flow_remainder();
        let columns = self.fit_width_plan.columns().iter().copied().filter(|column| column.display_column_index() == display_index).collect::<Vec<_>>();
        let first = *columns.first()?;
        for column in &columns {
            let start = self.column_offset_within_display(*column);
            let end = start + column.display_height();
            if remainder <= end {
                let column_y = ((remainder - start) / column.display_height().max(1.0)).clamp(0.0, 1.0);
                return Some((*column, column_y));
            }
        }
        Some((*columns.last().unwrap_or(&first), 1.0))
    }

    fn set_flow_offset(&mut self, offset: f32, cx: &mut Context<Self>) {
        let previous_base = self.flow_base_index();
        self.flow_offset = offset.clamp(0.0, self.max_flow_offset());
        self.scroll_handle.set_offset(point(Pixels::ZERO, Pixels::ZERO));
        if let Some((column, _)) = self.flow_anchor() {
            self.set_cursor(FitWidthCursor { page_index: column.page_index(), page_column_index: column.page_column_index() });
        }
        if self.flow_base_index() != previous_base {
            self.request_current_pages(cx);
        } else {
            self.emit_page_changed(cx);
            cx.notify();
        }
    }

    fn atomic_flow_offsets(&self) -> Vec<f32> {
        let mut offsets = vec![0.0, self.max_flow_offset()];
        offsets.extend(self.fit_width_plan.columns().iter().copied().map(|column| self.flow_offset_for_column(column, 0.0)));
        for (&(page_index, trim_margins), bands) in &self.page_break_avoidance {
            if trim_margins != self.trim_margins {
                continue;
            }
            offsets.extend(bands.iter().filter_map(|(top, _)| self.flow_offset_for_page_y(page_index, *top)));
        }
        offsets.retain(|offset| offset.is_finite() && *offset >= 0.0 && *offset <= self.max_flow_offset());
        offsets.sort_by(f32::total_cmp);
        offsets.dedup_by(|left, right| (*left - *right).abs() < 0.5);
        offsets
    }

    fn current_page_position(&self) -> f32 {
        let Some((column, column_y)) = self.flow_anchor().or_else(|| self.current_cursor().and_then(|cursor| self.column_for_cursor(cursor)).map(|column| (column, 0.0))) else {
            return 0.0;
        };
        let display_y = column.column_point_to_page((0.5, column_y)).1;
        self.pages.iter().find(|(page_index, _)| *page_index == column.page_index()).map(|(_, page)| page.crop.display_point_to_full((0.5, display_y)).1).unwrap_or(display_y)
    }

    fn restore_page_position(&mut self, full_page_position: f32) {
        let columns = self.fit_width_plan.columns().iter().filter(|column| column.page_index() == self.current_page).collect::<Vec<_>>();
        if columns.is_empty() {
            self.current_page_column = 0;
            return;
        }
        let display_y = self.pages.iter().find(|(page_index, _)| *page_index == self.current_page).map(|(_, page)| page.crop.full_point_to_display((0.5, full_page_position)).1).unwrap_or(full_page_position);
        self.current_page_column = columns.iter().position(|column| column.page_point_to_column((0.5, display_y)).is_some()).unwrap_or_else(|| columns.len().saturating_sub(1)) as u32;
        self.flow_offset = self.flow_offset_for_page_y(self.current_page, display_y).unwrap_or(0.0).clamp(0.0, self.max_flow_offset());
    }
}

/// The zoom at which `columns` pages fill the viewport width.
fn width_fit_zoom(columns: usize, available_width: f32, page_width: f32) -> f32 {
    let columns = columns.max(1) as f32;
    let gaps = (columns - 1.0) * FIT_WIDTH_COLUMN_GAP;
    let usable_width = (available_width - FIT_WIDTH_SURFACE_PADDING * 2.0 - gaps).max(1.0);
    (usable_width / columns / page_width.max(f32::MIN_POSITIVE)).max(f32::MIN_POSITIVE)
}

/// How many columns of `column_width` fit across the viewport.
fn columns_that_fit(column_width: f32, available_width: f32) -> usize {
    let usable_width = (available_width - FIT_WIDTH_SURFACE_PADDING * 2.0).max(1.0);
    let columns = ((usable_width + FIT_WIDTH_COLUMN_GAP) / (column_width.max(f32::MIN_POSITIVE) + FIT_WIDTH_COLUMN_GAP)).floor();
    (columns.max(1.0) as usize).clamp(MIN_VISIBLE_PAGE_COUNT, MAX_VISIBLE_PAGE_COUNT)
}

/// Settles a [`PdfZoomMode`] into the zoom, column width and column count the
/// layout runs on.
///
/// One page filling the width is the ceiling in every mode: pages are only ever
/// split downwards, so anything wider would hang off the side with no way to
/// reach it.
fn resolve_layout(zoom_mode: PdfZoomMode, available_width: f32, available_height: f32, page_size: (f32, f32)) -> ResolvedLayout {
    let (page_width, page_height) = page_size;
    let widest_zoom = width_fit_zoom(MIN_VISIBLE_PAGE_COUNT, available_width, page_width);
    let (columns, zoom) = match zoom_mode {
        PdfZoomMode::FitWidth { columns } => {
            let columns = columns.clamp(MIN_VISIBLE_PAGE_COUNT, MAX_VISIBLE_PAGE_COUNT);
            (columns, width_fit_zoom(columns, available_width, page_width))
        }
        PdfZoomMode::Fixed { zoom } => {
            let zoom = zoom.clamp(MIN_ZOOM, widest_zoom);
            (columns_that_fit(page_width * zoom, available_width), zoom)
        }
        PdfZoomMode::FitHeight => {
            let zoom = (available_height / page_height.max(f32::MIN_POSITIVE)).clamp(MIN_ZOOM, widest_zoom);
            (columns_that_fit(page_width * zoom, available_width), zoom)
        }
    };
    ResolvedLayout { columns, column_width: (page_width * zoom).max(1.0), zoom }
}

fn visible_flow_indices(current: usize, total: usize, page_count: usize) -> Vec<usize> {
    if total == 0 {
        return Vec::new();
    }
    let page_count = page_count.max(1);
    let first = flow_window_start(current, total, page_count);
    (first..(first + page_count).min(total)).collect()
}

fn flow_window_start(current: usize, total: usize, page_count: usize) -> usize {
    current.min(total.saturating_sub(page_count.max(1)))
}

fn split_flow_offset(offset: f32, column_height: f32) -> (usize, f32) {
    let column_height = column_height.max(1.0);
    let offset = offset.max(0.0);
    ((offset / column_height).floor() as usize, offset.rem_euclid(column_height))
}

fn fit_page_flow_column_height(zoom_mode: PdfZoomMode, visible_page_count: usize, viewport_height: f32, columns: &[FitWidthColumn]) -> f32 {
    let viewport_height = viewport_height.max(1.0);
    if zoom_mode != PdfZoomMode::FitHeight || visible_page_count <= 1 {
        return viewport_height;
    }
    let tallest_page = columns.iter().copied().map(FitWidthColumn::display_height).fold(0.0, f32::max);
    if tallest_page <= 0.0 { viewport_height } else { (tallest_page + PDF_PAGE_BOUNDARY_GAP).min(viewport_height) }
}

fn next_snap_offset(current: f32, offsets: &[f32]) -> Option<f32> {
    offsets.iter().copied().find(|offset| *offset > current + 0.5)
}

fn previous_snap_offset(current: f32, offsets: &[f32]) -> Option<f32> {
    offsets.iter().copied().rev().find(|offset| *offset < current - 0.5)
}

fn forward_page_snap_offset(current: f32, target: f32, offsets: &[f32]) -> Option<f32> {
    offsets.iter().copied().take_while(|offset| *offset <= target + 0.5).filter(|offset| *offset > current + 0.5).last().or_else(|| next_snap_offset(current, offsets))
}

fn backward_page_snap_offset(current: f32, target: f32, offsets: &[f32]) -> Option<f32> {
    offsets.iter().copied().find(|offset| *offset >= target - 0.5 && *offset < current - 0.5).or_else(|| previous_snap_offset(current, offsets))
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
        Self { base_byte_budget: byte_budget, byte_budget, byte_len: 0, pages: HashMap::new(), recency: VecDeque::new() }
    }

    /// Raises the budget for as long as one render window needs more than it.
    ///
    /// A window that does not fit evicts the very pages it is about to ask for
    /// again, and the re-request renders and evicts in turn — the reader would
    /// render forever without ever painting. Holding the window is always
    /// cheaper than that loop.
    fn reserve(&mut self, window_bytes: usize) {
        self.byte_budget = self.base_byte_budget.max(window_bytes);
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
        Some(format!("gpui-pdf-column-{}-{}-{}", self.page_index, self.column.page_column_index(), self.instance_index).into())
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
            for (_, rect, style, color, active) in &self.annotations {
                if *active {
                    window.paint_quad(fill(normalized_bounds(*rect, prepaint.column_bounds), rgba(0x2563eb28)));
                }
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
        let annotation_hits = self.annotations.iter().map(|(id, rect, _, _, _)| (id.clone(), *rect)).collect::<Vec<_>>();
        let mouse_down_target = self.view.clone();
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || event.button != MouseButton::Left || !hitbox.is_hovered(window) {
                return;
            }
            mouse_down_target.update(cx, |view, cx| {
                view.focus.focus(window, cx);
                let normalized = normalized_point(event.position, column_bounds);
                if let Some((id, _)) = annotation_hits.iter().find(|(_, rect)| normalized_rect_contains(*rect, normalized)) {
                    cx.emit(PdfViewEvent::AnnotationActivated { id: id.clone(), position: event.position });
                    cx.stop_propagation();
                    return;
                }
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        self.device_pixel_ratio = window.scale_factor();
        if self.pages.is_empty() {
            return match self.error.clone() {
                Some(message) => components::reader_document_message(message, theme).into_any_element(),
                None if self.session.is_none() => components::reader_document_message("Opening PDF…", theme).into_any_element(),
                None => components::reader_document_message("Rendering PDF page…", theme).into_any_element(),
            };
        }
        let view = cx.entity();
        let column_width = self.page_display_width();
        let column_height = self.flow_column_height();
        let base_index = self.flow_base_index();
        let remainder = self.flow_remainder();
        let total = self.display_column_count().unwrap_or(0);
        let build_logical_column = |display_column_index: usize, instance_index: usize| {
            let mut fragments = Vec::new();
            for column in self.fit_width_plan.columns().iter().copied().filter(|column| column.display_column_index() == display_column_index) {
                if column.leading_page_gap() > 0.0 {
                    fragments.push(div().h(px(column.leading_page_gap())).flex_shrink_0().bg(theme.page_bg).into_any_element());
                }
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
                                    column.page_rect_to_column(page_bounds).map(|bounds| (annotation.id.clone(), bounds, annotation.style, annotation.color, self.active_annotation_id.as_deref() == Some(annotation.id.as_str())))
                                })
                            })
                            .collect();
                        PdfPageElement { view: view.clone(), focus: self.focus.clone(), page_index, column, page, selection, annotations, search_hits, interaction_style: self.interaction_style, instance_index }.into_any_element()
                    })
                    .unwrap_or_else(|| div().w(px(column.display_width())).h(px(column.display_height())).flex_shrink_0().into_any_element());
                fragments.push(element);
            }
            div()
                .id(("pdf-logical-column", instance_index))
                .w(px(column_width))
                .h(px(column_height))
                .flex()
                .flex_col()
                .flex_shrink_0()
                .when(self.zoom_mode == PdfZoomMode::FitHeight, |column| column.items_center().justify_center())
                .children(fragments)
        };
        let mut screen_columns = Vec::new();
        for slot_index in 0..self.visible_page_count().max(1) {
            if !screen_columns.is_empty() {
                screen_columns.push(div().w(px(FIT_WIDTH_COLUMN_GAP)).h_full().flex_shrink_0().bg(theme.border).into_any_element());
            }
            let current_index = base_index + slot_index;
            let mut stream = Vec::new();
            if current_index < total {
                stream.push(build_logical_column(current_index, slot_index * 2).into_any_element());
            }
            if current_index + 1 < total {
                stream.push(build_logical_column(current_index + 1, slot_index * 2 + 1).into_any_element());
            }
            screen_columns
                .push(div().id(("pdf-flow-slot", slot_index)).w(px(column_width)).h(px(column_height)).overflow_hidden().flex_shrink_0().child(div().w_full().flex().flex_col().mt(px(-remainder)).children(stream)).into_any_element());
        }
        let document = div().id("pdf-flow").w_full().h(px(column_height)).flex().flex_row().items_start().flex_shrink_0().children(screen_columns);
        let wheel_target = cx.entity();
        components::reader_pdf_scroll_surface(&self.scroll_handle, theme, cx)
            .on_scroll_wheel(move |event, _, cx| {
                let (vertical, horizontal) = match event.delta {
                    ScrollDelta::Pixels(delta) => (f32::from(delta.y), f32::from(delta.x)),
                    ScrollDelta::Lines(delta) => (delta.y * LINE_SCROLL_HEIGHT, delta.x * LINE_SCROLL_HEIGHT),
                };
                let flow_delta = if vertical != 0.0 { vertical } else { horizontal };
                if flow_delta != 0.0 {
                    wheel_target.update(cx, |view, cx| view.sync_flow_scroll_target(flow_delta, cx));
                    cx.stop_propagation();
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

fn normalized_rect_contains(rect: NormalizedRect, point: (f32, f32)) -> bool {
    rect.left() <= point.0 && point.0 <= rect.right() && rect.top() <= point.1 && point.1 <= rect.bottom()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapped_flow_keeps_screen_columns_fixed_while_the_remainder_moves_vertically() {
        assert_eq!(visible_flow_indices(0, 8, 3), vec![0, 1, 2]);
        assert_eq!(visible_flow_indices(1, 8, 3), vec![1, 2, 3]);
        assert_eq!(visible_flow_indices(2, 8, 3), vec![2, 3, 4]);
        assert_eq!(visible_flow_indices(7, 8, 3), vec![5, 6, 7]);
        assert_eq!(split_flow_offset(375.0, 800.0), (0, 375.0));
        assert_eq!(split_flow_offset(975.0, 800.0), (1, 175.0));
    }

    #[test]
    fn arrow_navigation_snaps_to_one_atomic_boundary() {
        let offsets = [0.0, 120.0, 185.0, 430.0];
        assert_eq!(next_snap_offset(0.0, &offsets), Some(120.0));
        assert_eq!(next_snap_offset(120.0, &offsets), Some(185.0));
        assert_eq!(previous_snap_offset(185.0, &offsets), Some(120.0));
        assert_eq!(previous_snap_offset(0.0, &offsets), None);
    }

    #[test]
    fn keyboard_page_navigation_stops_at_atomic_boundaries_near_the_viewport_step() {
        let offsets = [0.0, 120.0, 185.0, 430.0, 610.0];

        assert_eq!(forward_page_snap_offset(0.0, 200.0, &offsets), Some(185.0));
        assert_eq!(forward_page_snap_offset(185.0, 385.0, &offsets), Some(430.0));
        assert_eq!(backward_page_snap_offset(430.0, 230.0, &offsets), Some(185.0));
        assert_eq!(backward_page_snap_offset(610.0, 410.0, &offsets), Some(430.0));
    }

    #[test]
    fn annotation_hit_testing_uses_column_normalized_coordinates() {
        let rect = NormalizedRect::new(0.2, 0.3, 0.4, 0.2);
        assert!(normalized_rect_contains(rect, (0.2, 0.3)));
        assert!(normalized_rect_contains(rect, (0.6, 0.5)));
        assert!(!normalized_rect_contains(rect, (0.19, 0.4)));
        assert!(!normalized_rect_contains(rect, (0.4, 0.51)));
    }

    #[test]
    fn a_fixed_zoom_decides_how_many_pages_fit_across() {
        // Just under half the viewport, since two columns also cost a gap.
        assert_eq!(columns_that_fit(499.0, 1_000.0), 2);
        assert_eq!(columns_that_fit(500.0, 1_000.0), 1, "two of these plus the gap overflow by a pixel");
        assert_eq!(columns_that_fit(332.0, 1_000.0), 3);
        assert_eq!(columns_that_fit(1_000.0, 1_000.0), 1);
        assert_eq!(columns_that_fit(4_000.0, 1_000.0), 1, "a page wider than the viewport still occupies one column");
    }

    #[test]
    fn fit_width_and_fixed_zoom_are_two_ends_of_one_quantity() {
        let fitted = resolve_layout(PdfZoomMode::FitWidth { columns: 2 }, 1_000.0, 800.0, (500.0, 700.0));
        assert_eq!(fitted.columns, 2);

        // Feeding that zoom back in as a fixed zoom must reproduce the layout
        // it came from.
        let fixed = resolve_layout(PdfZoomMode::Fixed { zoom: fitted.zoom }, 1_000.0, 800.0, (500.0, 700.0));
        assert_eq!(fixed.columns, 2);
        assert!((fixed.column_width - fitted.column_width).abs() < 0.001);
    }

    #[test]
    fn fit_height_takes_its_zoom_from_the_page_height() {
        let layout = resolve_layout(PdfZoomMode::FitHeight, 1_000.0, 800.0, (490.0, 800.0));
        assert!((layout.zoom - 1.0).abs() < 0.001, "an 800pt page in 800px of height is one pixel per point");
        assert_eq!(layout.columns, 2, "at that zoom two 490px pages fit across 1000px");
    }

    #[test]
    fn no_mode_lets_a_page_grow_wider_than_the_viewport() {
        for mode in [PdfZoomMode::FitHeight, PdfZoomMode::Fixed { zoom: 99.0 }, PdfZoomMode::FitWidth { columns: 1 }] {
            let layout = resolve_layout(mode, 400.0, 5_000.0, (500.0, 700.0));
            assert!(layout.column_width <= 400.0 + 0.001, "{mode:?} produced a {}px column in a 400px viewport", layout.column_width);
        }
    }

    #[test]
    fn fit_page_spreads_use_the_fitted_page_height_as_the_vertical_stride() {
        let viewport = FitWidthViewport::new(400.0, 1_600.0).expect("viewport");
        let page = FitWidthPage::new(0, 600.0, 800.0).expect("page");
        let plan = FitWidthPlan::fit_page([page], viewport);
        let fitted_height = plan.columns()[0].display_height();

        assert!((fit_page_flow_column_height(PdfZoomMode::FitHeight, 2, 1_600.0, plan.columns()) - (fitted_height + PDF_PAGE_BOUNDARY_GAP)).abs() < 0.001);
        assert_eq!(fit_page_flow_column_height(PdfZoomMode::FitHeight, 1, 1_600.0, plan.columns()), 1_600.0);
        assert_eq!(fit_page_flow_column_height(PdfZoomMode::FitWidth { columns: 2 }, 2, 1_600.0, plan.columns()), 1_600.0);
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
