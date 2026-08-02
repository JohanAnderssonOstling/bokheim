use html_dom::{Document, DocumentLineage, DomNodeId, MemoryUsageReport, NodeRef};
use kurbo::Vec2;
use rustc_data_structures::fx::FxHashMap;
use std::hash::{Hash, Hasher};
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicU32, Ordering};

/// Implement bitwise `PartialEq`/`Eq`/`Hash` for a style sub-struct so it can be
/// deduplicated in `StyleStore`. Float fields are compared and hashed by their
/// bit pattern (exact equality), which is exactly what sharing identical computed
/// styles needs.
macro_rules! impl_style_key {
    ($t:ty { $($f:ident),* } floats { $($ff:ident),* }) => {
        impl PartialEq for $t {
            fn eq(&self, other: &Self) -> bool {
                $( self.$f == other.$f && )* $( self.$ff.to_bits() == other.$ff.to_bits() && )* true
            }
        }
        impl Eq for $t {}
        impl Hash for $t {
            fn hash<H: Hasher>(&self, state: &mut H) {
                $( self.$f.hash(state); )*
                $( state.write_u32(self.$ff.to_bits()); )*
            }
        }
    };
}

// ============================================================================
// Sub-Structs for Servo-inspired Style Splitting
// ============================================================================

/// Computed letter/word spacing.
///
/// CSS percentages remain relative to the used font size of the element where
/// the inherited value is finally used. Lengths are resolved to absolute pixels
/// during style computation, so keeping the two components separate preserves
/// CSS inheritance semantics without exposing parser-specific calc trees to
/// layout.
#[derive(Clone, Copy, Debug, Default)]
pub struct TextSpacing {
    absolute_px: f32,
    font_size_fraction: f32,
}

impl TextSpacing {
    pub const ZERO: Self = Self { absolute_px: 0.0, font_size_fraction: 0.0 };

    pub fn new(absolute_px: f32, font_size_fraction: f32) -> Option<Self> {
        (absolute_px.is_finite() && font_size_fraction.is_finite()).then_some(Self { absolute_px, font_size_fraction })
    }

    pub fn from_px(absolute_px: f32) -> Option<Self> {
        Self::new(absolute_px, 0.0)
    }

    pub fn absolute_px(self) -> f32 {
        self.absolute_px
    }

    pub fn font_size_fraction(self) -> f32 {
        self.font_size_fraction
    }

    pub fn resolve(self, used_font_size: f32) -> f32 {
        let resolved = self.absolute_px as f64 + self.font_size_fraction as f64 * used_font_size as f64;
        resolved.clamp(-(f32::MAX as f64), f32::MAX as f64) as f32
    }

    fn is_finite(self) -> bool {
        self.absolute_px.is_finite() && self.font_size_fraction.is_finite()
    }
}

impl PartialEq for TextSpacing {
    fn eq(&self, other: &Self) -> bool {
        self.absolute_px.to_bits() == other.absolute_px.to_bits() && self.font_size_fraction.to_bits() == other.font_size_fraction.to_bits()
    }
}

impl Eq for TextSpacing {}

impl Hash for TextSpacing {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u32(self.absolute_px.to_bits());
        state.write_u32(self.font_size_fraction.to_bits());
    }
}

/// Computed `tab-size`, represented without parser-specific units.
///
/// The private fields and checked constructors make negative and non-finite
/// tab intervals unrepresentable at the style/layout boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum TabSizeKind {
    Spaces,
    LengthPx,
}

#[derive(Clone, Copy, Debug)]
pub struct TabSize {
    kind: TabSizeKind,
    value: f32,
}

impl TabSize {
    pub const DEFAULT: Self = Self { kind: TabSizeKind::Spaces, value: 8.0 };

    pub fn spaces(value: f32) -> Option<Self> {
        (value.is_finite() && value >= 0.0).then_some(Self { kind: TabSizeKind::Spaces, value })
    }

    pub fn length_px(value: f32) -> Option<Self> {
        (value.is_finite() && value >= 0.0).then_some(Self { kind: TabSizeKind::LengthPx, value })
    }

    pub fn kind(self) -> TabSizeKind {
        self.kind
    }

    pub fn value(self) -> f32 {
        self.value
    }
}

impl PartialEq for TabSize {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind && self.value.to_bits() == other.value.to_bits()
    }
}

impl Eq for TabSize {}

impl Hash for TabSize {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.kind.hash(state);
        state.write_u32(self.value.to_bits());
    }
}

/// Font properties (inherited)
#[derive(Clone, Debug)]
pub struct Font {
    pub font_size: f32,
    pub font_weight: u16,
    pub font_style: FontStyle,
    pub font_family: Option<StyleStringId>,
    pub font_variant_small_caps: bool,
}

/// Text properties (inherited)
#[derive(Clone, Debug)]
pub struct InheritedText {
    pub color: u32,
    pub line_height: f32,
    /// Unitless `line-height` multiplier (0.0 = none). Numbers inherit as
    /// numbers, so descendants recompute `line_height` against their own font
    /// size; lengths/percentages inherit only the resolved `line_height` px.
    pub line_height_number: f32,
    pub letter_spacing: TextSpacing,
    pub word_spacing: TextSpacing,
    pub tab_size: TabSize,
    pub text_align: TextAlign,
    pub text_align_last: TextAlign,
    /// True when `text-align-last` was explicitly set to a non-`auto` value, so
    /// a later `text-align` no longer mirrors itself into `text_align_last`.
    pub text_align_last_explicit: bool,
    pub text_indent: LengthPct,
    pub text_indent_hanging: bool,
    pub text_indent_each_line: bool,
    pub white_space: WhiteSpace,
    pub word_break: WordBreak,
    pub overflow_wrap: OverflowWrap,
    pub text_transform: TextTransform,
    pub list_style_type: ListStyleType,
    pub list_style_position: ListStylePosition,
    /// Interned URL string of `list-style-image` (`None` for `none`). Resolved to
    /// an image resource when the marker box is built.
    pub list_style_image: Option<StyleStringId>,
}

/// Box model properties (reset)
#[derive(Clone, Debug)]
pub struct BoxModel {
    pub display: Display,
    pub box_sizing: BoxSizing,
    pub vertical_align: VerticalAlignValue,
    pub float: Float,
    pub clear: Clear,
    pub border_collapse: BorderCollapseMode,
    pub border_spacing_horizontal: f32,
    pub border_spacing_vertical: f32,
    pub empty_cells: EmptyCellsMode,
    pub caption_side: CaptionSide,
    pub width: PreferredSize,
    pub height: PreferredSize,
    pub min_width: PreferredSize,
    pub min_height: PreferredSize,
    pub max_width: PreferredSize,
    pub max_height: PreferredSize,
    pub margin_top: LengthPct,
    pub margin_bottom: LengthPct,
    pub margin_left: LengthPct,
    pub margin_right: LengthPct,
    pub padding_top: LengthPct,
    pub padding_bottom: LengthPct,
    pub padding_left: LengthPct,
    pub padding_right: LengthPct,
}

/// Border properties (reset)
#[derive(Clone, Debug)]
pub struct Border {
    pub border_top_width: f32,
    pub border_right_width: f32,
    pub border_bottom_width: f32,
    pub border_left_width: f32,
    pub border_top_color: u32,
    pub border_right_color: u32,
    pub border_bottom_color: u32,
    pub border_left_color: u32,
    pub border_top_style: BorderStyle,
    pub border_right_style: BorderStyle,
    pub border_bottom_style: BorderStyle,
    pub border_left_style: BorderStyle,
}

/// Background properties (reset)
#[derive(Clone, Debug)]
pub struct Background {
    pub background_color: u32,
    pub text_decoration: u8, // bitflags: 1=underline, 2=overline, 4=line-through
}

