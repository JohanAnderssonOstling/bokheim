use crate::style::declarations::normalize as normalize_declarations;
use crate::style::prepared::{EffectiveRuleId, PreparedRuleSet};
use crate::style::selectors::SelectorIndex;
use crate::style::tab_size::{CASCADE_MARKER as TAB_SIZE_CASCADE_MARKER, ParsedTabSize, parse as parse_tab_size};
use crate::style::text_spacing::{LETTER_SPACING_MARKER, ParsedSpacing, WORD_SPACING_MARKER, parse as parse_text_spacing};
use crate::style::white_space::{CASCADE_MARKER as WHITE_SPACE_CASCADE_MARKER, from_marker_tokens};
use html_dom::{Document, DomNodeId};
use html_style_model::{
    Background, Border, BorderCollapseMode, BorderStyle, BoxModel, BoxSizing, CaptionSide, Clear, ComputedStyleValueError, ComputedStyles, ComputedStylesBuilder, Display, EmptyCellsMode, Float, Font, FontStyle, InheritedText, LengthPct,
    OverflowWrap, PreferredSize, StyleIndices, StyleStringId, TabSize, TextAlign, TextSpacing, TextTransform, VerticalAlignValue, WhiteSpace, WordBreak,
};
use lightningcss::printer::{Printer, PrinterOptions};
use lightningcss::properties::Property;
use lightningcss::properties::border::{BorderSideWidth, LineStyle};
use lightningcss::properties::custom::{Token, TokenList, TokenOrValue};
use lightningcss::properties::font::LineHeight as CssLineHeight;
use lightningcss::properties::font::{AbsoluteFontWeight, FontFamily, FontSize, FontWeight, VerticalAlign, VerticalAlignKeyword};
use lightningcss::properties::size::{MaxSize, Size};
use lightningcss::properties::text::Spacing;
use lightningcss::stylesheet::{ParserOptions, StyleAttribute};
use lightningcss::traits::{ParseWithOptions, ToCss};
use lightningcss::values::calc::{Calc, MathFunction};
use lightningcss::values::color::{CssColor, SystemColor};
use lightningcss::values::length::{Length, LengthPercentage, LengthPercentageOrAuto, LengthValue};
use rustc_data_structures::fx::FxHashMap;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

// ============================================================================
// Style Resolver - applies CSS styles to Document boxes
// ============================================================================
/// Mutable accumulator the cascade writes into: the same split sub-structs the
/// `StyleStore` ultimately stores, bundled so resolution can mutate one value.
/// Inherited groups (`font`, `text`) are cloned from the parent; reset groups
/// start from their defaults.
#[derive(Clone, Default)]
struct WorkingStyle {
    font: Font,
    text: InheritedText,
    box_model: BoxModel,
    border: Border,
    background: Background,
    line_height_spec: Option<CssLineHeight>,
    /// Whether a border color was declared per side (top, right, bottom,
    /// left). Undeclared sides take the initial value `currentColor`.
    border_color_explicit: [bool; 4],
}

impl WorkingStyle {
    /// Move the resolved sub-styles into the document's deduped style store.
    fn intern(self, styles: &mut ComputedStylesBuilder) -> Result<StyleIndices, ComputedStyleValueError> {
        styles.push(self.font, self.text, self.box_model, self.border, self.background)
    }
}

/// Matched rule with specificity for cascade sorting
#[derive(Clone, Copy)]
struct MatchedRule {
    specificity: u32,
    id: EffectiveRuleId,
}

/// The parent element's interned computed style, used to resolve the CSS-wide
/// keywords `inherit` and `unset` (and `initial` via [`ParentStyle::initial`]).
struct ParentStyle {
    font: Font,
    text: InheritedText,
    box_model: BoxModel,
    border: Border,
    background: Background,
}

impl ParentStyle {
    fn for_node(doc: &Document, styles: &ComputedStylesBuilder, node_idx: DomNodeId) -> Self {
        if let Some(parent_idx) = doc.get_dom_parent(node_idx) {
            if let Some(indices) = styles.style_for_node(parent_idx) {
                return Self {
                    font: styles.font_style(indices).expect("builder-issued style handle").clone(),
                    text: styles.text_style(indices).expect("builder-issued style handle").clone(),
                    box_model: styles.box_model_style(indices).expect("builder-issued style handle").clone(),
                    border: styles.border_style(indices).expect("builder-issued style handle").clone(),
                    background: styles.background_style(indices).expect("builder-issued style handle").clone(),
                };
            }
        }
        Self::initial(doc.root_font_size())
    }

    /// The initial value of every property (`font-size: medium` resolves to
    /// the document's root font size).
    fn initial(root_font_size: f32) -> Self {
        let mut font = Font::default();
        font.font_size = root_font_size;
        Self { font, text: InheritedText::default(), box_model: BoxModel::default(), border: Border::default(), background: Background::default() }
    }
}

/// Resolve styles for all DOM elements
/// This works with the DOM tree before boxes are created
#[derive(Clone, Debug, Default)]
pub(crate) struct ResolveStyleTimings {
    pub selector_index: Duration,
    pub resolver_setup: Duration,
    pub selector_matching: Duration,
    pub cascade: Duration,
    pub style_store: Duration,
}

pub(crate) fn resolve_styles_for_dom_timed<'sheet, 'css>(doc: &Document, prepared: &PreparedRuleSet<'sheet, 'css>) -> (ComputedStyles, ResolveStyleTimings) {
    // Build selector index for fast candidate lookup
    let started = Instant::now();
    let index = SelectorIndex::from_prepared(prepared);
    let mut timings = ResolveStyleTimings { selector_index: started.elapsed(), ..ResolveStyleTimings::default() };

    // Process each DOM element
    let started = Instant::now();
    let mut custom_maps: Vec<FxHashMap<String, TokenList<'css>>> = vec![FxHashMap::default(); doc.node_count()];
    let mut computed_styles = ComputedStylesBuilder::new(doc);
    let mut candidate_scratch = Vec::new();
    let mut candidate_seen = rustc_data_structures::fx::FxHashSet::default();
    let mut ancestor_filters = vec![AncestorFilter::default(); doc.node_count()];
    timings.resolver_setup = started.elapsed();
    for node_idx in doc.node_ids() {
        // Skip text nodes
        if doc.is_text_node(node_idx) {
            continue;
        }

        let ancestor_filter = if let Some(parent_idx) = doc.get_dom_parent(node_idx) {
            let mut filter = ancestor_filters[parent_idx.index()];
            extend_ancestor_filter_dom(doc, parent_idx, &mut filter);
            filter
        } else {
            AncestorFilter::default()
        };
        ancestor_filters[node_idx.index()] = ancestor_filter;
        let parent_custom = doc.get_dom_parent(node_idx).and_then(|parent_idx| custom_maps.get(parent_idx.index()));

        let (style, custom_map) = compute_style_for_dom_element(doc, &mut computed_styles, node_idx, &index, prepared, parent_custom, &mut candidate_scratch, &mut candidate_seen, &ancestor_filter, &mut timings);

        if let Some(slot) = custom_maps.get_mut(node_idx.index()) {
            *slot = custom_map;
        }

        let started = Instant::now();
        let style_indices = style.intern(&mut computed_styles).expect("resolver built style values should pass style validation");
        computed_styles.set_node_style(node_idx, style_indices).expect("resolver only assigns styles to nodes from its source document");
        timings.style_store += started.elapsed();
    }
    let started = Instant::now();
    let styles = computed_styles.finish().expect("resolver must assign a valid computed style to every element");
    timings.style_store += started.elapsed();
    (styles, timings)
}

/// The cascade runs twice over every declaration block: `font-size` and
/// `color` are resolved in the first pass, so `em`-based lengths and
/// `currentColor` anywhere in the cascade see the element's final values
/// regardless of declaration order.
#[derive(Clone, Copy, PartialEq)]
enum CascadePhase {
    FontSizeAndColor,
    Remaining,
}

/// Apply CSS declarations to a computed style
fn apply_declarations<'a>(
    doc: &Document, styles: &mut ComputedStylesBuilder, style: &mut WorkingStyle, declarations: &[Property<'a>], parent_font_size: f32, custom_properties: Option<&FxHashMap<String, TokenList<'a>>>, phase: CascadePhase, parent: &ParentStyle,
) {
    let mut var_map: HashMap<&str, TokenList<'a>> = HashMap::new();
    if let Some(map) = custom_properties {
        for (name, value) in map {
            var_map.insert(name.as_str(), value.clone());
        }
    }

    for property in declarations {
        match property {
            Property::Custom(custom) => {
                let raw_name = custom.name.as_ref();
                if raw_name.starts_with("--") {
                    var_map.insert(raw_name, custom.value.clone());
                    continue;
                }
            }
            Property::Unparsed(unparsed) if unparsed.property_id.name().starts_with("--") => {
                var_map.insert(unparsed.property_id.name(), unparsed.value.clone());
                continue;
            }
            _ => {}
        }

        if let Property::Unparsed(unparsed) = property {
            if !var_map.is_empty() {
                if let Ok(resolved) = unparsed.clone().substitute_variables(&var_map) {
                    apply_property_in_phase(doc, styles, style, &resolved, parent_font_size, phase, parent);
                    continue;
                }
                if let Some(resolved) = resolve_single_var_property(unparsed, &var_map) {
                    apply_property_in_phase(doc, styles, style, &resolved, parent_font_size, phase, parent);
                    continue;
                }
            }
        }

        apply_property_in_phase(doc, styles, style, property, parent_font_size, phase, parent);
    }
}

fn apply_property_in_phase<'a>(doc: &Document, styles: &mut ComputedStylesBuilder, style: &mut WorkingStyle, property: &Property<'a>, parent_font_size: f32, phase: CascadePhase, parent: &ParentStyle) {
    // CSS-wide keywords copy already-computed values, so applying them in both
    // phases is idempotent and keeps cascade order within each phase.
    if try_apply_css_wide_keyword(style, property, parent, doc.root_font_size()) {
        return;
    }
    match phase {
        CascadePhase::FontSizeAndColor => match property {
            Property::FontSize(_) | Property::Color(_) => apply_property(doc, styles, style, property, parent_font_size),
            Property::Font(font) => {
                style.font.font_size = font_size_to_px(&font.size, parent_font_size, doc.root_font_size());
            }
            _ => {}
        },
        CascadePhase::Remaining => match property {
            // Already final from the first pass.
            Property::FontSize(_) | Property::Color(_) => {}
            // Apply the shorthand but keep the font size resolved by the first
            // pass, re-binding the shorthand's line-height to it.
            Property::Font(_) => {
                let final_font_size = style.font.font_size;
                apply_property(doc, styles, style, property, parent_font_size);
                style.font.font_size = final_font_size;
                if let Some(spec) = style.line_height_spec.as_ref() {
                    style.text.line_height = parse_line_height(spec, final_font_size, doc.root_font_size());
                }
            }
            _ => apply_property(doc, styles, style, property, parent_font_size),
        },
    }
}

