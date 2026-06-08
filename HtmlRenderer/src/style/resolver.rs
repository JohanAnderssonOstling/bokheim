use crate::document::{
    Background, Border, BorderCollapseMode, BorderStyle, BoxModel, CaptionSide, ComputedStyle, Display, Document, EmptyCellsMode, Font, FontStyle, InheritedText, PreferredSize, StyleIndices, TextAlign, TextTransform, VerticalAlignValue,
    WhiteSpace,
};
use crate::style::selectors::{RuleKey, SelectorIndex};
use lightningcss::printer::{Printer, PrinterOptions};
use lightningcss::properties::Property;
use lightningcss::properties::border::{BorderSideWidth, LineStyle};
use lightningcss::properties::custom::{Token, TokenList, TokenOrValue};
use lightningcss::properties::font::{AbsoluteFontWeight, FontFamily, FontSize, FontWeight, VerticalAlign, VerticalAlignKeyword};
use lightningcss::properties::size::{MaxSize, Size};
use lightningcss::properties::text::Spacing;
use lightningcss::rules::CssRule;
use lightningcss::stylesheet::{ParserOptions, StyleAttribute, StyleSheet};
use lightningcss::traits::{ParseWithOptions, ToCss};
use lightningcss::values::color::CssColor;
use lightningcss::values::length::{LengthPercentage, LengthPercentageOrAuto, LengthValue};
use rustc_data_structures::fx::FxHashMap;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::{Mutex, OnceLock};

// ============================================================================
// Style Resolver - applies CSS styles to Document boxes
// ============================================================================
const FORCE_LIBERATION_SERIF: bool = true;
const LIBERATION_SERIF_NAME: &str = "Liberation Serif";
const MONOSPACE_FAMILY: &str = "monospace";

/// Split a ComputedStyle into sub-structs and push to document's split style vecs
/// Returns StyleIndices pointing to the newly pushed styles
fn push_split_style(doc: &mut Document, style: &ComputedStyle) -> StyleIndices {
    let font = Font { font_size: style.font_size, font_weight: style.font_weight, font_style: style.font_style, font_family: style.font_family, font_variant_small_caps: style.font_variant_small_caps };

    let text = InheritedText {
        color: style.color,
        line_height: style.line_height,
        letter_spacing: style.letter_spacing,
        word_spacing: style.word_spacing,
        text_align: style.text_align,
        text_align_last: style.text_align_last,
        text_indent: style.text_indent,
        text_indent_hanging: style.text_indent_hanging,
        text_indent_each_line: style.text_indent_each_line,
        white_space: style.white_space,
        text_transform: style.text_transform,
    };

    let box_model = BoxModel {
        display: style.display,
        vertical_align: style.vertical_align,
        border_collapse: style.border_collapse,
        border_spacing_horizontal: style.border_spacing_horizontal,
        border_spacing_vertical: style.border_spacing_vertical,
        empty_cells: style.empty_cells,
        caption_side: style.caption_side,
        width: style.width,
        height: style.height,
        min_width: style.min_width,
        min_height: style.min_height,
        max_width: style.max_width,
        max_height: style.max_height,
        margin_top: style.margin_top,
        margin_bottom: style.margin_bottom,
        margin_left: style.margin_left,
        margin_right: style.margin_right,
        padding_top: style.padding_top,
        padding_bottom: style.padding_bottom,
        padding_left: style.padding_left,
        padding_right: style.padding_right,
    };

    let border = Border {
        border_top_width: style.border_top_width,
        border_right_width: style.border_right_width,
        border_bottom_width: style.border_bottom_width,
        border_left_width: style.border_left_width,
        border_top_color: style.border_top_color,
        border_right_color: style.border_right_color,
        border_bottom_color: style.border_bottom_color,
        border_left_color: style.border_left_color,
        border_top_style: style.border_top_style,
        border_right_style: style.border_right_style,
        border_bottom_style: style.border_bottom_style,
        border_left_style: style.border_left_style,
    };

    let background = Background { background_color: style.background_color, text_decoration: style.text_decoration };

    doc.push_style(font, text, box_model, border, background)
}

/// Matched rule with specificity for cascade sorting
struct MatchedRule {
    specificity: u32,
    source_order: usize,
    key: RuleKey,
}

/// Resolve styles for all DOM elements
/// This works with the DOM tree before boxes are created
pub fn resolve_styles_for_dom<'a>(doc: &mut Document, stylesheets: &[StyleSheet<'a, 'a>]) {
    // Build selector index for fast candidate lookup
    let index = SelectorIndex::from_stylesheets(stylesheets);
    let forced_family = if FORCE_LIBERATION_SERIF { Some(doc.intern_string(LIBERATION_SERIF_NAME)) } else { None };

    // Process each DOM element
    let mut custom_maps: Vec<FxHashMap<String, TokenList<'a>>> = vec![FxHashMap::default(); doc.node_count()];

    for node_idx in 0..doc.node_count() {
        // Skip text nodes
        if doc.is_text_node(node_idx) {
            continue;
        }

        let parent_custom = doc.get_dom_parent(node_idx).and_then(|parent_idx| custom_maps.get(parent_idx));

        let (mut style, custom_map) = compute_style_for_dom_element(doc, node_idx, &index, stylesheets, parent_custom);

        if let Some(slot) = custom_maps.get_mut(node_idx) {
            *slot = custom_map;
        }

        if let Some(family) = forced_family {
            let keep_original = style.font_family.and_then(|idx| doc.string(idx).to_lowercase().split(',').next().map(|s| s.trim().to_string())).map(|name| name == MONOSPACE_FAMILY).unwrap_or(false);
            if !keep_original {
                style.font_family = Some(family);
            }
        }

        // Store style in split Vecs
        let style_indices = push_split_style(doc, &style);

        // Update the style reference for this DOM element.
        doc.set_dom_style_indices(node_idx, Some(style_indices));
    }
}