impl_style_key!(Font { font_weight, font_style, font_family, font_variant_small_caps } floats { font_size });
impl_style_key!(InheritedText {
    color, letter_spacing, word_spacing, tab_size, text_align, text_align_last, text_align_last_explicit, text_indent, text_indent_hanging, text_indent_each_line, white_space, word_break, overflow_wrap, text_transform,
    list_style_type, list_style_position, list_style_image
} floats { line_height, line_height_number });
impl_style_key!(BoxModel {
    display, box_sizing, vertical_align, float, clear, border_collapse, empty_cells, caption_side,
    width, height, min_width, min_height, max_width, max_height,
    margin_top, margin_bottom, margin_left, margin_right,
    padding_top, padding_bottom, padding_left, padding_right
} floats {
    border_spacing_horizontal, border_spacing_vertical
});
impl_style_key!(Border {
    border_top_color, border_right_color, border_bottom_color, border_left_color,
    border_top_style, border_right_style, border_bottom_style, border_left_style
} floats {
    border_top_width, border_right_width, border_bottom_width, border_left_width
});
impl_style_key!(Background { background_color, text_decoration } floats { });

/// Opaque handle to one entry in each split style table.
///
/// Handles can only be produced by a [`ComputedStylesBuilder`] and carry the
/// identity of that builder's store. Their component indices cannot be forged
/// or rewritten by downstream stages:
///
/// ```compile_fail
/// let _ = html_style_model::StyleIndices {
///     font_idx: 0,
///     text_idx: 0,
///     box_idx: 0,
///     border_idx: 0,
///     bg_idx: 0,
/// };
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StyleIndices {
    store_id: NonZeroU32,
    font_idx: u32,
    text_idx: u32,
    box_idx: u32,
    border_idx: u32,
    bg_idx: u32,
}

/// An opaque index into the string table owned by one computed-style result.
///
/// CSS-created strings deliberately do not use the parsed document's string
/// interner. This keeps styling from mutating parsing output and lets either
/// representation evolve independently.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct StyleStringId {
    store_id: NonZeroU32,
    index: u32,
}

/// Immutable computed-style side table keyed by DOM node ID.
///
/// This is deliberately separate from [`html_dom::Document`]: parsing owns the
/// DOM, while style resolution produces this value. Downstream stages can
/// retain or replace either representation independently.
pub struct ComputedStyles {
    document_lineage: DocumentLineage,
    node_styles: Vec<Option<StyleIndices>>,
    store: StyleStore,
    strings: Vec<String>,
}

/// Construction-only form used by the style stage. Finishing consumes the
/// deduplication maps, so they are not retained by layout or rendering.
pub struct ComputedStylesBuilder {
    document_lineage: DocumentLineage,
    node_styles: Vec<Option<StyleIndices>>,
    element_nodes: Vec<bool>,
    store: StyleStore,
    strings: Vec<String>,
    string_dedup: FxHashMap<String, StyleStringId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComputedStylesBuildError {
    NodeOutOfBounds { node: usize },
    TextNodeCannotHaveStyle { node: usize },
    MissingElementStyle { node: usize },
    InvalidStyleIndex { node: usize },
    InvalidStyleValue(ComputedStyleValueError),
}

impl std::fmt::Display for ComputedStylesBuildError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NodeOutOfBounds { node } => write!(formatter, "node {node} is outside the source document"),
            Self::TextNodeCannotHaveStyle { node } => write!(formatter, "text node {node} cannot have a computed element style"),
            Self::MissingElementStyle { node } => write!(formatter, "element node {node} has no computed style"),
            Self::InvalidStyleIndex { node } => write!(formatter, "node {node} contains an index from another style store"),
            Self::InvalidStyleValue(error) => write!(formatter, "computed style value is invalid: {error}"),
        }
    }
}

impl std::error::Error for ComputedStylesBuildError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComputedStyleValueError {
    NonFiniteValue { property: &'static str, field: &'static str, value: u32 },
    OutOfRangeValue { property: &'static str, field: &'static str, value: u32, min: Option<u32>, max: Option<u32> },
    MustBePositive { property: &'static str, field: &'static str, value: u32 },
    InvalidFontWeight { value: u16 },
    ForeignStyleStringId { property: &'static str, field: &'static str, expected_store: u32, actual_store: u32 },
}

impl std::fmt::Display for ComputedStyleValueError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonFiniteValue { property, field, value } => write!(formatter, "non-finite value in {property}.{field}: {}", f32::from_bits(*value)),
            Self::OutOfRangeValue { property, field, value, min, max } => match (min, max) {
                (Some(min), Some(max)) => write!(formatter, "value in {property}.{field}={}: expected within [{}, {}]", f32::from_bits(*value), f32::from_bits(*min), f32::from_bits(*max)),
                (Some(min), None) => write!(formatter, "value in {property}.{field}={}: expected >= {}", f32::from_bits(*value), f32::from_bits(*min)),
                (None, Some(max)) => write!(formatter, "value in {property}.{field}={}: expected <= {}", f32::from_bits(*value), f32::from_bits(*max)),
                (None, None) => write!(formatter, "value in {property}.{field}={} is invalid", f32::from_bits(*value)),
            },
            Self::MustBePositive { property, field, value } => {
                write!(formatter, "value in {property}.{field}={} must be > 0.0", f32::from_bits(*value))
            }
            Self::InvalidFontWeight { value } => write!(formatter, "font weight {value} is outside supported range"),
            Self::ForeignStyleStringId { property, field, expected_store, actual_store } => {
                write!(formatter, "string handle in {property}.{field} belongs to a different style store (expected {expected_store}, got {actual_store})")
            }
        }
    }
}

impl std::error::Error for ComputedStyleValueError {}

fn must_be_finite(property: &'static str, field: &'static str, value: f32) -> Result<(), ComputedStyleValueError> {
    if value.is_finite() { Ok(()) } else { Err(ComputedStyleValueError::NonFiniteValue { property, field, value: value.to_bits() }) }
}

fn must_be_non_negative(property: &'static str, field: &'static str, value: f32) -> Result<(), ComputedStyleValueError> {
    must_be_finite(property, field, value)?;
    if value >= 0.0 { Ok(()) } else { Err(ComputedStyleValueError::OutOfRangeValue { property, field, value: value.to_bits(), min: Some(0.0f32.to_bits()), max: None }) }
}

fn must_be_positive(property: &'static str, field: &'static str, value: f32) -> Result<(), ComputedStyleValueError> {
    must_be_finite(property, field, value)?;
    if value > 0.0 { Ok(()) } else { Err(ComputedStyleValueError::MustBePositive { property, field, value: value.to_bits() }) }
}

fn validate_style_string_id(property: &'static str, field: &'static str, string_id: StyleStringId, store_id: NonZeroU32) -> Result<(), ComputedStyleValueError> {
    let actual_store = string_id.store_id.get();
    if actual_store == store_id.get() { Ok(()) } else { Err(ComputedStyleValueError::ForeignStyleStringId { property, field, expected_store: store_id.get(), actual_store }) }
}

impl Font {
    pub fn validate(&self, store_id: NonZeroU32) -> Result<(), ComputedStyleValueError> {
        must_be_positive("Font", "font_size", self.font_size)?;
        if self.font_weight == 0 || self.font_weight > 1000 {
            return Err(ComputedStyleValueError::InvalidFontWeight { value: self.font_weight });
        }
        if let Some(font_family) = self.font_family {
            validate_style_string_id("Font", "font_family", font_family, store_id)?;
        }
        Ok(())
    }
}

