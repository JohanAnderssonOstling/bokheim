use crate::document::{
    AnchorPosition, ComputedStyles, ComputedStylesValidationError, DecorationFragment, Document, GlyphAdvanceRun, GlyphId, GlyphMetrics, GlyphOffsetRun, ImageFragment, InlineContent, InlineKind, InlineRun, LayoutMode, LayoutState,
    LayoutTree, Line, ListItemMarker, ListStylePosition, MemoryUsageReport, StyleIndices,
};
use html_dom::ImageResource;
use kurbo::{Point, Size};
use rustc_data_structures::fx::FxHashMap;
use std::fmt;
use std::ops::Range;

/// Stable renderer-facing line geometry.
///
/// Contract ranges can only be produced by a validated layout result:
///
/// ```compile_fail
/// let _ = html_layout::RenderLine {
///     glyphs: 9..2,
///     point: kurbo::Point::ZERO,
///     height: 0.0,
///     baseline: 0.0,
///     word_spacing: 0.0,
///     letter_spacing: 0.0,
/// };
/// ```
#[derive(Clone, Debug)]
pub struct RenderLine {
    glyphs: Range<u32>,
    point: Point,
    height: f64,
    baseline: f64,
    word_spacing: f64,
    letter_spacing: f64,
}

#[derive(Clone, Debug)]
pub struct ImageMetrics {
    sizes: Vec<(u32, u32)>,
}

impl Default for ImageMetrics {
    fn default() -> Self {
        Self { sizes: Vec::new() }
    }
}

impl ImageMetrics {
    pub fn from_document(document: &Document) -> Self {
        let mut sizes = Vec::with_capacity(document.images().len());
        for image in document.images() {
            sizes.push((image.width, image.height));
        }
        Self { sizes }
    }

    pub fn set(&mut self, image_idx: u32, width: u32, height: u32) {
        let idx = image_idx as usize;
        if self.sizes.len() <= idx {
            self.sizes.resize(idx + 1, (0, 0));
        }
        self.sizes[idx] = (width, height);
    }

    pub fn get(&self, image_idx: u32) -> Option<(u32, u32)> {
        self.sizes.get(image_idx as usize).copied()
    }

    pub fn display_size(&self, image_idx: u32, resource: &ImageResource) -> (f64, f64) {
        let (width, height) = self.get(image_idx).unwrap_or((resource.width, resource.height));
        let width = resource.width_attr.unwrap_or(width).max(1) as f64;
        let height = resource.height_attr.unwrap_or(height).max(1) as f64;
        (width, height)
    }
}

impl RenderLine {
    fn from_line(line: &Line) -> Self {
        Self { glyphs: line.glyphs.clone(), point: line.point, height: line.height, baseline: line.baseline, word_spacing: line.word_spacing, letter_spacing: line.letter_spacing }
    }

    pub fn glyphs(&self) -> Range<u32> {
        self.glyphs.clone()
    }

    pub fn start(&self) -> u32 {
        self.glyphs.start
    }

    pub fn end(&self) -> u32 {
        self.glyphs.end
    }

    pub fn point(&self) -> Point {
        self.point
    }

    pub fn point_x(&self) -> f64 {
        self.point.x
    }

    pub fn point_y(&self) -> f64 {
        self.point.y
    }

    pub fn height(&self) -> f64 {
        self.height
    }

    pub fn baseline(&self) -> f64 {
        self.baseline
    }

    pub fn word_spacing(&self) -> f64 {
        self.word_spacing
    }

    pub fn letter_spacing(&self) -> f64 {
        self.letter_spacing
    }
}

pub struct RenderLines<'a> {
    lines: &'a [Line],
}

impl<'a> RenderLines<'a> {
    fn new(lines: &'a [Line]) -> Self {
        Self { lines }
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn get(&self, idx: usize) -> Option<RenderLine> {
        self.lines.get(idx).map(RenderLine::from_line)
    }

    pub fn first(&self) -> Option<RenderLine> {
        self.lines.first().map(RenderLine::from_line)
    }

    pub fn last(&self) -> Option<RenderLine> {
        self.lines.last().map(RenderLine::from_line)
    }

    pub fn iter(&self) -> impl Iterator<Item = RenderLine> + '_ {
        self.lines.iter().map(RenderLine::from_line)
    }
}

#[derive(Clone, Debug)]
pub struct RenderGlyphOffsetRun {
    range: Range<u32>,
    offset: f32,
}

impl RenderGlyphOffsetRun {
    fn from_run(run: &GlyphOffsetRun) -> Self {
        Self { range: run.range.clone(), offset: run.offset }
    }

    pub fn range(&self) -> Range<u32> {
        self.range.clone()
    }

    pub fn offset(&self) -> f64 {
        self.offset as f64
    }
}

pub struct RenderGlyphOffsetRuns<'a> {
    runs: &'a [GlyphOffsetRun],
}

#[derive(Clone, Debug)]
pub struct RenderGlyphAdvanceRun {
    range: Range<u32>,
    advance: f32,
}

impl RenderGlyphAdvanceRun {
    fn from_run(run: &GlyphAdvanceRun) -> Self {
        Self { range: run.range.clone(), advance: run.advance }
    }

    pub fn range(&self) -> Range<u32> {
        self.range.clone()
    }

    pub fn advance(&self) -> f64 {
        self.advance as f64
    }
}

pub struct RenderGlyphAdvanceRuns<'a> {
    runs: &'a [GlyphAdvanceRun],
}

impl<'a> RenderGlyphAdvanceRuns<'a> {
    fn new(runs: &'a [GlyphAdvanceRun]) -> Self {
        Self { runs }
    }

    pub fn len(&self) -> usize {
        self.runs.len()
    }
    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }
    pub fn get(&self, idx: usize) -> Option<RenderGlyphAdvanceRun> {
        self.runs.get(idx).map(RenderGlyphAdvanceRun::from_run)
    }
    pub fn iter(&self) -> impl Iterator<Item = RenderGlyphAdvanceRun> + '_ {
        self.runs.iter().map(RenderGlyphAdvanceRun::from_run)
    }
}

impl<'a> RenderGlyphOffsetRuns<'a> {
    fn new(runs: &'a [GlyphOffsetRun]) -> Self {
        Self { runs }
    }

    pub fn iter(&self) -> impl Iterator<Item = RenderGlyphOffsetRun> + '_ {
        self.runs.iter().map(RenderGlyphOffsetRun::from_run)
    }

    pub fn len(&self) -> usize {
        self.runs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    pub fn get(&self, idx: usize) -> Option<RenderGlyphOffsetRun> {
        self.runs.get(idx).map(RenderGlyphOffsetRun::from_run)
    }
}

#[derive(Clone, Debug)]
pub struct RenderDecoration {
    rect: kurbo::Rect,
    color: u32,
    is_inline: bool,
}

impl RenderDecoration {
    fn from_fragment(fragment: &DecorationFragment) -> Self {
        Self { rect: fragment.rect, color: fragment.color, is_inline: fragment.is_inline }
    }

    pub fn rect(&self) -> kurbo::Rect {
        self.rect
    }

    pub fn color(&self) -> u32 {
        self.color
    }

    pub fn is_inline(&self) -> bool {
        self.is_inline
    }
}

pub struct RenderDecorations<'a> {
    decorations: &'a [DecorationFragment],
}

impl<'a> RenderDecorations<'a> {
    fn new(decorations: &'a [DecorationFragment]) -> Self {
        Self { decorations }
    }