pub fn is_property_supported(property: &Property) -> bool {
    if matches!(
        property,
        Property::FontSize(_)
            | Property::FontWeight(_)
            | Property::FontStyle(_)
            | Property::FontFamily(_)
            | Property::Font(_)
            | Property::LineHeight(_)
            | Property::LetterSpacing(_)
            | Property::WordSpacing(_)
            | Property::TextAlign(_)
            | Property::TextAlignLast(_, _)
            | Property::TextIndent(_)
            | Property::TextTransform(_)
            | Property::FontVariantCaps(_)
            | Property::TextDecorationLine(_, _)
            | Property::TextDecoration(_, _)
            | Property::VerticalAlign(_)
            | Property::WhiteSpace(_)
            | Property::Color(_)
            | Property::BackgroundColor(_)
            | Property::Background(_)
            | Property::Width(_)
            | Property::Height(_)
            | Property::MinWidth(_)
            | Property::MinHeight(_)
            | Property::MaxWidth(_)
            | Property::MaxHeight(_)
            | Property::MarginTop(_)
            | Property::MarginBottom(_)
            | Property::MarginLeft(_)
            | Property::MarginRight(_)
            | Property::Margin(_)
            | Property::MarginBlock(_)
            | Property::MarginBlockStart(_)
            | Property::MarginBlockEnd(_)
            | Property::MarginInline(_)
            | Property::MarginInlineStart(_)
            | Property::MarginInlineEnd(_)
            | Property::PaddingTop(_)
            | Property::PaddingBottom(_)
            | Property::PaddingLeft(_)
            | Property::PaddingRight(_)
            | Property::Padding(_)
            | Property::BorderWidth(_)
            | Property::BorderTopWidth(_)
            | Property::BorderRightWidth(_)
            | Property::BorderBottomWidth(_)
            | Property::BorderLeftWidth(_)
            | Property::BorderColor(_)
            | Property::BorderTopColor(_)
            | Property::BorderRightColor(_)
            | Property::BorderBottomColor(_)
            | Property::BorderLeftColor(_)
            | Property::BorderStyle(_)
            | Property::BorderTopStyle(_)
            | Property::BorderRightStyle(_)
            | Property::BorderBottomStyle(_)
            | Property::BorderLeftStyle(_)
            | Property::Border(_)
            | Property::BorderTop(_)
            | Property::BorderBottom(_)
            | Property::BorderLeft(_)
            | Property::BorderRight(_)
            | Property::Hyphens(_, _)
            | Property::Display(_)
            | Property::BorderSpacing(_)
    ) {
        return true;
    }

    if matches!(property, Property::Custom(_)) {
        return true;
    }

    if let Property::Unparsed(unparsed) = property {
        return is_supported_property_name(unparsed.property_id.name());
    }

    false
}

fn is_supported_property_name(name: &str) -> bool {
    matches!(
        name,
        "font-size"
            | "font-weight"
            | "font-style"
            | "font-family"
            | "font"
            | "line-height"
            | "letter-spacing"
            | "word-spacing"
            | "text-align"
            | "text-align-last"
            | "text-indent"
            | "text-transform"
            | "font-variant-caps"
            | "text-decoration-line"
            | "text-decoration"
            | "vertical-align"
            | "white-space"
            | "color"
            | "background-color"
            | "background"
            | "width"
            | "height"
            | "min-width"
            | "min-height"
            | "max-width"
            | "max-height"
            | "margin-top"
            | "margin-bottom"
            | "margin-left"
            | "margin-right"
            | "margin"
            | "margin-block"
            | "margin-block-start"
            | "margin-block-end"
            | "margin-inline"
            | "margin-inline-start"
            | "margin-inline-end"
            | "padding-top"
            | "padding-bottom"
            | "padding-left"
            | "padding-right"
            | "padding"
            | "border-width"
            | "border-top-width"
            | "border-right-width"
            | "border-bottom-width"
            | "border-left-width"
            | "border-color"
            | "border-top-color"
            | "border-right-color"
            | "border-bottom-color"
            | "border-left-color"
            | "border-style"
            | "border-top-style"
            | "border-right-style"
            | "border-bottom-style"
            | "border-left-style"
            | "border"
            | "border-top"
            | "border-bottom"
            | "border-left"
            | "border-right"
            | "hyphens"
            | "page-break-before"
            | "page-break-after"
            | "page-break-inside"
            | "break-before"
            | "break-after"
            | "break-inside"
            | "display"
    )
}

