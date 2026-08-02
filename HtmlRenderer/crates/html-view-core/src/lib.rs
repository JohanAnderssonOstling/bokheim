use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use kurbo::{Point, Size};
use peniko::{Color, Image};
use reader_style::ReaderInteractionStyle;

pub use html_layout::{DocumentTocNode, FontStyle, GlyphId, GlyphMetric, GlyphRegistry, StyleStringId};
pub use html_pipeline::RootFontSize;
use html_pipeline::{EarliestStage, PipelineInputs, PipelineSession};
use html_resources::ImagePipeline;
pub use html_resources::{FileSystemProvider, ResourceProvider, TocEntry};

pub mod layout {
    pub use html_layout::{LaidOutDocument, LayoutConstraintError, LayoutConstraints, LayoutTimings, PreparedDocument, ShapeError, ShapedDocument};
}

pub mod parser {
    pub use html_pipeline::{BuildPipelineTimings, DocumentFactory, ParsedHtml, parse_html_document};
}

pub mod resources {
    pub use html_resources::{FileSystemProvider, ImagePipelinePoll, ResourceProvider, TocEntry};
}

pub use html_layout::{GlyphShaper, LaidOutDocument};

mod session;
pub use session::{FramePainter, PointerDownOptions, PointerMoveOutcome, PreparedFrame, RendererSession};

const DEFAULT_DOCUMENT_CACHE_CAPACITY: usize = 3;

struct CachedDocument {
    session: PipelineSession,
    inputs: PipelineInputs,
    document: LaidOutDocument,
}

#[derive(Clone, Debug)]
pub enum RendererCommand {
    NavigateToHref(String),
    NavigateHistory(i8),
    SetPosition { doc: usize, glyph: Option<u32> },
    SetCfiPosition(Option<String>),
    SetColumnWidth(f64),
    SetFontSize(f32),
    SetScale(f64),
    SetLineNumbers(bool),
    SetSearchActive(bool),
    SetSearchQuery(String),
    NavigateSearch(i8),
}

#[derive(Clone, Debug)]
pub enum RendererEvent {
    PositionChanged { doc: usize, glyph: Option<u32> },
    CfiChanged(Option<String>),
    NavAnchorChanged { doc: usize, anchor: Option<String> },
    TitleChanged(Option<String>),
    TocChanged(Vec<TocEntry>),
    MatchInfo { current: usize, total: usize },
    HistoryAvailability { back: bool, forward: bool },
    SearchActiveChanged(bool),
    ColumnWidthChanged(f64),
    FontSizeChanged(f32),
    ScaleChanged(f64),
    LineNumbersChanged(bool),
    OperationFailed { operation: RendererOperation, message: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RendererOperation {
    CopySelection,
    CopyImage,
}

impl fmt::Display for RendererOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::CopySelection => "copy selection",
            Self::CopyImage => "copy image",
        })
    }
}

pub trait RendererHost {
    fn request_repaint(&self);
    fn request_style(&self);
    fn schedule(&self, delay: Duration, f: Box<dyn FnOnce() + Send>);
    fn schedule_repaint(&self, delay: Duration);
    fn set_clipboard(&self, text: &str) -> Result<(), String>;
    fn set_clipboard_image(&self, width: usize, height: usize, rgba: Vec<u8>) -> Result<(), String>;
    fn emit(&self, event: RendererEvent);
}

pub trait Painter {
    fn fill_rect(&mut self, rect: kurbo::Rect, color: Color);
    fn draw_glyph(&mut self, glyph: GlyphId, origin: kurbo::Point);
    fn draw_image(&mut self, image: &Image, hash: &[u8], rect: kurbo::Rect);
}

#[derive(Default)]
pub struct RecordingPainter {
    pub glyphs: Vec<(GlyphId, kurbo::Point)>,
    pub fills: Vec<(kurbo::Rect, Color)>,
    pub images: Vec<kurbo::Rect>,
}