fn apply_property<'a>(doc: &Document, styles: &mut ComputedStylesBuilder, style: &mut WorkingStyle, property: &Property<'a>, parent_font_size: f32) {
    match property {
        // Font properties
        Property::FontSize(size) => {
            style.font.font_size = font_size_to_px(size, parent_font_size, doc.root_font_size());
            if let Some(spec) = style.line_height_spec.as_ref() {
                style.text.line_height = parse_line_height(spec, style.font.font_size, doc.root_font_size());
            } else if style.text.line_height_number != 0.0 {
                // An inherited unitless line-height re-resolves against this
                // element's own font size.
                style.text.line_height = style.font.font_size * style.text.line_height_number;
            }
        }
        Property::FontWeight(weight) => {
            style.font.font_weight = font_weight_to_u16(weight);
        }
        Property::FontStyle(fs) => {
            use lightningcss::properties::font::FontStyle as LcFontStyle;
            style.font.font_style = match fs {
                LcFontStyle::Normal => FontStyle::Normal,
                LcFontStyle::Italic => FontStyle::Italic,
                LcFontStyle::Oblique(_) => FontStyle::Italic,
            };
        }
        Property::FontFamily(families) => {
            style.font.font_family = Some(intern_font_family(styles, families));
        }
        Property::Font(font) => {
            style.font.font_size = font_size_to_px(&font.size, parent_font_size, doc.root_font_size());
            style.font.font_weight = font_weight_to_u16(&font.weight);
            use lightningcss::properties::font::FontStyle as LcFontStyle;
            style.font.font_style = match font.style {
                LcFontStyle::Normal => FontStyle::Normal,
                LcFontStyle::Italic => FontStyle::Italic,
                LcFontStyle::Oblique(_) => FontStyle::Italic,
            };
            style.line_height_spec = Some(font.line_height.clone());
            style.text.line_height_number = line_height_number(&font.line_height);
            style.text.line_height = parse_line_height(&font.line_height, style.font.font_size, doc.root_font_size());
            style.font.font_family = Some(intern_font_family(styles, &font.family));
            use lightningcss::properties::font::FontVariantCaps;
            style.font.font_variant_small_caps = matches!(font.variant_caps, FontVariantCaps::SmallCaps);
        }

        // Text properties
        Property::LineHeight(lh) => {
            style.line_height_spec = Some(lh.clone());
            style.text.line_height_number = line_height_number(lh);
            style.text.line_height = parse_line_height(lh, style.font.font_size, doc.root_font_size());
        }
        Property::LetterSpacing(spacing) => {
            style.text.letter_spacing = spacing_to_text_spacing(spacing, style.font.font_size, doc.root_font_size());
        }
        Property::WordSpacing(spacing) => {
            style.text.word_spacing = spacing_to_text_spacing(spacing, style.font.font_size, doc.root_font_size());
        }
        Property::TextAlign(ta) => {
            use lightningcss::properties::text::TextAlign as LcTextAlign;
            style.text.text_align = match ta {
                LcTextAlign::Left => TextAlign::Left,
                LcTextAlign::Right => TextAlign::Right,
                LcTextAlign::Center => TextAlign::Center,
                LcTextAlign::Justify => TextAlign::Justify,
                LcTextAlign::Start => TextAlign::Left,
                LcTextAlign::End => TextAlign::Right,
                LcTextAlign::JustifyAll => TextAlign::Justify,
                // For horizontal LTR, `match-parent` computes to the parent's
                // value — which is what this element inherited.
                LcTextAlign::MatchParent => style.text.text_align,
            };
            // `text-align-last: auto` (the default) follows `text-align`.
            if !style.text.text_align_last_explicit {
                style.text.text_align_last = style.text.text_align;
            }
        }
        Property::TextAlignLast(ta, _) => {
            use lightningcss::properties::text::TextAlignLast as LcTextAlignLast;
            let explicit = match ta {
                LcTextAlignLast::Auto => None,
                LcTextAlignLast::Left => Some(TextAlign::Left),
                LcTextAlignLast::Right => Some(TextAlign::Right),
                LcTextAlignLast::Center => Some(TextAlign::Center),
                LcTextAlignLast::Justify => Some(TextAlign::Justify),
                LcTextAlignLast::Start => Some(TextAlign::Left),
                LcTextAlignLast::End => Some(TextAlign::Right),
                _ => None,
            };
            style.text.text_align_last_explicit = explicit.is_some();
            style.text.text_align_last = explicit.unwrap_or(style.text.text_align);
        }
        Property::TextIndent(ti) => {
            style.text.text_indent = length_percentage_to_lengthpct(&ti.value, style.font.font_size, doc.root_font_size());
            style.text.text_indent_hanging = ti.hanging;
            style.text.text_indent_each_line = ti.each_line;
        }
        Property::TextTransform(tt) => {
            use lightningcss::properties::text::TextTransformCase;
            style.text.text_transform = match tt.case {
                TextTransformCase::Uppercase => TextTransform::Uppercase,
                TextTransformCase::Lowercase => TextTransform::Lowercase,
                TextTransformCase::Capitalize => TextTransform::Capitalize,
                _ => TextTransform::None,
            };
        }
        Property::FontVariantCaps(caps) => {
            use lightningcss::properties::font::FontVariantCaps;
            style.font.font_variant_small_caps = matches!(caps, FontVariantCaps::SmallCaps);
        }
        Property::ListStyleType(lst) => {
            style.text.list_style_type = map_list_style_type(lst);
        }
        Property::ListStylePosition(pos) => {
            style.text.list_style_position = map_list_style_position(pos);
        }
        Property::ListStyleImage(image) => {
            style.text.list_style_image = list_style_image_to_interned(styles, image);
        }
        Property::ListStyle(ls) => {
            style.text.list_style_type = map_list_style_type(&ls.list_style_type);
            style.text.list_style_position = map_list_style_position(&ls.position);
            style.text.list_style_image = list_style_image_to_interned(styles, &ls.image);
        }
        Property::TextDecorationLine(line, _) => {
            apply_text_decoration(&mut style.background.text_decoration, line);
        }
        Property::TextDecoration(td, _) => {
            apply_text_decoration(&mut style.background.text_decoration, &td.line);
        }
        Property::VerticalAlign(va) => {
            style.box_model.vertical_align = match va {
                VerticalAlign::Keyword(keyword) => match keyword {
                    VerticalAlignKeyword::Baseline => VerticalAlignValue::Baseline,
                    VerticalAlignKeyword::Sub => VerticalAlignValue::Sub,
                    VerticalAlignKeyword::Super => VerticalAlignValue::Super,
                    VerticalAlignKeyword::Top => VerticalAlignValue::Top,
                    VerticalAlignKeyword::TextTop => VerticalAlignValue::TextTop,
                    VerticalAlignKeyword::Middle => VerticalAlignValue::Middle,
                    VerticalAlignKeyword::Bottom => VerticalAlignValue::Bottom,
                    VerticalAlignKeyword::TextBottom => VerticalAlignValue::TextBottom,
                },
                VerticalAlign::Length(lp) => match lp {
                    LengthPercentage::Dimension(len) => VerticalAlignValue::Length(length_to_px(len, style.font.font_size, doc.root_font_size())),
                    LengthPercentage::Percentage(p) => VerticalAlignValue::Percent(p.0),
                    LengthPercentage::Calc(_) => VerticalAlignValue::Baseline,
                },
            };
        }
        Property::WhiteSpace(ws) => {
            use lightningcss::properties::text::WhiteSpace as LcWhiteSpace;
            style.text.white_space = match ws {
                LcWhiteSpace::Normal => WhiteSpace::Normal,
                LcWhiteSpace::Pre => WhiteSpace::Pre,
                LcWhiteSpace::NoWrap => WhiteSpace::NoWrap,
                LcWhiteSpace::PreWrap => WhiteSpace::PreWrap,
                LcWhiteSpace::PreLine => WhiteSpace::PreLine,
                LcWhiteSpace::BreakSpaces => WhiteSpace::BreakSpaces,
            };
        }

        // Color
        Property::Color(c) => {
            style.text.color = css_color_to_u32(c, style.text.color);
        }
        Property::BackgroundColor(c) => {
            style.background.background_color = css_color_to_u32(c, style.text.color);
        }
        Property::Background(bg) => {
            // The color may only appear in the shorthand's last layer; earlier
            // layers carry the transparent default.
            if let Some(layer) = bg.last() {
                style.background.background_color = css_color_to_u32(&layer.color, style.text.color);
            }
        }

        // Size
        Property::Width(size) => {
            style.box_model.width = size_to_preferred(size, style.font.font_size, doc.root_font_size());
        }
        Property::Height(size) => {
            style.box_model.height = size_to_preferred(size, style.font.font_size, doc.root_font_size());
        }
        Property::BorderSpacing(spacing) => {
            style.box_model.border_spacing_horizontal = length_to_px_from_length(&spacing.0, style.font.font_size, doc.root_font_size());
            style.box_model.border_spacing_vertical = length_to_px_from_length(&spacing.1, style.font.font_size, doc.root_font_size());
        }
        Property::MinWidth(size) => {
            style.box_model.min_width = size_to_preferred(size, style.font.font_size, doc.root_font_size());
        }
        Property::MinHeight(size) => {
            style.box_model.min_height = size_to_preferred(size, style.font.font_size, doc.root_font_size());
        }
        Property::MaxWidth(max_size) => {
            style.box_model.max_width = max_size_to_preferred(max_size, style.font.font_size, doc.root_font_size());
        }
        Property::MaxHeight(max_size) => {
            style.box_model.max_height = max_size_to_preferred(max_size, style.font.font_size, doc.root_font_size());
        }

        // Margin
        Property::MarginTop(m) => {
            style.box_model.margin_top = length_or_auto_to_lengthpct(m, style.font.font_size, doc.root_font_size());
        }
        Property::MarginBottom(m) => {
            style.box_model.margin_bottom = length_or_auto_to_lengthpct(m, style.font.font_size, doc.root_font_size());
        }
        Property::MarginLeft(m) => {
            style.box_model.margin_left = length_or_auto_to_lengthpct(m, style.font.font_size, doc.root_font_size());
        }
        Property::MarginRight(m) => {
            style.box_model.margin_right = length_or_auto_to_lengthpct(m, style.font.font_size, doc.root_font_size());
        }
        Property::Margin(m) => {
            style.box_model.margin_top = length_or_auto_to_lengthpct(&m.top, style.font.font_size, doc.root_font_size());
            style.box_model.margin_bottom = length_or_auto_to_lengthpct(&m.bottom, style.font.font_size, doc.root_font_size());
            style.box_model.margin_left = length_or_auto_to_lengthpct(&m.left, style.font.font_size, doc.root_font_size());
            style.box_model.margin_right = length_or_auto_to_lengthpct(&m.right, style.font.font_size, doc.root_font_size());
        }
        Property::MarginBlock(m) => {
            style.box_model.margin_top = length_or_auto_to_lengthpct(&m.block_start, style.font.font_size, doc.root_font_size());
            style.box_model.margin_bottom = length_or_auto_to_lengthpct(&m.block_end, style.font.font_size, doc.root_font_size());
        }
        Property::MarginBlockStart(m) => {
            style.box_model.margin_top = length_or_auto_to_lengthpct(m, style.font.font_size, doc.root_font_size());
        }
        Property::MarginBlockEnd(m) => {
            style.box_model.margin_bottom = length_or_auto_to_lengthpct(m, style.font.font_size, doc.root_font_size());
        }
        Property::MarginInline(m) => {
            style.box_model.margin_left = length_or_auto_to_lengthpct(&m.inline_start, style.font.font_size, doc.root_font_size());
            style.box_model.margin_right = length_or_auto_to_lengthpct(&m.inline_end, style.font.font_size, doc.root_font_size());
        }
        Property::MarginInlineStart(m) => {
            style.box_model.margin_left = length_or_auto_to_lengthpct(m, style.font.font_size, doc.root_font_size());
        }
        Property::MarginInlineEnd(m) => {
            style.box_model.margin_right = length_or_auto_to_lengthpct(m, style.font.font_size, doc.root_font_size());
        }

        // Padding - uses LengthPercentageOrAuto in lightningcss
        Property::PaddingTop(p) => {
            style.box_model.padding_top = length_or_auto_to_lengthpct(p, style.font.font_size, doc.root_font_size());
        }
        Property::PaddingBottom(p) => {
            style.box_model.padding_bottom = length_or_auto_to_lengthpct(p, style.font.font_size, doc.root_font_size());
        }
        Property::PaddingLeft(p) => {
            style.box_model.padding_left = length_or_auto_to_lengthpct(p, style.font.font_size, doc.root_font_size());
        }
        Property::PaddingRight(p) => {
            style.box_model.padding_right = length_or_auto_to_lengthpct(p, style.font.font_size, doc.root_font_size());
        }
        Property::Padding(p) => {
            style.box_model.padding_top = length_or_auto_to_lengthpct(&p.top, style.font.font_size, doc.root_font_size());
            style.box_model.padding_bottom = length_or_auto_to_lengthpct(&p.bottom, style.font.font_size, doc.root_font_size());
            style.box_model.padding_left = length_or_auto_to_lengthpct(&p.left, style.font.font_size, doc.root_font_size());
            style.box_model.padding_right = length_or_auto_to_lengthpct(&p.right, style.font.font_size, doc.root_font_size());
        }
        // Logical padding. Assumes a horizontal LTR writing mode (inline-start =
        // left), which holds for the EPUB content this renderer targets.
        Property::PaddingBlock(p) => {
            style.box_model.padding_top = length_or_auto_to_lengthpct(&p.block_start, style.font.font_size, doc.root_font_size());
            style.box_model.padding_bottom = length_or_auto_to_lengthpct(&p.block_end, style.font.font_size, doc.root_font_size());
        }
        Property::PaddingBlockStart(p) => {
            style.box_model.padding_top = length_or_auto_to_lengthpct(p, style.font.font_size, doc.root_font_size());
        }
        Property::PaddingBlockEnd(p) => {
            style.box_model.padding_bottom = length_or_auto_to_lengthpct(p, style.font.font_size, doc.root_font_size());
        }
        Property::PaddingInline(p) => {
            style.box_model.padding_left = length_or_auto_to_lengthpct(&p.inline_start, style.font.font_size, doc.root_font_size());
            style.box_model.padding_right = length_or_auto_to_lengthpct(&p.inline_end, style.font.font_size, doc.root_font_size());
        }
        Property::PaddingInlineStart(p) => {
            style.box_model.padding_left = length_or_auto_to_lengthpct(p, style.font.font_size, doc.root_font_size());
        }
        Property::PaddingInlineEnd(p) => {
            style.box_model.padding_right = length_or_auto_to_lengthpct(p, style.font.font_size, doc.root_font_size());
        }

        // Border (per-side)
        Property::BorderWidth(bw) => {
            style.border.border_top_width = border_width_to_px(&bw.top, style.font.font_size, doc.root_font_size());
            style.border.border_right_width = border_width_to_px(&bw.right, style.font.font_size, doc.root_font_size());
            style.border.border_bottom_width = border_width_to_px(&bw.bottom, style.font.font_size, doc.root_font_size());
            style.border.border_left_width = border_width_to_px(&bw.left, style.font.font_size, doc.root_font_size());
        }
        Property::BorderTopWidth(w) => {
            style.border.border_top_width = border_width_to_px(w, style.font.font_size, doc.root_font_size());
        }
        Property::BorderRightWidth(w) => {
            style.border.border_right_width = border_width_to_px(w, style.font.font_size, doc.root_font_size());
        }
        Property::BorderBottomWidth(w) => {
            style.border.border_bottom_width = border_width_to_px(w, style.font.font_size, doc.root_font_size());
        }
        Property::BorderLeftWidth(w) => {
            style.border.border_left_width = border_width_to_px(w, style.font.font_size, doc.root_font_size());
        }
        Property::BorderColor(bc) => {
            style.border.border_top_color = css_color_to_u32(&bc.top, style.text.color);
            style.border.border_right_color = css_color_to_u32(&bc.right, style.text.color);
            style.border.border_bottom_color = css_color_to_u32(&bc.bottom, style.text.color);
            style.border.border_left_color = css_color_to_u32(&bc.left, style.text.color);
            style.border_color_explicit = [true; 4];
        }
        Property::BorderTopColor(c) => {
            style.border.border_top_color = css_color_to_u32(c, style.text.color);
            style.border_color_explicit[0] = true;
        }
        Property::BorderRightColor(c) => {
            style.border.border_right_color = css_color_to_u32(c, style.text.color);
            style.border_color_explicit[1] = true;
        }
        Property::BorderBottomColor(c) => {
            style.border.border_bottom_color = css_color_to_u32(c, style.text.color);
            style.border_color_explicit[2] = true;
        }
        Property::BorderLeftColor(c) => {
            style.border.border_left_color = css_color_to_u32(c, style.text.color);
            style.border_color_explicit[3] = true;
        }
        Property::BorderStyle(bs) => {
            style.border.border_top_style = line_style_to_border_style(&bs.top);
            style.border.border_right_style = line_style_to_border_style(&bs.right);
            style.border.border_bottom_style = line_style_to_border_style(&bs.bottom);
            style.border.border_left_style = line_style_to_border_style(&bs.left);
        }
        Property::BorderTopStyle(s) => {
            style.border.border_top_style = line_style_to_border_style(s);
        }
        Property::BorderRightStyle(s) => {
            style.border.border_right_style = line_style_to_border_style(s);
        }
        Property::BorderBottomStyle(s) => {
            style.border.border_bottom_style = line_style_to_border_style(s);
        }
        Property::BorderLeftStyle(s) => {
            style.border.border_left_style = line_style_to_border_style(s);
        }
        Property::Border(b) => {
            let width = border_width_to_px(&b.width, style.font.font_size, doc.root_font_size());
            let color = css_color_to_u32(&b.color, style.text.color);
            let border_style = line_style_to_border_style(&b.style);
            style.border.border_top_width = width;
            style.border.border_right_width = width;
            style.border.border_bottom_width = width;
            style.border.border_left_width = width;
            style.border.border_top_color = color;
            style.border.border_right_color = color;
            style.border.border_bottom_color = color;
            style.border.border_left_color = color;
            style.border.border_top_style = border_style;
            style.border.border_right_style = border_style;
            style.border.border_bottom_style = border_style;
            style.border.border_left_style = border_style;
            style.border_color_explicit = [true; 4];
        }
        Property::BorderTop(b) => {
            style.border.border_top_width = border_width_to_px(&b.width, style.font.font_size, doc.root_font_size());
            style.border.border_top_color = css_color_to_u32(&b.color, style.text.color);
            style.border.border_top_style = line_style_to_border_style(&b.style);
            style.border_color_explicit[0] = true;
        }
        Property::BorderRight(b) => {
            style.border.border_right_width = border_width_to_px(&b.width, style.font.font_size, doc.root_font_size());
            style.border.border_right_color = css_color_to_u32(&b.color, style.text.color);
            style.border.border_right_style = line_style_to_border_style(&b.style);
            style.border_color_explicit[1] = true;
        }
        Property::BorderBottom(b) => {
            style.border.border_bottom_width = border_width_to_px(&b.width, style.font.font_size, doc.root_font_size());
            style.border.border_bottom_color = css_color_to_u32(&b.color, style.text.color);
            style.border.border_bottom_style = line_style_to_border_style(&b.style);
            style.border_color_explicit[2] = true;
        }
        Property::BorderLeft(b) => {
            style.border.border_left_width = border_width_to_px(&b.width, style.font.font_size, doc.root_font_size());
            style.border.border_left_color = css_color_to_u32(&b.color, style.text.color);
            style.border.border_left_style = line_style_to_border_style(&b.style);
            style.border_color_explicit[3] = true;
        }

        // Display
        Property::Display(d) => {
            use lightningcss::properties::display::{Display as LcDisplay, DisplayInside, DisplayKeyword, DisplayOutside};
            style.box_model.display = match d {
                LcDisplay::Keyword(kw) => match kw {
                    DisplayKeyword::None => Display::None,
                    DisplayKeyword::TableRowGroup => Display::TableRowGroup,
                    DisplayKeyword::TableHeaderGroup => Display::TableHeaderGroup,
                    DisplayKeyword::TableFooterGroup => Display::TableFooterGroup,
                    DisplayKeyword::TableRow => Display::TableRow,
                    DisplayKeyword::TableColumnGroup => Display::TableColumnGroup,
                    DisplayKeyword::TableColumn => Display::TableColumn,
                    DisplayKeyword::TableCell => Display::TableCell,
                    DisplayKeyword::TableCaption => Display::TableCaption,
                    _ => Display::Block,
                },
                LcDisplay::Pair(pair) if pair.is_list_item => Display::ListItem,
                LcDisplay::Pair(pair) => match (&pair.outside, &pair.inside) {
                    (DisplayOutside::Block, DisplayInside::Table) => Display::Table,
                    (DisplayOutside::Inline, DisplayInside::Table) => Display::InlineTable,
                    (DisplayOutside::Block, _) => Display::Block,
                    (DisplayOutside::Inline, _) => Display::Inline,
                    (DisplayOutside::RunIn, _) => Display::Block,
                },
            };
        }
        Property::Custom(custom) => match custom.name.as_ref().to_ascii_lowercase().as_str() {
            "float" => {
                if let Some(value) = single_ident_keyword(&custom.value).map(str::to_ascii_lowercase) {
                    style.box_model.float = match value.as_str() {
                        "left" | "inline-start" => Float::Left,
                        "right" | "inline-end" => Float::Right,
                        "none" => Float::None,
                        _ => style.box_model.float,
                    };
                }
            }
            "clear" => {
                if let Some(value) = single_ident_keyword(&custom.value).map(str::to_ascii_lowercase) {
                    style.box_model.clear = match value.as_str() {
                        "left" | "inline-start" => Clear::Left,
                        "right" | "inline-end" => Clear::Right,
                        "both" => Clear::Both,
                        "none" => Clear::None,
                        _ => style.box_model.clear,
                    };
                }
            }
            _ => {}
        },
        Property::Hyphens(_, _) => {
            // Not supported yet.
        }
        Property::WordBreak(wb) => {
            use lightningcss::properties::text::WordBreak as LcWordBreak;
            style.text.word_break = match wb {
                LcWordBreak::Normal => WordBreak::Normal,
                LcWordBreak::BreakAll => WordBreak::BreakAll,
                LcWordBreak::KeepAll => WordBreak::KeepAll,
                // Legacy value: behaves as `overflow-wrap: break-word`.
                LcWordBreak::BreakWord => {
                    if style.text.overflow_wrap == OverflowWrap::Normal {
                        style.text.overflow_wrap = OverflowWrap::BreakWord;
                    }
                    WordBreak::Normal
                }
            };
        }
        Property::OverflowWrap(ow) | Property::WordWrap(ow) => {
            use lightningcss::properties::text::OverflowWrap as LcOverflowWrap;
            style.text.overflow_wrap = match ow {
                LcOverflowWrap::Normal => OverflowWrap::Normal,
                LcOverflowWrap::BreakWord => OverflowWrap::BreakWord,
                LcOverflowWrap::Anywhere => OverflowWrap::Anywhere,
            };
        }
        Property::BoxSizing(bs, _) => {
            use lightningcss::properties::size::BoxSizing as LcBoxSizing;
            style.box_model.box_sizing = match bs {
                LcBoxSizing::ContentBox => BoxSizing::ContentBox,
                LcBoxSizing::BorderBox => BoxSizing::BorderBox,
            };
        }

        _ => {
            if let Property::Unparsed(unparsed) = property {
                match unparsed.property_id.name() {
                    "page-break-before" | "page-break-after" | "page-break-inside" | "break-before" | "break-after" | "break-inside" => {
                        // Not supported yet.
                        return;
                    }
                    "border-collapse" => {
                        if let Some(value) = first_ident_token(&unparsed.value) {
                            style.box_model.border_collapse = match value.as_str() {
                                "collapse" => BorderCollapseMode::Collapse,
                                "separate" => BorderCollapseMode::Separate,
                                _ => style.box_model.border_collapse,
                            };
                            return;
                        }
                    }
                    "empty-cells" => {
                        if let Some(value) = first_ident_token(&unparsed.value) {
                            style.box_model.empty_cells = match value.as_str() {
                                "hide" => EmptyCellsMode::Hide,
                                "show" => EmptyCellsMode::Show,
                                _ => style.box_model.empty_cells,
                            };
                            return;
                        }
                    }
                    "caption-side" => {
                        if let Some(value) = first_ident_token(&unparsed.value) {
                            style.box_model.caption_side = match value.as_str() {
                                "bottom" => CaptionSide::Bottom,
                                "top" => CaptionSide::Top,
                                _ => style.box_model.caption_side,
                            };
                            return;
                        }
                    }
                    "float" => {
                        if let Some(value) = single_ident_keyword(&unparsed.value).map(str::to_ascii_lowercase) {
                            style.box_model.float = match value.as_str() {
                                "left" | "inline-start" => Float::Left,
                                "right" | "inline-end" => Float::Right,
                                "none" => Float::None,
                                _ => style.box_model.float,
                            };
                            return;
                        }
                    }
                    "clear" => {
                        if let Some(value) = single_ident_keyword(&unparsed.value).map(str::to_ascii_lowercase) {
                            style.box_model.clear = match value.as_str() {
                                "left" | "inline-start" => Clear::Left,
                                "right" | "inline-end" => Clear::Right,
                                "both" => Clear::Both,
                                "none" => Clear::None,
                                _ => style.box_model.clear,
                            };
                            return;
                        }
                    }
                    _ => {}
                }
            }
            let prop_name = property.property_id().name().to_string();
            if matches!(prop_name.as_str(), "float" | "clear" | "css-float" | "css-clear") {
                if let Ok(css) = property.to_css_string(false, PrinterOptions::default()) {
                    if let Some(value) = css.split(':').nth(1).map(|v| v.trim().to_ascii_lowercase()) {
                        let value = value.as_str();
                        if prop_name == "float" {
                            style.box_model.float = match value {
                                "left" | "inline-start" => Float::Left,
                                "right" | "inline-end" => Float::Right,
                                "none" => Float::None,
                                _ => style.box_model.float,
                            };
                            return;
                        }
                        style.box_model.clear = match value {
                            "left" | "inline-start" => Clear::Left,
                            "right" | "inline-end" => Clear::Right,
                            "both" => Clear::Both,
                            "none" => Clear::None,
                            _ => style.box_model.clear,
                        };
                        return;
                    }
                }
            }
            log_unhandled_property(property);
        }
    }
}

