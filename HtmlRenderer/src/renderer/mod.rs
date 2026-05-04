use std::borrow::Cow;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::{env, fs};

use floem::ViewId;
use floem::action::exec_after;
use floem::kurbo::{Point, Rect, Size};
use floem::peniko::{Brush, Color};
use floem::prelude::Decorators;
use floem::reactive::{RwSignal, SignalUpdate, create_rw_signal};

use crate::document::{BoxType, Document, ImageFragment, InlineKind, Line};
use crate::layout;
use crate::resources::{ImagePipeline, ResourceProvider};
use crate::text::glyph_cache::GlyphCache;

mod highlight;
mod load;
mod nav;
pub(crate) mod painter;
mod render;
mod selection;
mod view;

pub(crate) use painter::{Painter, SoftwarePainter};

// =============================================================================
// Paint Direction
// =============================================================================

/// Direction of document rendering - affects how pages are laid out
#[derive(Clone, Copy, PartialEq)]
enum PaintDirection {
    /// Render from start_offset_y downward (normal reading direction)
    Forward,
    /// Render from end_offset_y upward (when navigating backward)
    Backward,
}

/// Target position when loading a new document
enum DocAnchor {
    /// Start at beginning of document
    Start,
    /// Start at end of document
    End,
    /// Start at specific glyph position
    #[allow(dead_code)]
    Glyph(u32),
}

// =============================================================================
// Render State (per-frame mutable state passed through paint calls)
// =============================================================================

/// Mutable state accumulated during a single render pass
struct RenderState {
    /// Cumulative Y offset from column breaks (lines pushed to next column)
    y_offset: f64,
    /// Current column index being rendered (0-based)
    col_index: f64,
    /// Set to true when rendering should stop (past visible area)
    should_stop: bool,
    /// Accumulated time spent in draw_text calls (for profiling)
    draw_text_time: Duration,
}

// =============================================================================
// Helper Types
// =============================================================================

/// Metrics for positioning inline decorations within column gaps
#[derive(Clone, Copy)]
struct InlineMetrics {
    /// X offset to center inline decorations in the gap
    offset: f64,
}

/// Cached screen position for a rendered line
#[derive(Clone, Copy)]
struct LineScreen {
    /// Screen coordinates where line was rendered
    point: Point,
    /// Which column the line was rendered in
    col_index: i32,
}

/// Text formatting state for markdown extraction
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct FormatState {
    bold: bool,
    italic: bool,
    strike: bool,
    code: bool,
}

// =============================================================================
// Column Layout - Computed once per viewport resize
// =============================================================================

/// Column-based layout parameters, recomputed when viewport changes
struct ColumnLayout {
    /// Width of each text column in pixels
    col_width: f64,
    /// Base column width before user scaling adjustments
    base_col_width: f64,
    /// Number of columns that fit in the viewport
    col_count: f64,
    /// Horizontal gap between columns
    col_gap: f64,
    /// Current viewport size
    size: Size,
    /// Display scale factor (for HiDPI)
    scale: f64,
}

impl Default for ColumnLayout {
    fn default() -> Self {
        Self { col_width: 600.0, base_col_width: 600.0, col_count: 0.0, col_gap: 0.0, size: Size::ZERO, scale: 1.0 }
    }
}

// =============================================================================
// Viewport State - Scroll position and pagination
// =============================================================================

/// Scroll position and pagination state
struct ViewportState {
    /// Current paint direction (forward/backward)
    direction: PaintDirection,
    /// Document Y offset for forward painting (top of visible area)
    start_offset_y: f64,
    /// Document Y offset for backward painting (bottom of visible area)
    end_offset_y: f64,
    /// Computed start offset when painting backward
    back_start_offset_y: f64,
    /// Anchor line index for backward painting
    back_anchor_end_line: Option<usize>,
    /// Whether we've reached the end of the document
    reached_end: bool,
    /// Start offset for next page (used in pagination)
    next_start_offset_y: f64,
}

impl Default for ViewportState {
    fn default() -> Self {
        Self { direction: PaintDirection::Forward, start_offset_y: 0.0, end_offset_y: 0.0, back_start_offset_y: 0.0, back_anchor_end_line: None, reached_end: false, next_start_offset_y: 0.0 }
    }
}