impl Painter for RecordingPainter {
    fn fill_rect(&mut self, rect: kurbo::Rect, color: Color) {
        self.fills.push((rect, color));
    }

    fn draw_glyph(&mut self, glyph: GlyphId, origin: kurbo::Point) {
        self.glyphs.push((glyph, origin));
    }

    fn draw_image(&mut self, _image: &Image, _hash: &[u8], rect: kurbo::Rect) {
        self.images.push(rect);
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum PaintDirection {
    Forward,
    Backward,
}

pub enum DocAnchor {
    Start,
    End,
    Glyph(u32),
}

pub struct RenderState {
    pub y_offset: f64,
    pub col_index: f64,
    pub should_stop: bool,
}

#[derive(Clone, Copy)]
pub struct InlineMetrics {
    pub offset: f64,
}

#[derive(Clone, Copy)]
pub struct LineScreen {
    pub point: Point,
    pub col_index: i32,
}

pub struct PagePositions {
    pub start_line: usize,
    pub last_line: Option<usize>,
    pub positions: Vec<Option<LineScreen>>,
    pub reached_end: bool,
    pub next_start_offset_y: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FormatState {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
}

pub struct ColumnLayout {
    pub col_width: f64,
    pub base_col_width: f64,
    pub col_count: f64,
    pub col_gap: f64,
    pub size: Size,
    pub scale: f64,
}

impl Default for ColumnLayout {
    fn default() -> Self {
        Self { col_width: 600.0, base_col_width: 600.0, col_count: 0.0, col_gap: 0.0, size: Size::ZERO, scale: 1.0 }
    }
}

pub struct ViewportState {
    pub direction: PaintDirection,
    pub start_offset_y: f64,
    pub end_offset_y: f64,
    pub back_start_offset_y: f64,
    pub back_anchor_end_line: Option<usize>,
    pub reached_end: bool,
    pub next_start_offset_y: f64,
}

impl Default for ViewportState {
    fn default() -> Self {
        Self { direction: PaintDirection::Forward, start_offset_y: 0.0, end_offset_y: 0.0, back_start_offset_y: 0.0, back_anchor_end_line: None, reached_end: false, next_start_offset_y: 0.0 }
    }
}

#[derive(Default)]
pub struct FrameCache {
    pub current_page_start_line: Option<usize>,
    pub current_page_end_line: Option<usize>,
    pub last_line_positions: Vec<Option<LineScreen>>,
}

#[derive(Default)]
pub struct SelectionState {
    pub is_selecting: bool,
    pub selection_anchor: Option<u32>,
    pub selection_active: Option<u32>,
    pub pending_link_glyph: Option<u32>,
    pub selected_text: Option<String>,
    pub selected_text_markdown: Option<String>,
    pub link_cursor_active: bool,
    pub select_whole_lines: bool,
}

pub struct HighlightState {
    pub query: String,
    pub matches: Vec<(u32, u32)>,
    pub current_match: usize,
    pub search_active: bool,
}

impl Default for HighlightState {
    fn default() -> Self {
        Self { query: String::new(), matches: Vec::new(), current_match: 0, search_active: false }
    }
}

pub struct NavigationState {
    pub document_uris: Vec<String>,
    pub current_doc_index: usize,
    pub last_position_update: Option<(usize, Option<u32>)>,
    pub last_cfi_position: Option<Option<String>>,
    pub last_nav_anchor: Option<(usize, Option<String>)>,
    pub last_history_availability: Option<(bool, bool)>,
    pub nav_anchor_glyph: Option<u32>,
    pub pending_nav_anchor_update: bool,
    pub location_history: Vec<(usize, Option<String>)>,
    pub location_history_index: usize,
    pub toc_anchor_strings_by_doc: Vec<Vec<String>>,
    pub toc_anchor_filter: Option<HashSet<u16>>,
}

pub struct LineNumberCache {
    pub digits: [GlyphId; 10],
    pub space: GlyphId,
    pub gap: f64,
}

impl Default for LineNumberCache {
    fn default() -> Self {
        Self { digits: [0; 10], space: 0, gap: 8.0 }
    }
}

pub(crate) struct RendererCore {
    host: Rc<dyn RendererHost>,
    document: LaidOutDocument,
    pipeline_session: PipelineSession,
    pipeline_inputs: PipelineInputs,
    image_pipeline: ImagePipeline,
    provider: Arc<dyn ResourceProvider>,
    root_font_size: f32,
    layout: ColumnLayout,
    viewport: ViewportState,
    document_cache: HashMap<usize, CachedDocument>,
    document_cache_order: VecDeque<usize>,
    document_cache_capacity: usize,
    frame: FrameCache,
    selection: SelectionState,
    highlight: HighlightState,
    nav: NavigationState,
    line_numbers: LineNumberCache,
    show_line_numbers: bool,
    origin: Point,
    interaction_style: ReaderInteractionStyle,
}

impl RendererCore {
    pub fn document(&self) -> &LaidOutDocument {
        &self.document
    }

    pub fn root_font_size(&self) -> f32 {
        self.root_font_size
    }

    pub fn preferred_column_width(&self) -> f64 {
        self.layout.base_col_width
    }

    pub fn scale(&self) -> f64 {
        self.layout.scale
    }

    pub fn line_numbers_enabled(&self) -> bool {
        self.show_line_numbers
    }

    pub fn search_active(&self) -> bool {
        self.highlight.search_active
    }

    pub fn link_cursor_active(&self) -> bool {
        self.selection.link_cursor_active
    }

    pub fn origin(&self) -> Point {
        self.origin
    }

    pub fn set_origin(&mut self, origin: Point) {
        self.origin = origin;
    }

    pub fn set_interaction_style(&mut self, style: ReaderInteractionStyle) {
        if self.interaction_style == style {
            return;
        }
        self.interaction_style = style;
        self.host.request_repaint();
    }

    pub fn to_layout_point(&self, position: Point) -> Point {
        let scale = self.layout.scale;
        let dx = self.origin.x * (scale - 1.0);
        let dy = self.origin.y * (scale - 1.0);
        Point::new((position.x + dx) / scale, (position.y + dy) / scale)
    }

    pub fn set_link_cursor_active(&mut self, active: bool) {
        if self.selection.link_cursor_active != active {
            self.selection.link_cursor_active = active;
            self.host.request_style();
        }
    }

    pub(crate) fn emit_operation_failed(&self, operation: RendererOperation, message: String) {
        self.host.emit(RendererEvent::OperationFailed { operation, message });
    }

    pub fn new(host: Rc<dyn RendererHost>, loaded: load::LoadedRenderDocument, glyph_shaper: &mut impl GlyphShaper, provider: Arc<dyn ResourceProvider>, document_uris: Vec<String>, current_doc_index: usize) -> Self {
        let mut document = loaded.document;
        let root_font_size = loaded.inputs.style_environment.root_font_size as f32;
        let image_resources = document.render_view().images().to_vec();
        let line_numbers = Self::build_line_number_cache(glyph_shaper, document.auxiliary_glyph_registry());
        let image_pipeline = ImagePipeline::new(Arc::new(image_resources), provider.clone());
        Self {
            host,
            document,
            pipeline_session: loaded.session,
            pipeline_inputs: loaded.inputs,
            image_pipeline,
            provider,
            root_font_size,
            layout: ColumnLayout::default(),
            viewport: ViewportState::default(),
            document_cache: HashMap::new(),
            document_cache_order: VecDeque::new(),
            document_cache_capacity: DEFAULT_DOCUMENT_CACHE_CAPACITY,
            frame: FrameCache::default(),
            selection: SelectionState::default(),
            highlight: HighlightState::default(),
            nav: NavigationState {
                document_uris,
                current_doc_index,
                last_position_update: None,
                last_cfi_position: None,
                last_nav_anchor: None,
                last_history_availability: None,
                nav_anchor_glyph: None,
                pending_nav_anchor_update: false,
                location_history: Vec::new(),
                location_history_index: 0,
                toc_anchor_strings_by_doc: Vec::new(),
                toc_anchor_filter: None,
            },
            line_numbers,
            show_line_numbers: true,
            origin: Point::ZERO,
            interaction_style: ReaderInteractionStyle::default(),
        }
    }

    fn next_pipeline_inputs_for_layout(&self, viewport_width: f64) -> PipelineInputs {
        let mut next = self.pipeline_inputs.clone();
        next.layout = html_pipeline::LayoutConstraints { viewport_width, line_height: self.pipeline_inputs.layout.line_height };
        next
    }

    fn next_pipeline_inputs_for_style(&self, root_font_size: f32) -> PipelineInputs {
        let mut next = self.pipeline_inputs.clone();
        next.style_environment.root_font_size = root_font_size.max(1.0).round() as u32;
        next.layout = html_pipeline::LayoutConstraints { viewport_width: next.layout.viewport_width, line_height: next.style_environment.root_font_size as f64 };
        next
    }

    fn apply_pipeline_inputs(&mut self, requested_inputs: PipelineInputs, glyph_shaper: &mut impl GlyphShaper) -> bool {
        let anchor_glyph = self.nav.nav_anchor_glyph.or_else(|| self.current_glyph_position());
        let anchor_cfi = self.current_cfi();

        let update = match self.pipeline_session.update(requested_inputs.clone(), glyph_shaper) {
            Ok(update) => update,
            Err(_) => {
                return false;
            }
        };

        let Some(document) = self.pipeline_session.document().cloned() else {
            return false;
        };
        self.document = document;
        self.pipeline_inputs = requested_inputs;
        self.root_font_size = self.pipeline_inputs.style_environment.root_font_size as f32;
        if matches!(update.stage, EarliestStage::Parse | EarliestStage::Style | EarliestStage::Prepare | EarliestStage::Shape) {
            self.line_numbers = Self::build_line_number_cache(glyph_shaper, self.document.auxiliary_glyph_registry());
        }

        self.reset_view_state();

        if update.anchors_preserved {
            if let Some(glyph_idx) = anchor_glyph {
                self.restore_glyph_position(glyph_idx);
            }
        } else if let Some(cfi) = anchor_cfi {
            if !self.restore_position_from_cfi(&cfi)
                && let Some(glyph_idx) = anchor_glyph
            {
                self.restore_glyph_position(glyph_idx);
            }
        }

        self.host.request_repaint();
        true
    }

    pub fn emit_state_snapshot(&mut self) {
        self.host.emit(RendererEvent::TitleChanged(self.document.render_view().title().map(str::to_owned)));
        self.host.emit(RendererEvent::TocChanged(Self::build_document_toc_entries_for(&self.document)));
        self.host.emit(RendererEvent::HistoryAvailability { back: self.nav.location_history_index > 0, forward: self.nav.location_history_index + 1 < self.nav.location_history.len() });
        self.host.emit(RendererEvent::SearchActiveChanged(self.highlight.search_active));
        self.update_match_signal();
        self.host.emit(RendererEvent::ColumnWidthChanged(self.layout.base_col_width));
        self.host.emit(RendererEvent::FontSizeChanged(self.root_font_size));
        self.host.emit(RendererEvent::ScaleChanged(self.layout.scale));
        self.host.emit(RendererEvent::LineNumbersChanged(self.show_line_numbers));
        self.update_nav_signal();
    }

    pub fn apply(&mut self, glyph_shaper: &mut impl GlyphShaper, command: RendererCommand) {
        match command {
            RendererCommand::NavigateToHref(href) if !href.is_empty() => {
                self.navigate_to_href(glyph_shaper, &href);
            }
            RendererCommand::NavigateToHref(_) => {}
            RendererCommand::NavigateHistory(step) if step < 0 => {
                self.navigate_history_back(glyph_shaper);
            }
            RendererCommand::NavigateHistory(step) if step > 0 => {
                self.navigate_history_forward(glyph_shaper);
            }
            RendererCommand::NavigateHistory(_) => {}
            RendererCommand::SetPosition { doc, glyph } => {
                self.set_position(glyph_shaper, doc, glyph);
            }
            RendererCommand::SetCfiPosition(Some(cfi)) => {
                self.set_cfi_position(&cfi);
            }
            RendererCommand::SetCfiPosition(None) => {}
            RendererCommand::SetColumnWidth(width) => self.set_col_width(glyph_shaper, width),
            RendererCommand::SetFontSize(size) => self.set_root_font_size(glyph_shaper, size),
            RendererCommand::SetScale(scale) => self.set_scale(scale),
            RendererCommand::SetLineNumbers(enabled) => self.set_show_line_numbers(enabled),
            RendererCommand::SetSearchActive(active) => self.set_search_active(active),
            RendererCommand::SetSearchQuery(query) => self.set_search_query(&query),
            RendererCommand::NavigateSearch(step) if step > 0 => self.next_match(),
            RendererCommand::NavigateSearch(_) => self.prev_match(),
        }
    }

    pub(crate) fn set_position(&mut self, glyph_shaper: &mut impl GlyphShaper, document: usize, glyph: Option<u32>) {
        if document != self.nav.current_doc_index {
            self.load_document_at(glyph_shaper, document, glyph.map(DocAnchor::Glyph).unwrap_or(DocAnchor::Start));
        } else if let Some(glyph) = glyph {
            self.restore_glyph_position(glyph);
            self.host.request_repaint();
        }
    }

    pub(crate) fn set_cfi_position(&mut self, cfi: &str) -> bool {
        let restored = self.restore_position_from_cfi(cfi);
        if restored {
            self.host.request_repaint();
        }
        restored
    }

    pub(crate) fn set_search_active(&mut self, active: bool) {
        if self.highlight.search_active == active {
            return;
        }
        self.highlight.search_active = active;
        self.host.emit(RendererEvent::SearchActiveChanged(active));
        if !active {
            self.set_search_query("");
        }
        self.host.request_repaint();
    }

    pub fn document_toc(&self) -> Vec<TocEntry> {
        Self::build_document_toc_entries_for(&self.document)
    }

    pub fn set_toc_anchor_strings_by_doc(&mut self, anchors_by_doc: Vec<Vec<String>>) {
        self.nav.toc_anchor_strings_by_doc = anchors_by_doc;
        let document_count = self.nav.document_uris.len();
        if self.nav.toc_anchor_strings_by_doc.len() < document_count {
            self.nav.toc_anchor_strings_by_doc.resize_with(document_count, Vec::new);
        }
        self.update_toc_anchor_filter();
    }

    pub fn set_search_query(&mut self, query: &str) {
        self.highlight.query = query.to_string();
        self.highlight.matches = self.find_matches(query);
        self.highlight.current_match = 0;
        self.update_match_signal();
        if !self.highlight.matches.is_empty() {
            self.navigate_to_current_match();
        } else {
            self.host.request_repaint();
        }
    }

    pub fn next_match(&mut self) {
        if self.highlight.matches.is_empty() {
            return;
        }
        self.highlight.current_match = (self.highlight.current_match + 1) % self.highlight.matches.len();
        self.update_match_signal();
        self.navigate_to_current_match();
    }

    pub fn prev_match(&mut self) {
        if self.highlight.matches.is_empty() {
            return;
        }
        let count = self.highlight.matches.len();
        self.highlight.current_match = (self.highlight.current_match + count - 1) % count;
        self.update_match_signal();
        self.navigate_to_current_match();
    }
}

#[path = "renderer/selection.rs"]
mod selection;

#[path = "renderer/highlight.rs"]
mod highlight;

#[path = "renderer/render.rs"]
mod render;

#[path = "renderer/nav.rs"]
mod nav;

#[path = "renderer/interaction.rs"]
mod interaction;

pub mod load;