    pub fn iter(&self) -> impl Iterator<Item = RenderDecoration> + '_ {
        self.decorations.iter().map(RenderDecoration::from_fragment)
    }

    pub fn len(&self) -> usize {
        self.decorations.len()
    }

    pub fn is_empty(&self) -> bool {
        self.decorations.is_empty()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RenderImageFragment {
    line_idx: usize,
    image_idx: u32,
    offset: Point,
    size: Size,
}

impl RenderImageFragment {
    fn from_fragment(fragment: &ImageFragment) -> Self {
        Self { line_idx: fragment.line_idx, image_idx: fragment.image_idx, offset: fragment.offset, size: fragment.size }
    }

    pub fn line_idx(&self) -> usize {
        self.line_idx
    }

    pub fn image_idx(&self) -> u32 {
        self.image_idx
    }

    pub fn offset(&self) -> Point {
        self.offset
    }

    pub fn size(&self) -> Size {
        self.size
    }
}

pub struct RenderImageFragments<'a> {
    fragments: &'a [ImageFragment],
    indexes: &'a [usize],
}

impl<'a> RenderImageFragments<'a> {
    fn new(fragments: &'a [ImageFragment], indexes: &'a [usize]) -> Self {
        Self { fragments, indexes }
    }

    pub fn len(&self) -> usize {
        self.indexes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.indexes.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = RenderImageFragment> + '_ {
        self.indexes.iter().filter_map(move |&idx| self.fragments.get(idx).map(RenderImageFragment::from_fragment))
    }
}

pub struct RenderAllImageFragments<'a> {
    fragments: &'a [ImageFragment],
}

impl<'a> RenderAllImageFragments<'a> {
    fn new(fragments: &'a [ImageFragment]) -> Self {
        Self { fragments }
    }

    pub fn len(&self) -> usize {
        self.fragments.len()
    }

    pub fn is_empty(&self) -> bool {
        self.fragments.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = RenderImageFragment> + '_ {
        self.fragments.iter().map(RenderImageFragment::from_fragment)
    }
}

/// Stable semantic text-run view. Its glyph range is layout-owned and cannot
/// be forged by renderer crates.
///
/// ```compile_fail
/// let _ = html_layout::RenderTextRun {
///     kind: (),
///     box_idx: 0,
///     dom_text_node: None,
///     glyphs: 5..1,
/// };
/// ```
#[derive(Clone, Debug)]
pub struct RenderTextRun {
    kind: TextRunKind,
    box_idx: usize,
    dom_text_node: Option<u32>,
    glyphs: Range<u32>,
}

#[derive(Clone, Copy, Debug)]
enum TextRunKind {
    Text,
    Marker,
}

impl RenderTextRun {
    pub fn is_marker(&self) -> bool {
        matches!(self.kind, TextRunKind::Marker)
    }

    pub fn box_idx(&self) -> usize {
        self.box_idx
    }

    pub fn dom_text_node(&self) -> Option<u32> {
        self.dom_text_node
    }

    pub fn glyphs(&self) -> Range<u32> {
        self.glyphs.clone()
    }
}

pub struct RenderTextRuns<'a> {
    runs: &'a [InlineRun],
    idx: usize,
    kind_filter: Option<TextRunKind>,
    only_box: Option<usize>,
}

impl<'a> RenderTextRuns<'a> {
    fn new(runs: &'a [InlineRun], kind_filter: Option<TextRunKind>) -> Self {
        Self { runs, idx: 0, kind_filter, only_box: None }
    }

    pub fn only_box(mut self, box_idx: usize) -> Self {
        self.only_box = Some(box_idx);
        self
    }
}

impl<'a> Iterator for RenderTextRuns<'a> {
    type Item = RenderTextRun;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let idx = self.idx;
            self.idx += 1;

            let run = self.runs.get(idx)?;
            if self.only_box.is_some_and(|box_idx| box_idx != run.box_idx as usize) {
                continue;
            }

            if let Some(kind_filter) = self.kind_filter {
                match (&run.kind, kind_filter) {
                    (InlineKind::Text { glyphs }, TextRunKind::Text) => {
                        return Some(RenderTextRun { kind: TextRunKind::Text, box_idx: run.box_idx as usize, dom_text_node: run.dom_text_node, glyphs: glyphs.clone() });
                    }
                    (InlineKind::Marker { glyphs }, TextRunKind::Marker) => {
                        return Some(RenderTextRun { kind: TextRunKind::Marker, box_idx: run.box_idx as usize, dom_text_node: run.dom_text_node, glyphs: glyphs.clone() });
                    }
                    _ => continue,
                }
            }

            match &run.kind {
                InlineKind::Text { glyphs } => return Some(RenderTextRun { kind: TextRunKind::Text, box_idx: run.box_idx as usize, dom_text_node: run.dom_text_node, glyphs: glyphs.clone() }),
                InlineKind::Marker { glyphs } => return Some(RenderTextRun { kind: TextRunKind::Marker, box_idx: run.box_idx as usize, dom_text_node: run.dom_text_node, glyphs: glyphs.clone() }),
                _ => continue,
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RenderListItemMarker {
    marker_box: u32,
    position: ListStylePosition,
}

impl RenderListItemMarker {
    fn from_model(marker: ListItemMarker) -> Self {
        Self { marker_box: marker.marker_box, position: marker.position }
    }

    pub fn marker_box(self) -> usize {
        self.marker_box as usize
    }

    pub fn position(self) -> ListStylePosition {
        self.position
    }
}

pub struct RenderAnchorPosition {
    y: f64,
    order: u32,
}

impl RenderAnchorPosition {
    fn new(y: f64, order: u32) -> Self {
        Self { y, order }
    }

    pub fn y(&self) -> f64 {
        self.y
    }

    pub fn order(&self) -> u32 {
        self.order
    }
}

pub struct RenderAnchorPositions<'a> {
    anchors: &'a FxHashMap<u16, AnchorPosition>,
}

impl<'a> RenderAnchorPositions<'a> {
    fn new(anchors: &'a FxHashMap<u16, AnchorPosition>) -> Self {
        Self { anchors }
    }

    pub fn get(&self, id_idx: u16) -> Option<RenderAnchorPosition> {
        self.anchors.get(&id_idx).copied().map(|pos| RenderAnchorPosition::new(pos.y, pos.order))
    }

    pub fn iter(&self) -> impl Iterator<Item = (u16, RenderAnchorPosition)> + '_ {
        self.anchors.iter().map(|(&id_idx, &pos)| (id_idx, RenderAnchorPosition::new(pos.y, pos.order)))
    }
}

#[derive(Clone, Copy)]
pub struct RenderView<'a> {
    doc: &'a LaidOutDocument,
}

impl<'a> RenderView<'a> {
    pub fn root_font_size(&self) -> f32 {
        self.doc.root_font_size()
    }