/// Whether a property inherits by default (decides what `unset` means).
fn is_inherited_property(name: &str) -> bool {
    matches!(
        name,
        "color"
            | "font"
            | "font-size"
            | "font-weight"
            | "font-style"
            | "font-family"
            | "font-variant-caps"
            | "line-height"
            | "letter-spacing"
            | "word-spacing"
            | "tab-size"
            | "text-align"
            | "text-align-last"
            | "text-indent"
            | "text-transform"
            | "white-space"
            | "word-break"
            | "overflow-wrap"
            | "word-wrap"
            | "list-style"
            | "list-style-type"
            | "list-style-position"
            | "list-style-image"
            | "border-collapse"
            | "border-spacing"
            | "caption-side"
            | "empty-cells"
            | "hyphens"
    )
}

/// Handle the CSS-wide keywords `inherit`, `initial`, `unset`, and `revert`.
/// lightningcss leaves them as `Unparsed` (typed properties only hold concrete
/// values), so they would otherwise be silently dropped. Returns true when the
/// declaration was consumed.
fn try_apply_css_wide_keyword(style: &mut WorkingStyle, property: &Property, parent: &ParentStyle, root_font_size: f32) -> bool {
    let Property::Unparsed(unparsed) = property else {
        return false;
    };
    let Some(keyword) = single_ident_keyword(&unparsed.value).map(str::to_ascii_lowercase) else {
        return false;
    };

    let name = unparsed.property_id.name();
    let initial;
    let src: &ParentStyle = match keyword.as_str() {
        "inherit" => parent,
        "initial" => {
            initial = ParentStyle::initial(root_font_size);
            &initial
        }
        "unset" => {
            if is_inherited_property(name) {
                parent
            } else {
                initial = ParentStyle::initial(root_font_size);
                &initial
            }
        }
        // `revert` rolls back to the previous cascade origin. UA rules apply
        // earlier in the same cascade here, so keeping the value accumulated
        // so far is the closest approximation.
        "revert" | "revert-layer" => return true,
        _ => return false,
    };

    match name {
        "color" => style.text.color = src.text.color,
        "font" => {
            style.font = src.font.clone();
            style.text.line_height = src.text.line_height;
            style.text.line_height_number = src.text.line_height_number;
            style.line_height_spec = None;
        }
        "font-size" => style.font.font_size = src.font.font_size,
        "font-weight" => style.font.font_weight = src.font.font_weight,
        "font-style" => style.font.font_style = src.font.font_style,
        "font-family" => style.font.font_family = src.font.font_family,
        "font-variant-caps" => style.font.font_variant_small_caps = src.font.font_variant_small_caps,
        "line-height" => {
            style.text.line_height = src.text.line_height;
            style.text.line_height_number = src.text.line_height_number;
            style.line_height_spec = None;
        }
        "letter-spacing" => style.text.letter_spacing = src.text.letter_spacing,
        "word-spacing" => style.text.word_spacing = src.text.word_spacing,
        "tab-size" => style.text.tab_size = src.text.tab_size,
        "text-align" => {
            style.text.text_align = src.text.text_align;
            if !style.text.text_align_last_explicit {
                style.text.text_align_last = style.text.text_align;
            }
        }
        "text-align-last" => {
            style.text.text_align_last = src.text.text_align_last;
            style.text.text_align_last_explicit = src.text.text_align_last_explicit;
        }
        "text-indent" => {
            style.text.text_indent = src.text.text_indent;
            style.text.text_indent_hanging = src.text.text_indent_hanging;
            style.text.text_indent_each_line = src.text.text_indent_each_line;
        }
        "text-transform" => style.text.text_transform = src.text.text_transform,
        "white-space" => style.text.white_space = src.text.white_space,
        "word-break" => style.text.word_break = src.text.word_break,
        "overflow-wrap" | "word-wrap" => style.text.overflow_wrap = src.text.overflow_wrap,
        "box-sizing" => style.box_model.box_sizing = src.box_model.box_sizing,
        "list-style" => {
            style.text.list_style_type = src.text.list_style_type;
            style.text.list_style_position = src.text.list_style_position;
            style.text.list_style_image = src.text.list_style_image;
        }
        "list-style-type" => style.text.list_style_type = src.text.list_style_type,
        "list-style-position" => style.text.list_style_position = src.text.list_style_position,
        "list-style-image" => style.text.list_style_image = src.text.list_style_image,
        "text-decoration" | "text-decoration-line" => style.background.text_decoration = src.background.text_decoration,
        "background" | "background-color" => style.background.background_color = src.background.background_color,
        "vertical-align" => style.box_model.vertical_align = src.box_model.vertical_align,
        "display" => style.box_model.display = src.box_model.display,
        "float" => style.box_model.float = src.box_model.float,
        "clear" => style.box_model.clear = src.box_model.clear,
        "border-collapse" => style.box_model.border_collapse = src.box_model.border_collapse,
        "border-spacing" => {
            style.box_model.border_spacing_horizontal = src.box_model.border_spacing_horizontal;
            style.box_model.border_spacing_vertical = src.box_model.border_spacing_vertical;
        }
        "caption-side" => style.box_model.caption_side = src.box_model.caption_side,
        "empty-cells" => style.box_model.empty_cells = src.box_model.empty_cells,
        "width" => style.box_model.width = src.box_model.width,
        "height" => style.box_model.height = src.box_model.height,
        "min-width" => style.box_model.min_width = src.box_model.min_width,
        "min-height" => style.box_model.min_height = src.box_model.min_height,
        "max-width" => style.box_model.max_width = src.box_model.max_width,
        "max-height" => style.box_model.max_height = src.box_model.max_height,
        "margin-top" => style.box_model.margin_top = src.box_model.margin_top,
        "margin-bottom" => style.box_model.margin_bottom = src.box_model.margin_bottom,
        "margin-left" => style.box_model.margin_left = src.box_model.margin_left,
        "margin-right" => style.box_model.margin_right = src.box_model.margin_right,
        "margin" => {
            style.box_model.margin_top = src.box_model.margin_top;
            style.box_model.margin_bottom = src.box_model.margin_bottom;
            style.box_model.margin_left = src.box_model.margin_left;
            style.box_model.margin_right = src.box_model.margin_right;
        }
        "padding-top" => style.box_model.padding_top = src.box_model.padding_top,
        "padding-bottom" => style.box_model.padding_bottom = src.box_model.padding_bottom,
        "padding-left" => style.box_model.padding_left = src.box_model.padding_left,
        "padding-right" => style.box_model.padding_right = src.box_model.padding_right,
        "padding" => {
            style.box_model.padding_top = src.box_model.padding_top;
            style.box_model.padding_bottom = src.box_model.padding_bottom;
            style.box_model.padding_left = src.box_model.padding_left;
            style.box_model.padding_right = src.box_model.padding_right;
        }
        "border"
        | "border-width"
        | "border-style"
        | "border-color"
        | "border-top"
        | "border-right"
        | "border-bottom"
        | "border-left"
        | "border-top-width"
        | "border-right-width"
        | "border-bottom-width"
        | "border-left-width"
        | "border-top-style"
        | "border-right-style"
        | "border-bottom-style"
        | "border-left-style"
        | "border-top-color"
        | "border-right-color"
        | "border-bottom-color"
        | "border-left-color" => {
            apply_border_keyword(style, name, src);
        }
        _ => return false,
    }
    true
}