impl InheritedText {
    pub fn validate(&self, store_id: NonZeroU32) -> Result<(), ComputedStyleValueError> {
        // `line-height` uses 0.0 to mean CSS `normal`; all non-sentinel values
        // must be finite and non-negative.
        must_be_finite("InheritedText", "line_height", self.line_height)?;
        if self.line_height < 0.0 {
            return Err(ComputedStyleValueError::OutOfRangeValue { property: "InheritedText", field: "line_height", value: self.line_height.to_bits(), min: Some(0.0f32.to_bits()), max: None });
        }
        must_be_non_negative("InheritedText", "line_height_number", self.line_height_number)?;
        debug_assert!(self.letter_spacing.is_finite());
        debug_assert!(self.word_spacing.is_finite());
        debug_assert!(self.tab_size.value().is_finite() && self.tab_size.value() >= 0.0);
        self.text_indent.validate("InheritedText", "text_indent", true)?;
        if let Some(list_style_image) = self.list_style_image {
            validate_style_string_id("InheritedText", "list_style_image", list_style_image, store_id)?;
        }
        Ok(())
    }
}

impl BoxModel {
    pub fn validate(&self, _store_id: NonZeroU32) -> Result<(), ComputedStyleValueError> {
        self.width.validate("BoxModel", "width", false)?;
        self.height.validate("BoxModel", "height", false)?;
        self.min_width.validate("BoxModel", "min_width", false)?;
        self.min_height.validate("BoxModel", "min_height", false)?;
        self.max_width.validate("BoxModel", "max_width", false)?;
        self.max_height.validate("BoxModel", "max_height", false)?;
        must_be_non_negative("BoxModel", "border_spacing_horizontal", self.border_spacing_horizontal)?;
        must_be_non_negative("BoxModel", "border_spacing_vertical", self.border_spacing_vertical)?;
        self.margin_top.validate("BoxModel", "margin_top", true)?;
        self.margin_bottom.validate("BoxModel", "margin_bottom", true)?;
        self.margin_left.validate("BoxModel", "margin_left", true)?;
        self.margin_right.validate("BoxModel", "margin_right", true)?;
        self.padding_top.validate("BoxModel", "padding_top", false)?;
        self.padding_bottom.validate("BoxModel", "padding_bottom", false)?;
        self.padding_left.validate("BoxModel", "padding_left", false)?;
        self.padding_right.validate("BoxModel", "padding_right", false)?;
        match self.vertical_align {
            VerticalAlignValue::Length(value) => must_be_finite("BoxModel", "vertical_align.length", value)?,
            VerticalAlignValue::Percent(value) => must_be_finite("BoxModel", "vertical_align.percent", value)?,
            _ => {}
        }
        Ok(())
    }
}

impl Border {
    pub fn validate(&self, _store_id: NonZeroU32) -> Result<(), ComputedStyleValueError> {
        must_be_non_negative("Border", "border_top_width", self.border_top_width)?;
        must_be_non_negative("Border", "border_right_width", self.border_right_width)?;
        must_be_non_negative("Border", "border_bottom_width", self.border_bottom_width)?;
        must_be_non_negative("Border", "border_left_width", self.border_left_width)?;
        Ok(())
    }
}

impl Background {
    pub fn validate(&self, _store_id: NonZeroU32) -> Result<(), ComputedStyleValueError> {
        Ok(())
    }
}

impl LengthPct {
    fn validate(&self, property: &'static str, field: &'static str, allow_negative: bool) -> Result<(), ComputedStyleValueError> {
        match self {
            LengthPct::Px(px) => {
                if allow_negative {
                    must_be_finite(property, field, *px)
                } else {
                    must_be_non_negative(property, field, *px)
                }
            }
            LengthPct::Pct(pct) => {
                if allow_negative {
                    must_be_finite(property, field, *pct)
                } else {
                    must_be_non_negative(property, field, *pct)
                }
            }
        }
    }
}

impl PreferredSize {
    fn validate(&self, property: &'static str, field: &'static str, allow_negative: bool) -> Result<(), ComputedStyleValueError> {
        match self {
            Self::Auto => Ok(()),
            Self::Px(px) => {
                if allow_negative {
                    must_be_finite(property, field, *px)
                } else {
                    must_be_non_negative(property, field, *px)
                }
            }
            Self::Percent(pct) => {
                if allow_negative {
                    must_be_finite(property, field, *pct)
                } else {
                    must_be_non_negative(property, field, *pct)
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComputedStylesValidationError {
    WrongDocument,
    WrongNodeCount { expected: usize, actual: usize },
    MissingElementStyle { node: usize },
    UnexpectedTextStyle { node: usize },
    InvalidStyleIndex { node: usize },
}

impl std::fmt::Display for ComputedStylesValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongDocument => formatter.write_str("computed styles belong to a different document"),
            Self::WrongNodeCount { expected, actual } => write!(formatter, "computed-style node count is {actual}, expected {expected}"),
            Self::MissingElementStyle { node } => write!(formatter, "element node {node} has no computed style"),
            Self::UnexpectedTextStyle { node } => write!(formatter, "text node {node} unexpectedly has a computed style"),
            Self::InvalidStyleIndex { node } => write!(formatter, "node {node} contains an out-of-bounds style index"),
        }
    }
}

impl std::error::Error for ComputedStylesValidationError {}

impl ComputedStylesBuilder {
    pub fn new(document: &Document) -> Self {
        let mut store = StyleStore::new(next_style_store_id());
        store.ensure_defaults();
        let element_nodes = document.nodes().map(|(_, node)| matches!(node, NodeRef::Element(_))).collect();
        Self { document_lineage: document.lineage().clone(), node_styles: vec![None; document.node_count()], element_nodes, store, strings: Vec::new(), string_dedup: FxHashMap::default() }
    }

    pub fn intern_string(&mut self, value: &str) -> StyleStringId {
        if let Some(&id) = self.string_dedup.get(value) {
            return id;
        }
        let id = StyleStringId { store_id: self.store.store_id, index: self.strings.len() as u32 };
        let owned = value.to_owned();
        self.string_dedup.insert(owned.clone(), id);
        self.strings.push(owned);
        id
    }

    pub fn string(&self, id: StyleStringId) -> Option<&str> {
        (id.store_id == self.store.store_id).then(|| self.strings.get(id.index as usize)).flatten().map(String::as_str)
    }

    pub fn push(&mut self, font: Font, text: InheritedText, box_model: BoxModel, border: Border, background: Background) -> Result<StyleIndices, ComputedStyleValueError> {
        font.validate(self.store.store_id)?;
        text.validate(self.store.store_id)?;
        box_model.validate(self.store.store_id)?;
        border.validate(self.store.store_id)?;
        background.validate(self.store.store_id)?;

        Ok(self.store.push(font, text, box_model, border, background))
    }

    pub fn default_indices(&self) -> StyleIndices {
        self.store.default_indices()
    }

    pub fn set_node_style(&mut self, node_idx: DomNodeId, style: StyleIndices) -> Result<(), ComputedStylesBuildError> {
        let node = node_idx.index();
        let Some(is_element) = self.element_nodes.get(node).copied() else {
            return Err(ComputedStylesBuildError::NodeOutOfBounds { node });
        };
        if !is_element {
            return Err(ComputedStylesBuildError::TextNodeCannotHaveStyle { node });
        }
        let slot = &mut self.node_styles[node];
        *slot = Some(style);
        Ok(())
    }

    pub fn style_for_node(&self, node_idx: DomNodeId) -> Option<StyleIndices> {
        self.node_styles.get(node_idx.index()).copied().flatten()
    }

    pub fn view(&self, indices: StyleIndices) -> Option<StyleView<'_>> {
        self.store.view(indices)
    }

    pub fn font_style(&self, indices: StyleIndices) -> Option<&Font> {
        self.store.font_style(indices)
    }

    pub fn text_style(&self, indices: StyleIndices) -> Option<&InheritedText> {
        self.store.text_style(indices)
    }

    pub fn box_model_style(&self, indices: StyleIndices) -> Option<&BoxModel> {
        self.store.box_model_style(indices)
    }

    pub fn border_style(&self, indices: StyleIndices) -> Option<&Border> {
        self.store.border_style(indices)
    }

    pub fn background_style(&self, indices: StyleIndices) -> Option<&Background> {
        self.store.background_style(indices)
    }

    pub fn finish(mut self) -> Result<ComputedStyles, ComputedStylesBuildError> {
        self.store.validate_values().map_err(ComputedStylesBuildError::InvalidStyleValue)?;
        for (node, (&is_element, style)) in self.element_nodes.iter().zip(self.node_styles.iter().copied()).enumerate() {
            if is_element && style.is_none() {
                return Err(ComputedStylesBuildError::MissingElementStyle { node });
            }
            if let Some(indices) = style
                && !self.store.contains(indices)
            {
                return Err(ComputedStylesBuildError::InvalidStyleIndex { node });
            }
        }
        self.store.discard_deduplication_maps();
        Ok(ComputedStyles { document_lineage: self.document_lineage, node_styles: self.node_styles, store: self.store, strings: self.strings })
    }
}

impl ComputedStyles {
    pub fn node_count(&self) -> usize {
        self.node_styles.len()
    }