    pub fn title(&self) -> Option<&'a str> {
        self.doc.title()
    }

    pub fn images(&self) -> &'a [html_dom::ImageResource] {
        self.doc.images()
    }

    pub fn document_toc_entries(&self) -> &'a [html_dom::DocumentTocNode] {
        self.doc.document_toc_entries()
    }

    pub fn string(&self, index: u16) -> &'a str {
        self.doc.string(index)
    }

    pub fn lookup_string(&self, value: &str) -> Option<u16> {
        self.doc.lookup_string(value)
    }

    pub fn glyph_count(&self) -> usize {
        self.doc.glyphs().len()
    }

    pub fn glyph_at(&self, glyph_idx: usize) -> Option<GlyphId> {
        self.doc.glyph_at(glyph_idx)
    }

    pub fn glyph_slice(&self, range: Range<u32>) -> Option<&'a [GlyphId]> {
        let start = usize::try_from(range.start).ok()?;
        let end = usize::try_from(range.end).ok()?;
        self.doc.glyphs().get(start..end)
    }

    pub fn glyph_metric(&self, glyph: GlyphId) -> Option<crate::GlyphMetric> {
        self.doc.glyph_metrics().get_checked(glyph)
    }

    pub fn link_href_for_glyph(&self, glyph_idx: u32) -> Option<u16> {
        self.doc.link_href_for_glyph(glyph_idx)
    }

    pub fn anchor_glyph(&self, id_idx: u16) -> Option<u32> {
        self.doc.anchor_glyph(id_idx)
    }

    pub fn anchor_glyphs(&self) -> impl Iterator<Item = (u16, u32)> + '_ {
        self.doc.anchor_glyphs().iter().map(|(&id_idx, &glyph_idx)| (id_idx, glyph_idx))
    }

    pub fn line_count(&self) -> usize {
        self.doc.line_count()
    }

    pub fn lines(&self) -> RenderLines<'a> {
        RenderLines::new(self.doc.lines())
    }

    pub fn line(&self, idx: usize) -> Option<RenderLine> {
        self.doc.line(idx).map(RenderLine::from_line)
    }

    pub fn line_glyph_offsets(&self, line_idx: usize) -> Option<RenderGlyphOffsetRuns<'a>> {
        self.doc.line_glyph_offsets().get(line_idx).map(|runs| RenderGlyphOffsetRuns::new(runs))
    }

    pub fn line_glyph_advances(&self, line_idx: usize) -> Option<RenderGlyphAdvanceRuns<'a>> {
        self.doc.line_glyph_advances().get(line_idx).map(|runs| RenderGlyphAdvanceRuns::new(runs))
    }

    pub fn decorations(&self) -> RenderDecorations<'a> {
        RenderDecorations::new(self.doc.decorations())
    }

    pub fn images_for_line(&self, line_idx: usize) -> RenderImageFragments<'a> {
        let indexes = self.doc.image_fragments_by_line().get(line_idx).map_or(&[] as &'a [usize], |indexes| indexes.as_slice());
        RenderImageFragments::new(self.doc.image_fragments(), indexes)
    }

    pub fn all_image_fragments(&self) -> RenderAllImageFragments<'a> {
        RenderAllImageFragments::new(self.doc.image_fragments())
    }

    pub fn text_runs(&self) -> RenderTextRuns<'a> {
        RenderTextRuns::new(self.doc.inline_runs(), Some(TextRunKind::Text))
    }

    pub fn marker_runs(&self) -> RenderTextRuns<'a> {
        RenderTextRuns::new(self.doc.inline_runs(), Some(TextRunKind::Marker))
    }

    pub fn box_count(&self) -> usize {
        self.doc.box_count()
    }

    pub fn box_tag(&self, box_idx: usize) -> Option<&str> {
        self.doc.layout_tree.box_at(box_idx).and_then(|layout_box| layout_box.get_element(self.doc.document()).map(|element| element.tag()))
    }

    pub fn box_id(&self, box_idx: usize) -> Option<u16> {
        self.doc.layout_tree.box_at(box_idx)?.get_element(self.doc.document()).and_then(|element| element.id_idx())
    }

    pub fn box_href(&self, box_idx: usize) -> Option<u16> {
        let href_name = self.doc.document().lookup_string("href")?;
        self.doc.layout_tree.box_at(box_idx).and_then(|layout_box| layout_box.get_element(self.doc.document()).and_then(|element| element.attr_value_idx_no_namespace(href_name)))
    }

    pub fn box_point(&self, box_idx: usize) -> Option<kurbo::Point> {
        self.doc.layout_tree.box_at(box_idx).map(|layout_box| layout_box.point())
    }

    pub fn box_size(&self, box_idx: usize) -> Option<kurbo::Size> {
        self.doc.layout_tree.box_at(box_idx).map(|layout_box| layout_box.size())
    }

    pub fn box_parent(&self, box_idx: usize) -> Option<usize> {
        self.doc.layout_tree.box_at(box_idx).and_then(|layout_box| layout_box.parent().map(|parent| parent as usize))
    }

    pub fn box_text_format(&self, box_idx: usize) -> BoxTextFormat {
        self.doc.box_text_format(box_idx)
    }

    pub fn list_marker(&self, box_idx: usize) -> Option<RenderListItemMarker> {
        self.doc.list_marker(box_idx)
    }

    pub fn is_table_cell_box(&self, box_idx: usize) -> bool {
        self.doc.is_table_cell_box(box_idx)
    }

    pub fn is_block_box(&self, box_idx: usize) -> bool {
        self.doc.is_block_box(box_idx)
    }

    pub fn is_anonymous_box(&self, box_idx: usize) -> bool {
        self.doc.is_anonymous_box(box_idx)
    }

    pub fn is_table_box(&self, box_idx: usize) -> bool {
        self.doc.is_table_box(box_idx)
    }

    pub fn is_block_container_box(&self, box_idx: usize) -> bool {
        self.doc.is_block_container_box(box_idx)
    }

    pub fn is_inline_box(&self, box_idx: usize) -> bool {
        self.doc.is_inline_box(box_idx)
    }

    pub fn ancestors(&self, box_idx: usize) -> AncestorIter<'a> {
        AncestorIter { doc: self.doc, current: self.box_parent(box_idx) }
    }

    pub fn anchor_positions(&self) -> RenderAnchorPositions<'a> {
        RenderAnchorPositions::new(&self.doc.layout_state.anchor_positions)
    }

    pub fn anchor_position(&self, id_idx: u16) -> Option<RenderAnchorPosition> {
        self.anchor_positions().get(id_idx)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoxTextFormat {
    pub font_weight: u16,
    pub font_style: html_style_model::FontStyle,
    pub text_decoration: u8,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutConstraints {
    viewport_width: f64,
    line_height: f64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LayoutConstraintError {
    InvalidViewportWidth,
    InvalidLineHeight,
}

impl LayoutConstraints {
    pub fn new(viewport_width: f64, line_height: f64) -> Result<Self, LayoutConstraintError> {
        if !viewport_width.is_finite() || viewport_width < 0.0 {
            return Err(LayoutConstraintError::InvalidViewportWidth);
        }
        if !line_height.is_finite() || line_height <= 0.0 {
            return Err(LayoutConstraintError::InvalidLineHeight);
        }
        Ok(Self { viewport_width, line_height })
    }

    pub fn viewport_width(self) -> f64 {
        self.viewport_width
    }

    pub fn line_height(self) -> f64 {
        self.line_height
    }
}

impl fmt::Display for LayoutConstraintError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidViewportWidth => formatter.write_str("viewport width must be finite and non-negative"),
            Self::InvalidLineHeight => formatter.write_str("line height must be finite and positive"),
        }
    }
}

impl std::error::Error for LayoutConstraintError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrepareError(ComputedStylesValidationError);

impl PrepareError {
    pub fn reason(self) -> ComputedStylesValidationError {
        self.0
    }
}

impl fmt::Display for PrepareError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "cannot prepare layout input: {}", self.0)
    }
}

impl std::error::Error for PrepareError {}

/// Styled content with a stable box tree and unshaped inline text.
///
/// This is the only input accepted by [`PreparedDocument::shape`]. Its fields
/// are private so callers cannot pair layout inputs with a different DOM.
/// Skipping shaping is rejected by the type system:
///
/// ```compile_fail
/// fn skip_shaping(document: html_layout::PreparedDocument) {
///     let _ = document.layout(800.0, 20.0);
/// }
/// ```
///
/// Raw style/layout storage is not part of the stage contract:
///
/// ```compile_fail
/// fn reach_through(document: &html_layout::PreparedDocument) {
///     let _ = document.styles();
/// }
/// ```
#[derive(Clone)]
pub struct PreparedInputs {
    document: std::sync::Arc<Document>,
    styles: std::sync::Arc<ComputedStyles>,
    layout_tree: std::sync::Arc<LayoutTree>,
    inline_content: InlineContent,
}