/// Apply CSS declarations to a computed style
fn apply_declarations<'a>(doc: &mut Document, style: &mut ComputedStyle, declarations: &[Property<'a>], parent_font_size: f32, custom_properties: Option<&FxHashMap<String, TokenList<'a>>>) {
    let mut var_map: HashMap<&str, TokenList<'a>> = HashMap::new();
    let mut saw_text_align_last = false;
    if let Some(map) = custom_properties {
        for (name, value) in map {
            var_map.insert(name.as_str(), value.clone());
        }
    }

    for property in declarations {
        match property {
            Property::Custom(custom) => {
                let raw_name = custom.name.as_ref();
                let name = if raw_name.starts_with("--") {
                    raw_name
                } else {
                    continue;
                };
                var_map.insert(name, custom.value.clone());
                continue;
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
                    if matches!(resolved, Property::TextAlignLast(_, _)) {
                        saw_text_align_last = true;
                    }
                    apply_property(doc, style, &resolved, parent_font_size);
                    if matches!(resolved, Property::TextAlign(_)) && !saw_text_align_last {
                        style.text_align_last = style.text_align;
                    }
                    continue;
                }
                if let Some(resolved) = resolve_single_var_property(unparsed, &var_map) {
                    if matches!(resolved, Property::TextAlignLast(_, _)) {
                        saw_text_align_last = true;
                    }
                    apply_property(doc, style, &resolved, parent_font_size);
                    if matches!(resolved, Property::TextAlign(_)) && !saw_text_align_last {
                        style.text_align_last = style.text_align;
                    }
                    continue;
                }
            }
        }

        if matches!(property, Property::TextAlignLast(_, _)) {
            saw_text_align_last = true;
        }
        apply_property(doc, style, property, parent_font_size);
        if matches!(property, Property::TextAlign(_)) && !saw_text_align_last {
            style.text_align_last = style.text_align;
        }
    }
}