    pub fn style_for_node(&self, node_idx: DomNodeId) -> Option<StyleIndices> {
        self.node_styles.get(node_idx.index()).copied().flatten()
    }

    pub fn string(&self, id: StyleStringId) -> Option<&str> {
        (id.store_id == self.store.store_id).then(|| self.strings.get(id.index as usize)).flatten().map(String::as_str)
    }

    pub fn default_indices(&self) -> StyleIndices {
        self.store.default_indices()
    }

    /// Preserve inherited text/font parts for an anonymous box while resetting
    /// its non-inherited parts. A handle from another style result is rejected.
    pub fn anonymous_box_indices(&self, parent: StyleIndices) -> Option<StyleIndices> {
        if !self.store.contains(parent) {
            return None;
        }
        let defaults = self.store.default_indices();
        Some(StyleIndices { store_id: self.store.store_id, font_idx: parent.font_idx, text_idx: parent.text_idx, box_idx: defaults.box_idx, border_idx: defaults.border_idx, bg_idx: defaults.bg_idx })
    }

    pub fn validate_for(&self, document: &Document) -> Result<(), ComputedStylesValidationError> {
        if !self.document_lineage.matches(document.lineage()) {
            return Err(ComputedStylesValidationError::WrongDocument);
        }
        if self.node_styles.len() != document.node_count() {
            return Err(ComputedStylesValidationError::WrongNodeCount { expected: document.node_count(), actual: self.node_styles.len() });
        }
        for (node_id, style) in document.node_ids().zip(self.node_styles.iter().copied()) {
            let node = node_id.index();
            match (document.node_ref(node_id), style) {
                (Some(NodeRef::Element(_)), None) => return Err(ComputedStylesValidationError::MissingElementStyle { node }),
                (Some(NodeRef::Text(_)), Some(_)) => return Err(ComputedStylesValidationError::UnexpectedTextStyle { node }),
                (_, Some(indices)) if !self.store.contains(indices) => return Err(ComputedStylesValidationError::InvalidStyleIndex { node }),
                _ => {}
            }
        }
        Ok(())
    }

    pub fn view(&self, indices: StyleIndices) -> Option<StyleView<'_>> {
        self.store.view(indices)
    }

    pub fn font_style(&self, indices: StyleIndices) -> Option<&Font> {
        self.store.font_style(indices)
    }

    pub fn text_style(&self, indices: StyleIndices) -> Option<&InheritedText> {
        self.store.text_style(indices)
    }

    pub fn box_model_style(&self, indices: StyleIndices) -> Option<&BoxModel> {
        self.store.box_model_style(indices)
    }

    pub fn border_style(&self, indices: StyleIndices) -> Option<&Border> {
        self.store.border_style(indices)
    }

    pub fn background_style(&self, indices: StyleIndices) -> Option<&Background> {
        self.store.background_style(indices)
    }

    pub fn memory_usage_report(&self) -> MemoryUsageReport {
        let mut report = MemoryUsageReport::new();
        report.add_slice_storage::<Option<StyleIndices>>("ComputedStyles.node_styles.storage", self.node_styles.capacity(), self.node_styles.len());
        report.extend_prefixed("ComputedStyles.store", self.store.memory_usage_report());
        report.add_slice_storage::<String>("ComputedStyles.strings.storage", self.strings.capacity(), self.strings.len());
        for value in &self.strings {
            report.add("ComputedStyles.strings.contents", value.capacity(), value.len());
        }
        report
    }
}

/// Computed-style storage (Servo-inspired split styles). Produced by style
/// resolution and consumed read-only by box construction, shaping, and layout.
/// `StyleIndices` from the `ComputedStyles` node side table index these vecs.
struct StyleStore {
    store_id: NonZeroU32,
    font_styles: Vec<Font>,
    text_styles: Vec<InheritedText>,
    box_styles: Vec<BoxModel>,
    border_styles: Vec<Border>,
    bg_styles: Vec<Background>,
    // Dedup maps: identical computed sub-styles share one index, so runs of
    // similarly-styled elements (and inherited groups shared down subtrees)
    // collapse to a single stored entry.
    font_dedup: FxHashMap<Font, u32>,
    text_dedup: FxHashMap<InheritedText, u32>,
    box_dedup: FxHashMap<BoxModel, u32>,
    border_dedup: FxHashMap<Border, u32>,
    bg_dedup: FxHashMap<Background, u32>,
}

static NEXT_STYLE_STORE_ID: AtomicU32 = AtomicU32::new(1);

fn next_style_store_id() -> NonZeroU32 {
    let id = NEXT_STYLE_STORE_ID.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| current.checked_add(1)).expect("computed-style store identity space exhausted");
    NonZeroU32::new(id).expect("style store IDs start at one")
}

/// Append `value` unless an identical one already exists, returning its index.
fn intern_style<T: Clone + Eq + Hash>(values: &mut Vec<T>, dedup: &mut FxHashMap<T, u32>, value: T) -> u32 {
    if let Some(&idx) = dedup.get(&value) {
        return idx;
    }
    let idx = values.len() as u32;
    dedup.insert(value.clone(), idx);
    values.push(value);
    idx
}

impl StyleStore {
    fn new(store_id: NonZeroU32) -> Self {
        Self {
            store_id,
            font_styles: Vec::new(),
            text_styles: Vec::new(),
            box_styles: Vec::new(),
            border_styles: Vec::new(),
            bg_styles: Vec::new(),
            font_dedup: FxHashMap::default(),
            text_dedup: FxHashMap::default(),
            box_dedup: FxHashMap::default(),
            border_dedup: FxHashMap::default(),
            bg_dedup: FxHashMap::default(),
        }
    }

    fn contains(&self, indices: StyleIndices) -> bool {
        indices.store_id == self.store_id
            && (indices.font_idx as usize) < self.font_styles.len()
            && (indices.text_idx as usize) < self.text_styles.len()
            && (indices.box_idx as usize) < self.box_styles.len()
            && (indices.border_idx as usize) < self.border_styles.len()
            && (indices.bg_idx as usize) < self.bg_styles.len()
    }

    #[inline]
    fn owns(&self, indices: StyleIndices) -> bool {
        indices.store_id == self.store_id
    }