fn apply_border_keyword(style: &mut WorkingStyle, name: &str, src: &ParentStyle) {
    let side_shorthand = matches!(name, "border" | "border-top" | "border-right" | "border-bottom" | "border-left");
    let widths = side_shorthand || name == "border-width" || name.ends_with("-width");
    let styles = side_shorthand || name == "border-style" || name.ends_with("-style");
    let colors = side_shorthand || name == "border-color" || name.ends_with("-color");
    let sides: &[usize] = match name {
        n if n.starts_with("border-top") => &[0],
        n if n.starts_with("border-right") => &[1],
        n if n.starts_with("border-bottom") => &[2],
        n if n.starts_with("border-left") => &[3],
        _ => &[0, 1, 2, 3],
    };
    for &side in sides {
        if widths {
            let value = [src.border.border_top_width, src.border.border_right_width, src.border.border_bottom_width, src.border.border_left_width][side];
            *[&mut style.border.border_top_width, &mut style.border.border_right_width, &mut style.border.border_bottom_width, &mut style.border.border_left_width][side] = value;
        }
        if styles {
            let value = [src.border.border_top_style, src.border.border_right_style, src.border.border_bottom_style, src.border.border_left_style][side];
            *[&mut style.border.border_top_style, &mut style.border.border_right_style, &mut style.border.border_bottom_style, &mut style.border.border_left_style][side] = value;
        }
        if colors {
            let value = [src.border.border_top_color, src.border.border_right_color, src.border.border_bottom_color, src.border.border_left_color][side];
            *[&mut style.border.border_top_color, &mut style.border.border_right_color, &mut style.border.border_bottom_color, &mut style.border.border_left_color][side] = value;
            style.border_color_explicit[side] = true;
        }
    }
}

fn parse_line_height(lh: &lightningcss::properties::font::LineHeight, font_size: f32, root_font_size: f32) -> f32 {
    use lightningcss::properties::font::LineHeight;
    match lh {
        LineHeight::Normal => 0.0,
        LineHeight::Number(n) => font_size * n,
        LineHeight::Length(lp) => length_percentage_to_px(lp, font_size, root_font_size),
    }
}

/// The unitless multiplier of a `line-height` value (0.0 for lengths,
/// percentages, and `normal`, which inherit as resolved px / the sentinel).
fn line_height_number(lh: &lightningcss::properties::font::LineHeight) -> f32 {
    use lightningcss::properties::font::LineHeight;
    match lh {
        LineHeight::Number(n) => *n,
        _ => 0.0,
    }
}

fn apply_text_decoration(decoration: &mut u8, line: &lightningcss::properties::text::TextDecorationLine) {
    *decoration = 0;
    if line.contains(lightningcss::properties::text::TextDecorationLine::Underline) {
        *decoration |= 1;
    }
    if line.contains(lightningcss::properties::text::TextDecorationLine::Overline) {
        *decoration |= 2;
    }
    if line.contains(lightningcss::properties::text::TextDecorationLine::LineThrough) {
        *decoration |= 4;
    }
}

// ============================================================================
// Conversion helpers
// ============================================================================
fn collect_custom_properties<'a>(custom_properties: &mut FxHashMap<String, TokenList<'a>>, parent_custom: &FxHashMap<String, TokenList<'a>>, declarations: &[Property<'a>]) {
    for property in declarations {
        let (name, value) = match property {
            Property::Custom(custom) => {
                let raw_name = custom.name.as_ref();
                let name = if raw_name.starts_with("--") { raw_name.to_string() } else { format!("--{}", raw_name) };
                (name, &custom.value)
            }
            Property::Unparsed(unparsed) if unparsed.property_id.name().starts_with("--") => (unparsed.property_id.name().to_string(), &unparsed.value),
            _ => continue,
        };
        if let Some(keyword) = single_ident_keyword(value)
            && apply_custom_keyword(custom_properties, parent_custom, &name, keyword)
        {
            continue;
        }
        custom_properties.insert(name, value.clone());
    }
}

fn single_ident_keyword<'a>(tokens: &'a TokenList<'a>) -> Option<&'a str> {
    let mut iter = tokens.0.iter().filter(|token| !is_ignorable_token(token));
    let first = iter.next()?;
    if iter.next().is_some() {
        return None;
    }
    match first {
        TokenOrValue::Token(Token::Ident(ident)) => Some(ident.as_ref()),
        _ => None,
    }
}

fn is_ignorable_token(token: &TokenOrValue) -> bool {
    matches!(token, TokenOrValue::Token(Token::WhiteSpace(_)) | TokenOrValue::Token(Token::Comment(_)))
}

fn apply_custom_keyword<'a>(custom_properties: &mut FxHashMap<String, TokenList<'a>>, parent_custom: &FxHashMap<String, TokenList<'a>>, name: &str, keyword: &str) -> bool {
    let keyword = keyword.to_ascii_lowercase();
    match keyword.as_str() {
        "inherit" | "unset" => {
            if let Some(value) = parent_custom.get(name) {
                custom_properties.insert(name.to_string(), value.clone());
            } else {
                custom_properties.remove(name);
            }
        }
        "initial" | "revert" | "revert-layer" => {
            custom_properties.remove(name);
        }
        _ => return false,
    }
    true
}

fn resolve_single_var_property<'a>(unparsed: &lightningcss::properties::custom::UnparsedProperty<'a>, var_map: &HashMap<&str, TokenList<'a>>) -> Option<Property<'a>> {
    let [TokenOrValue::Var(var)] = unparsed.value.0.as_slice() else {
        return None;
    };
    let replacement = var_map.get(var.name.ident.as_ref())?;
    let css = token_list_to_css_string(replacement)?;
    let leaked: &'a str = Box::leak(css.into_boxed_str());
    Property::parse_string(unparsed.property_id.clone(), leaked, ParserOptions::default()).ok()
}

fn token_list_to_css_string(tokens: &TokenList<'_>) -> Option<String> {
    let mut out = String::new();
    write_token_list(tokens, &mut out)?;
    Some(out)
}

fn write_token_list(tokens: &TokenList<'_>, out: &mut String) -> Option<()> {
    for token in &tokens.0 {
        match token {
            TokenOrValue::Color(color) => {
                let mut printer = Printer::new(out, PrinterOptions::default());
                color.to_css(&mut printer).ok()?;
            }
            TokenOrValue::Function(function) => {
                out.push_str(function.name.as_ref());
                out.push('(');
                write_token_list(&function.arguments, out)?;
                out.push(')');
            }
            TokenOrValue::Length(length) => {
                let mut printer = Printer::new(out, PrinterOptions::default());
                length.to_css(&mut printer).ok()?;
            }
            TokenOrValue::DashedIdent(ident) => {
                let mut printer = Printer::new(out, PrinterOptions::default());
                ident.to_css(&mut printer).ok()?;
            }
            TokenOrValue::AnimationName(name) => {
                let mut printer = Printer::new(out, PrinterOptions::default());
                name.to_css(&mut printer).ok()?;
            }
            TokenOrValue::Token(token) => {
                let mut printer = Printer::new(out, PrinterOptions::default());
                token.to_css(&mut printer).ok()?;
            }
            _ => return None,
        }
    }
    Some(())
}