fn apply_property<'a>(doc: &mut Document, style: &mut ComputedStyle, property: &Property<'a>, parent_font_size: f32) {
    match property {
        // Font properties
        Property::FontSize(size) => {
            style.font_size = font_size_to_px(size, parent_font_size, doc.root_font_size());
        }
        Property::FontWeight(weight) => {
            style.font_weight = font_weight_to_u16(weight);
        }
        Property::FontStyle(fs) => {
            use lightningcss::properties::font::FontStyle as LcFontStyle;
            style.font_style = match fs {
                LcFontStyle::Normal => FontStyle::Normal,
                LcFontStyle::Italic => FontStyle::Italic,
                LcFontStyle::Oblique(_) => FontStyle::Italic,
            };
        }
        Property::FontFamily(families) => {
            style.font_family = Some(intern_font_family(doc, families));
        }
        Property::Font(font) => {
            style.font_size = font_size_to_px(&font.size, parent_font_size, doc.root_font_size());
            style.font_weight = font_weight_to_u16(&font.weight);
            use lightningcss::properties::font::FontStyle as LcFontStyle;
            style.font_style = match font.style {
                LcFontStyle::Normal => FontStyle::Normal,
                LcFontStyle::Italic => FontStyle::Italic,
                LcFontStyle::Oblique(_) => FontStyle::Italic,
            };
            style.line_height = parse_line_height(&font.line_height, style.font_size, doc.root_font_size());
            style.font_family = Some(intern_font_family(doc, &font.family));
            use lightningcss::properties::font::FontVariantCaps;
            style.font_variant_small_caps = matches!(font.variant_caps, FontVariantCaps::SmallCaps);
        }

        // Text properties
        Property::LineHeight(lh) => {
            style.line_height = parse_line_height(lh, style.font_size, doc.root_font_size());
        }
        Property::LetterSpacing(spacing) => {
            style.letter_spacing = spacing_to_px(spacing, style.font_size, doc.root_font_size());
        }
        Property::WordSpacing(spacing) => {
            style.word_spacing = spacing_to_px(spacing, style.font_size, doc.root_font_size());
        }
        Property::TextAlign(ta) => {
            use lightningcss::properties::text::TextAlign as LcTextAlign;
            style.text_align = match ta {
                LcTextAlign::Left => TextAlign::Left,
                LcTextAlign::Right => TextAlign::Right,
                LcTextAlign::Center => TextAlign::Center,
                LcTextAlign::Justify => TextAlign::Justify,
                LcTextAlign::Start => TextAlign::Left,
                LcTextAlign::End => TextAlign::Right,
                _ => TextAlign::Left,
            };
        }
        Property::TextAlignLast(ta, _) => {
            use lightningcss::properties::text::TextAlignLast as LcTextAlignLast;
            style.text_align_last = match ta {
                LcTextAlignLast::Auto => style.text_align, // inherit from text-align
                LcTextAlignLast::Left => TextAlign::Left,
                LcTextAlignLast::Right => TextAlign::Right,
                LcTextAlignLast::Center => TextAlign::Center,
                LcTextAlignLast::Justify => TextAlign::Justify,
                LcTextAlignLast::Start => TextAlign::Left,
                LcTextAlignLast::End => TextAlign::Right,
                _ => style.text_align,
            };
        }
        Property::TextIndent(ti) => {
            style.text_indent = length_percentage_to_px(&ti.value, style.font_size, doc.root_font_size());
            style.text_indent_hanging = ti.hanging;
            style.text_indent_each_line = ti.each_line;
        }
        Property::TextTransform(tt) => {
            use lightningcss::properties::text::TextTransformCase;
            style.text_transform = match tt.case {
                TextTransformCase::Uppercase => TextTransform::Uppercase,
                TextTransformCase::Lowercase => TextTransform::Lowercase,
                TextTransformCase::Capitalize => TextTransform::Capitalize,
                _ => TextTransform::None,
            };
        }
        Property::FontVariantCaps(caps) => {
            use lightningcss::properties::font::FontVariantCaps;
            style.font_variant_small_caps = matches!(caps, FontVariantCaps::SmallCaps);
        }
        Property::TextDecorationLine(line, _) => {
            apply_text_decoration(style, line);
        }
        Property::TextDecoration(td, _) => {
            apply_text_decoration(style, &td.line);
        }
        Property::VerticalAlign(va) => {
            style.vertical_align = match va {
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
                    LengthPercentage::Dimension(len) => VerticalAlignValue::Length(length_to_px(len, style.font_size, doc.root_font_size())),
                    LengthPercentage::Percentage(p) => VerticalAlignValue::Percent(p.0),
                    LengthPercentage::Calc(_) => VerticalAlignValue::Baseline,
                },
            };
        }
        Property::WhiteSpace(ws) => {
            use lightningcss::properties::text::WhiteSpace as LcWhiteSpace;
            style.white_space = match ws {
                LcWhiteSpace::Normal => WhiteSpace::Normal,
                LcWhiteSpace::Pre => WhiteSpace::Pre,
                LcWhiteSpace::NoWrap => WhiteSpace::NoWrap,
                LcWhiteSpace::PreWrap => WhiteSpace::PreWrap,
                LcWhiteSpace::PreLine => WhiteSpace::PreLine,
                LcWhiteSpace::BreakSpaces => WhiteSpace::PreWrap,
            };
        }

        // Color
        Property::Color(c) => {
            style.color = css_color_to_u32(c);
        }
        Property::BackgroundColor(c) => {
            style.background_color = css_color_to_u32(c);
        }
        Property::Background(bg) => {
            if !bg.is_empty() {
                style.background_color = css_color_to_u32(&bg[0].color);
            }
        }

        // Size
        Property::Width(size) => {
            style.width = size_to_preferred(size, style.font_size, doc.root_font_size());
        }
        Property::Height(size) => {
            style.height = size_to_preferred(size, style.font_size, doc.root_font_size());
        }
        Property::BorderSpacing(spacing) => {
            style.border_spacing_horizontal = length_to_px_from_length(&spacing.0, style.font_size, doc.root_font_size());
            style.border_spacing_vertical = length_to_px_from_length(&spacing.1, style.font_size, doc.root_font_size());
        }
        Property::MinWidth(size) => {
            style.min_width = size_to_preferred(size, style.font_size, doc.root_font_size());
        }
        Property::MinHeight(size) => {
            style.min_height = size_to_preferred(size, style.font_size, doc.root_font_size());
        }
        Property::MaxWidth(max_size) => {
            style.max_width = max_size_to_preferred(max_size, style.font_size, doc.root_font_size());
        }
        Property::MaxHeight(max_size) => {
            style.max_height = max_size_to_preferred(max_size, style.font_size, doc.root_font_size());
        }

        // Margin
        Property::MarginTop(m) => {
            style.margin_top = length_or_auto_to_px(m, style.font_size, doc.root_font_size());
        }
        Property::MarginBottom(m) => {
            style.margin_bottom = length_or_auto_to_px(m, style.font_size, doc.root_font_size());
        }
        Property::MarginLeft(m) => {
            style.margin_left = length_or_auto_to_px(m, style.font_size, doc.root_font_size());
        }
        Property::MarginRight(m) => {
            style.margin_right = length_or_auto_to_px(m, style.font_size, doc.root_font_size());
        }
        Property::Margin(m) => {
            style.margin_top = length_or_auto_to_px(&m.top, style.font_size, doc.root_font_size());
            style.margin_bottom = length_or_auto_to_px(&m.bottom, style.font_size, doc.root_font_size());
            style.margin_left = length_or_auto_to_px(&m.left, style.font_size, doc.root_font_size());
            style.margin_right = length_or_auto_to_px(&m.right, style.font_size, doc.root_font_size());
        }
        Property::MarginBlock(m) => {
            style.margin_top = length_or_auto_to_px(&m.block_start, style.font_size, doc.root_font_size());
            style.margin_bottom = length_or_auto_to_px(&m.block_end, style.font_size, doc.root_font_size());
        }
        Property::MarginBlockStart(m) => {
            style.margin_top = length_or_auto_to_px(m, style.font_size, doc.root_font_size());
        }
        Property::MarginBlockEnd(m) => {
            style.margin_bottom = length_or_auto_to_px(m, style.font_size, doc.root_font_size());
        }
        Property::MarginInline(m) => {
            style.margin_left = length_or_auto_to_px(&m.inline_start, style.font_size, doc.root_font_size());
            style.margin_right = length_or_auto_to_px(&m.inline_end, style.font_size, doc.root_font_size());
        }
        Property::MarginInlineStart(m) => {
            style.margin_left = length_or_auto_to_px(m, style.font_size, doc.root_font_size());
        }
        Property::MarginInlineEnd(m) => {
            style.margin_right = length_or_auto_to_px(m, style.font_size, doc.root_font_size());
        }

        // Padding - uses LengthPercentageOrAuto in lightningcss
        Property::PaddingTop(p) => {
            style.padding_top = length_or_auto_to_px(p, style.font_size, doc.root_font_size());
        }
        Property::PaddingBottom(p) => {
            style.padding_bottom = length_or_auto_to_px(p, style.font_size, doc.root_font_size());
        }
        Property::PaddingLeft(p) => {
            style.padding_left = length_or_auto_to_px(p, style.font_size, doc.root_font_size());
        }
        Property::PaddingRight(p) => {
            style.padding_right = length_or_auto_to_px(p, style.font_size, doc.root_font_size());
        }
        Property::Padding(p) => {
            style.padding_top = length_or_auto_to_px(&p.top, style.font_size, doc.root_font_size());
            style.padding_bottom = length_or_auto_to_px(&p.bottom, style.font_size, doc.root_font_size());
            style.padding_left = length_or_auto_to_px(&p.left, style.font_size, doc.root_font_size());
            style.padding_right = length_or_auto_to_px(&p.right, style.font_size, doc.root_font_size());
        }

        // Border (per-side)
        Property::BorderWidth(bw) => {
            style.border_top_width = border_width_to_px(&bw.top, style.font_size, doc.root_font_size());
            style.border_right_width = border_width_to_px(&bw.right, style.font_size, doc.root_font_size());
            style.border_bottom_width = border_width_to_px(&bw.bottom, style.font_size, doc.root_font_size());
            style.border_left_width = border_width_to_px(&bw.left, style.font_size, doc.root_font_size());
        }
        Property::BorderTopWidth(w) => {
            style.border_top_width = border_width_to_px(w, style.font_size, doc.root_font_size());
        }
        Property::BorderRightWidth(w) => {
            style.border_right_width = border_width_to_px(w, style.font_size, doc.root_font_size());
        }
        Property::BorderBottomWidth(w) => {
            style.border_bottom_width = border_width_to_px(w, style.font_size, doc.root_font_size());
        }
        Property::BorderLeftWidth(w) => {
            style.border_left_width = border_width_to_px(w, style.font_size, doc.root_font_size());
        }
        Property::BorderColor(bc) => {
            style.border_top_color = css_color_to_u32(&bc.top);
            style.border_right_color = css_color_to_u32(&bc.right);
            style.border_bottom_color = css_color_to_u32(&bc.bottom);
            style.border_left_color = css_color_to_u32(&bc.left);
        }
        Property::BorderTopColor(c) => {
            style.border_top_color = css_color_to_u32(c);
        }
        Property::BorderRightColor(c) => {
            style.border_right_color = css_color_to_u32(c);
        }
        Property::BorderBottomColor(c) => {
            style.border_bottom_color = css_color_to_u32(c);
        }
        Property::BorderLeftColor(c) => {
            style.border_left_color = css_color_to_u32(c);
        }
        Property::BorderStyle(bs) => {
            style.border_top_style = line_style_to_border_style(&bs.top);
            style.border_right_style = line_style_to_border_style(&bs.right);
            style.border_bottom_style = line_style_to_border_style(&bs.bottom);
            style.border_left_style = line_style_to_border_style(&bs.left);
        }
        Property::BorderTopStyle(s) => {
            style.border_top_style = line_style_to_border_style(s);
        }
        Property::BorderRightStyle(s) => {
            style.border_right_style = line_style_to_border_style(s);
        }
        Property::BorderBottomStyle(s) => {
            style.border_bottom_style = line_style_to_border_style(s);
        }
        Property::BorderLeftStyle(s) => {
            style.border_left_style = line_style_to_border_style(s);
        }
        Property::Border(b) => {
            let width = border_width_to_px(&b.width, style.font_size, doc.root_font_size());
            let color = css_color_to_u32(&b.color);
            let border_style = line_style_to_border_style(&b.style);
            style.border_top_width = width;
            style.border_right_width = width;
            style.border_bottom_width = width;
            style.border_left_width = width;
            style.border_top_color = color;
            style.border_right_color = color;
            style.border_bottom_color = color;
            style.border_left_color = color;
            style.border_top_style = border_style;
            style.border_right_style = border_style;
            style.border_bottom_style = border_style;
            style.border_left_style = border_style;
        }
        Property::BorderTop(b) => {
            style.border_top_width = border_width_to_px(&b.width, style.font_size, doc.root_font_size());
            style.border_top_color = css_color_to_u32(&b.color);
            style.border_top_style = line_style_to_border_style(&b.style);
        }
        Property::BorderRight(b) => {
            style.border_right_width = border_width_to_px(&b.width, style.font_size, doc.root_font_size());
            style.border_right_color = css_color_to_u32(&b.color);
            style.border_right_style = line_style_to_border_style(&b.style);
        }
        Property::BorderBottom(b) => {
            style.border_bottom_width = border_width_to_px(&b.width, style.font_size, doc.root_font_size());
            style.border_bottom_color = css_color_to_u32(&b.color);
            style.border_bottom_style = line_style_to_border_style(&b.style);
        }
        Property::BorderLeft(b) => {
            style.border_left_width = border_width_to_px(&b.width, style.font_size, doc.root_font_size());
            style.border_left_color = css_color_to_u32(&b.color);
            style.border_left_style = line_style_to_border_style(&b.style);
        }

        // Display
        Property::Display(d) => {
            use lightningcss::properties::display::{Display as LcDisplay, DisplayInside, DisplayKeyword, DisplayOutside};
            style.display = match d {
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
                LcDisplay::Pair(pair) => match (&pair.outside, &pair.inside) {
                    (DisplayOutside::Block, DisplayInside::Table) => Display::Table,
                    (DisplayOutside::Inline, DisplayInside::Table) => Display::InlineTable,
                    (DisplayOutside::Block, _) => Display::Block,
                    (DisplayOutside::Inline, _) => Display::Inline,
                    (DisplayOutside::RunIn, _) => Display::Block,
                },
            };
        }
        Property::Custom(_) => {}
        Property::Hyphens(_, _) => {
            // Not supported yet.
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
                            style.border_collapse = match value {
                                "collapse" => BorderCollapseMode::Collapse,
                                "separate" => BorderCollapseMode::Separate,
                                _ => style.border_collapse,
                            };
                            return;
                        }
                    }
                    "empty-cells" => {
                        if let Some(value) = first_ident_token(&unparsed.value) {
                            style.empty_cells = match value {
                                "hide" => EmptyCellsMode::Hide,
                                "show" => EmptyCellsMode::Show,
                                _ => style.empty_cells,
                            };
                            return;
                        }
                    }
                    "caption-side" => {
                        if let Some(value) = first_ident_token(&unparsed.value) {
                            style.caption_side = match value {
                                "bottom" => CaptionSide::Bottom,
                                "top" => CaptionSide::Top,
                                _ => style.caption_side,
                            };
                            return;
                        }
                    }
                    _ => {}
                }
            }
            log_unhandled_property(property);
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