    fn default_indices(&self) -> StyleIndices {
        debug_assert!(!self.font_styles.is_empty());
        StyleIndices { store_id: self.store_id, font_idx: 0, text_idx: 0, box_idx: 0, border_idx: 0, bg_idx: 0 }
    }

    /// Ensure default styles exist at index 0 for fallback. Interning here so the
    /// dedup maps know index 0, letting later default styles share it.
    pub fn ensure_defaults(&mut self) {
        if self.font_styles.is_empty() {
            intern_style(&mut self.font_styles, &mut self.font_dedup, Font::default());
        }
        if self.text_styles.is_empty() {
            intern_style(&mut self.text_styles, &mut self.text_dedup, InheritedText::default());
        }
        if self.box_styles.is_empty() {
            intern_style(&mut self.box_styles, &mut self.box_dedup, BoxModel::default());
        }
        if self.border_styles.is_empty() {
            intern_style(&mut self.border_styles, &mut self.border_dedup, Border::default());
        }
        if self.bg_styles.is_empty() {
            intern_style(&mut self.bg_styles, &mut self.bg_dedup, Background::default());
        }
    }

    /// Intern one computed style block, sharing each sub-style's index with an
    /// identical existing one. Returns the indices that address it.
    pub fn push(&mut self, font: Font, text: InheritedText, box_model: BoxModel, border: Border, background: Background) -> StyleIndices {
        StyleIndices {
            store_id: self.store_id,
            font_idx: intern_style(&mut self.font_styles, &mut self.font_dedup, font),
            text_idx: intern_style(&mut self.text_styles, &mut self.text_dedup, text),
            box_idx: intern_style(&mut self.box_styles, &mut self.box_dedup, box_model),
            border_idx: intern_style(&mut self.border_styles, &mut self.border_dedup, border),
            bg_idx: intern_style(&mut self.bg_styles, &mut self.bg_dedup, background),
        }
    }

    fn discard_deduplication_maps(&mut self) {
        self.font_dedup = FxHashMap::default();
        self.text_dedup = FxHashMap::default();
        self.box_dedup = FxHashMap::default();
        self.border_dedup = FxHashMap::default();
        self.bg_dedup = FxHashMap::default();
    }

    fn validate_values(&self) -> Result<(), ComputedStyleValueError> {
        for font in &self.font_styles {
            font.validate(self.store_id)?;
        }
        for text in &self.text_styles {
            text.validate(self.store_id)?;
        }
        for box_model in &self.box_styles {
            box_model.validate(self.store_id)?;
        }
        for border in &self.border_styles {
            border.validate(self.store_id)?;
        }
        for background in &self.bg_styles {
            background.validate(self.store_id)?;
        }
        Ok(())
    }

    /// Resolve a flat `StyleView` from a branded style handle.
    pub fn view(&self, indices: StyleIndices) -> Option<StyleView<'_>> {
        self.owns(indices).then(|| StyleView {
            font: &self.font_styles[indices.font_idx as usize],
            text: &self.text_styles[indices.text_idx as usize],
            box_model: &self.box_styles[indices.box_idx as usize],
            border: &self.border_styles[indices.border_idx as usize],
            background: &self.bg_styles[indices.bg_idx as usize],
        })
    }

    pub fn font_style(&self, indices: StyleIndices) -> Option<&Font> {
        self.owns(indices).then(|| &self.font_styles[indices.font_idx as usize])
    }

    pub fn text_style(&self, indices: StyleIndices) -> Option<&InheritedText> {
        self.owns(indices).then(|| &self.text_styles[indices.text_idx as usize])
    }

    pub fn box_model_style(&self, indices: StyleIndices) -> Option<&BoxModel> {
        self.owns(indices).then(|| &self.box_styles[indices.box_idx as usize])
    }

    pub fn border_style(&self, indices: StyleIndices) -> Option<&Border> {
        self.owns(indices).then(|| &self.border_styles[indices.border_idx as usize])
    }

    pub fn background_style(&self, indices: StyleIndices) -> Option<&Background> {
        self.owns(indices).then(|| &self.bg_styles[indices.bg_idx as usize])
    }

    pub fn memory_usage_report(&self) -> MemoryUsageReport {
        let mut report = MemoryUsageReport::new();
        report.add_slice_storage::<Font>("StyleStore.font_styles.storage", self.font_styles.capacity(), self.font_styles.len());
        report.add_slice_storage::<InheritedText>("StyleStore.text_styles.storage", self.text_styles.capacity(), self.text_styles.len());
        report.add_slice_storage::<BoxModel>("StyleStore.box_styles.storage", self.box_styles.capacity(), self.box_styles.len());
        report.add_slice_storage::<Border>("StyleStore.border_styles.storage", self.border_styles.capacity(), self.border_styles.len());
        report.add_slice_storage::<Background>("StyleStore.bg_styles.storage", self.bg_styles.capacity(), self.bg_styles.len());
        report.add_slice_storage::<(Font, u32)>("StyleStore.font_dedup.storage", self.font_dedup.capacity(), self.font_dedup.len());
        report.add_slice_storage::<(InheritedText, u32)>("StyleStore.text_dedup.storage", self.text_dedup.capacity(), self.text_dedup.len());
        report.add_slice_storage::<(BoxModel, u32)>("StyleStore.box_dedup.storage", self.box_dedup.capacity(), self.box_dedup.len());
        report.add_slice_storage::<(Border, u32)>("StyleStore.border_dedup.storage", self.border_dedup.capacity(), self.border_dedup.len());
        report.add_slice_storage::<(Background, u32)>("StyleStore.bg_dedup.storage", self.bg_dedup.capacity(), self.bg_dedup.len());
        report
    }
}

/// Style view - returned by get_style(), provides flat access to all fields
#[derive(Clone, Copy, Debug)]
pub struct StyleView<'a> {
    pub font: &'a Font,
    pub text: &'a InheritedText,
    pub box_model: &'a BoxModel,
    pub border: &'a Border,
    pub background: &'a Background,
}

impl<'a> StyleView<'a> {
    // Font accessors
    pub fn font_size(&self) -> f32 {
        self.font.font_size
    }
    pub fn font_weight(&self) -> u16 {
        self.font.font_weight
    }
    pub fn font_style(&self) -> FontStyle {
        self.font.font_style
    }
    pub fn font_family(&self) -> Option<StyleStringId> {
        self.font.font_family
    }
    pub fn font_variant_small_caps(&self) -> bool {
        self.font.font_variant_small_caps
    }

    // Text accessors
    pub fn color(&self) -> u32 {
        self.text.color
    }
    pub fn line_height(&self) -> f32 {
        self.text.line_height
    }
    pub fn letter_spacing(&self) -> f32 {
        self.text.letter_spacing.resolve(self.font.font_size)
    }
    pub fn word_spacing(&self) -> f32 {
        self.text.word_spacing.resolve(self.font.font_size)
    }
    pub fn tab_size(&self) -> TabSize {
        self.text.tab_size
    }
    pub fn text_align(&self) -> TextAlign {
        self.text.text_align
    }
    pub fn text_align_last(&self) -> TextAlign {
        self.text.text_align_last
    }
    pub fn text_indent(&self) -> LengthPct {
        self.text.text_indent
    }
    pub fn text_indent_hanging(&self) -> bool {
        self.text.text_indent_hanging
    }
    pub fn text_indent_each_line(&self) -> bool {
        self.text.text_indent_each_line
    }
    pub fn white_space(&self) -> WhiteSpace {
        self.text.white_space
    }
    pub fn word_break(&self) -> WordBreak {
        self.text.word_break
    }
    pub fn overflow_wrap(&self) -> OverflowWrap {
        self.text.overflow_wrap
    }
    pub fn text_transform(&self) -> TextTransform {
        self.text.text_transform
    }
    pub fn list_style_type(&self) -> ListStyleType {
        self.text.list_style_type
    }
    pub fn list_style_position(&self) -> ListStylePosition {
        self.text.list_style_position
    }
    pub fn list_style_image(&self) -> Option<StyleStringId> {
        self.text.list_style_image
    }