// =============================================================================
// Frame Cache - Transient state rebuilt each paint pass
// =============================================================================

/// Per-frame rendering cache, rebuilt on each paint
struct FrameCache {
    /// Index of first visible line on current page
    current_page_start_line: Option<usize>,
    /// Index of last visible line on current page
    current_page_end_line: Option<usize>,
    /// Screen positions of all lines (None if not visible)
    last_line_positions: Vec<Option<LineScreen>>,
}

impl Default for FrameCache {
    fn default() -> Self {
        Self { current_page_start_line: None, current_page_end_line: None, last_line_positions: Vec::new() }
    }
}

// =============================================================================
// Selection State - Text selection and extraction
// =============================================================================

/// State for text selection via mouse drag
struct SelectionState {
    /// Whether a drag selection is currently in progress
    is_selecting: bool,
    /// Glyph index where selection started (anchor point)
    selection_anchor: Option<u32>,
    /// Glyph index where selection currently ends (moves with mouse)
    selection_active: Option<u32>,
    /// Glyph index of a clicked link (pending navigation on mouse up)
    pending_link_glyph: Option<u32>,
    /// Extracted plain text from current selection
    selected_text: Option<String>,
    /// Extracted markdown from current selection
    selected_text_markdown: Option<String>,
    /// Whether cursor is currently over a clickable link
    link_cursor_active: bool,
    /// Whether current selection should snap to whole lines.
    select_whole_lines: bool,
}

impl Default for SelectionState {
    fn default() -> Self {
        Self { is_selecting: false, selection_anchor: None, selection_active: None, pending_link_glyph: None, selected_text: None, selected_text_markdown: None, link_cursor_active: false, select_whole_lines: false }
    }
}

// =============================================================================
// Highlight State - Text search and highlight
// =============================================================================

/// State for full-document text search and highlight navigation
struct HighlightState {
    /// Current search query (empty = no highlight)
    query: String,
    /// All matches as (start_glyph, end_glyph) pairs, sorted by start
    matches: Vec<(u32, u32)>,
    /// Index into `matches` for the currently focused match
    current_match: usize,
    /// Signal: (current_1based, total) — 0 total means no active search
    match_info: RwSignal<(usize, usize)>,
    /// Whether the search bar should be shown — toggled by Ctrl+F
    search_active: RwSignal<bool>,
    /// Pending query from the UI — polled in paint and consumed
    search_query_pending: RwSignal<Option<String>>,
    /// Pending navigation step set by UI: Some(1) = next, Some(-1) = prev
    search_nav: RwSignal<Option<i8>>,
}

impl HighlightState {
    fn new() -> Self {
        Self { query: String::new(), matches: Vec::new(), current_match: 0, match_info: create_rw_signal((0, 0)), search_active: create_rw_signal(false), search_query_pending: create_rw_signal(None), search_nav: create_rw_signal(None) }
    }
}

/// Signals for applying reader setting changes from external UI controls.
struct ReaderSettingsSignals {
    /// Pending column width request (px)
    column_width_pending: RwSignal<Option<f64>>,
    /// Pending root font size request (px)
    font_size_pending: RwSignal<Option<f32>>,
    /// Pending render scale request
    scale_pending: RwSignal<Option<f64>>,
    /// Pending line number visibility request
    line_numbers_pending: RwSignal<Option<bool>>,
    /// Current base column width (px)
    current_column_width: RwSignal<f64>,
    /// Current root font size (px)
    current_font_size: RwSignal<f32>,
    /// Current render scale
    current_scale: RwSignal<f64>,
    /// Current line number visibility
    current_line_numbers: RwSignal<bool>,
}

impl ReaderSettingsSignals {
    fn new(initial_column_width: f64, initial_font_size: f32, initial_scale: f64, initial_line_numbers: bool) -> Self {
        Self {
            column_width_pending: create_rw_signal(None),
            font_size_pending: create_rw_signal(None),
            scale_pending: create_rw_signal(None),
            line_numbers_pending: create_rw_signal(None),
            current_column_width: create_rw_signal(initial_column_width),
            current_font_size: create_rw_signal(initial_font_size),
            current_scale: create_rw_signal(initial_scale),
            current_line_numbers: create_rw_signal(initial_line_numbers),
        }
    }
}