fn apply_text_decoration(style: &mut ComputedStyle, line: &lightningcss::properties::text::TextDecorationLine) {
    style.text_decoration = 0;
    if line.contains(lightningcss::properties::text::TextDecorationLine::Underline) {
        style.text_decoration |= 1;
    }
    if line.contains(lightningcss::properties::text::TextDecorationLine::Overline) {
        style.text_decoration |= 2;
    }
    if line.contains(lightningcss::properties::text::TextDecorationLine::LineThrough) {
        style.text_decoration |= 4;
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
        if let Some(keyword) = single_ident_keyword(value) {
            apply_custom_keyword(custom_properties, parent_custom, &name, keyword);
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

fn apply_custom_keyword<'a>(custom_properties: &mut FxHashMap<String, TokenList<'a>>, parent_custom: &FxHashMap<String, TokenList<'a>>, name: &str, keyword: &str) {
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
        _ => {}
    }
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
    for token in &tokens.0 {
        match token {
            TokenOrValue::Color(color) => {
                let mut printer = Printer::new(&mut out, PrinterOptions::default());
                color.to_css(&mut printer).ok()?;
            }
            TokenOrValue::Token(Token::Ident(ident)) => out.push_str(ident.as_ref()),
            TokenOrValue::Token(Token::Hash(hash)) => {
                out.push('#');
                out.push_str(hash.as_ref());
            }
            TokenOrValue::Token(Token::WhiteSpace(_)) => out.push(' '),
            _ => return None,
        }
    }
    Some(out)
}

fn first_ident_token<'a>(tokens: &'a TokenList<'a>) -> Option<&'a str> {
    for token in &tokens.0 {
        if let TokenOrValue::Token(Token::Ident(ident)) = token {
            return Some(ident.as_ref());
        }
    }
    None
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
        LengthValue::Pt(pt) => pt * 1.333333,
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

fn length_or_auto_to_px(value: &LengthPercentageOrAuto, font_size: f32, root_font_size: f32) -> f32 {
    match value {
        LengthPercentageOrAuto::Auto => 0.0,
        LengthPercentageOrAuto::LengthPercentage(lp) => length_percentage_to_px(lp, font_size, root_font_size),
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

fn spacing_to_px(spacing: &Spacing, font_size: f32, root_font_size: f32) -> f32 {
    use lightningcss::values::length::Length;
    match spacing {
        Spacing::Normal => 0.0,
        Spacing::Length(len) => match len {
            Length::Value(v) => length_to_px(v, font_size, root_font_size),
            Length::Calc(_) => 0.0,
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
        LineStyle::None => BorderStyle::None,
        LineStyle::Solid => BorderStyle::Solid,
        LineStyle::Dashed => BorderStyle::Dashed,
        LineStyle::Dotted => BorderStyle::Dotted,
        _ => BorderStyle::None,
    }
}

fn css_color_to_u32(color: &CssColor) -> u32 {
    match color {
        CssColor::RGBA(rgba) => ((rgba.red as u32) << 24) | ((rgba.green as u32) << 16) | ((rgba.blue as u32) << 8) | (rgba.alpha as u32),
        CssColor::CurrentColor => 0x000000FF,
        _ => 0x000000FF,
    }
}

fn intern_font_family(doc: &mut Document, families: &[FontFamily]) -> u16 {
    let mut out = String::new();
    for (i, family) in families.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        let _ = family.to_css(&mut Printer::new(&mut out, PrinterOptions::default()));
    }
    doc.intern_string(&out)
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
fn compute_style_for_dom_element<'a>(
    doc: &mut Document, node_idx: usize, index: &SelectorIndex, stylesheets: &[StyleSheet<'a, 'a>], parent_custom: Option<&FxHashMap<String, TokenList<'a>>>,
) -> (ComputedStyle, FxHashMap<String, TokenList<'a>>) {
    let mut matched_rules: Vec<MatchedRule> = Vec::new();

    // Get element info for candidate lookup
    let tag = doc.get_dom_tag(node_idx).unwrap_or("").to_string();
    let id = doc.get_dom_id(node_idx).map(|s| s.to_string());
    let classes: Option<String> = {
        let classes: Vec<_> = doc.get_dom_classes(node_idx).collect();
        if classes.is_empty() { None } else { Some(classes.join(" ")) }
    };

    // Build ancestor filter for fast rejection
    let ancestor_filter = build_ancestor_filter_dom(doc, node_idx);

    // Get candidate rule indices
    let candidates = index.get_candidates(tag.as_str(), id.as_deref(), classes.as_deref());

    // Check each candidate
    for key in candidates {
        if let Some(CssRule::Style(style_rule)) = stylesheet_rule(stylesheets, key) {
            for selector in &style_rule.selectors.0 {
                if !selector_might_match_dom(selector, &ancestor_filter, doc, node_idx) {
                    continue;
                }
                if selector_matches_dom(selector, doc, node_idx) {
                    matched_rules.push(MatchedRule { specificity: selector.specificity(), source_order: key.source_order, key });
                    break;
                }
            }
        }
    }

    // Sort by specificity, then source order (cascade)
    matched_rules.sort_by(|a, b| a.specificity.cmp(&b.specificity).then_with(|| a.source_order.cmp(&b.source_order)));

    // Build computed style from matched rules (same as box-based version)
    let mut style = get_inherited_style_dom(doc, node_idx);
    let parent_font_size = style.font_size;
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
    for matched in matched_rules.iter() {
        if let Some(CssRule::Style(style_rule)) = stylesheet_rule(stylesheets, matched.key) {
            collect_custom_properties(&mut custom_properties, &parent_custom, &style_rule.declarations.declarations);
        }
    }
    if let Some(inline_style) = &inline_style {
        collect_custom_properties(&mut custom_properties, &parent_custom, &inline_style.declarations.declarations);
    }
    if let Some(inline_style_attr) = &inline_style_attr {
        collect_inline_style_custom_properties(&mut custom_properties, &parent_custom, inline_style_attr);
    }
    for matched in matched_rules.iter() {
        if let Some(CssRule::Style(style_rule)) = stylesheet_rule(stylesheets, matched.key) {
            collect_custom_properties(&mut custom_properties, &parent_custom, &style_rule.declarations.important_declarations);
        }
    }
    if let Some(inline_style) = &inline_style {
        collect_custom_properties(&mut custom_properties, &parent_custom, &inline_style.declarations.important_declarations);
    }

    // Apply normal declarations
    for matched in matched_rules.iter() {
        if let Some(CssRule::Style(style_rule)) = stylesheet_rule(stylesheets, matched.key) {
            apply_declarations(doc, &mut style, &style_rule.declarations.declarations, parent_font_size, Some(&custom_properties));
        }
    }

    if let Some(inline_style) = &inline_style {
        apply_declarations(doc, &mut style, &inline_style.declarations.declarations, parent_font_size, Some(&custom_properties));
    }

    // Apply important declarations
    for matched in matched_rules.iter() {
        if let Some(CssRule::Style(style_rule)) = stylesheet_rule(stylesheets, matched.key) {
            apply_declarations(doc, &mut style, &style_rule.declarations.important_declarations, parent_font_size, Some(&custom_properties));
        }
    }

    if let Some(inline_style) = &inline_style {
        apply_declarations(doc, &mut style, &inline_style.declarations.important_declarations, parent_font_size, Some(&custom_properties));
    }

    (style, custom_properties)
}

fn stylesheet_rule<'s, 'a>(stylesheets: &'s [StyleSheet<'a, 'a>], key: RuleKey) -> Option<&'s CssRule<'a>> {
    stylesheets.get(key.stylesheet_idx)?.rules.0.get(key.rule_idx)
}

// Helper functions for DOM-based style resolution
use crate::style::selectors::AncestorFilter;

/// Get inherited style from parent DOM element
fn get_inherited_style_dom(doc: &Document, node_idx: usize) -> ComputedStyle {
    if let Some(parent_idx) = doc.get_dom_parent(node_idx) {
        if let Some(parent_style) = doc.dom_style_indices(parent_idx) {
            let font = &doc.font_style(parent_style.font_idx);
            let text = &doc.text_style(parent_style.text_idx);
            return ComputedStyle {
                // Inherited properties from Font
                font_size: font.font_size,
                font_weight: font.font_weight,
                font_style: font.font_style,
                font_family: font.font_family,
                font_variant_small_caps: font.font_variant_small_caps,
                // Inherited properties from InheritedText
                color: text.color,
                line_height: text.line_height,
                letter_spacing: text.letter_spacing,
                word_spacing: text.word_spacing,
                text_align: text.text_align,
                text_align_last: text.text_align_last,
                text_indent: text.text_indent,
                text_indent_hanging: text.text_indent_hanging,
                text_indent_each_line: text.text_indent_each_line,
                white_space: text.white_space,
                text_transform: text.text_transform,
                // Not inherited
                text_decoration: 0,
                vertical_align: VerticalAlignValue::Baseline,
                border_collapse: BorderCollapseMode::Separate,
                border_spacing_horizontal: 0.0,
                border_spacing_vertical: 0.0,
                empty_cells: EmptyCellsMode::Show,
                caption_side: CaptionSide::Top,
                // Not inherited - use defaults
                background_color: 0x00000000,
                width: PreferredSize::Auto,
                height: PreferredSize::Auto,
                min_width: PreferredSize::Auto,
                min_height: PreferredSize::Auto,
                max_width: PreferredSize::Auto,
                max_height: PreferredSize::Auto,
                margin_top: 0.0,
                margin_bottom: 0.0,
                margin_left: 0.0,
                margin_right: 0.0,
                padding_top: 0.0,
                padding_bottom: 0.0,
                padding_left: 0.0,
                padding_right: 0.0,
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
                display: Display::Block,
            };
        }
    }
    // No parent - use root_font_size as the base font size
    ComputedStyle { font_size: doc.root_font_size(), ..ComputedStyle::default() }
}

/// Build ancestor filter for DOM nodes (optimization)
fn build_ancestor_filter_dom(doc: &Document, node_idx: usize) -> AncestorFilter {
    let mut filter = AncestorFilter::new();

    for ancestor_idx in doc.dom_ancestors(node_idx) {
        if let Some(tag) = doc.get_dom_tag(ancestor_idx) {
            filter.insert(tag);
        }

        if let Some(id) = doc.get_dom_id(ancestor_idx) {
            filter.insert(id);
        }

        for class in doc.get_dom_classes(ancestor_idx) {
            filter.insert(class);
        }
    }

    filter
}

fn selector_might_match_dom(selector: &lightningcss::selector::Selector, ancestor_filter: &AncestorFilter, _doc: &Document, _node_idx: usize) -> bool {
    use crate::style::selectors_dom::selector_might_match_with_filter;
    selector_might_match_with_filter(selector, ancestor_filter)
}

fn selector_matches_dom(selector: &lightningcss::selector::Selector, doc: &Document, node_idx: usize) -> bool {
    use crate::style::selectors_dom::selector_matches_dom_node;
    selector_matches_dom_node(selector, doc, node_idx)
}

fn inline_style_attribute(doc: &Document, node_idx: usize) -> Option<String> {
    let style_attr = doc.get_dom_attr(node_idx, "style")?;
    if style_attr.trim().is_empty() {
        return None;
    }
    Some(style_attr.to_string())
}

fn parse_inline_style_attribute<'a>(doc: &Document, node_idx: usize) -> Option<StyleAttribute<'a>> {
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

        if let Some(keyword) = single_ident_keyword(&tokens) {
            apply_custom_keyword(custom_properties, parent_custom, name, keyword);
            continue;
        }

        custom_properties.insert(name.to_string(), tokens);
    }
}

#[cfg(test)]
mod tests {
    use crate::document::{Display, Document, ElementRef};
    use crate::parser::DocumentFactory;

    #[test]
    fn inline_style_attribute_overrides_computed_style() {
        let html = "<html><body><p style=\"display: inline; color: red;\">Hello</p></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, None);

        let p_idx = find_body(&document).children().next().expect("paragraph should be child of body");
        let style = document.dom_style_indices(p_idx).expect("paragraph style should exist");

        assert_eq!(document.box_model_style(style.box_idx).display, Display::Inline);
        assert_eq!(document.text_style(style.text_idx).color, 0xFF0000FF);
    }

    #[test]
    fn inline_custom_property_resolves_var_reference() {
        let html = "<html><body><p style=\"--accent: red; color: var(--accent);\">Hello</p></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, None);

        let p_idx = find_body(&document).children().next().expect("paragraph should be child of body");
        let style = document.dom_style_indices(p_idx).expect("paragraph style should exist");

        assert_eq!(document.text_style(style.text_idx).color, 0xFF0000FF);
    }

    #[test]
    fn invalid_author_css_does_not_drop_default_styles_or_boxes() {
        let html = "<html><body><p>Hello</p></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline_css_chunks(html, &["p { color: red"]);

        let p_idx = (0..document.box_count()).find(|&idx| document.get_tag(idx).eq_ignore_ascii_case("p")).expect("paragraph box should still be built");
        let style = document.box_at(p_idx).and_then(|box_| box_.style()).expect("paragraph box should have computed style");

        assert_eq!(document.box_model_style(style.box_idx).display, Display::Block);
    }

    #[test]
    fn later_css_chunks_win_for_equal_specificity() {
        let html = "<html><body><p>Hello</p></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline_css_chunks(html, &["p { color: red; }", "p { color: blue; }"]);

        let p_idx = find_body(&document).children().next().expect("paragraph should be child of body");
        let style = document.dom_style_indices(p_idx).expect("paragraph style should exist");

        assert_eq!(document.text_style(style.text_idx).color, 0x0000FFFF);
    }

    fn find_body(document: &Document) -> ElementRef<'_> {
        let html_idx = document.dom_root().expect("html root should exist");
        let html = document.element_ref(html_idx).expect("html should be element");
        let body_idx = html.children().find(|&child_idx| document.element_ref(child_idx).is_some_and(|element| element.tag() == "body")).expect("body should be child of html");
        document.element_ref(body_idx).expect("body should be element")
    }
}