#[derive(Clone)]
pub struct PreparedDocument {
    inputs: std::sync::Arc<PreparedInputs>,
}

#[derive(Clone)]
pub struct ShapedText {
    inline_content: InlineContent,
    glyph_metrics: GlyphMetrics,
    link_glyph_hrefs: FxHashMap<u32, u16>,
    anchor_glyphs: FxHashMap<u16, u32>,
}

#[derive(Clone)]
pub struct ShapedDocument {
    inputs: std::sync::Arc<PreparedInputs>,
    shaped: std::sync::Arc<ShapedText>,
}

impl PreparedDocument {
    pub fn try_new(document: Document, styles: ComputedStyles) -> Result<Self, PrepareError> {
        styles.validate_for(&document).map_err(PrepareError)?;
        let mut layout_tree = LayoutTree::default();
        let mut inline_content = InlineContent::default();
        crate::layout::build_layout_inputs(&document, &styles, &mut layout_tree, &mut inline_content);
        Ok(Self { inputs: std::sync::Arc::new(PreparedInputs { document: std::sync::Arc::new(document), styles: std::sync::Arc::new(styles), layout_tree: std::sync::Arc::new(layout_tree), inline_content }) })
    }

    pub(crate) fn document(&self) -> &Document {
        self.inputs.document.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn styles(&self) -> &ComputedStyles {
        self.inputs.styles.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn style_view(&self, indices: StyleIndices) -> crate::document::StyleView<'_> {
        self.inputs.styles.view(indices).expect("validated style handle")
    }

    #[cfg(test)]
    pub(crate) fn layout_tree(&self) -> &LayoutTree {
        self.inputs.layout_tree.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn inline_content(&self) -> &InlineContent {
        &self.inputs.inline_content
    }

    pub fn box_count(&self) -> usize {
        self.inputs.layout_tree.box_count()
    }

    /// Estimate bytes retained for the prepared stage cache.
    pub fn memory_usage_bytes(&self) -> usize {
        self.memory_usage_report().total_bytes()
    }

    /// Report bytes retained for the prepared stage cache.
    pub fn memory_usage_report(&self) -> MemoryUsageReport {
        let mut report = MemoryUsageReport::new();
        report.extend_prefixed("PreparedDocument.document", self.document().memory_usage_report());
        report.extend_prefixed("PreparedDocument.styles", self.inputs.styles.memory_usage_report());
        report.extend_prefixed("PreparedDocument.layout_tree", self.inputs.layout_tree.memory_usage_report());
        report.extend_prefixed("PreparedDocument.inline_content", self.inputs.inline_content.memory_usage_report());
        report
    }

    #[cfg(test)]
    pub(crate) fn box_at(&self, box_idx: usize) -> Option<&crate::document::LayoutBox> {
        self.inputs.layout_tree.box_at(box_idx)
    }

    pub fn get_tag(&self, box_idx: usize) -> &str {
        self.inputs.layout_tree.box_at(box_idx).and_then(|layout_box| layout_box.get_element(self.document())).map(|element| element.tag()).unwrap_or("")
    }

    pub fn get_id(&self, box_idx: usize) -> Option<&str> {
        self.inputs.layout_tree.box_at(box_idx)?.get_element(self.document())?.id_idx().map(|id| self.document().string(id))
    }

    pub(crate) fn box_style_indices(&self, box_idx: usize) -> Option<StyleIndices> {
        self.inputs.layout_tree.box_at(box_idx)?.style()
    }

    pub fn box_text_color(&self, box_idx: usize) -> Option<u32> {
        let indices = self.box_style_indices(box_idx)?;
        Some(self.inputs.styles.text_style(indices)?.color)
    }

    pub fn shape(&self, glyph_shaper: &mut impl crate::GlyphShaper) -> Result<ShapedDocument, crate::ShapeError> {
        glyph_shaper.reset();
        let mut inline_content = self.inputs.inline_content.clone();
        let mut glyph_metrics = GlyphMetrics::default();
        crate::shaping::shape_document(&self.inputs.styles, &self.inputs.layout_tree, &mut inline_content, glyph_shaper, &mut glyph_metrics)?;
        let link_glyph_hrefs = collect_link_glyph_hrefs(&self.inputs.document, &self.inputs.layout_tree, &inline_content).into_iter().collect();
        let anchor_glyphs = collect_anchor_glyphs(&self.inputs.document, &self.inputs.layout_tree, &inline_content).into_iter().collect();
        Ok(ShapedDocument { inputs: self.inputs.clone(), shaped: std::sync::Arc::new(ShapedText { inline_content, glyph_metrics, link_glyph_hrefs, anchor_glyphs }) })
    }
}

/// A prepared document whose inline text has concrete glyph IDs and metrics.
/// Layout constraints can change without repeating parsing, styling, box-tree
/// construction, or shaping.
impl ShapedDocument {
    pub(crate) fn document(&self) -> &Document {
        self.inputs.document.as_ref()
    }

    pub fn glyphs(&self) -> &[GlyphId] {
        self.shaped.inline_content.glyphs()
    }

    pub(crate) fn glyph_metrics(&self) -> &GlyphMetrics {
        &self.shaped.glyph_metrics
    }

    /// Estimate bytes retained for the shaped stage cache.
    pub fn memory_usage_bytes(&self) -> usize {
        self.memory_usage_report().total_bytes()
    }

    /// Report bytes retained for the shaped stage cache.
    pub fn memory_usage_report(&self) -> MemoryUsageReport {
        let mut report = MemoryUsageReport::new();
        report.extend_prefixed("ShapedDocument.document", self.document().memory_usage_report());
        report.extend_prefixed("ShapedDocument.styles", self.inputs.styles.memory_usage_report());
        report.extend_prefixed("ShapedDocument.layout_tree", self.inputs.layout_tree.memory_usage_report());
        report.extend_prefixed("ShapedDocument.inline_content", self.shaped.inline_content.memory_usage_report());
        report.extend_prefixed("ShapedDocument.glyph_metrics", self.shaped.glyph_metrics.memory_usage_report());
        report.add_slice_storage::<(u32, u16)>("ShapedDocument.link_glyph_hrefs.storage", self.shaped.link_glyph_hrefs.capacity(), self.shaped.link_glyph_hrefs.len());
        report.add_slice_storage::<(u16, u32)>("ShapedDocument.anchor_glyphs.storage", self.shaped.anchor_glyphs.capacity(), self.shaped.anchor_glyphs.len());
        report
    }

    #[cfg(test)]
    pub(crate) fn layout_parts_mut(&self) -> (&Document, &ComputedStyles, LayoutTree, &InlineContent, &GlyphMetrics) {
        (&self.inputs.document, &self.inputs.styles, self.inputs.layout_tree.as_ref().clone(), &self.shaped.inline_content, &self.shaped.glyph_metrics)
    }

    pub fn layout(self, constraints: LayoutConstraints) -> LaidOutDocument {
        let image_metrics = ImageMetrics::from_document(self.inputs.document.as_ref());
        self.layout_with_metrics(constraints, &image_metrics)
    }

    pub fn layout_with_timings(self, constraints: LayoutConstraints) -> (LaidOutDocument, crate::LayoutTimings) {
        let image_metrics = ImageMetrics::from_document(self.inputs.document.as_ref());
        self.layout_with_timings_and_metrics(constraints, &image_metrics)
    }

    pub fn layout_with_metrics(self, constraints: LayoutConstraints, image_metrics: &ImageMetrics) -> LaidOutDocument {
        self.layout_impl(constraints, image_metrics, None)
    }

    pub fn layout_with_timings_and_metrics(self, constraints: LayoutConstraints, image_metrics: &ImageMetrics) -> (LaidOutDocument, crate::LayoutTimings) {
        let mut timings = crate::LayoutTimings::default();
        let document = self.layout_impl(constraints, image_metrics, Some(&mut timings));
        (document, timings)
    }

    fn layout_impl(self, constraints: LayoutConstraints, image_metrics: &ImageMetrics, timings: Option<&mut crate::LayoutTimings>) -> LaidOutDocument {
        let mut layout_state = LayoutState::default();
        let mut layout_tree = self.inputs.layout_tree.as_ref().clone();
        crate::layout::layout_with_timings(
            &self.inputs.document,
            &self.inputs.styles,
            &mut layout_tree,
            &self.shaped.inline_content,
            &self.shaped.glyph_metrics,
            image_metrics,
            constraints.viewport_width,
            constraints.line_height,
            &mut layout_state,
            timings,
        );
        let mut laid_out = LaidOutDocument { inputs: self.inputs, layout_tree, shaped: self.shaped, layout_state };
        laid_out.rebuild_anchor_positions();
        laid_out
    }
}

/// A fully shaped and laid-out document.
///
/// No public constructor exists: values can only be produced from a
/// [`ShapedDocument`]. Relayout mutates only layout-owned data, retaining the
/// immutable shaping inputs and reusing top-level layout buffers.
#[derive(Clone)]
pub struct LaidOutDocument {
    inputs: std::sync::Arc<PreparedInputs>,
    layout_tree: LayoutTree,
    shaped: std::sync::Arc<ShapedText>,
    layout_state: LayoutState,
}

impl LaidOutDocument {
    pub fn render_view(&self) -> RenderView<'_> {
        RenderView { doc: self }
    }

    pub fn relayout(&mut self, constraints: LayoutConstraints) {
        let image_metrics = ImageMetrics::from_document(self.document());
        self.relayout_impl(constraints, &image_metrics, None);
    }

    pub fn relayout_with_timings(&mut self, constraints: LayoutConstraints) -> crate::LayoutTimings {
        let mut timings = crate::LayoutTimings::default();
        let image_metrics = ImageMetrics::from_document(self.document());
        self.relayout_impl(constraints, &image_metrics, Some(&mut timings));
        timings
    }

    pub fn relayout_with_metrics(&mut self, constraints: LayoutConstraints, image_metrics: &ImageMetrics) {
        self.relayout_impl(constraints, image_metrics, None);
    }

    pub fn relayout_with_metrics_and_timings(&mut self, constraints: LayoutConstraints, image_metrics: &ImageMetrics) -> crate::LayoutTimings {
        let mut timings = crate::LayoutTimings::default();
        self.relayout_impl(constraints, image_metrics, Some(&mut timings));
        timings
    }

    fn relayout_impl(&mut self, constraints: LayoutConstraints, image_metrics: &ImageMetrics, timings: Option<&mut crate::LayoutTimings>) {
        crate::layout::layout_with_timings(
            &self.inputs.document,
            &self.inputs.styles,
            &mut self.layout_tree,
            &self.shaped.inline_content,
            &self.shaped.glyph_metrics,
            image_metrics,
            constraints.viewport_width,
            constraints.line_height,
            &mut self.layout_state,
            timings,
        );
        self.rebuild_anchor_positions();
    }

    fn rebuild_anchor_positions(&mut self) {
        self.layout_state.anchor_positions.clear();
        for box_idx in 0..self.layout_tree.box_count() {
            let Some(id_idx) = box_id(self.document(), &self.layout_tree, box_idx) else {
                continue;
            };
            let mut y = self.anchor_glyph(id_idx).and_then(|glyph| self.layout_state.lines.iter().find(|line| line.glyphs.contains(&glyph)).map(|line| line.point.y));
            if y.is_none() {
                let mut current = Some(box_idx);
                while let Some(idx) = current {
                    let layout_box = self.layout_tree.box_at(idx);
                    if let Some(layout_box) = layout_box {
                        if layout_box.size().height > 0.0 || layout_box.point().y != 0.0 {
                            y = Some(layout_box.point().y);
                            break;
                        }
                        current = layout_box.parent().map(|parent| parent as usize);
                    } else {
                        break;
                    }
                }
            }
            let Some(y) = y else { continue };
            let order = self.anchor_glyph(id_idx).unwrap_or(box_idx as u32);
            self.layout_state
                .anchor_positions
                .entry(id_idx)
                .and_modify(|existing| {
                    if y < existing.y || (y == existing.y && order < existing.order) {
                        *existing = AnchorPosition { y, order };
                    }
                })
                .or_insert(AnchorPosition { y, order });
        }
    }

    pub(crate) fn document(&self) -> &Document {
        &self.inputs.document
    }

    pub(crate) fn root_font_size(&self) -> f32 {
        self.inputs.document.root_font_size()
    }

    pub(crate) fn title(&self) -> Option<&str> {
        self.inputs.document.title()
    }

    pub(crate) fn images(&self) -> &[html_dom::ImageResource] {
        self.inputs.document.images()
    }

    pub(crate) fn document_toc_entries(&self) -> &[html_dom::DocumentTocNode] {
        self.inputs.document.document_toc_entries()
    }

    pub(crate) fn string(&self, index: u16) -> &str {
        self.inputs.document.string(index)
    }

    pub(crate) fn lookup_string(&self, value: &str) -> Option<u16> {
        self.inputs.document.lookup_string(value)
    }

    pub(crate) fn glyph_metrics(&self) -> &GlyphMetrics {
        &self.shaped.glyph_metrics
    }

    pub(crate) fn link_href_for_glyph(&self, glyph_idx: u32) -> Option<u16> {
        self.shaped.link_glyph_hrefs.get(&glyph_idx).copied()
    }

    pub(crate) fn anchor_glyph(&self, id_idx: u16) -> Option<u32> {
        self.shaped.anchor_glyphs.get(&id_idx).copied()
    }

    pub(crate) fn anchor_glyphs(&self) -> &FxHashMap<u16, u32> {
        &self.shaped.anchor_glyphs
    }

    /// Creates a scoped, append-only registry for renderer-owned auxiliary glyphs.
    /// Existing glyph IDs and document layout remain unchanged.
    pub fn auxiliary_glyph_registry(&mut self) -> crate::GlyphRegistry<'_> {
        crate::GlyphRegistry::new(&mut std::sync::Arc::make_mut(&mut self.shaped).glyph_metrics)
    }