// =============================================================================
// Navigation State - Multi-document navigation (e.g., EPUB chapters)
// =============================================================================

/// State for navigating between multiple documents (e.g., EPUB chapters)
struct NavigationState {
    /// List of document URIs in reading order
    document_uris: Vec<String>,
    /// Index of currently loaded document
    current_doc_index: usize,
    /// Reactive signal broadcasting (doc_index, glyph_position) for UI
    position_update: RwSignal<(usize, Option<u32>)>,
    /// Reactive signal broadcasting current position as EPUB CFI string
    cfi_position: RwSignal<Option<String>>,
    /// Reactive signal broadcasting (doc_index, anchor_id) for UI
    nav_anchor_signal: RwSignal<(usize, Option<String>)>,
    /// Reactive signal used to request navigation via href (e.g., TOC)
    nav_request_signal: RwSignal<Option<String>>,
    /// Reactive signal used to request history navigation: -1 back, +1 forward
    history_nav_signal: RwSignal<Option<i8>>,
    /// Whether link history can currently move backward
    can_history_back: RwSignal<bool>,
    /// Whether link history can currently move forward
    can_history_forward: RwSignal<bool>,
    /// Saved glyph position for persistence across sessions
    nav_anchor_glyph: Option<u32>,
    /// Flag to update nav_anchor_glyph after next paint completes
    pending_nav_anchor_update: bool,
    /// History of visited in-book locations (doc index + optional anchor id)
    location_history: Vec<(usize, Option<String>)>,
    /// Current cursor into `location_history`
    location_history_index: usize,
    toc_anchor_strings_by_doc: Vec<Vec<String>>,
    toc_anchor_filter: Option<HashSet<u16>>,
}

// =============================================================================
// Line Number Cache - Precomputed glyphs for line number rendering
// =============================================================================

/// Cached glyphs for rendering line numbers in the gutter
struct LineNumberCache {
    /// Glyph indices for digits 0-9
    digits: [u16; 10],
    /// Glyph index for space character (separator after number)
    space: u16,
    /// Horizontal gap between line numbers and content
    gap: f64,
}

impl Default for LineNumberCache {
    fn default() -> Self {
        Self { digits: [0; 10], space: 0, gap: 8.0 }
    }
}

type ClipboardWriter = Arc<dyn Fn(String) -> Result<(), String> + Send + Sync>;

// =============================================================================
// HTMLRenderer - Main renderer combining all state
// =============================================================================

pub struct HTMLRenderer {
    // --- Core ---
    id: ViewId,
    /// The parsed and laid-out document
    document: Document,
    /// Cache of shaped text glyphs
    glyph_cache: GlyphCache,
    /// Async pipeline for loading and decoding images
    image_pipeline: ImagePipeline,
    /// Resource provider for loading external files (CSS, images, etc.)
    provider: Arc<dyn ResourceProvider>,
    /// Base font size in pixels (default 16.0)
    root_font_size: f32,

    // --- Grouped State ---
    /// Column layout parameters (recomputed on viewport resize)
    layout: ColumnLayout,
    /// Scroll position and pagination state
    viewport: ViewportState,
    /// Per-frame rendering cache
    frame: FrameCache,
    /// Text selection state
    selection: SelectionState,
    /// Text search / highlight state
    highlight: HighlightState,
    /// External UI setting request signals
    settings: ReaderSettingsSignals,
    /// Multi-document navigation state
    nav: NavigationState,
    /// Current document title (from HTML head)
    doc_title_signal: RwSignal<Option<String>>,
    /// Current document header TOC entries (h1..h6 hierarchy)
    document_toc_signal: RwSignal<Vec<crate::TocEntry>>,
    /// Long-lived clipboard handle to keep ownership on Linux/Wayland.
    persistent_clipboard: Option<arboard::Clipboard>,
    /// Backend-agnostic clipboard writer used by copy actions
    clipboard_writer: ClipboardWriter,
    /// Line number rendering cache
    line_numbers: LineNumberCache,
    show_line_numbers: bool,
    origin: Point,
}