    // Box model accessors
    pub fn display(&self) -> Display {
        self.box_model.display
    }
    pub fn box_sizing(&self) -> BoxSizing {
        self.box_model.box_sizing
    }
    pub fn vertical_align(&self) -> VerticalAlignValue {
        self.box_model.vertical_align
    }
    pub fn float(&self) -> Float {
        self.box_model.float
    }
    pub fn clear(&self) -> Clear {
        self.box_model.clear
    }
    pub fn border_collapse(&self) -> BorderCollapseMode {
        self.box_model.border_collapse
    }
    pub fn border_spacing_horizontal(&self) -> f32 {
        self.box_model.border_spacing_horizontal
    }
    pub fn border_spacing_vertical(&self) -> f32 {
        self.box_model.border_spacing_vertical
    }
    pub fn empty_cells(&self) -> EmptyCellsMode {
        self.box_model.empty_cells
    }
    pub fn caption_side(&self) -> CaptionSide {
        self.box_model.caption_side
    }
    pub fn width(&self) -> PreferredSize {
        self.box_model.width
    }
    pub fn height(&self) -> PreferredSize {
        self.box_model.height
    }
    pub fn min_width(&self) -> PreferredSize {
        self.box_model.min_width
    }
    pub fn min_height(&self) -> PreferredSize {
        self.box_model.min_height
    }
    pub fn max_width(&self) -> PreferredSize {
        self.box_model.max_width
    }
    pub fn max_height(&self) -> PreferredSize {
        self.box_model.max_height
    }
    pub fn margin_top(&self) -> LengthPct {
        self.box_model.margin_top
    }
    pub fn margin_bottom(&self) -> LengthPct {
        self.box_model.margin_bottom
    }
    pub fn margin_left(&self) -> LengthPct {
        self.box_model.margin_left
    }
    pub fn margin_right(&self) -> LengthPct {
        self.box_model.margin_right
    }
    pub fn padding_top(&self) -> LengthPct {
        self.box_model.padding_top
    }
    pub fn padding_bottom(&self) -> LengthPct {
        self.box_model.padding_bottom
    }
    pub fn padding_left(&self) -> LengthPct {
        self.box_model.padding_left
    }
    pub fn padding_right(&self) -> LengthPct {
        self.box_model.padding_right
    }

    // Border accessors
    pub fn border_top_width(&self) -> f32 {
        self.border.border_top_width
    }
    pub fn border_right_width(&self) -> f32 {
        self.border.border_right_width
    }
    pub fn border_bottom_width(&self) -> f32 {
        self.border.border_bottom_width
    }
    pub fn border_left_width(&self) -> f32 {
        self.border.border_left_width
    }
    pub fn border_top_color(&self) -> u32 {
        self.border.border_top_color
    }
    pub fn border_right_color(&self) -> u32 {
        self.border.border_right_color
    }
    pub fn border_bottom_color(&self) -> u32 {
        self.border.border_bottom_color
    }
    pub fn border_left_color(&self) -> u32 {
        self.border.border_left_color
    }
    pub fn border_top_style(&self) -> BorderStyle {
        self.border.border_top_style
    }
    pub fn border_right_style(&self) -> BorderStyle {
        self.border.border_right_style
    }
    pub fn border_bottom_style(&self) -> BorderStyle {
        self.border.border_bottom_style
    }
    pub fn border_left_style(&self) -> BorderStyle {
        self.border.border_left_style
    }

    // Background accessors
    pub fn background_color(&self) -> u32 {
        self.background.background_color
    }
    pub fn text_decoration(&self) -> u8 {
        self.background.text_decoration
    }

    // Helper methods (moved from ComputedStyle). Margin/padding percentages
    // resolve against the containing block's inline-size, so these take the
    // containing-block width `cb_width` (CSS 2.1 §8.3, §8.4).
    pub fn get_horizontal_margin_padding(&self, cb_width: f64) -> f64 {
        self.get_horizontal_margin(cb_width) + self.get_horizontal_padding(cb_width)
    }

    pub fn get_horizontal_margin(&self, cb_width: f64) -> f64 {
        self.margin_left().resolve(cb_width) + self.margin_right().resolve(cb_width)
    }

    pub fn get_vertical_margin_padding(&self, cb_width: f64) -> f64 {
        self.margin_top().resolve(cb_width) + self.margin_bottom().resolve(cb_width) + self.padding_top().resolve(cb_width) + self.padding_bottom().resolve(cb_width)
    }

    pub fn get_horizontal_padding(&self, cb_width: f64) -> f64 {
        self.padding_left().resolve(cb_width) + self.padding_right().resolve(cb_width)
    }

    pub fn get_vertical_padding(&self, cb_width: f64) -> f64 {
        self.padding_top().resolve(cb_width) + self.padding_bottom().resolve(cb_width)
    }

    pub fn get_top_left_padding(&self, cb_width: f64) -> Vec2 {
        Vec2::new(self.padding_left().resolve(cb_width), self.padding_top().resolve(cb_width))
    }

    pub fn should_parent_sibling_collapse(&self) -> bool {
        self.padding_top().is_zero() && self.padding_bottom().is_zero() && self.padding_left().is_zero() && self.padding_right().is_zero()
    }
}

// Default implementations for sub-structs
impl Default for Font {
    fn default() -> Self {
        Self { font_size: 16.0, font_weight: 400, font_style: FontStyle::Normal, font_family: None, font_variant_small_caps: false }
    }
}

impl Default for InheritedText {
    fn default() -> Self {
        Self {
            color: 0x000000FF,
            line_height: 0.0,
            line_height_number: 0.0,
            letter_spacing: TextSpacing::ZERO,
            word_spacing: TextSpacing::ZERO,
            tab_size: TabSize::DEFAULT,
            text_align: TextAlign::Left,
            text_align_last: TextAlign::Left,
            text_align_last_explicit: false,
            text_indent: LengthPct::Px(0.0),
            text_indent_hanging: false,
            text_indent_each_line: false,
            white_space: WhiteSpace::Normal,
            word_break: WordBreak::Normal,
            overflow_wrap: OverflowWrap::Normal,
            text_transform: TextTransform::None,
            list_style_type: ListStyleType::Disc,
            list_style_position: ListStylePosition::Outside,
            list_style_image: None,
        }
    }
}

impl Default for BoxModel {
    fn default() -> Self {
        Self {
            display: Display::Block,
            box_sizing: BoxSizing::ContentBox,
            vertical_align: VerticalAlignValue::Baseline,
            float: Float::None,
            clear: Clear::None,
            border_collapse: BorderCollapseMode::Separate,
            border_spacing_horizontal: 0.0,
            border_spacing_vertical: 0.0,
            empty_cells: EmptyCellsMode::Show,
            caption_side: CaptionSide::Top,
            width: PreferredSize::Auto,
            height: PreferredSize::Auto,
            min_width: PreferredSize::Auto,
            min_height: PreferredSize::Auto,
            max_width: PreferredSize::Auto,
            max_height: PreferredSize::Auto,
            margin_top: LengthPct::Px(0.0),
            margin_bottom: LengthPct::Px(0.0),
            margin_left: LengthPct::Px(0.0),
            margin_right: LengthPct::Px(0.0),
            padding_top: LengthPct::Px(0.0),
            padding_bottom: LengthPct::Px(0.0),
            padding_left: LengthPct::Px(0.0),
            padding_right: LengthPct::Px(0.0),
        }
    }
}