    pub(crate) fn glyphs(&self) -> &[GlyphId] {
        self.shaped.inline_content.glyphs()
    }

    pub(crate) fn glyph_at(&self, glyph_idx: usize) -> Option<GlyphId> {
        self.shaped.inline_content.glyph_at(glyph_idx)
    }

    pub(crate) fn glyph_slice(&self, range: Range<u32>) -> &[GlyphId] {
        self.shaped.inline_content.glyph_slice(range)
    }

    /// Returns (text_node_idx, char_offset) for the glyph.
    pub(crate) fn get_dom_node_for_glyph(&self, glyph_idx: u32) -> Option<(u32, usize)> {
        for run in self.shaped.inline_content.inline_runs() {
            if let InlineKind::Text { glyphs } = &run.kind {
                if glyphs.contains(&glyph_idx) {
                    let node_idx = run.dom_text_node?;
                    let char_offset = (glyph_idx - glyphs.start) as usize;
                    return Some((node_idx, char_offset));
                }
            }
        }
        None
    }

    pub(crate) fn box_element(&self, box_idx: usize) -> Option<html_dom::ElementRef<'_>> {
        self.layout_tree.box_at(box_idx)?.get_element(self.document())
    }

    pub(crate) fn box_dom_element_idx(&self, box_idx: usize) -> Option<u32> {
        self.layout_tree.box_at(box_idx)?.dom_element()
    }

    pub(crate) fn get_tag(&self, box_idx: usize) -> &str {
        self.box_element(box_idx).map(|element| element.tag()).unwrap_or("")
    }

    pub(crate) fn get_id(&self, box_idx: usize) -> Option<&str> {
        self.get_box_id(box_idx).map(|idx| self.document().string(idx))
    }

    pub(crate) fn get_box_id(&self, box_idx: usize) -> Option<u16> {
        self.box_element(box_idx).and_then(|e| e.id_idx())
    }

    pub(crate) fn get_box_href(&self, box_idx: usize) -> Option<u16> {
        let href_name = self.document().lookup_string("href")?;
        self.box_element(box_idx).and_then(|e| e.attr_value_idx_no_namespace(href_name))
    }

    pub(crate) fn get_box_image_idx(&self, box_idx: usize) -> Option<u32> {
        let node_idx = self.layout_tree.box_at(box_idx)?.dom_element()?;
        let dom_node = self.document().node_id_from_raw(node_idx)?;
        self.document().element_ref(dom_node).and_then(|element| element.image_idx())
    }

    pub(crate) fn box_layout_mode(&self, box_idx: usize) -> Option<&LayoutMode> {
        self.layout_tree.box_at(box_idx).map(|layout_box| layout_box.layout_mode())
    }

    pub(crate) fn box_style_indices(&self, box_idx: usize) -> Option<StyleIndices> {
        self.layout_tree.box_at(box_idx)?.style()
    }

    pub(crate) fn box_text_format(&self, box_idx: usize) -> BoxTextFormat {
        let indices = self.box_style_indices(box_idx).unwrap_or_else(|| self.inputs.styles.default_indices());
        let style = self.inputs.styles.view(indices).expect("validated style handle");
        BoxTextFormat { font_weight: style.font_weight(), font_style: style.font_style(), text_decoration: style.text_decoration() }
    }

    pub(crate) fn list_marker(&self, box_idx: usize) -> Option<RenderListItemMarker> {
        self.layout_tree.list_marker(box_idx).map(RenderListItemMarker::from_model)
    }

    pub(crate) fn root_box(&self) -> Option<usize> {
        self.layout_tree.root_box()
    }

    pub(crate) fn box_count(&self) -> usize {
        self.layout_tree.box_count()
    }

    pub(crate) fn is_table_cell_box(&self, box_idx: usize) -> bool {
        matches!(self.box_layout_mode(box_idx), Some(LayoutMode::TableCell(_)))
    }

    pub(crate) fn is_block_box(&self, box_idx: usize) -> bool {
        matches!(self.box_layout_mode(box_idx), Some(LayoutMode::Block(_)))
    }

    pub(crate) fn is_anonymous_box(&self, box_idx: usize) -> bool {
        matches!(self.box_layout_mode(box_idx), Some(LayoutMode::Anonymous(_)))
    }

    pub(crate) fn is_table_box(&self, box_idx: usize) -> bool {
        matches!(self.box_layout_mode(box_idx), Some(LayoutMode::Table(_)))
    }

    pub(crate) fn is_block_container_box(&self, box_idx: usize) -> bool {
        matches!(self.box_layout_mode(box_idx), Some(LayoutMode::Block(_) | LayoutMode::Table(_) | LayoutMode::TableCell(_)))
    }

    pub(crate) fn is_inline_box(&self, box_idx: usize) -> bool {
        matches!(self.box_layout_mode(box_idx), Some(LayoutMode::Inline(_)))
    }

    pub(crate) fn box_anchor_y_candidate(&self, box_idx: usize) -> Option<f64> {
        let layout_box = self.layout_tree.box_at(box_idx)?;
        (layout_box.size().height > 0.0 || layout_box.point().y != 0.0).then_some(layout_box.point().y)
    }

    pub(crate) fn get_box_point(&self, box_idx: usize) -> Point {
        self.layout_tree.get_box_point(box_idx)
    }

    pub(crate) fn get_box_size(&self, box_idx: usize) -> Size {
        self.layout_tree.get_box_size(box_idx)
    }

    pub(crate) fn inline_runs_empty(&self) -> bool {
        self.shaped.inline_content.inline_runs_empty()
    }

    pub(crate) fn inline_runs(&self) -> &[InlineRun] {
        self.shaped.inline_content.inline_runs()
    }

    pub(crate) fn inline_run(&self, run_idx: usize) -> Option<&InlineRun> {
        self.shaped.inline_content.inline_run(run_idx)
    }

    pub(crate) fn glyph(&self, glyph_idx: u32) -> Option<GlyphId> {
        self.shaped.inline_content.glyph_at(glyph_idx as usize)
    }

    pub(crate) fn text_runs(&self) -> Vec<(usize, Range<u32>)> {
        self.shaped
            .inline_content
            .inline_runs()
            .iter()
            .filter_map(|run| match &run.kind {
                InlineKind::Text { glyphs } if glyphs.start < glyphs.end => Some((run.box_idx as usize, glyphs.clone())),
                _ => None,
            })
            .collect()
    }

    pub(crate) fn get_classes(&self, box_idx: usize) -> impl Iterator<Item = &str> {
        self.box_dom_element_idx(box_idx).and_then(|node_idx| self.document().node_id_from_raw(node_idx)).and_then(|node_id| self.document().element_ref(node_id)).into_iter().flat_map(|element| element.classes())
    }

    pub(crate) fn has_class(&self, box_idx: usize, class: &str) -> bool {
        self.get_classes(box_idx).any(|c| c == class)
    }

    pub(crate) fn get_parent(&self, box_idx: usize) -> Option<usize> {
        self.layout_tree.box_at(box_idx)?.parent().map(|p| p as usize)
    }

    pub(crate) fn ancestors(&self, box_idx: usize) -> AncestorIter<'_> {
        AncestorIter { doc: self, current: self.layout_tree.box_at(box_idx).and_then(|b| b.parent().map(|parent| parent as usize)) }
    }

    pub(crate) fn print_box_tree(&self) {
        self.layout_tree.print_box_tree(self.document(), &self.inputs.styles);
    }

    pub(crate) fn lines(&self) -> &[Line] {
        self.layout_state.lines.as_slice()
    }

    pub(crate) fn line(&self, idx: usize) -> Option<&Line> {
        self.layout_state.lines.get(idx)
    }

    pub(crate) fn line_count(&self) -> usize {
        self.layout_state.lines.len()
    }

    pub(crate) fn line_glyph_offsets(&self) -> &[Vec<GlyphOffsetRun>] {
        self.layout_state.line_glyph_offsets.as_slice()
    }

    pub(crate) fn line_glyph_advances(&self) -> &[Vec<GlyphAdvanceRun>] {
        self.layout_state.line_glyph_advances.as_slice()
    }

    pub(crate) fn decorations(&self) -> &[DecorationFragment] {
        self.layout_state.decorations.as_slice()
    }

    pub(crate) fn image_fragments(&self) -> &[ImageFragment] {
        self.layout_state.image_fragments.as_slice()
    }

    pub(crate) fn image_fragments_by_line(&self) -> &[Vec<usize>] {
        self.layout_state.image_fragments_by_line.as_slice()
    }

    pub fn memory_usage_report(&self) -> MemoryUsageReport {
        let mut report = MemoryUsageReport::new();
        report.extend_prefixed("LaidOutDocument.document", self.document().memory_usage_report());
        report.extend_prefixed("LaidOutDocument.styles", self.inputs.styles.memory_usage_report());
        report.extend_prefixed("LaidOutDocument.layout_tree", self.layout_tree.memory_usage_report());
        report.extend_prefixed("LaidOutDocument.inline_content", self.shaped.inline_content.memory_usage_report());
        report.extend_prefixed("LaidOutDocument.layout_state", self.layout_state.memory_usage_report());
        report.extend_prefixed("LaidOutDocument.glyph_metrics", self.glyph_metrics().memory_usage_report());
        report.add_slice_storage::<(u32, u16)>("LaidOutDocument.link_glyph_hrefs.storage", self.shaped.link_glyph_hrefs.capacity(), self.shaped.link_glyph_hrefs.len());
        report.add_slice_storage::<(u16, u32)>("LaidOutDocument.anchor_glyphs.storage", self.shaped.anchor_glyphs.capacity(), self.shaped.anchor_glyphs.len());
        report
    }

    pub fn memory_usage_bytes(&self) -> usize {
        self.memory_usage_report().total_bytes()
    }
}