impl HTMLRenderer {
    fn default_clipboard_writer() -> ClipboardWriter {
        Arc::new(|text: String| {
            let mut clipboard = arboard::Clipboard::new().map_err(|arboard_error| format!("arboard init failed: {arboard_error}"))?;
            if let Err(arboard_error) = clipboard.set_text(text.clone()) {
                floem::Clipboard::set_contents(text).map_err(|floem_error| format!("arboard set failed: {arboard_error}; floem clipboard failed: {floem_error:?}"))?;
            }
            Ok(())
        })
    }

    pub fn set_clipboard_writer<F>(&mut self, writer: F)
    where
        F: Fn(String) -> Result<(), String> + Send + Sync + 'static,
    {
        self.clipboard_writer = Arc::new(writer);
    }

    fn ensure_persistent_clipboard(&mut self) -> Result<&mut arboard::Clipboard, String> {
        if self.persistent_clipboard.is_none() {
            self.persistent_clipboard = arboard::Clipboard::new().ok();
        }
        self.persistent_clipboard.as_mut().ok_or_else(|| "failed to initialize arboard clipboard".to_string())
    }

    fn set_clipboard_text(&mut self, text: String) -> Result<(), String> {
        // Keep a long-lived clipboard alive so Linux/Wayland retains selection ownership.
        if let Ok(clipboard) = self.ensure_persistent_clipboard() {
            clipboard.set_text(text.clone()).map_err(|error| format!("arboard set_text failed: {error}"))?;
            return Ok(());
        }
        (self.clipboard_writer)(text)
    }

    fn set_clipboard_image(&mut self, width: usize, height: usize, rgba: Vec<u8>) -> Result<(), String> {
        let clipboard = self.ensure_persistent_clipboard()?;
        clipboard.set_image(arboard::ImageData { width, height, bytes: Cow::Owned(rgba) }).map_err(|error| format!("arboard set_image failed: {error}"))
    }

    fn copy_selection_to_clipboard(&mut self) {
        if let Some(text) = self.selection.selected_text_markdown.as_ref() {
            if let Err(error) = self.set_clipboard_text(text.clone()) {
                eprintln!("clipboard write failed: {error}");
            }
        }
    }

    fn copy_image_to_clipboard(&mut self, image_idx: u32) {
        let Some(decoded) = self.image_pipeline.get_decoded(image_idx) else {
            eprintln!("clipboard image copy failed: image {image_idx} is not decoded yet");
            return;
        };
        let width = decoded.image.width as usize;
        let height = decoded.image.height as usize;
        let rgba = decoded.image.data.data().to_vec();
        if let Err(error) = self.set_clipboard_image(width, height, rgba) {
            eprintln!("clipboard image copy failed: {error}");
        }
    }

    fn build_document_toc_entries_for(doc: &Document) -> Vec<crate::TocEntry> {
        fn convert_nodes(doc: &Document, nodes: &[crate::document::DocumentTocNode]) -> Vec<crate::TocEntry> {
            nodes.iter().map(|node| crate::TocEntry { title: doc.strings.get(node.title).to_string(), link: doc.strings.get(node.href).to_string(), children: convert_nodes(doc, node.children.as_slice()) }).collect()
        }

        convert_nodes(doc, doc.document_toc_entries.as_slice())
    }

    pub fn set_render_column_width(&mut self, width: f64) {
        self.set_col_width(width);
    }

    fn configure_layout_for_viewport(&mut self, viewport_size: Size) {
        const MIN_GAP: f64 = 40.0;
        self.layout.size = viewport_size;
        self.layout.size /= self.layout.scale;
        let effective_col_width = if self.layout.size.width > 0.0 { self.layout.base_col_width.min(self.layout.size.width) } else { self.layout.base_col_width };

        // Avoid full relayout on tiny width deltas while the window is being resized.
        // Reflow is expensive and frequent reflow causes visible jitter.
        if (effective_col_width - self.layout.col_width).abs() > 24.0 {
            self.layout.col_width = effective_col_width;
            self.relayout_for_column_width();
        }
        let raw_col_count = ((self.layout.size.width + MIN_GAP) / (self.layout.col_width + MIN_GAP)).floor();
        self.layout.col_count = raw_col_count.max(1.0);
        self.layout.col_gap = ((self.layout.size.width - self.layout.col_count * self.layout.col_width) / (self.layout.col_count + 1.0)).max(0.0);
    }