impl Default for Border {
    fn default() -> Self {
        Self {
            border_top_width: 0.0,
            border_right_width: 0.0,
            border_bottom_width: 0.0,
            border_left_width: 0.0,
            border_top_color: 0x000000FF,
            border_right_color: 0x000000FF,
            border_bottom_color: 0x000000FF,
            border_left_color: 0x000000FF,
            border_top_style: BorderStyle::None,
            border_right_style: BorderStyle::None,
            border_bottom_style: BorderStyle::None,
            border_left_style: BorderStyle::None,
        }
    }
}

impl Default for Background {
    fn default() -> Self {
        Self { background_color: 0x00000000, text_decoration: 0 }
    }
}

// Default constants for reset groups (used by anonymous boxes)
pub const DEFAULT_BOX_MODEL: BoxModel = BoxModel {
    display: Display::Block,
    box_sizing: BoxSizing::ContentBox,
    vertical_align: VerticalAlignValue::Baseline,
    float: Float::None,
    clear: Clear::None,
    border_collapse: BorderCollapseMode::Separate,
    border_spacing_horizontal: 0.0,
    border_spacing_vertical: 0.0,
    empty_cells: EmptyCellsMode::Show,
    caption_side: CaptionSide::Top,
    width: PreferredSize::Auto,
    height: PreferredSize::Auto,
    min_width: PreferredSize::Auto,
    min_height: PreferredSize::Auto,
    max_width: PreferredSize::Auto,
    max_height: PreferredSize::Auto,
    margin_top: LengthPct::Px(0.0),
    margin_bottom: LengthPct::Px(0.0),
    margin_left: LengthPct::Px(0.0),
    margin_right: LengthPct::Px(0.0),
    padding_top: LengthPct::Px(0.0),
    padding_bottom: LengthPct::Px(0.0),
    padding_left: LengthPct::Px(0.0),
    padding_right: LengthPct::Px(0.0),
};

pub const DEFAULT_BORDER: Border = Border {
    border_top_width: 0.0,
    border_right_width: 0.0,
    border_bottom_width: 0.0,
    border_left_width: 0.0,
    border_top_color: 0x000000FF,
    border_right_color: 0x000000FF,
    border_bottom_color: 0x000000FF,
    border_left_color: 0x000000FF,
    border_top_style: BorderStyle::None,
    border_right_style: BorderStyle::None,
    border_bottom_style: BorderStyle::None,
    border_left_style: BorderStyle::None,
};

pub const DEFAULT_BACKGROUND: Background = Background { background_color: 0x00000000, text_decoration: 0 };

/// A box-model length (margin, padding, or text-indent) that may be a fixed
/// pixel value or a percentage of the containing block's inline-size.
///
/// CSS resolves percentage margins/padding/text-indent against the containing
/// block *width* — even for the top/bottom edges (CSS 2.1 §8.3, §8.4) — which is
/// not known during style resolution. So these keep this form after the cascade
/// and are resolved to pixels at layout time via [`LengthPct::resolve`]. Pixel
/// values (including resolved `em`/`rem`, which are font-relative, not
/// container-relative) are stored directly as `Px`.
#[derive(Clone, Copy, Debug)]
pub enum LengthPct {
    Px(f32),
    /// Fraction of the containing block inline-size (`0.05` == `5%`).
    Pct(f32),
}

impl LengthPct {
    /// Resolve to pixels against the containing-block inline-size `cb_width`.
    pub fn resolve(self, cb_width: f64) -> f64 {
        match self {
            LengthPct::Px(px) => px as f64,
            LengthPct::Pct(fraction) => cb_width * fraction as f64,
        }
    }

    /// True when this contributes no length regardless of containing-block width.
    /// Used by margin-collapsing edge checks, which only care about zero-ness.
    pub fn is_zero(self) -> bool {
        match self {
            LengthPct::Px(px) => px == 0.0,
            LengthPct::Pct(fraction) => fraction == 0.0,
        }
    }
}

impl Default for LengthPct {
    fn default() -> Self {
        LengthPct::Px(0.0)
    }
}

impl PartialEq for LengthPct {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (LengthPct::Px(a), LengthPct::Px(b)) => a.to_bits() == b.to_bits(),
            (LengthPct::Pct(a), LengthPct::Pct(b)) => a.to_bits() == b.to_bits(),
            _ => false,
        }
    }
}
impl Eq for LengthPct {}
impl Hash for LengthPct {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            LengthPct::Px(v) | LengthPct::Pct(v) => state.write_u32(v.to_bits()),
        }
    }
}