fn canonical_text_spacing_tokens<'a>(spacing: TextSpacing) -> Option<TokenList<'a>> {
    let absolute_px = spacing.absolute_px();
    let percentage = spacing.font_size_fraction() * 100.0;
    let css = if percentage == 0.0 {
        format!("{absolute_px}px")
    } else if absolute_px == 0.0 {
        format!("{percentage}%")
    } else if percentage.is_sign_negative() {
        format!("calc({absolute_px}px - {}%)", percentage.abs())
    } else {
        format!("calc({absolute_px}px + {percentage}%)")
    };
    let leaked: &'a str = Box::leak(css.into_boxed_str());
    TokenList::parse_string_with_options(leaked, ParserOptions::default()).ok()
}

fn canonical_tab_size_tokens<'a>(tab_size: TabSize) -> Option<TokenList<'a>> {
    let css = match tab_size.kind() {
        html_style_model::TabSizeKind::Spaces => tab_size.value().to_string(),
        html_style_model::TabSizeKind::LengthPx => format!("{}px", tab_size.value()),
    };
    let leaked: &'a str = Box::leak(css.into_boxed_str());
    TokenList::parse_string_with_options(leaked, ParserOptions::default()).ok()
}

/// First ident token, lowercased: CSS keywords are case-insensitive.
fn first_ident_token(tokens: &TokenList<'_>) -> Option<String> {
    for token in &tokens.0 {
        if let TokenOrValue::Token(Token::Ident(ident)) = token {
            return Some(ident.as_ref().to_ascii_lowercase());
        }
    }
    None
}

fn map_list_style_position(pos: &lightningcss::properties::list::ListStylePosition) -> html_style_model::ListStylePosition {
    use html_style_model::ListStylePosition as Out;
    use lightningcss::properties::list::ListStylePosition as Lc;
    match pos {
        Lc::Inside => Out::Inside,
        Lc::Outside => Out::Outside,
    }
}

/// Map lightningcss `list-style-type` onto the supported set. `@counter-style`
/// names, `symbols()`, custom strings, and predefined styles we do not format
/// fall back to `decimal`, per CSS counter-style fallback.
fn map_list_style_type(lst: &lightningcss::properties::list::ListStyleType) -> html_style_model::ListStyleType {
    use html_style_model::ListStyleType as Out;
    use lightningcss::properties::list::{CounterStyle, ListStyleType as Lc, PredefinedCounterStyle as P};
    match lst {
        Lc::None => Out::None,
        Lc::String(_) => Out::Disc,
        Lc::CounterStyle(CounterStyle::Predefined(p)) => match p {
            P::Disc => Out::Disc,
            P::Circle => Out::Circle,
            P::Square => Out::Square,
            P::Decimal => Out::Decimal,
            P::DecimalLeadingZero => Out::DecimalLeadingZero,
            P::LowerAlpha | P::LowerLatin => Out::LowerAlpha,
            P::UpperAlpha | P::UpperLatin => Out::UpperAlpha,
            P::LowerRoman => Out::LowerRoman,
            P::UpperRoman => Out::UpperRoman,
            _ => Out::Decimal,
        },
        Lc::CounterStyle(_) => Out::Decimal,
    }
}

/// Intern the URL of a `list-style-image`. Gradients / `image-set()` / `none`
/// yield `None`. The interned string index (a `u16`) is widened into the
/// style's `u32` slot; it is resolved to an image resource when the marker box
/// is built.
fn list_style_image_to_interned(styles: &mut ComputedStylesBuilder, image: &lightningcss::values::image::Image) -> Option<StyleStringId> {
    use lightningcss::values::image::Image;
    match image {
        Image::Url(url) => {
            let raw = url.url.as_ref().trim();
            if raw.is_empty() { None } else { Some(styles.intern_string(raw)) }
        }
        _ => None,
    }
}

fn font_size_to_px(size: &FontSize, parent_font_size: f32, root_font_size: f32) -> f32 {
    match size {
        FontSize::Length(lp) => length_percentage_to_px(lp, parent_font_size, root_font_size),
        FontSize::Absolute(abs) => {
            use lightningcss::properties::font::AbsoluteFontSize;
            match abs {
                AbsoluteFontSize::XXSmall => 9.0,
                AbsoluteFontSize::XSmall => 10.0,
                AbsoluteFontSize::Small => 13.0,
                AbsoluteFontSize::Medium => 16.0,
                AbsoluteFontSize::Large => 18.0,
                AbsoluteFontSize::XLarge => 24.0,
                AbsoluteFontSize::XXLarge => 32.0,
                AbsoluteFontSize::XXXLarge => 48.0,
            }
        }
        FontSize::Relative(rel) => {
            use lightningcss::properties::font::RelativeFontSize;
            match rel {
                RelativeFontSize::Smaller => parent_font_size * 0.833,
                RelativeFontSize::Larger => parent_font_size * 1.2,
            }
        }
    }
}

fn font_weight_to_u16(weight: &FontWeight) -> u16 {
    match weight {
        FontWeight::Absolute(abs) => match abs {
            AbsoluteFontWeight::Normal => 400,
            AbsoluteFontWeight::Bold => 700,
            AbsoluteFontWeight::Weight(w) => *w as u16,
        },
        FontWeight::Bolder => 700,
        FontWeight::Lighter => 300,
    }
}

fn length_to_px(length: &LengthValue, parent_font_size: f32, root_font_size: f32) -> f32 {
    match length {
        LengthValue::Px(px) => *px,
        LengthValue::Em(em) => em * parent_font_size,
        LengthValue::Rem(rem) => rem * root_font_size,
        LengthValue::Pt(pt) => pt * (96.0 / 72.0),
        LengthValue::In(inches) => inches * 96.0,
        LengthValue::Cm(cm) => cm * (96.0 / 2.54),
        LengthValue::Mm(mm) => mm * (96.0 / 25.4),
        LengthValue::Q(q) => q * (96.0 / 101.6),
        LengthValue::Pc(pc) => pc * 16.0,
        // No font metrics available here; use the spec's 0.5em fallback.
        LengthValue::Ex(ex) => ex * parent_font_size * 0.5,
        LengthValue::Ch(ch) => ch * parent_font_size * 0.5,
        _ => parent_font_size,
    }
}

fn length_percentage_to_px(lp: &LengthPercentage, parent_font_size: f32, root_font_size: f32) -> f32 {
    match lp {
        LengthPercentage::Dimension(len) => length_to_px(len, parent_font_size, root_font_size),
        LengthPercentage::Percentage(p) => parent_font_size * p.0,
        LengthPercentage::Calc(_) => parent_font_size,
    }
}

fn parsed_text_spacing_to_computed(parsed: &ParsedSpacing, font_size: f32, root_font_size: f32) -> Option<TextSpacing> {
    match parsed {
        ParsedSpacing::Normal => Some(TextSpacing::ZERO),
        ParsedSpacing::Value(value) => length_percentage_to_text_spacing(value, font_size, root_font_size),
    }
}

fn parsed_tab_size_to_computed(parsed: &ParsedTabSize, font_size: f32, root_font_size: f32) -> Option<TabSize> {
    match parsed {
        ParsedTabSize::Spaces(value) => TabSize::spaces(*value),
        ParsedTabSize::Length(value) => {
            if let LengthPercentage::Calc(calc) = value
                && let Some(number) = calc_tab_number(calc)
            {
                return TabSize::spaces(number.max(0.0));
            }
            let resolved = length_percentage_to_text_spacing(value, font_size, root_font_size)?;
            (resolved.font_size_fraction() == 0.0).then(|| TabSize::length_px(resolved.absolute_px().max(0.0))).flatten()
        }
    }
}

fn calc_tab_number(calc: &Calc<LengthPercentage>) -> Option<f32> {
    match calc {
        Calc::Value(value) => match value.as_ref() {
            LengthPercentage::Calc(nested) => calc_tab_number(nested),
            _ => None,
        },
        Calc::Number(value) => Some(*value),
        Calc::Sum(left, right) => Some(calc_tab_number(left)? + calc_tab_number(right)?),
        Calc::Product(factor, value) => Some(*factor * calc_tab_number(value)?),
        Calc::Function(function) => match function.as_ref() {
            MathFunction::Calc(value) => calc_tab_number(value),
            MathFunction::Min(values) => values.iter().map(calc_tab_number).reduce(|left, right| Some(left?.min(right?)))?,
            MathFunction::Max(values) => values.iter().map(calc_tab_number).reduce(|left, right| Some(left?.max(right?)))?,
            MathFunction::Clamp(min, value, max) => Some(calc_tab_number(value)?.clamp(calc_tab_number(min)?, calc_tab_number(max)?)),
            MathFunction::Abs(value) => Some(calc_tab_number(value)?.abs()),
            MathFunction::Sign(value) => Some(calc_tab_number(value)?.signum()),
            _ => None,
        },
    }
}

fn length_percentage_to_text_spacing(value: &LengthPercentage, font_size: f32, root_font_size: f32) -> Option<TextSpacing> {
    use lightningcss::values::percentage::DimensionPercentage;
    match value {
        DimensionPercentage::Dimension(length) => TextSpacing::from_px(length_to_px(length, font_size, root_font_size)),
        DimensionPercentage::Percentage(percentage) => TextSpacing::new(0.0, percentage.0),
        DimensionPercentage::Calc(calc) => {
            let (absolute_px, font_size_fraction) = calc_length_percentage_components(calc, font_size, root_font_size)?;
            TextSpacing::new(absolute_px, font_size_fraction)
        }
    }
}

fn calc_length_percentage_components(calc: &Calc<LengthPercentage>, font_size: f32, root_font_size: f32) -> Option<(f32, f32)> {
    match calc {
        Calc::Value(value) => {
            let spacing = length_percentage_to_text_spacing(value, font_size, root_font_size)?;
            Some((spacing.absolute_px(), spacing.font_size_fraction()))
        }
        Calc::Number(value) => (*value == 0.0).then_some((0.0, 0.0)),
        Calc::Sum(left, right) => {
            let left = calc_length_percentage_components(left, font_size, root_font_size)?;
            let right = calc_length_percentage_components(right, font_size, root_font_size)?;
            Some((left.0 + right.0, left.1 + right.1))
        }
        Calc::Product(factor, value) => {
            let value = calc_length_percentage_components(value, font_size, root_font_size)?;
            Some((*factor * value.0, *factor * value.1))
        }
        Calc::Function(function) => match function.as_ref() {
            MathFunction::Calc(value) => calc_length_percentage_components(value, font_size, root_font_size),
            // Non-linear math functions cannot retain an unresolved percentage
            // as a two-component computed value without keeping the parser AST.
            _ => None,
        },
    }
}

/// Convert a margin/padding value into a `LengthPct`. Lengths (including `em`/`rem`)
/// resolve to pixels now; percentages are kept as a fraction to be resolved against
/// the containing-block width at layout time. `auto` margins are treated as `0`
/// (auto-margin centering is not implemented), matching prior behavior.
fn length_or_auto_to_lengthpct(value: &LengthPercentageOrAuto, font_size: f32, root_font_size: f32) -> LengthPct {
    match value {
        LengthPercentageOrAuto::Auto => LengthPct::Px(0.0),
        LengthPercentageOrAuto::LengthPercentage(lp) => length_percentage_to_lengthpct(lp, font_size, root_font_size),
    }
}

/// Convert a `<length-percentage>` (e.g. `text-indent`) into a `LengthPct`,
/// deferring percentage resolution to layout (against the containing-block width).
fn length_percentage_to_lengthpct(lp: &LengthPercentage, font_size: f32, root_font_size: f32) -> LengthPct {
    match lp {
        LengthPercentage::Dimension(len) => LengthPct::Px(length_to_px(len, font_size, root_font_size)),
        LengthPercentage::Percentage(p) => LengthPct::Pct(p.0),
        LengthPercentage::Calc(_) => LengthPct::Px(0.0),
    }
}

fn size_to_preferred(size: &Size, font_size: f32, root_font_size: f32) -> PreferredSize {
    match size {
        Size::Auto => PreferredSize::Auto,
        Size::LengthPercentage(lp) => match lp {
            LengthPercentage::Dimension(len) => PreferredSize::Px(length_to_px(len, font_size, root_font_size)),
            LengthPercentage::Percentage(p) => PreferredSize::Percent(p.0),
            LengthPercentage::Calc(_) => PreferredSize::Auto,
        },
        _ => PreferredSize::Auto,
    }
}

fn max_size_to_preferred(size: &MaxSize, font_size: f32, root_font_size: f32) -> PreferredSize {
    match size {
        MaxSize::None => PreferredSize::Auto,
        MaxSize::LengthPercentage(lp) => match lp {
            LengthPercentage::Dimension(len) => PreferredSize::Px(length_to_px(len, font_size, root_font_size)),
            LengthPercentage::Percentage(p) => PreferredSize::Percent(p.0),
            LengthPercentage::Calc(_) => PreferredSize::Auto,
        },
        _ => PreferredSize::Auto,
    }
}