    pub fn render_current_page_to_png(&mut self, width: u32, height: u32) -> Result<Vec<u8>, String> {
        if width == 0 || height == 0 {
            return Err("width and height must be greater than 0".to_string());
        }

        self.configure_layout_for_viewport(Size::new(width as f64, height as f64));
        let mut painter = SoftwarePainter::new(width, height);
        let prev_show_line_numbers = self.show_line_numbers;
        self.show_line_numbers = false;
        match self.viewport.direction {
            PaintDirection::Forward => {
                self.paint_forward(&mut painter);
            }
            PaintDirection::Backward => {
                self.paint_backward(&mut painter);
            }
        }
        self.show_line_numbers = prev_show_line_numbers;

        if self.nav.pending_nav_anchor_update {
            self.update_nav_anchor_from_layout();
            self.persist_nav_position();
            self.nav.pending_nav_anchor_update = false;
        }
        self.update_nav_signal();
        painter.encode_png()
    }

    pub fn from_document(document: Document, glyph_cache: GlyphCache, provider: Arc<dyn ResourceProvider>, document_uris: Vec<String>, current_doc_index: usize, nav_signal: RwSignal<(usize, Option<u32>)>) -> Self {
        // initialize renderer state and caches
        let root_font_size = document.root_font_size;
        let doc_title = document.title.clone();
        let document_toc_entries = Self::build_document_toc_entries_for(&document);
        let mut glyph_cache = glyph_cache;
        let line_numbers = Self::build_line_number_cache(&mut glyph_cache);
        let image_pipeline = ImagePipeline::new(Arc::new(document.images.clone()), provider.clone());

        Self {
            id: ViewId::new(),
            document,
            glyph_cache,
            image_pipeline,
            provider,
            root_font_size,
            layout: ColumnLayout::default(),
            viewport: ViewportState::default(),
            frame: FrameCache::default(),
            selection: SelectionState::default(),
            highlight: HighlightState::new(),
            settings: ReaderSettingsSignals::new(600.0, root_font_size, 1.0, true),
            nav: NavigationState {
                document_uris,
                current_doc_index,
                position_update: nav_signal,
                cfi_position: create_rw_signal(None),
                nav_anchor_signal: create_rw_signal((current_doc_index, None)),
                nav_request_signal: create_rw_signal(None),
                history_nav_signal: create_rw_signal(None),
                can_history_back: create_rw_signal(false),
                can_history_forward: create_rw_signal(false),
                nav_anchor_glyph: None,
                pending_nav_anchor_update: false,
                location_history: Vec::new(),
                location_history_index: 0,
                toc_anchor_strings_by_doc: Vec::new(),
                toc_anchor_filter: None,
            },
            line_numbers,
            show_line_numbers: true,
            doc_title_signal: create_rw_signal(doc_title),
            document_toc_signal: create_rw_signal(document_toc_entries),
            persistent_clipboard: arboard::Clipboard::new().ok(),
            clipboard_writer: Self::default_clipboard_writer(),
            origin: Point::ZERO,
        }
        .keyboard_navigable()
        .style(move |style| {
            style
                .width_full()
                .height_full()
                .flex_grow(1.0)
                .min_width(0)
                .min_height(0)
                .background(Brush::Solid(Color::WHITE))
        })
    }

    fn build_line_number_cache(glyph_cache: &mut GlyphCache) -> LineNumberCache {
        let mut digits = [0u16; 10];
        for i in 0..10 {
            let c = char::from(b'0' + i as u8);
            digits[i] = glyph_cache.shape_with_font_properties(
                c,
                12.0, // font_size
                400,  // font_weight (normal)
                crate::document::FontStyle::Normal,
                0x000000FF, // black color
                None,       // font_family
                None,       // family string
            );
        }
        let space = glyph_cache.shape_with_font_properties(
            ' ',
            12.0, // font_size
            400,  // font_weight (normal)
            crate::document::FontStyle::Normal,
            0x000000FF, // black color
            None,       // font_family
            None,       // family string
        );
        LineNumberCache { digits, space, gap: 8.0 }
    }

    pub fn from_source_path(path: PathBuf) -> Self {
        Self::from_source_path_with_nav(path, None)
    }

    pub fn from_epub_bytes(bytes: Vec<u8>) -> Self {
        Self::from_epub_bytes_with_nav(bytes, None)
    }