pub fn resolve_preffered_size(preferred_size: PreferredSize, auto: f64, parent_size: f64) -> f64 {
    match preferred_size {
        PreferredSize::Auto => auto,
        PreferredSize::Px(px) => px as f64,
        PreferredSize::Percent(pct) => parent_size * pct as f64,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FontStyle {
    #[default]
    Normal,
    Italic,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextAlign {
    #[default]
    Left,
    Right,
    Center,
    Justify,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum WhiteSpace {
    #[default]
    Normal,
    Pre,
    NoWrap,
    PreWrap,
    PreLine,
    BreakSpaces,
    PreserveBreaksNoWrap,
    BreakSpacesNoWrap,
}

impl WhiteSpace {
    pub fn allows_wrap(self) -> bool {
        !matches!(self, Self::Pre | Self::NoWrap | Self::PreserveBreaksNoWrap | Self::BreakSpacesNoWrap)
    }

    pub fn collapses_spaces(self) -> bool {
        matches!(self, Self::Normal | Self::NoWrap | Self::PreLine | Self::PreserveBreaksNoWrap)
    }

    pub fn preserves_newlines(self) -> bool {
        !matches!(self, Self::Normal | Self::NoWrap)
    }

    pub fn preserves_spaces(self) -> bool {
        matches!(self, Self::Pre | Self::PreWrap | Self::BreakSpaces | Self::BreakSpacesNoWrap)
    }
}

/// Computed `word-break`. The legacy `break-word` value behaves like
/// `overflow-wrap: break-word` and is mapped onto that property instead.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum WordBreak {
    #[default]
    Normal,
    /// Allow a break between any two characters.
    BreakAll,
    /// Forbid breaks inside CJK "words". Treated like `Normal` here, since the
    /// default breaker already only breaks at spaces.
    KeepAll,
}

/// Computed `overflow-wrap` (and its legacy alias `word-wrap`). Both
/// non-normal values allow an emergency mid-word break when a word cannot fit
/// on a line by itself; the min-content sizing distinction between them is not
/// modeled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum OverflowWrap {
    #[default]
    Normal,
    BreakWord,
    Anywhere,
}

/// Computed `box-sizing`: whether `width`/`height` (and their min/max) specify
/// the content box or the border box.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BoxSizing {
    #[default]
    ContentBox,
    BorderBox,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BorderStyle {
    #[default]
    None,
    Solid,
    Dashed,
    Dotted,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextTransform {
    #[default]
    None,
    Uppercase,
    Lowercase,
    Capitalize,
}

/// Computed `list-style-type`. Unsupported `@counter-style`/predefined values
/// from lightningcss fall back to `Decimal` (numeric) per CSS counter-style
/// fallback rules; symbolic aliases collapse onto these.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ListStyleType {
    None,
    #[default]
    Disc,
    Circle,
    Square,
    Decimal,
    DecimalLeadingZero,
    LowerAlpha,
    UpperAlpha,
    LowerRoman,
    UpperRoman,
}

impl ListStyleType {
    /// True for the bullet glyph types whose marker text is fixed (no counter).
    pub fn is_bullet(self) -> bool {
        matches!(self, ListStyleType::Disc | ListStyleType::Circle | ListStyleType::Square)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ListStylePosition {
    #[default]
    Outside,
    Inside,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Display {
    #[default]
    Block,
    Inline,
    None,
    ListItem,
    // Table display types
    Table,
    InlineTable,
    TableRowGroup,
    TableHeaderGroup,
    TableFooterGroup,
    TableRow,
    TableColumnGroup,
    TableColumn,
    TableCell,
    TableCaption,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Float {
    #[default]
    None,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Clear {
    #[default]
    None,
    Left,
    Right,
    Both,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BorderCollapseMode {
    Collapse,
    #[default]
    Separate,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum EmptyCellsMode {
    Hide,
    #[default]
    Show,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CaptionSide {
    Bottom,
    #[default]
    Top,
}

#[derive(Clone, Copy, Debug, Default)]
pub enum VerticalAlignValue {
    #[default]
    Baseline,
    Sub,
    Super,
    Top,
    TextTop,
    Middle,
    Bottom,
    TextBottom,
    Length(f32),
    Percent(f32),
}

impl PartialEq for VerticalAlignValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (VerticalAlignValue::Length(a), VerticalAlignValue::Length(b)) => a.to_bits() == b.to_bits(),
            (VerticalAlignValue::Percent(a), VerticalAlignValue::Percent(b)) => a.to_bits() == b.to_bits(),
            _ => std::mem::discriminant(self) == std::mem::discriminant(other),
        }
    }
}
impl Eq for VerticalAlignValue {}
impl Hash for VerticalAlignValue {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            VerticalAlignValue::Length(v) | VerticalAlignValue::Percent(v) => state.write_u32(v.to_bits()),
            _ => {}
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub enum PreferredSize {
    #[default]
    Auto,
    Px(f32),
    Percent(f32),
}

impl PartialEq for PreferredSize {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (PreferredSize::Px(a), PreferredSize::Px(b)) => a.to_bits() == b.to_bits(),
            (PreferredSize::Percent(a), PreferredSize::Percent(b)) => a.to_bits() == b.to_bits(),
            _ => std::mem::discriminant(self) == std::mem::discriminant(other),
        }
    }
}
impl Eq for PreferredSize {}
impl Hash for PreferredSize {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            PreferredSize::Px(v) | PreferredSize::Percent(v) => state.write_u32(v.to_bits()),
            PreferredSize::Auto => {}
        }
    }
}

#[cfg(test)]
mod boundary_tests {
    use super::{ComputedStyleValueError, ComputedStylesBuilder, Font, InheritedText, StyleIndices, StyleStringId};
    use html_dom::Document;
    use std::mem::size_of;

    #[test]
    fn style_handles_are_branded_by_their_store() {
        let document = Document::new();
        let mut first = ComputedStylesBuilder::new(&document);
        let second = ComputedStylesBuilder::new(&document);
        let foreign = first.default_indices();
        let foreign_string = first.intern_string("first store");

        assert!(!second.store.contains(foreign));
        assert!(second.view(foreign).is_none());
        assert!(second.font_style(foreign).is_none());
        assert!(second.string(foreign_string).is_none());
    }

    #[test]
    fn store_brand_uses_the_option_niche_without_growing_node_entries() {
        assert_eq!(size_of::<Option<StyleIndices>>(), size_of::<StyleIndices>());
        assert_eq!(size_of::<Option<StyleStringId>>(), size_of::<StyleStringId>());
    }

    #[test]
    fn computed_style_push_rejects_non_finite_values() {
        let document = Document::new();
        let mut builder = ComputedStylesBuilder::new(&document);

        let mut font = Font::default();
        font.font_size = f32::NAN;
        assert!(matches!(builder.push(font, InheritedText::default(), Default::default(), Default::default(), Default::default()), Err(ComputedStyleValueError::NonFiniteValue { property: "Font", field: "font_size", .. })));

        assert!(super::TextSpacing::new(f32::INFINITY, 0.0).is_none());
        assert!(super::TextSpacing::new(0.0, f32::NAN).is_none());

        let mut box_model = super::BoxModel::default();
        box_model.width = super::PreferredSize::Percent(f32::NAN);
        assert!(matches!(builder.push(Font::default(), InheritedText::default(), box_model, Default::default(), Default::default()), Err(ComputedStyleValueError::NonFiniteValue { property: "BoxModel", field: "width", .. })));

        let mut padding = super::BoxModel::default();
        padding.padding_left = super::LengthPct::Px(f32::INFINITY);
        assert!(matches!(builder.push(Font::default(), InheritedText::default(), padding, Default::default(), Default::default()), Err(ComputedStyleValueError::NonFiniteValue { property: "BoxModel", field: "padding_left", .. })));

        let mut border = super::Border::default();
        border.border_top_width = f32::NAN;
        assert!(matches!(builder.push(Font::default(), InheritedText::default(), Default::default(), border, Default::default()), Err(ComputedStyleValueError::NonFiniteValue { property: "Border", field: "border_top_width", .. })));
    }

    #[test]
    fn computed_style_push_enforces_positive_sizes_and_supported_font_weight() {
        let document = Document::new();
        let mut builder = ComputedStylesBuilder::new(&document);

        let mut font = Font::default();
        font.font_size = 0.0;
        assert!(matches!(builder.push(font, InheritedText::default(), Default::default(), Default::default(), Default::default()), Err(ComputedStyleValueError::MustBePositive { property: "Font", field: "font_size", .. })));

        let mut font = Font::default();
        font.font_weight = 1001;
        assert!(matches!(builder.push(font, InheritedText::default(), Default::default(), Default::default(), Default::default()), Err(ComputedStyleValueError::InvalidFontWeight { value: 1001 })));

        let mut text = InheritedText::default();
        text.line_height = -1.0;
        assert!(matches!(builder.push(Font::default(), text, Default::default(), Default::default(), Default::default()), Err(ComputedStyleValueError::OutOfRangeValue { property: "InheritedText", field: "line_height", .. })));

        assert!(builder.push(Font::default(), InheritedText::default(), Default::default(), Default::default(), Default::default()).is_ok(), "zero line-height sentinel remains valid");
    }

    #[test]
    fn finish_revalidates_stored_values_as_defense_in_depth() {
        let document = Document::new();
        let mut builder = ComputedStylesBuilder::new(&document);
        builder.store.font_styles[0].font_size = f32::NAN;

        assert!(matches!(builder.finish(), Err(super::ComputedStylesBuildError::InvalidStyleValue(ComputedStyleValueError::NonFiniteValue { property: "Font", field: "font_size", .. }))));
    }

    #[test]
    fn computed_style_push_accepts_negative_margins_and_rejects_negative_border_width() {
        let document = Document::new();
        let mut builder = ComputedStylesBuilder::new(&document);

        let mut box_model = super::BoxModel::default();
        box_model.margin_left = crate::LengthPct::Px(-4.0);
        assert!(builder.push(Font::default(), InheritedText::default(), box_model, Default::default(), Default::default()).is_ok());

        let mut border = super::Border::default();
        border.border_left_width = -0.5;
        assert!(matches!(builder.push(Font::default(), InheritedText::default(), Default::default(), border, Default::default()), Err(ComputedStyleValueError::OutOfRangeValue { property: "Border", field: "border_left_width", .. })));
    }

    #[test]
    fn computed_style_push_rejects_foreign_style_string_ids() {
        let document = Document::new();
        let mut first = ComputedStylesBuilder::new(&document);
        let foreign = first.intern_string("foreign");
        let mut builder = ComputedStylesBuilder::new(&document);

        let mut font = Font::default();
        font.font_family = Some(foreign);
        assert!(matches!(builder.push(font, InheritedText::default(), Default::default(), Default::default(), Default::default()), Err(ComputedStyleValueError::ForeignStyleStringId { .. })));
    }
}