pub struct AncestorIter<'a> {
    doc: &'a LaidOutDocument,
    current: Option<usize>,
}

impl<'a> Iterator for AncestorIter<'a> {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        let idx = self.current?;
        self.current = self.doc.layout_tree.box_at(idx).and_then(|b| b.parent().map(|parent| parent as usize));
        Some(idx as usize)
    }
}

fn box_element<'a>(document: &'a Document, layout_tree: &LayoutTree, box_idx: usize) -> Option<html_dom::ElementRef<'a>> {
    layout_tree.box_at(box_idx)?.get_element(document)
}

fn box_id(document: &Document, layout_tree: &LayoutTree, box_idx: usize) -> Option<u16> {
    box_element(document, layout_tree, box_idx).and_then(|element| element.id_idx())
}

fn box_href(document: &Document, layout_tree: &LayoutTree, box_idx: usize) -> Option<u16> {
    let href_name = document.lookup_string("href")?;
    box_element(document, layout_tree, box_idx).and_then(|element| element.attr_value_idx_no_namespace(href_name))
}

fn parent(layout_tree: &LayoutTree, box_idx: usize) -> Option<usize> {
    layout_tree.box_at(box_idx)?.parent().map(|parent| parent as usize)
}

fn is_block_container(layout_tree: &LayoutTree, box_idx: usize) -> bool {
    matches!(layout_tree.box_at(box_idx).map(|layout_box| layout_box.layout_mode()), Some(LayoutMode::Block(_) | LayoutMode::Table(_) | LayoutMode::TableCell(_)))
}