    pub fn from_source_path_with_nav(path: PathBuf, nav_state: Option<&str>) -> Self {
        let (provider, document_uris, start_index) = load::collect_documents(path);
        Self::from_provider_with_nav(provider, document_uris, start_index, nav_state)
    }

    pub fn from_epub_bytes_with_nav(bytes: Vec<u8>, nav_state: Option<&str>) -> Self {
        let (provider, document_uris, start_index) = load::collect_documents_from_epub_bytes(bytes);
        Self::from_provider_with_nav(provider, document_uris, start_index, nav_state)
    }

    fn from_provider_with_nav(provider: Arc<dyn ResourceProvider>, document_uris: Vec<String>, start_index: usize, nav_state: Option<&str>) -> Self {
        if document_uris.is_empty() {
            panic!("No HTML documents found for source");
        }
        let nav_signal = create_rw_signal((start_index, None));

        // Get nav state (raw string for CFI, or parsed position for old format)
        let nav_state_str = nav_state.map(|s| s.to_string()).or_else(|| Self::nav_state_path().and_then(|path| fs::read_to_string(path).ok()));

        let (doc_index, glyph_idx) = nav_state_str.as_deref().and_then(Self::parse_nav_state).unwrap_or((start_index, 0));
        let doc_index = doc_index.min(document_uris.len().saturating_sub(1));

        let (document, glyph_cache) = load::load_document(provider.clone(), &document_uris[doc_index], 20.0, 600.0);
        let mut renderer = HTMLRenderer::from_document(document, glyph_cache, provider, document_uris, doc_index, nav_signal);

        // If nav_state was a CFI, re-parse it with the loaded document to get exact position
        if let Some(nav_str) = nav_state_str.as_deref() {
            if nav_str.trim().starts_with("epubcfi(") {
                if renderer.restore_position_from_cfi(nav_str.trim()) {
                    // CFI restoration successful, update nav signal and return
                    renderer.update_nav_signal();
                    return renderer;
                }
            }
        }

        // Fallback to glyph_idx from old format
        if glyph_idx > 0 {
            renderer.restore_glyph_position(glyph_idx);
        }
        if renderer.nav.nav_anchor_glyph.is_none() {
            renderer.nav.nav_anchor_glyph = renderer.current_glyph_position();
        }
        renderer.update_nav_signal();
        renderer
    }

    pub fn position_update_signal(&self) -> RwSignal<(usize, Option<u32>)> {
        self.nav.position_update
    }

    pub fn cfi_position_signal(&self) -> RwSignal<Option<String>> {
        self.nav.cfi_position
    }

    pub fn nav_anchor_signal(&self) -> RwSignal<(usize, Option<String>)> {
        self.nav.nav_anchor_signal
    }

    pub fn current_column_width(&self) -> f64 {
        self.layout.base_col_width
    }

    pub fn current_font_size(&self) -> f32 {
        self.root_font_size
    }

    pub fn current_scale(&self) -> f64 {
        self.layout.scale
    }

    pub fn current_column_width_signal(&self) -> RwSignal<f64> {
        self.settings.current_column_width
    }

    pub fn current_font_size_signal(&self) -> RwSignal<f32> {
        self.settings.current_font_size
    }

    pub fn current_scale_signal(&self) -> RwSignal<f64> {
        self.settings.current_scale
    }

    pub fn current_line_numbers_enabled(&self) -> bool {
        self.show_line_numbers
    }

    pub fn current_line_numbers_signal(&self) -> RwSignal<bool> {
        self.settings.current_line_numbers
    }

    /// Signal used by external UI to request a column width update.
    /// Set to Some(width_px). The renderer consumes it on the next paint.
    pub fn column_width_request_signal(&self) -> RwSignal<Option<f64>> {
        self.settings.column_width_pending
    }

    /// Signal used by external UI to request root font size changes.
    /// Set to Some(size_px). The renderer consumes it on the next paint.
    pub fn font_size_request_signal(&self) -> RwSignal<Option<f32>> {
        self.settings.font_size_pending
    }

    /// Signal used by external UI to request zoom/scale changes.
    /// Set to Some(scale). The renderer consumes it on the next paint.
    pub fn scale_request_signal(&self) -> RwSignal<Option<f64>> {
        self.settings.scale_pending
    }