fn spacing_to_text_spacing(spacing: &Spacing, font_size: f32, root_font_size: f32) -> TextSpacing {
    match spacing {
        Spacing::Normal => TextSpacing::ZERO,
        Spacing::Length(len) => match len {
            Length::Value(v) => TextSpacing::from_px(length_to_px(v, font_size, root_font_size)).unwrap_or(TextSpacing::ZERO),
            Length::Calc(calc) => calc_length_to_px(calc, font_size, root_font_size).and_then(TextSpacing::from_px).unwrap_or(TextSpacing::ZERO),
        },
    }
}

fn calc_length_to_px(calc: &Calc<Length>, font_size: f32, root_font_size: f32) -> Option<f32> {
    match calc {
        Calc::Value(length) => match length.as_ref() {
            Length::Value(value) => Some(length_to_px(value, font_size, root_font_size)),
            Length::Calc(nested) => calc_length_to_px(nested, font_size, root_font_size),
        },
        Calc::Number(value) => (*value == 0.0).then_some(0.0),
        Calc::Sum(left, right) => Some(calc_length_to_px(left, font_size, root_font_size)? + calc_length_to_px(right, font_size, root_font_size)?),
        Calc::Product(factor, value) => Some(*factor * calc_length_to_px(value, font_size, root_font_size)?),
        Calc::Function(function) => match function.as_ref() {
            MathFunction::Calc(value) => calc_length_to_px(value, font_size, root_font_size),
            MathFunction::Min(values) => values.iter().map(|value| calc_length_to_px(value, font_size, root_font_size)).reduce(|left, right| Some(left?.min(right?)))?,
            MathFunction::Max(values) => values.iter().map(|value| calc_length_to_px(value, font_size, root_font_size)).reduce(|left, right| Some(left?.max(right?)))?,
            MathFunction::Clamp(min, value, max) => Some(calc_length_to_px(value, font_size, root_font_size)?.clamp(calc_length_to_px(min, font_size, root_font_size)?, calc_length_to_px(max, font_size, root_font_size)?)),
            MathFunction::Abs(value) => Some(calc_length_to_px(value, font_size, root_font_size)?.abs()),
            MathFunction::Hypot(values) => {
                let squared = values.iter().try_fold(0.0, |sum, value| {
                    let value = calc_length_to_px(value, font_size, root_font_size)?;
                    Some(sum + value * value)
                })?;
                Some(squared.sqrt())
            }
            _ => None,
        },
    }
}

fn length_to_px_from_length(length: &lightningcss::values::length::Length, font_size: f32, root_font_size: f32) -> f32 {
    match length {
        lightningcss::values::length::Length::Value(v) => length_to_px(v, font_size, root_font_size),
        lightningcss::values::length::Length::Calc(_) => 0.0,
    }
}

fn border_width_to_px(width: &BorderSideWidth, font_size: f32, root_font_size: f32) -> f32 {
    use lightningcss::values::length::Length;
    match width {
        BorderSideWidth::Thin => 1.0,
        BorderSideWidth::Medium => 3.0,
        BorderSideWidth::Thick => 5.0,
        BorderSideWidth::Length(len) => match len {
            Length::Value(v) => length_to_px(v, font_size, root_font_size),
            Length::Calc(_) => 3.0, // Default to medium for calc
        },
    }
}

fn line_style_to_border_style(style: &LineStyle) -> BorderStyle {
    match style {
        LineStyle::None | LineStyle::Hidden => BorderStyle::None,
        LineStyle::Solid => BorderStyle::Solid,
        LineStyle::Dashed => BorderStyle::Dashed,
        LineStyle::Dotted => BorderStyle::Dotted,
        // The 3D styles (and `double`) paint as a plain line rather than
        // disappearing; only `none`/`hidden` suppress the border.
        LineStyle::Double | LineStyle::Groove | LineStyle::Ridge | LineStyle::Inset | LineStyle::Outset => BorderStyle::Solid,
    }
}

/// `current_color` is the element's resolved `color`, substituted for
/// `currentColor` (e.g. the default border color). `color` itself is cascaded
/// in an earlier pass, so it is already final when reset properties resolve.
fn css_color_to_u32(color: &CssColor, current_color: u32) -> u32 {
    match color {
        CssColor::RGBA(rgba) => ((rgba.red as u32) << 24) | ((rgba.green as u32) << 16) | ((rgba.blue as u32) << 8) | (rgba.alpha as u32),
        CssColor::CurrentColor => current_color,
        CssColor::System(system) => system_color_to_u32(system),
        _ => 0x000000FF,
    }
}

/// Resolve CSS system colors to fixed light-theme values (the renderer has no
/// OS theme to consult). Unlisted ones are text colors and fall back to black.
fn system_color_to_u32(color: &SystemColor) -> u32 {
    use SystemColor as S;
    match color {
        S::Mark => 0xFFFF00FF,
        S::Canvas | S::Field | S::Window | S::InfoBackground | S::Menu | S::ButtonFace | S::Scrollbar | S::AppWorkspace => 0xFFFFFFFF,
        S::LinkText => 0x0000EEFF,
        S::VisitedText => 0x551A8BFF,
        S::ActiveText => 0xEE0000FF,
        S::GrayText => 0x808080FF,
        S::Highlight | S::SelectedItem | S::AccentColor => 0x0078D7FF,
        S::HighlightText | S::SelectedItemText | S::AccentColorText => 0xFFFFFFFF,
        _ => 0x000000FF,
    }
}

fn intern_font_family(styles: &mut ComputedStylesBuilder, families: &[FontFamily]) -> StyleStringId {
    let mut out = String::new();
    for (i, family) in families.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        let _ = family.to_css(&mut Printer::new(&mut out, PrinterOptions::default()));
    }
    styles.intern_string(&out)
}

fn log_unhandled_property(property: &Property) {
    static UNHANDLED_PROPERTIES: OnceLock<Mutex<HashSet<u64>>> = OnceLock::new();
    let set = UNHANDLED_PROPERTIES.get_or_init(|| Mutex::new(HashSet::new()));
    let mut hasher = DefaultHasher::new();
    std::mem::discriminant(property).hash(&mut hasher);
    let id = hasher.finish();
    let mut guard = set.lock().unwrap();
    if guard.insert(id) {
        println!("Unhandled CSS property: {:?}", property);
    }
}

/// Compute style for a single DOM element (used in new pipeline)
fn compute_style_for_dom_element<'sheet, 'css>(
    doc: &Document, computed_styles: &mut ComputedStylesBuilder, node_idx: DomNodeId, index: &SelectorIndex, prepared: &PreparedRuleSet<'sheet, 'css>, parent_custom: Option<&FxHashMap<String, TokenList<'css>>>,
    candidate_scratch: &mut Vec<EffectiveRuleId>, candidate_seen: &mut rustc_data_structures::fx::FxHashSet<EffectiveRuleId>, ancestor_filter: &AncestorFilter, timings: &mut ResolveStyleTimings,
) -> (WorkingStyle, FxHashMap<String, TokenList<'css>>) {
    let matching_started = Instant::now();
    let mut matched_rules: Vec<MatchedRule> = Vec::new();

    // Get element info for candidate lookup
    let tag = doc.get_dom_tag(node_idx).unwrap_or("");
    let id = doc.get_dom_id(node_idx);

    // Get candidate rule indices
    index.collect_candidates(tag, id, doc.get_dom_classes(node_idx), candidate_scratch, candidate_seen);

    // Check each candidate. A rule cascades with the most specific of its
    // matching selectors, not the first one that happens to match.
    for id in candidate_scratch.iter().copied() {
        let style_rule = prepared.get(id).style_rule();
        let specificity = style_rule.selectors.0.iter().filter(|selector| selector_might_match_dom(selector, ancestor_filter, doc, node_idx) && selector_matches_dom(selector, doc, node_idx)).map(|selector| selector.specificity()).max();
        if let Some(specificity) = specificity {
            matched_rules.push(MatchedRule { specificity, id });
        }
    }

    let mut important_rules = matched_rules.clone();
    matched_rules.sort_by(|a, b| prepared.get(a.id).priority().compare_normal(a.specificity, prepared.get(b.id).priority(), b.specificity));
    important_rules.sort_by(|a, b| prepared.get(a.id).priority().compare_important(a.specificity, prepared.get(b.id).priority(), b.specificity));
    timings.selector_matching += matching_started.elapsed();

    // Build computed style from matched rules (same as box-based version)
    let cascade_started = Instant::now();
    let mut style = get_inherited_style_dom(doc, computed_styles, node_idx);
    let parent_font_size = style.font.font_size;
    let mut custom_properties = parent_custom.map(|map| map.clone()).unwrap_or_default();
    let parent_custom = parent_custom.cloned().unwrap_or_default();
    let mut ancestor_indices: Vec<_> = doc.dom_ancestors(node_idx).collect();
    ancestor_indices.reverse();
    for ancestor_idx in ancestor_indices {
        if let Some(style_attr) = inline_style_attribute(doc, ancestor_idx) {
            collect_inline_style_custom_properties(&mut custom_properties, &parent_custom, &style_attr);
        }
    }
    let inline_style_attr = inline_style_attribute(doc, node_idx);
    let inline_style = parse_inline_style_attribute(doc, node_idx);

    // Collect custom properties
    for matched in &matched_rules {
        let style_rule = prepared.get(matched.id).style_rule();
        collect_custom_properties(&mut custom_properties, &parent_custom, &style_rule.declarations.declarations);
    }
    if let Some(inline_style) = &inline_style {
        collect_custom_properties(&mut custom_properties, &parent_custom, &inline_style.declarations.declarations);
    }
    if let Some(inline_style_attr) = &inline_style_attr {
        collect_inline_style_custom_properties(&mut custom_properties, &parent_custom, inline_style_attr);
    }
    for matched in important_rules.iter().filter(|matched| prepared.get(matched.id).priority().origin() == crate::style::prepared::CascadeOrigin::Author) {
        let style_rule = prepared.get(matched.id).style_rule();
        collect_custom_properties(&mut custom_properties, &parent_custom, &style_rule.declarations.important_declarations);
    }
    if let Some(inline_style) = &inline_style {
        collect_custom_properties(&mut custom_properties, &parent_custom, &inline_style.declarations.important_declarations);
    }
    for matched in important_rules.iter().filter(|matched| prepared.get(matched.id).priority().origin() == crate::style::prepared::CascadeOrigin::UserAgent) {
        let style_rule = prepared.get(matched.id).style_rule();
        collect_custom_properties(&mut custom_properties, &parent_custom, &style_rule.declarations.important_declarations);
    }

    // Run the full cascade (normal rules, inline normal, important rules,
    // inline important) twice: font-size and color first, then everything
    // else, so `em` lengths and `currentColor` resolve against final values.
    let parent_style = ParentStyle::for_node(doc, computed_styles, node_idx);
    for phase in [CascadePhase::FontSizeAndColor, CascadePhase::Remaining] {
        for matched in &matched_rules {
            let style_rule = prepared.get(matched.id).style_rule();
            apply_declarations(doc, computed_styles, &mut style, &style_rule.declarations.declarations, parent_font_size, Some(&custom_properties), phase, &parent_style);
        }

        if let Some(inline_style) = &inline_style {
            apply_declarations(doc, computed_styles, &mut style, &inline_style.declarations.declarations, parent_font_size, Some(&custom_properties), phase, &parent_style);
        }

        for matched in important_rules.iter().filter(|matched| prepared.get(matched.id).priority().origin() == crate::style::prepared::CascadeOrigin::Author) {
            let style_rule = prepared.get(matched.id).style_rule();
            apply_declarations(doc, computed_styles, &mut style, &style_rule.declarations.important_declarations, parent_font_size, Some(&custom_properties), phase, &parent_style);
        }

        if let Some(inline_style) = &inline_style {
            apply_declarations(doc, computed_styles, &mut style, &inline_style.declarations.important_declarations, parent_font_size, Some(&custom_properties), phase, &parent_style);
        }

        for matched in important_rules.iter().filter(|matched| prepared.get(matched.id).priority().origin() == crate::style::prepared::CascadeOrigin::UserAgent) {
            let style_rule = prepared.get(matched.id).style_rule();
            apply_declarations(doc, computed_styles, &mut style, &style_rule.declarations.important_declarations, parent_font_size, Some(&custom_properties), phase, &parent_style);
        }
    }

    if let Some(white_space) = custom_properties.get(WHITE_SPACE_CASCADE_MARKER).and_then(from_marker_tokens) {
        style.text.white_space = white_space;
    }
    for (marker, target) in [(LETTER_SPACING_MARKER, &mut style.text.letter_spacing), (WORD_SPACING_MARKER, &mut style.text.word_spacing)] {
        let Some(spacing) =
            custom_properties.get(marker).and_then(token_list_to_css_string).as_deref().and_then(parse_text_spacing).as_ref().and_then(|parsed| parsed_text_spacing_to_computed(parsed, style.font.font_size, doc.root_font_size()))
        else {
            continue;
        };
        *target = spacing;
        if let Some(tokens) = canonical_text_spacing_tokens(spacing) {
            custom_properties.insert(marker.to_string(), tokens);
        }
    }
    if let Some(tab_size) =
        custom_properties.get(TAB_SIZE_CASCADE_MARKER).and_then(token_list_to_css_string).as_deref().and_then(parse_tab_size).as_ref().and_then(|parsed| parsed_tab_size_to_computed(parsed, style.font.font_size, doc.root_font_size()))
    {
        style.text.tab_size = tab_size;
        if let Some(tokens) = canonical_tab_size_tokens(tab_size) {
            custom_properties.insert(TAB_SIZE_CASCADE_MARKER.to_string(), tokens);
        }
    }

    if let Some(raw_inline_style) = inline_style_attr.as_deref() {
        apply_raw_box_keywords(&mut style, raw_inline_style);
    }

    // The initial value of `border-*-color` is `currentColor`: sides drawn
    // without a declared color take the element's final text color. Only
    // paintable sides are touched so borderless styles stay deduplicated.
    let current_color = style.text.color;
    if !style.border_color_explicit[0] && style.border.border_top_style != BorderStyle::None {
        style.border.border_top_color = current_color;
    }
    if !style.border_color_explicit[1] && style.border.border_right_style != BorderStyle::None {
        style.border.border_right_color = current_color;
    }
    if !style.border_color_explicit[2] && style.border.border_bottom_style != BorderStyle::None {
        style.border.border_bottom_color = current_color;
    }
    if !style.border_color_explicit[3] && style.border.border_left_style != BorderStyle::None {
        style.border.border_left_color = current_color;
    }

    timings.cascade += cascade_started.elapsed();
    (style, custom_properties)
}