fn block_ancestor(layout_tree: &LayoutTree, box_idx: usize) -> Option<usize> {
    let mut current = Some(box_idx);
    while let Some(idx) = current {
        if is_block_container(layout_tree, idx) {
            return Some(idx);
        }
        current = parent(layout_tree, idx);
    }
    None
}

fn collect_link_glyph_hrefs(document: &Document, layout_tree: &LayoutTree, inline_content: &InlineContent) -> Vec<(u32, u16)> {
    let mut links = Vec::new();
    for run in inline_content.inline_runs() {
        let InlineKind::Text { glyphs } = &run.kind else { continue };
        if glyphs.is_empty() {
            continue;
        }
        let mut current = Some(run.box_idx as usize);
        let mut href = None;
        while let Some(box_idx) = current {
            if let Some(found) = box_href(document, layout_tree, box_idx) {
                href = Some(found);
                break;
            }
            current = parent(layout_tree, box_idx);
        }
        if let Some(href) = href {
            links.extend(glyphs.clone().map(|glyph| (glyph, href)));
        }
    }
    links
}

fn collect_anchor_glyphs(document: &Document, layout_tree: &LayoutTree, inline_content: &InlineContent) -> Vec<(u16, u32)> {
    let mut min_glyphs = vec![None; layout_tree.box_count()];
    for run in inline_content.inline_runs() {
        let InlineKind::Text { glyphs } = &run.kind else { continue };
        if glyphs.is_empty() {
            continue;
        }
        let mut current = Some(run.box_idx as usize);
        while let Some(idx) = current {
            let entry = &mut min_glyphs[idx];
            if entry.is_none_or(|existing| glyphs.start < existing) {
                *entry = Some(glyphs.start);
            }
            current = parent(layout_tree, idx);
        }
    }

    for box_idx in 0..layout_tree.box_count() {
        if box_id(document, layout_tree, box_idx).is_none() || min_glyphs[box_idx].is_some() {
            continue;
        }
        let Some(LayoutMode::Inline(range)) = layout_tree.box_at(box_idx).map(|layout_box| layout_box.layout_mode()) else {
            continue;
        };
        let run_idx = range.start as usize;
        let anchor_block = block_ancestor(layout_tree, box_idx);
        let following = inline_content.inline_runs().get(run_idx..).into_iter().flatten().find_map(|run| {
            let InlineKind::Text { glyphs } = &run.kind else { return None };
            (!glyphs.is_empty() && block_ancestor(layout_tree, run.box_idx as usize) == anchor_block).then_some(glyphs.start)
        });
        let preceding = || {
            inline_content.inline_runs().get(..run_idx).into_iter().flatten().rev().find_map(|run| {
                let InlineKind::Text { glyphs } = &run.kind else { return None };
                (!glyphs.is_empty() && block_ancestor(layout_tree, run.box_idx as usize) == anchor_block).then_some(glyphs.end - 1)
            })
        };
        min_glyphs[box_idx] = following.or_else(preceding);
    }

    (0..layout_tree.box_count()).filter_map(|idx| Some((box_id(document, layout_tree, idx)?, min_glyphs[idx]?))).collect()
}

#[cfg(test)]
mod stage_tests {
    use crate::ShapeError;
    use crate::document::{GlyphId, GlyphMetric, GlyphMetricError};
    use crate::test_support::{DocumentFactory, TestGlyphShaper};
    use crate::{GlyphShaper, LayoutConstraintError, LayoutConstraints, PreparedDocument};
    use html_style_model::{ComputedStylesBuildError, ComputedStylesBuilder, ComputedStylesValidationError, FontStyle};

    struct InvalidGlyphShaper;

    impl GlyphShaper for InvalidGlyphShaper {
        fn reset(&mut self) {}