    /// Signal used by external UI to request line number visibility.
    /// Set to Some(true/false). The renderer consumes it on the next paint.
    pub fn line_numbers_request_signal(&self) -> RwSignal<Option<bool>> {
        self.settings.line_numbers_pending
    }

    pub fn nav_request_signal(&self) -> RwSignal<Option<String>> {
        self.nav.nav_request_signal
    }

    pub fn history_nav_signal(&self) -> RwSignal<Option<i8>> {
        self.nav.history_nav_signal
    }

    pub fn can_history_back_signal(&self) -> RwSignal<bool> {
        self.nav.can_history_back
    }

    pub fn can_history_forward_signal(&self) -> RwSignal<bool> {
        self.nav.can_history_forward
    }

    pub fn document_title_signal(&self) -> RwSignal<Option<String>> {
        self.doc_title_signal
    }

    pub fn toc(&self) -> std::io::Result<Option<Vec<crate::TocEntry>>> {
        self.provider.toc()
    }

    pub fn document_toc(&self) -> Vec<crate::TocEntry> {
        Self::build_document_toc_entries_for(&self.document)
    }

    pub fn document_toc_signal(&self) -> RwSignal<Vec<crate::TocEntry>> {
        self.document_toc_signal
    }

    /// Get the current reading position as an EPUB CFI string
    pub fn get_current_cfi(&self) -> Option<String> {
        use crate::document::cfi;

        let glyph_idx = self.nav.nav_anchor_glyph.or_else(|| self.current_glyph_position())?;

        cfi::generate_cfi(&self.document, self.nav.current_doc_index, glyph_idx)
    }

    pub fn set_toc_anchor_strings_by_doc(&mut self, anchors_by_doc: Vec<Vec<String>>) {
        self.nav.toc_anchor_strings_by_doc = anchors_by_doc;
        if self.nav.toc_anchor_strings_by_doc.len() < self.nav.document_uris.len() {
            self.nav.toc_anchor_strings_by_doc.resize_with(self.nav.document_uris.len(), Vec::new);
        }
        self.update_toc_anchor_filter();
    }

    /// Set the search query. Finds all matches in the current document and
    /// navigates to the first one. Pass an empty string to clear highlights.
    pub fn set_search_query(&mut self, query: &str) {
        self.highlight.query = query.to_string();
        self.highlight.matches = self.find_matches(query);
        self.highlight.current_match = 0;
        self.update_match_signal();
        if !self.highlight.matches.is_empty() {
            self.navigate_to_current_match();
        } else {
            self.id.request_paint();
        }
    }

    /// Advance to the next match, wrapping around.
    pub fn next_match(&mut self) {
        if self.highlight.matches.is_empty() {
            return;
        }
        self.highlight.current_match = (self.highlight.current_match + 1) % self.highlight.matches.len();
        self.update_match_signal();
        self.navigate_to_current_match();
    }

    /// Go back to the previous match, wrapping around.
    pub fn prev_match(&mut self) {
        if self.highlight.matches.is_empty() {
            return;
        }
        let n = self.highlight.matches.len();
        self.highlight.current_match = (self.highlight.current_match + n - 1) % n;
        self.update_match_signal();
        self.navigate_to_current_match();
    }

    /// Signal carrying (current_1based, total). Both are 0 when there is no active search.
    pub fn match_info_signal(&self) -> RwSignal<(usize, usize)> {
        self.highlight.match_info
    }

    /// Signal that is true while the search bar should be visible.
    pub fn search_active_signal(&self) -> RwSignal<bool> {
        self.highlight.search_active
    }

    /// Signal used by the UI to request next (1) or previous (-1) match navigation.
    pub fn search_nav_signal(&self) -> RwSignal<Option<i8>> {
        self.highlight.search_nav
    }

    /// Signal used by the UI to submit a new search query.
    /// Set to Some(query) to trigger a search; the renderer consumes it on the next paint.
    pub fn search_query_signal(&self) -> RwSignal<Option<String>> {
        self.highlight.search_query_pending
    }
}

fn color_from_u32(color: u32) -> floem::peniko::Color {
    let r = (color >> 24) as u8;
    let g = (color >> 16) as u8;
    let b = (color >> 8) as u8;
    let a = (color & 0xFF) as u8;
    floem::peniko::Color::rgba8(r, g, b, a)
}