// Helper functions for DOM-based style resolution
use crate::style::selectors::AncestorFilter;

/// Build the starting style for an element: inherited groups (`font`, `text`)
/// copied from the parent, reset groups (`box_model`, `border`, `background`)
/// left at their defaults.
fn get_inherited_style_dom(doc: &Document, styles: &ComputedStylesBuilder, node_idx: DomNodeId) -> WorkingStyle {
    if let Some(parent_idx) = doc.get_dom_parent(node_idx) {
        if let Some(parent_style) = styles.style_for_node(parent_idx) {
            return WorkingStyle {
                font: styles.font_style(parent_style).expect("builder-issued parent style handle").clone(),
                text: styles.text_style(parent_style).expect("builder-issued parent style handle").clone(),
                box_model: BoxModel::default(),
                border: Border::default(),
                background: Background::default(),
                line_height_spec: None,
                border_color_explicit: [false; 4],
            };
        }
    }
    // No parent - use root_font_size as the base font size
    let mut style = WorkingStyle::default();
    style.font.font_size = doc.root_font_size();
    style
}

/// Build ancestor filter for DOM nodes (optimization)
fn extend_ancestor_filter_dom(doc: &Document, node_idx: DomNodeId, filter: &mut AncestorFilter) {
    if let Some(tag) = doc.get_dom_tag(node_idx) {
        filter.insert(tag);
    }

    if let Some(id) = doc.get_dom_id(node_idx) {
        filter.insert(id);
    }

    for class in doc.get_dom_classes(node_idx) {
        filter.insert(class);
    }
}

fn selector_might_match_dom(selector: &lightningcss::selector::Selector, ancestor_filter: &AncestorFilter, _doc: &Document, _node_idx: DomNodeId) -> bool {
    use crate::style::selectors_dom::selector_might_match_with_filter;
    selector_might_match_with_filter(selector, ancestor_filter)
}

fn selector_matches_dom(selector: &lightningcss::selector::Selector, doc: &Document, node_idx: DomNodeId) -> bool {
    use crate::style::selectors_dom::selector_matches_dom_node;
    selector_matches_dom_node(selector, doc, node_idx)
}

fn inline_style_attribute(doc: &Document, node_idx: DomNodeId) -> Option<String> {
    let style_attr = doc.get_dom_attr(node_idx, "style")?;
    if style_attr.trim().is_empty() {
        return None;
    }
    Some(normalize_declarations(style_attr))
}

fn parse_inline_style_attribute<'a>(doc: &Document, node_idx: DomNodeId) -> Option<StyleAttribute<'a>> {
    let style_attr = inline_style_attribute(doc, node_idx)?;
    let leaked: &'a str = Box::leak(style_attr.into_boxed_str());
    StyleAttribute::parse(leaked, ParserOptions::default()).ok()
}

fn collect_inline_style_custom_properties<'a>(custom_properties: &mut FxHashMap<String, TokenList<'a>>, parent_custom: &FxHashMap<String, TokenList<'a>>, style_attr: &str) {
    for declaration in style_attr.split(';') {
        let Some((name, value)) = declaration.split_once(':') else {
            continue;
        };
        let name = name.trim();
        if !name.starts_with("--") {
            continue;
        }

        let value = value.trim().strip_suffix("!important").unwrap_or(value.trim()).trim();
        let leaked_value: &'a str = Box::leak(value.to_string().into_boxed_str());
        let Ok(tokens) = TokenList::parse_string_with_options(leaked_value, ParserOptions::default()) else {
            continue;
        };

        if let Some(keyword) = single_ident_keyword(&tokens)
            && apply_custom_keyword(custom_properties, parent_custom, name, keyword)
        {
            continue;
        }

        custom_properties.insert(name.to_string(), tokens);
    }
}