        fn shape_glyph<'a>(
            &mut self, _glyph_metrics: &mut crate::GlyphRegistry<'a>, _ch: char, _font_size: f32, _font_weight: u16, _font_style: FontStyle, _color: u32, _font_family: Option<html_style_model::StyleStringId>, _family: Option<&str>,
        ) -> Result<GlyphId, ShapeError> {
            Ok(17)
        }
    }

    #[test]
    fn constraints_reject_non_finite_and_out_of_domain_values() {
        assert_eq!(LayoutConstraints::new(-1.0, 20.0), Err(LayoutConstraintError::InvalidViewportWidth));
        assert_eq!(LayoutConstraints::new(f64::NAN, 20.0), Err(LayoutConstraintError::InvalidViewportWidth));
        assert_eq!(LayoutConstraints::new(f64::INFINITY, 20.0), Err(LayoutConstraintError::InvalidViewportWidth));
        assert_eq!(LayoutConstraints::new(100.0, 0.0), Err(LayoutConstraintError::InvalidLineHeight));
        assert_eq!(LayoutConstraints::new(100.0, f64::NEG_INFINITY), Err(LayoutConstraintError::InvalidLineHeight));
        assert!(LayoutConstraints::new(0.0, 20.0).is_ok(), "a zero-width viewport is valid during window minimization");
    }

    #[test]
    fn shaping_rejects_unregistered_glyph_ids() {
        let mut factory = DocumentFactory::new();
        let prepared = factory.parse_with_new_pipeline("<html><body>text</body></html>", None);
        let error = prepared.shape(&mut InvalidGlyphShaper).err().expect("invalid shaper must be rejected");

        assert_eq!(error.glyph_id(), 17);
        assert_eq!(error.metrics_len(), 0);
    }

    #[derive(Default)]
    struct InvalidMetricShaper;

    impl GlyphShaper for InvalidMetricShaper {
        fn reset(&mut self) {}

        fn shape_glyph<'a>(
            &mut self, glyph_metrics: &mut crate::GlyphRegistry<'a>, ch: char, font_size: f32, _font_weight: u16, _font_style: FontStyle, _color: u32, _font_family: Option<html_style_model::StyleStringId>, _family: Option<&str>,
        ) -> Result<GlyphId, ShapeError> {
            let metric = GlyphMetric::try_new(ch, font_size, font_size * -1.0, font_size * 0.25, font_size * 0.75).map_err(ShapeError::rejected_metric)?;
            glyph_metrics.register(metric)
        }
    }

    #[test]
    fn shaping_rejects_invalid_glyph_metrics() {
        let mut factory = DocumentFactory::new();
        let prepared = factory.parse_with_new_pipeline("<html><body>text</body></html>", None);
        let error = prepared.shape(&mut InvalidMetricShaper).err().expect("invalid metric must be rejected");
        assert!(matches!(error.reason(), Some(GlyphMetricError::NegativeValue { field: "ascent", .. })));
    }

    #[test]
    fn preparation_rejects_styles_from_another_document() {
        let first = html_parse::parse_dom_document("<html><body><p>first</p></body></html>").expect("valid HTML");
        let second = html_parse::parse_dom_document("<html><body><p>second</p></body></html>").expect("valid HTML");
        let first_styles = html_style::style_document(first, &[]).into_parts().1;

        let error = PreparedDocument::try_new(second, first_styles).err().expect("cross-document styles must be rejected");

        assert_eq!(error.reason(), ComputedStylesValidationError::WrongDocument);
    }

    #[test]
    fn style_to_layout_boundary_moves_storage_without_cloning() {
        let document = html_parse::parse_dom_document("<html><body><p>text</p></body></html>").expect("valid HTML");
        let styled = html_style::style_document(document, &[]);
        let default_style = styled.styles().default_indices();
        let font_storage = styled.styles().font_style(default_style).expect("default style handle") as *const _;
        let (document, styles) = styled.into_parts();

        let prepared = PreparedDocument::try_new(document, styles).expect("valid styles must prepare");

        assert_eq!(prepared.styles().font_style(default_style).expect("preserved style handle") as *const _, font_storage);
    }

    #[test]
    fn incomplete_style_tables_cannot_be_finished() {
        let document = html_parse::parse_dom_document("<html><body><p>text</p></body></html>").expect("valid HTML");
        let error = ComputedStylesBuilder::new(&document).finish().err().expect("incomplete styles must not become a ComputedStyles value");

        assert!(matches!(error, ComputedStylesBuildError::MissingElementStyle { .. }));
    }

    #[test]
    fn relayout_reuses_phase_inputs_and_top_level_output_buffers() {
        let mut factory = DocumentFactory::new();
        let prepared = factory.parse_with_new_pipeline("<html><body><p id='first'>one two three four five six seven eight</p></body></html>", None);
        let mut shaper = TestGlyphShaper::new();
        let shaped = prepared.shape(&mut shaper).expect("test shaper must register every glyph");
        let glyph_count = shaped.glyphs().len();
        let metric_count = shaped.glyph_metrics().len();
        let constraints = LayoutConstraints::new(180.0, 20.0).unwrap();
        let mut document = shaped.layout(constraints);

        let lines_ptr = document.layout_state.lines.as_ptr();
        let offsets_ptr = document.layout_state.line_glyph_offsets.as_ptr();
        let advances_ptr = document.layout_state.line_glyph_advances.as_ptr();
        let image_lines_ptr = document.layout_state.image_fragments_by_line.as_ptr();
        let glyphs_ptr = document.glyphs().as_ptr();

        document.relayout(constraints);

        assert_eq!(document.glyphs().len(), glyph_count);
        assert_eq!(document.glyph_metrics().len(), metric_count);
        assert_eq!(document.glyphs().as_ptr(), glyphs_ptr);
        assert_eq!(document.layout_state.lines.as_ptr(), lines_ptr);
        assert_eq!(document.layout_state.line_glyph_offsets.as_ptr(), offsets_ptr);
        assert_eq!(document.layout_state.line_glyph_advances.as_ptr(), advances_ptr);
        assert_eq!(document.layout_state.image_fragments_by_line.as_ptr(), image_lines_ptr);
    }

    #[test]
    fn phase_wrappers_share_immutable_prepared_and_shaped_inputs() {
        let mut factory = DocumentFactory::new();
        let prepared = factory.parse_with_new_pipeline("<html><body><p>shared phase inputs</p></body></html>", None);
        let mut shaper = TestGlyphShaper::new();
        let shaped = prepared.shape(&mut shaper).expect("test shaper must register every glyph");
        let document = shaped.clone().layout(LayoutConstraints::new(180.0, 20.0).unwrap());

        assert!(std::sync::Arc::ptr_eq(&prepared.inputs, &shaped.inputs));
        assert!(std::sync::Arc::ptr_eq(&prepared.inputs, &document.inputs));
        assert!(std::sync::Arc::ptr_eq(&shaped.shaped, &document.shaped));
    }

    #[test]
    fn auxiliary_registry_can_only_append_valid_metrics() {
        let mut factory = DocumentFactory::new();
        let prepared = factory.parse_with_new_pipeline("<html><body>text</body></html>", None);
        let mut shaper = TestGlyphShaper::new();
        let shaped = prepared.shape(&mut shaper).expect("valid shaping");
        let constraints = LayoutConstraints::new(180.0, 20.0).unwrap();
        let mut document = shaped.layout(constraints);

        let original_len = document.glyph_metrics().len();
        let original_first = document.glyph_metrics().get_checked(0);
        let metric = GlyphMetric::try_new('#', 4.0, 3.0, 1.0, 0.0).expect("valid auxiliary metric");
        let appended = document.auxiliary_glyph_registry().register(metric).expect("append auxiliary glyph");

        assert_eq!(appended as usize, original_len);
        assert_eq!(document.glyph_metrics().len(), original_len + 1);
        assert_eq!(document.glyph_metrics().get_checked(0), original_first);
    }

    #[test]
    fn render_view_and_semantic_iterators_allocate_nothing() {
        use crate::allocation_test_support::count_allocations;
        use std::hint::black_box;

        let mut factory = DocumentFactory::new();
        let prepared = factory.parse_with_new_pipeline("<html><body><p id='anchor'>one two three</p></body></html>", None);
        let mut shaper = TestGlyphShaper::new();
        let document = prepared.shape(&mut shaper).expect("valid shaping").layout(LayoutConstraints::new(180.0, 20.0).unwrap());

        let (_, allocations) = count_allocations(|| {
            let view = black_box(document.render_view());
            black_box(view.lines().iter().count());
            black_box(view.text_runs().count());
            black_box(view.marker_runs().count());
            black_box(view.decorations().iter().count());
            black_box(view.all_image_fragments().iter().count());
            black_box(view.anchor_positions().iter().count());
            black_box(view.anchor_glyphs().count());
            black_box(view.glyph_slice(0..view.glyph_count() as u32));
            if let Some(glyph) = view.glyph_at(0) {
                black_box(view.glyph_metric(glyph));
            }
        });

        assert_eq!(allocations, 0, "render views must stay allocation-free");
    }
}