fn apply_raw_box_keywords(style: &mut WorkingStyle, style_attr: &str) {
    for declaration in style_attr.split(';') {
        let Some((name, value)) = declaration.split_once(':') else {
            continue;
        };
        // Property names and keyword values are case-insensitive.
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim().to_ascii_lowercase();
        let value = value.strip_suffix("!important").map(str::trim).unwrap_or(value.as_str());
        match name.as_str() {
            "float" => {
                style.box_model.float = match value {
                    "left" | "inline-start" => Float::Left,
                    "right" | "inline-end" => Float::Right,
                    "none" => Float::None,
                    _ => style.box_model.float,
                };
            }
            "clear" => {
                style.box_model.clear = match value {
                    "left" | "inline-start" => Clear::Left,
                    "right" | "inline-end" => Clear::Right,
                    "both" => Clear::Both,
                    "none" => Clear::None,
                    _ => style.box_model.clear,
                };
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::MatchedRule;
    use crate::document::{Clear, Display, Document, ElementRef, Float};
    use crate::parser::DocumentFactory;

    #[test]
    fn matched_rule_hot_path_record_stays_compact() {
        assert!(std::mem::size_of::<MatchedRule>() <= 12);
    }

    #[test]
    fn inline_style_attribute_overrides_computed_style() {
        let html = "<html><body><p style=\"display: inline; color: red;\">Hello</p></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, None);

        let p_idx = find_body(document.document()).children().next().expect("paragraph should be child of body");
        let style = document.style_for_node(p_idx).expect("paragraph style should exist");

        assert_eq!(document.box_model_style(style).expect("validated style handle").display, Display::Inline);
        assert_eq!(document.text_style(style).expect("validated style handle").color, 0xFF0000FF);
    }

    #[test]
    fn authored_font_family_is_preserved_and_inherited() {
        let html = "<html><body style=\"font-family: 'DejaVu Serif'\"><p>Hello</p></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, None);

        let p_idx = find_body(document.document()).children().next().expect("paragraph should be child of body");
        let style = document.style_for_node(p_idx).expect("paragraph style should exist");
        let family_idx = document.font_style(style).expect("validated style handle").font_family.expect("paragraph should inherit the authored family");

        assert!(document.styles().string(family_idx).expect("family should belong to the computed-style table").contains("DejaVu Serif"));
    }

    #[test]
    fn normal_font_weight_resolves_to_css_weight_400() {
        let html = "<html><body><p style=\"font-weight: normal\">Hello</p></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, None);

        let p_idx = find_body(document.document()).children().next().expect("paragraph should be child of body");
        let style = document.style_for_node(p_idx).expect("paragraph style should exist");

        assert_eq!(document.font_style(style).expect("validated style handle").font_weight, 400);
    }

    #[test]
    fn inline_custom_property_resolves_var_reference() {
        let html = "<html><body><p style=\"--accent: red; color: var(--accent);\">Hello</p></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, None);

        let p_idx = find_body(document.document()).children().next().expect("paragraph should be child of body");
        let style = document.style_for_node(p_idx).expect("paragraph style should exist");

        assert_eq!(document.text_style(style).expect("validated style handle").color, 0xFF0000FF);
    }

    #[test]
    fn word_break_overflow_wrap_and_box_sizing_parse() {
        use crate::document::{BoxSizing, OverflowWrap, WordBreak};
        let html = "<html><body><p style=\"word-break: break-all; overflow-wrap: break-word; box-sizing: border-box\">Hello</p></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, None);

        let p_idx = find_body(document.document()).children().next().expect("paragraph should be child of body");
        let style = document.style_for_node(p_idx).expect("paragraph style should exist");

        assert_eq!(document.text_style(style).expect("validated style handle").word_break, WordBreak::BreakAll);
        assert_eq!(document.text_style(style).expect("validated style handle").overflow_wrap, OverflowWrap::BreakWord);
        assert_eq!(document.box_model_style(style).expect("validated style handle").box_sizing, BoxSizing::BorderBox);
    }

    #[test]
    fn legacy_word_wrap_maps_to_overflow_wrap() {
        use crate::document::OverflowWrap;
        let html = "<html><body><p style=\"word-wrap: break-word\">Hello</p></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, None);

        let p_idx = find_body(document.document()).children().next().expect("paragraph should be child of body");
        let style = document.style_for_node(p_idx).expect("paragraph style should exist");

        assert_eq!(document.text_style(style).expect("validated style handle").overflow_wrap, OverflowWrap::BreakWord);
    }

    #[test]
    fn float_and_clear_parse_into_box_model() {
        let html = "<html><body><p style=\"float:left; clear:both\">Hello</p></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, None);

        let p_idx = find_body(document.document()).children().next().expect("paragraph should be child of body");
        let style = document.style_for_node(p_idx).expect("paragraph style should exist");
        let box_model = document.box_model_style(style).expect("validated style handle");

        assert_eq!(box_model.float, Float::Left);
        assert_eq!(box_model.clear, Clear::Both);
    }

    #[test]
    fn line_height_stays_bound_to_final_font_size() {
        let html = "<html><body><p style=\"line-height:0.9em; font-size:450%;\">Hello</p></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, None);

        let p_idx = find_body(document.document()).children().next().expect("paragraph should be child of body");
        let style = document.style_for_node(p_idx).expect("paragraph style should exist");
        let font_size = document.font_style(style).expect("validated style handle").font_size;
        let line_height = document.text_style(style).expect("validated style handle").line_height;

        assert!((line_height - font_size * 0.9).abs() < 0.01, "line-height should track the final font size");
    }

    #[test]
    fn invalid_author_css_does_not_drop_default_styles_or_boxes() {
        let html = "<html><body><p>Hello</p></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline_css_chunks(html, &["p { color: red"]);

        let p_idx = find_body(document.document()).children().next().expect("paragraph should be child of body");
        let style = document.style_for_node(p_idx).expect("paragraph should have computed style");

        assert_eq!(document.box_model_style(style).expect("validated style handle").display, Display::Block);
    }

    #[test]
    fn later_css_chunks_win_for_equal_specificity() {
        let html = "<html><body><p>Hello</p></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline_css_chunks(html, &["p { color: red; }", "p { color: blue; }"]);

        let p_idx = find_body(document.document()).children().next().expect("paragraph should be child of body");
        let style = document.style_for_node(p_idx).expect("paragraph style should exist");

        assert_eq!(document.text_style(style).expect("validated style handle").color, 0x0000FFFF);
    }

    #[test]
    fn wpt_important_author_font_size_beats_inline_style_and_controls_em_line_height() {
        // Static adaptation of WPT css/css-cascade/important-vs-inline-002.html:
        // https://github.com/web-platform-tests/wpt/blob/master/css/css-cascade/important-vs-inline-002.html
        let html = "<html><body><p class=\"outer\" style=\"font-size: 24px\">Text</p></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, Some(".outer { font-size: 18px !important; line-height: 2em; }"));

        let paragraph = find_body(document.document()).children().next().expect("paragraph");
        let style = document.style_for_node(paragraph).expect("paragraph style");

        assert_eq!(document.font_style(style).expect("font style").font_size, 18.0);
        assert_eq!(document.text_style(style).expect("text style").line_height, 36.0);
    }

    #[test]
    fn wpt_unset_inherits_color_but_resets_non_inherited_margin() {
        // Semantic subset of WPT css/css-cascade/all-prop-unset-color.html.
        // `all: unset` is decomposed here into supported longhands so this test
        // stays local to CSS-wide keyword resolution:
        // https://github.com/web-platform-tests/wpt/blob/master/css/css-cascade/all-prop-unset-color.html
        let html = "<html><body><div style=\"color: red; margin-left: 20px\"><span style=\"color: unset; margin-left: unset\">Text</span></div></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, None);

        let parent = find_body(document.document()).children().next().expect("parent div");
        let child = document.document().element_ref(parent).expect("parent element").children().find(|node| document.document().is_element_node(*node)).expect("child span");
        let style = document.style_for_node(child).expect("child style");

        assert_eq!(document.text_style(style).expect("text style").color, 0xFF0000FF);
        assert_eq!(document.box_model_style(style).expect("box style").margin_left, crate::document::LengthPct::Px(0.0));
    }

    #[test]
    fn current_color_border_uses_text_color() {
        let html = "<html><body><p style=\"color: red; border: 1px solid\">Hello</p></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, None);

        let p_idx = find_body(document.document()).children().next().expect("paragraph should be child of body");
        let style = document.style_for_node(p_idx).expect("paragraph style should exist");
        let border = document.border_style(style).expect("validated style handle");

        assert_eq!(border.border_top_color, 0xFF0000FF, "borders without an explicit color should use currentColor");
    }

    #[test]
    fn em_length_uses_final_font_size_regardless_of_order() {
        let html = "<html><body><p style=\"padding-left: 1em; font-size: 32px;\">Hello</p></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, None);

        let p_idx = find_body(document.document()).children().next().expect("paragraph should be child of body");
        let style = document.style_for_node(p_idx).expect("paragraph style should exist");
        let padding_left = document.box_model_style(style).expect("validated style handle").padding_left;

        assert_eq!(padding_left, crate::document::LengthPct::Px(32.0), "1em padding should resolve against the element's final font size");
    }

    #[test]
    fn em_length_sees_font_size_from_later_rule() {
        let html = "<html><body><p>Hello</p></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline_css_chunks(html, &["p { padding-left: 1em; }", "p { font-size: 32px; }"]);

        let p_idx = find_body(document.document()).children().next().expect("paragraph should be child of body");
        let style = document.style_for_node(p_idx).expect("paragraph style should exist");
        let padding_left = document.box_model_style(style).expect("validated style handle").padding_left;

        assert_eq!(padding_left, crate::document::LengthPct::Px(32.0), "em lengths should see font-size from any rule in the cascade");
    }

    #[test]
    fn text_align_last_survives_later_text_align_rule() {
        use crate::document::TextAlign;
        let html = "<html><body><p>Hello</p></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline_css_chunks(html, &["p { text-align-last: center; }", "p { text-align: justify; }"]);

        let p_idx = find_body(document.document()).children().next().expect("paragraph should be child of body");
        let style = document.style_for_node(p_idx).expect("paragraph style should exist");
        let text = document.text_style(style).expect("validated style handle");

        assert_eq!(text.text_align, TextAlign::Justify);
        assert_eq!(text.text_align_last, TextAlign::Center, "explicit text-align-last should not be clobbered by a later text-align rule");
    }

    #[test]
    fn text_align_still_syncs_text_align_last_when_not_explicit() {
        use crate::document::TextAlign;
        let html = "<html><body><p style=\"text-align: center;\">Hello</p></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, None);

        let p_idx = find_body(document.document()).children().next().expect("paragraph should be child of body");
        let style = document.style_for_node(p_idx).expect("paragraph style should exist");

        assert_eq!(document.text_style(style).expect("validated style handle").text_align_last, TextAlign::Center);
    }

    #[test]
    fn absolute_length_units_convert_to_px() {
        let html = "<html><body><p style=\"width: 1in; height: 2.54cm;\">Hello</p></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, None);

        let p_idx = find_body(document.document()).children().next().expect("paragraph should be child of body");
        let style = document.style_for_node(p_idx).expect("paragraph style should exist");
        let box_model = document.box_model_style(style).expect("validated style handle");

        assert_eq!(box_model.width, crate::document::PreferredSize::Px(96.0));
        assert_eq!(box_model.height, crate::document::PreferredSize::Px(96.0));
    }

    #[test]
    fn background_shorthand_color_comes_from_last_layer() {
        let html = "<html><body><p style=\"background: url('a.png'), url('b.png') blue;\">Hello</p></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, None);

        let p_idx = find_body(document.document()).children().next().expect("paragraph should be child of body");
        let style = document.style_for_node(p_idx).expect("paragraph style should exist");

        assert_eq!(document.background_style(style).expect("validated style handle").background_color, 0x0000FFFF);
    }

    #[test]
    fn float_keyword_is_case_insensitive() {
        let html = "<html><body><p style=\"FLOAT: LEFT\">Hello</p></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, None);

        let p_idx = find_body(document.document()).children().next().expect("paragraph should be child of body");
        let style = document.style_for_node(p_idx).expect("paragraph style should exist");

        assert_eq!(document.box_model_style(style).expect("validated style handle").float, Float::Left);
    }

    #[test]
    fn unitless_line_height_recomputes_for_child_font_size() {
        let html = "<html><body style=\"line-height: 1.5;\"><p style=\"font-size: 32px;\">Hello</p></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, None);

        let p_idx = find_body(document.document()).children().next().expect("paragraph should be child of body");
        let style = document.style_for_node(p_idx).expect("paragraph style should exist");
        let line_height = document.text_style(style).expect("validated style handle").line_height;

        assert!((line_height - 48.0).abs() < 0.01, "unitless line-height should recompute against the child's font size, got {line_height}");
    }

    #[test]
    fn length_line_height_inherits_as_px() {
        let html = "<html><body style=\"line-height: 20px;\"><p style=\"font-size: 32px;\">Hello</p></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, None);

        let p_idx = find_body(document.document()).children().next().expect("paragraph should be child of body");
        let style = document.style_for_node(p_idx).expect("paragraph style should exist");
        let line_height = document.text_style(style).expect("validated style handle").line_height;

        assert!((line_height - 20.0).abs() < 0.01, "length line-height should inherit as resolved px, got {line_height}");
    }

    #[test]
    fn structural_pseudo_classes_match() {
        let html = "<html><body><p>One</p><p>Two</p><p>Three</p></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline_css_chunks(html, &["p:first-child { color: red; } p:nth-child(2) { color: blue; } p:last-child { color: green; }"]);

        let doc = document.document();
        let paragraphs: Vec<_> = find_body(doc).children().collect();
        let color_of = |idx| document.text_style(document.style_for_node(idx).expect("style")).expect("validated style handle").color;

        assert_eq!(color_of(paragraphs[0]), 0xFF0000FF, ":first-child should match the first paragraph");
        assert_eq!(color_of(paragraphs[1]), 0x0000FFFF, ":nth-child(2) should match the second paragraph");
        assert_eq!(color_of(paragraphs[2]), 0x008000FF, ":last-child should match the third paragraph");
    }

    #[test]
    fn not_first_child_excludes_the_first_child() {
        let html = "<html><body><p>One</p><p>Two</p></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline_css_chunks(html, &["p:not(:first-child) { color: red; }"]);

        let doc = document.document();
        let paragraphs: Vec<_> = find_body(doc).children().collect();
        let color_of = |idx| document.text_style(document.style_for_node(idx).expect("style")).expect("validated style handle").color;

        assert_eq!(color_of(paragraphs[0]), 0x000000FF, ":not(:first-child) must not match the first child");
        assert_eq!(color_of(paragraphs[1]), 0xFF0000FF);
    }

    #[test]
    fn rule_cascades_with_most_specific_matching_selector() {
        let html = "<html><body><p class=\"foo\">Hello</p></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline_css_chunks(html, &["p, .foo { color: red; }", "p { color: blue; }"]);

        let p_idx = find_body(document.document()).children().next().expect("paragraph should be child of body");
        let style = document.style_for_node(p_idx).expect("paragraph style should exist");

        assert_eq!(document.text_style(style).expect("validated style handle").color, 0xFF0000FF, "the .foo selector's specificity should win over the later p rule");
    }

    #[test]
    fn undeclared_border_color_defaults_to_current_color() {
        let html = "<html><body><p style=\"color: red; border-width: 2px; border-style: solid;\">Hello</p></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, None);

        let p_idx = find_body(document.document()).children().next().expect("paragraph should be child of body");
        let style = document.style_for_node(p_idx).expect("paragraph style should exist");
        let border = document.border_style(style).expect("validated style handle");

        assert_eq!(border.border_top_color, 0xFF0000FF, "border-color's initial value is currentColor");
        assert_eq!(border.border_left_color, 0xFF0000FF);
    }

    #[test]
    fn only_anchors_with_href_get_link_styling() {
        let html = "<html><body><a id=\"target\">Anchor</a><a href=\"x.html\">Link</a></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, None);

        let doc = document.document();
        let anchors: Vec<_> = find_body(doc).children().collect();
        let plain = document.style_for_node(anchors[0]).expect("anchor style");
        let link = document.style_for_node(anchors[1]).expect("link style");

        assert_eq!(document.text_style(plain).expect("validated style handle").color, 0x000000FF, "href-less anchor must not be link-colored");
        assert_eq!(document.background_style(plain).expect("validated style handle").text_decoration, 0, "href-less anchor must not be underlined");
        assert_eq!(document.text_style(link).expect("validated style handle").color, 0x0000FFFF);
        assert_eq!(document.background_style(link).expect("validated style handle").text_decoration & 1, 1, "link should be underlined");
    }

    #[test]
    fn vertical_align_inherit_propagates_tbody_middle_to_cells() {
        use crate::document::VerticalAlignValue;
        let html = "<html><body><table><tbody><tr><td>X</td></tr></tbody></table></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, None);

        let doc = document.document();
        let mut node = find_body(doc).node_id();
        for _ in 0..4 {
            // table > tbody > tr > td
            node = doc.element_ref(node).expect("element").children().find(|&child| doc.element_ref(child).is_some()).expect("child element");
        }
        let td = doc.element_ref(node).expect("td");
        assert_eq!(td.tag(), "td");
        let style = document.style_for_node(node).expect("td style should exist");

        assert_eq!(document.box_model_style(style).expect("validated style handle").vertical_align, VerticalAlignValue::Middle, "the UA sheet's vertical-align: inherit chain should reach the cell");
    }

    #[test]
    fn text_align_initial_resets_inherited_alignment() {
        use crate::document::TextAlign;
        let html = "<html><body style=\"text-align: center;\"><p style=\"text-align: initial;\">Hello</p></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, None);

        let p_idx = find_body(document.document()).children().next().expect("paragraph should be child of body");
        let style = document.style_for_node(p_idx).expect("paragraph style should exist");

        assert_eq!(document.text_style(style).expect("validated style handle").text_align, TextAlign::Left);
    }

    #[test]
    fn list_item_keeps_inherited_text_align() {
        use crate::document::TextAlign;
        // The UA sheet's `li { text-align: match-parent }` must not reset
        // inherited justification.
        let html = "<html><body style=\"text-align: justify;\"><ul><li>Hello</li></ul></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, None);

        let doc = document.document();
        let ul_idx = find_body(doc).children().next().expect("ul should be child of body");
        let li_idx = doc.element_ref(ul_idx).expect("ul element").children().next().expect("li should be child of ul");
        let style = document.style_for_node(li_idx).expect("li style should exist");

        assert_eq!(document.text_style(style).expect("validated style handle").text_align, TextAlign::Justify);
    }

    #[test]
    fn mark_system_colors_resolve_to_yellow_on_black() {
        let html = "<html><body><mark>Hello</mark></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, None);

        let mark_idx = find_body(document.document()).children().next().expect("mark should be child of body");
        let style = document.style_for_node(mark_idx).expect("mark style should exist");

        assert_eq!(document.background_style(style).expect("validated style handle").background_color, 0xFFFF00FF, "Mark system color should be yellow");
        assert_eq!(document.text_style(style).expect("validated style handle").color, 0x000000FF, "MarkText system color should be black");
    }

    #[test]
    fn hr_inset_border_renders_as_solid_gray() {
        let html = "<html><body><hr></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, None);

        let hr_idx = find_body(document.document()).children().next().expect("hr should be child of body");
        let style = document.style_for_node(hr_idx).expect("hr style should exist");
        let border = document.border_style(style).expect("validated style handle");

        assert_eq!(border.border_top_style, crate::document::BorderStyle::Solid, "the UA sheet's `border-style: inset` should paint as solid");
        assert_eq!(border.border_top_width, 1.0);
        assert_eq!(border.border_top_color, 0x808080FF, "undeclared border color should take hr's gray text color");
    }

    fn find_body(document: &Document) -> ElementRef<'_> {
        let html_idx = document.dom_root().expect("html root should exist");
        let html = document.element_ref(html_idx).expect("html should be element");
        let body_idx = html.children().find(|&child_idx| document.element_ref(child_idx).is_some_and(|element| element.tag() == "body")).expect("body should be child of html");
        document.element_ref(body_idx).expect("body should be element")
    }
}
