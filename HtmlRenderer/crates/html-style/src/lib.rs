#[cfg(test)]
pub(crate) mod document {
    pub use html_dom::*;
    pub use html_style_model::*;
}

#[cfg(test)]
pub(crate) mod parser {
    pub(crate) struct DocumentFactory;

    impl DocumentFactory {
        pub(crate) fn new() -> Self {
            Self
        }

        pub(crate) fn parse_with_new_pipeline(&mut self, html: &str, css: Option<&str>) -> crate::StyledDocument {
            match css {
                Some(css) => self.parse_with_new_pipeline_css_chunks(html, &[css]),
                None => self.parse_with_new_pipeline_css_chunks(html, &[]),
            }
        }

        pub(crate) fn parse_with_new_pipeline_css_chunks(&mut self, html: &str, css: &[&str]) -> crate::StyledDocument {
            let document = html_parse::parse_dom_document(html).expect("parser produced a valid DOM");
            crate::style_document(document, css)
        }
    }
}

#[path = "style/mod.rs"]
mod style;

pub use style::DEFAULT_CSS;
pub use style::capabilities::property_name_is_supported;
pub use style::imports::{ImportLayer, StylesheetEntry, StylesheetImport, stylesheet_entries};

use html_dom::{Document, DomNodeId};
use html_style_model::{Background, Border, BoxModel, ComputedStyles, Font, InheritedText, StyleIndices, StyleView};
use lightningcss::properties::{Property, PropertyId};
use lightningcss::selector::SelectorList;
use lightningcss::stylesheet::{ParserOptions, StyleSheet};
use lightningcss::traits::ParseWithOptions;
use std::time::{Duration, Instant};

#[cfg(test)]
mod allocation_test_support {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    thread_local! {
        static ALLOCATION_COUNT: Cell<Option<usize>> = const { Cell::new(None) };
    }

    pub(crate) struct TrackingAllocator;

    fn record_allocation() {
        let _ = ALLOCATION_COUNT.try_with(|count| {
            if let Some(current) = count.get() {
                count.set(Some(current + 1));
            }
        });
    }

    unsafe impl GlobalAlloc for TrackingAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            record_allocation();
            unsafe { System.alloc(layout) }
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            record_allocation();
            unsafe { System.alloc_zeroed(layout) }
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            record_allocation();
            unsafe { System.realloc(ptr, layout, new_size) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    pub(crate) fn count_allocations<T>(operation: impl FnOnce() -> T) -> (T, usize) {
        ALLOCATION_COUNT.with(|count| {
            assert!(count.get().is_none(), "allocation counters cannot be nested");
            count.set(Some(0));
        });
        let output = operation();
        let allocations = ALLOCATION_COUNT.with(|count| count.take().expect("allocation tracking is active"));
        (output, allocations)
    }
}

#[cfg(test)]
#[global_allocator]
static TEST_ALLOCATOR: allocation_test_support::TrackingAllocator = allocation_test_support::TrackingAllocator;

/// Result of checking a declaration against the CSS parser owned by this
/// rendering stage. Unknown properties are kept distinct from invalid values
/// so conformance adapters cannot accidentally count them as passes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyValueSyntax {
    Valid,
    Invalid,
    UnsupportedProperty,
}

/// Whether a declaration's value is valid for its property, independent of
/// whether this renderer can compute that property.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertySyntax {
    Valid,
    Invalid,
}

/// Whether the style stage implements the property's computed behavior.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyCapability {
    Supported,
    Unsupported,
}

/// The two independent answers needed by feature queries and conformance
/// adapters. Valid CSS is not automatically a supported renderer feature.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeclarationSupport {
    pub syntax: PropertySyntax,
    pub capability: PropertyCapability,
}

pub fn declaration_support(property_name: &str, value: &str) -> DeclarationSupport {
    let normalized_name = property_name.to_ascii_lowercase();
    let syntax = declaration_syntax_impl(&normalized_name, value);
    let capability = if style::capabilities::property_name_is_supported(&normalized_name) { PropertyCapability::Supported } else { PropertyCapability::Unsupported };
    DeclarationSupport { syntax, capability }
}

pub fn declaration_syntax(property_name: &str, value: &str) -> PropertySyntax {
    declaration_syntax_impl(&property_name.to_ascii_lowercase(), value)
}

pub fn property_value_syntax(property_name: &str, value: &str) -> PropertyValueSyntax {
    let normalized_name = property_name.to_ascii_lowercase();
    let property_id = PropertyId::from(normalized_name.as_str());
    if matches!(property_id, PropertyId::Custom(_)) && !normalized_name.starts_with("--") && !style::capabilities::is_supported_property_name(&normalized_name) {
        return PropertyValueSyntax::UnsupportedProperty;
    }
    match declaration_syntax_impl(&normalized_name, value) {
        PropertySyntax::Valid => PropertyValueSyntax::Valid,
        PropertySyntax::Invalid => PropertyValueSyntax::Invalid,
    }
}

fn declaration_syntax_impl(normalized_name: &str, value: &str) -> PropertySyntax {
    if normalized_name == "white-space" && style::white_space::parse_shorthand(value).is_some() {
        return PropertySyntax::Valid;
    }
    if matches!(normalized_name, "letter-spacing" | "word-spacing") {
        let normalized_value = value.trim().to_ascii_lowercase();
        return if style::text_spacing::parse(value).is_some() || matches!(normalized_value.as_str(), "inherit" | "initial" | "unset" | "revert" | "revert-layer") || value.contains("var(") || value.contains("env(") {
            PropertySyntax::Valid
        } else {
            PropertySyntax::Invalid
        };
    }
    if normalized_name == "tab-size" {
        let normalized_value = value.trim().to_ascii_lowercase();
        return if style::tab_size::parse(value).is_some() || matches!(normalized_value.as_str(), "inherit" | "initial" | "unset" | "revert" | "revert-layer") || value.contains("var(") || value.contains("env(") {
            PropertySyntax::Valid
        } else {
            PropertySyntax::Invalid
        };
    }
    if let Some(valid) = style::box_syntax::property_value_is_valid(normalized_name, value) {
        return if valid { PropertySyntax::Valid } else { PropertySyntax::Invalid };
    }
    let property_id = PropertyId::from(normalized_name);
    let unknown_property = matches!(property_id, PropertyId::Custom(_)) && !normalized_name.starts_with("--") && !style::capabilities::is_supported_property_name(normalized_name);

    match Property::parse_string(property_id, value, ParserOptions::default()) {
        // Lightning CSS deliberately preserves values it cannot validate as
        // `Unparsed`. That is useful to a transforming stylesheet pipeline,
        // but this API is a syntax validator, so it must not turn that
        // recovery representation into a successful parse.
        Ok(Property::Unparsed(_)) if unknown_property || value.contains("var(") || value.contains("env(") => PropertySyntax::Valid,
        Ok(Property::Unparsed(_)) => PropertySyntax::Invalid,
        Ok(_) => PropertySyntax::Valid,
        Err(_) => PropertySyntax::Invalid,
    }
}

/// Parse selector syntax without exposing Lightning CSS types across the
/// style-stage boundary.
pub fn selector_syntax_is_valid(selector: &str) -> bool {
    SelectorList::parse_string_with_options(selector, ParserOptions::default()).is_ok_and(|selectors| style::selectors::selector_list_is_web_valid(&selectors))
}

/// Parse a complete stylesheet/rule string without exposing the parser's AST.
pub fn stylesheet_syntax_is_valid(css: &str) -> bool {
    StyleSheet::parse(css, ParserOptions::default()).is_ok()
}

/// A DOM whose cascade has been resolved.
///
/// The wrapped document is deliberately immutable. Changing the DOM or its
/// style inputs requires consuming this value with [`Self::into_unstyled`],
/// which prevents layout data from silently outliving the styles it used.
pub struct StyledDocument {
    document: Document,
    styles: ComputedStyles,
}

impl StyledDocument {
    pub fn document(&self) -> &Document {
        &self.document
    }

    pub fn styles(&self) -> &ComputedStyles {
        &self.styles
    }

    pub fn style_for_node(&self, node_idx: DomNodeId) -> Option<StyleIndices> {
        self.styles.style_for_node(node_idx)
    }

    pub fn style_view(&self, indices: StyleIndices) -> Option<StyleView<'_>> {
        self.styles.view(indices)
    }

    pub fn font_style(&self, indices: StyleIndices) -> Option<&Font> {
        self.styles.font_style(indices)
    }

    pub fn text_style(&self, indices: StyleIndices) -> Option<&InheritedText> {
        self.styles.text_style(indices)
    }

    pub fn box_model_style(&self, indices: StyleIndices) -> Option<&BoxModel> {
        self.styles.box_model_style(indices)
    }

    pub fn border_style(&self, indices: StyleIndices) -> Option<&Border> {
        self.styles.border_style(indices)
    }

    pub fn background_style(&self, indices: StyleIndices) -> Option<&Background> {
        self.styles.background_style(indices)
    }

    pub fn into_unstyled(self) -> Document {
        self.document
    }

    pub fn into_parts(self) -> (Document, ComputedStyles) {
        (self.document, self.styles)
    }
}

/// Resolve the user-agent stylesheet and all author stylesheet chunks.
///
/// Invalid author chunks are skipped, matching the renderer's previous
/// best-effort behavior. Lightning CSS values remain an implementation detail
/// of this crate rather than leaking into the pipeline API.
pub fn style_document<'css>(document: Document, css_chunks: &[&'css str]) -> StyledDocument {
    style_document_with_timings(document, css_chunks).0
}

#[derive(Clone, Debug, Default)]
pub struct StyleTimings {
    pub parse_default_css: Duration,
    pub parse_author_css: Duration,
    pub prepare_rules: Duration,
    pub resolve_styles: Duration,
    pub selector_index: Duration,
    pub resolver_setup: Duration,
    pub selector_matching: Duration,
    pub cascade: Duration,
    pub style_store: Duration,
}

pub fn style_document_with_timings<'css>(document: Document, css_chunks: &[&'css str]) -> (StyledDocument, StyleTimings) {
    let mut timings = StyleTimings::default();
    let normalized_chunks = css_chunks.iter().map(|css| style::declarations::normalize(css)).collect::<Vec<_>>();
    let mut stylesheets = Vec::with_capacity(css_chunks.len() + 1);
    let started = Instant::now();
    stylesheets.push(StyleSheet::parse(DEFAULT_CSS, ParserOptions::default()).expect("default CSS must parse"));
    timings.parse_default_css = started.elapsed();

    let started = Instant::now();
    for (idx, css) in normalized_chunks.iter().enumerate() {
        if css.trim().is_empty() {
            continue;
        }
        let options = ParserOptions { error_recovery: true, ..ParserOptions::default() };
        match StyleSheet::parse(css, options) {
            Ok(stylesheet) => stylesheets.push(stylesheet),
            Err(error) => eprintln!("Skipping CSS chunk {idx}: {error}"),
        }
    }
    timings.parse_author_css = started.elapsed();

    let started = Instant::now();
    let (user_agent, authors) = stylesheets.split_first().expect("the built-in user-agent stylesheet is always present");
    let prepared = style::prepared::ParsedStylesheetSet::new(user_agent, authors).prepare();
    timings.prepare_rules = started.elapsed();

    let started = Instant::now();
    let (styles, resolution) = style::resolver::resolve_styles_for_dom_timed(&document, &prepared);
    timings.resolve_styles = started.elapsed();
    timings.selector_index = resolution.selector_index;
    timings.resolver_setup = resolution.resolver_setup;
    timings.selector_matching = resolution.selector_matching;
    timings.cascade = resolution.cascade;
    timings.style_store = resolution.style_store;
    (StyledDocument { document, styles }, timings)
}

#[cfg(test)]
mod boundary_tests {
    use super::{DeclarationSupport, PropertyCapability, PropertySyntax, PropertyValueSyntax, declaration_support, property_value_syntax, selector_syntax_is_valid, style_document};
    use html_style_model::{Float, LengthPct, PreferredSize, TabSizeKind, WhiteSpace};

    #[test]
    fn property_syntax_keeps_invalid_and_unsupported_states_distinct() {
        assert_eq!(property_value_syntax("width", "10px"), PropertyValueSyntax::Valid);
        assert_eq!(property_value_syntax("width", "10px 20px"), PropertyValueSyntax::Invalid);
        assert_eq!(property_value_syntax("width", "var(--measure)"), PropertyValueSyntax::Valid);
        assert_eq!(property_value_syntax("not-a-renderer-property", "10px"), PropertyValueSyntax::UnsupportedProperty);
        assert_eq!(property_value_syntax("letter-spacing", "120%"), PropertyValueSyntax::Valid);
        assert_eq!(property_value_syntax("word-spacing", "calc(2ch - 30%)"), PropertyValueSyntax::Valid);
        assert_eq!(property_value_syntax("letter-spacing", "20"), PropertyValueSyntax::Invalid);
        assert_eq!(property_value_syntax("tab-size", "4"), PropertyValueSyntax::Valid);
        assert_eq!(property_value_syntax("tab-size", "2.5ch"), PropertyValueSyntax::Valid);
        assert_eq!(property_value_syntax("tab-size", "-20"), PropertyValueSyntax::Invalid);
        assert_eq!(property_value_syntax("tab-size", "-10px"), PropertyValueSyntax::Invalid);
        assert_eq!(property_value_syntax("tab-size", "20%"), PropertyValueSyntax::Invalid);
    }

    #[test]
    fn valid_syntax_and_renderer_capability_are_independent() {
        assert_eq!(declaration_support("width", "10px"), DeclarationSupport { syntax: PropertySyntax::Valid, capability: PropertyCapability::Supported });
        assert_eq!(declaration_support("width", "red"), DeclarationSupport { syntax: PropertySyntax::Invalid, capability: PropertyCapability::Supported });
        assert_eq!(declaration_support("opacity", "0.5"), DeclarationSupport { syntax: PropertySyntax::Valid, capability: PropertyCapability::Unsupported });
        assert_eq!(declaration_support("future-property", "some-value"), DeclarationSupport { syntax: PropertySyntax::Valid, capability: PropertyCapability::Unsupported });
    }

    #[test]
    fn strict_selector_validation_rejects_custom_recovery_nodes() {
        assert!(selector_syntax_is_valid("p:not(.excluded)"));
        assert!(selector_syntax_is_valid(":heading(1, 2)"));
        assert!(!selector_syntax_is_valid(":heading(2n)"));
        assert!(!selector_syntax_is_valid("p:not(:unknown)"));
        assert!(!selector_syntax_is_valid(".a:has(.b:has(.c))"));
        assert!(!selector_syntax_is_valid("p, :unknown"));
    }

    #[test]
    fn invalid_selector_lists_cannot_leak_declarations_into_the_cascade() {
        let styled = styled_paragraph("p { color: green; } p, :unknown { color: red; } p:not(:unknown) { color: red; }");
        assert_eq!(paragraph_color(&styled), 0x008000FF);
    }

    #[test]
    fn a_bad_author_rule_does_not_discard_the_whole_stylesheet_chunk() {
        let styled = styled_paragraph("p { color: green; } ::part(foo):lang(en) { color: red; }");
        assert_eq!(paragraph_color(&styled), 0x008000FF);
    }

    #[test]
    fn conditional_rules_are_evaluated_once_during_preparation() {
        let styled = styled_paragraph("p { color: black } @media all { p { color: red } } @media print { p { color: blue } } @supports (width: 10px) { p { color: green } } @supports (opacity: 0.5) { p { color: blue } }");
        assert_eq!(paragraph_color(&styled), 0x008000FF);
    }

    #[test]
    fn normal_layer_order_and_unlayered_precedence_are_honored() {
        let styled = styled_paragraph("@layer first, second; @layer second { p { color: red } } @layer first { p { color: blue } } p { color: green }");
        assert_eq!(paragraph_color(&styled), 0x008000FF);
    }

    #[test]
    fn important_layer_order_is_reversed() {
        let styled = styled_paragraph("@layer first, second; @layer second { p { color: red !important } } @layer first { p { color: green !important } } p { color: blue !important }");
        assert_eq!(paragraph_color(&styled), 0x008000FF);
    }

    #[test]
    fn declarations_directly_in_a_layer_outrank_its_nested_layers() {
        let styled = styled_paragraph("@layer outer { p { color: green } @layer inner { p { color: red } } }");
        assert_eq!(paragraph_color(&styled), 0x008000FF);
    }

    #[test]
    fn css_text_four_white_space_reaches_the_computed_style() {
        let styled = styled_paragraph("p { white-space: preserve-breaks nowrap; }");
        assert_eq!(paragraph_white_space(&styled), WhiteSpace::PreserveBreaksNoWrap);
    }

    #[test]
    fn later_white_space_declaration_resets_the_compatibility_state() {
        let styled = styled_paragraph("p { white-space: preserve-breaks nowrap; white-space: normal; }");
        assert_eq!(paragraph_white_space(&styled), WhiteSpace::Normal);
    }

    #[test]
    fn important_white_space_declaration_wins_through_the_compatibility_layer() {
        let styled = styled_paragraph("p { white-space: preserve-breaks nowrap !important; white-space: normal; }");
        assert_eq!(paragraph_white_space(&styled), WhiteSpace::PreserveBreaksNoWrap);
    }

    #[test]
    fn inline_css_text_four_white_space_reaches_the_computed_style() {
        let document = html_parse::parse_dom_document("<html><body><p style='white-space: break-spaces nowrap'>text</p></body></html>").expect("valid HTML");
        let styled = style_document(document, &[]);
        assert_eq!(paragraph_white_space(&styled), WhiteSpace::BreakSpacesNoWrap);
    }

    #[test]
    fn percentage_text_spacing_reaches_layout_as_font_relative_length() {
        let styled = styled_paragraph("p { font-size: 20px; letter-spacing: 120%; word-spacing: calc(50% + 2px); }");
        let paragraph = paragraph_node(&styled);
        let indices = styled.style_for_node(paragraph).expect("paragraph style");
        let text = styled.text_style(indices).expect("validated text style");
        let view = styled.style_view(indices).expect("validated style view");

        assert_eq!(text.letter_spacing.absolute_px(), 0.0);
        assert_eq!(text.letter_spacing.font_size_fraction(), 1.2);
        assert!((view.letter_spacing() - 24.0).abs() < 0.001);
        assert_eq!(text.word_spacing.absolute_px(), 2.0);
        assert_eq!(text.word_spacing.font_size_fraction(), 0.5);
        assert!((view.word_spacing() - 12.0).abs() < 0.001);
    }

    #[test]
    fn inherited_spacing_keeps_percent_relative_but_freezes_em_as_pixels() {
        let document = html_parse::parse_dom_document("<html><body><p><span>text</span></p></body></html>").expect("valid HTML");
        let styled = style_document(document, &["p { font-size: 20px; letter-spacing: calc(2em + 50%); } span { font-size: 10px; }"]);
        let span = styled.document().nodes().find_map(|(id, node)| matches!(node, html_dom::NodeRef::Element(element) if element.tag() == "span").then_some(id)).expect("span node");
        let indices = styled.style_for_node(span).expect("span style");
        let text = styled.text_style(indices).expect("validated text style");
        let view = styled.style_view(indices).expect("validated style view");

        assert_eq!(text.letter_spacing.absolute_px(), 40.0);
        assert_eq!(text.letter_spacing.font_size_fraction(), 0.5);
        assert!((view.letter_spacing() - 45.0).abs() < 0.001);
    }

    #[test]
    fn native_spacing_after_percentage_clears_internal_cascade_state() {
        let styled = styled_paragraph("p { font-size: 20px; letter-spacing: 120%; letter-spacing: 2em; }");
        let paragraph = paragraph_node(&styled);
        let indices = styled.style_for_node(paragraph).expect("paragraph style");
        let text = styled.text_style(indices).expect("validated text style");

        assert_eq!(text.letter_spacing.absolute_px(), 40.0);
        assert_eq!(text.letter_spacing.font_size_fraction(), 0.0);
    }

    #[test]
    fn tab_size_cascades_as_a_typed_inherited_value() {
        let document = html_parse::parse_dom_document("<html><body><p><span>text</span></p></body></html>").expect("valid HTML");
        let styled = style_document(document, &["p { tab-size: 2.5ch; } span { font-size: 40px; }"]);
        let span = styled.document().nodes().find_map(|(id, node)| matches!(node, html_dom::NodeRef::Element(element) if element.tag() == "span").then_some(id)).expect("span node");
        let indices = styled.style_for_node(span).expect("span style");
        let tab_size = styled.text_style(indices).expect("validated text style").tab_size;

        assert_eq!(tab_size.kind(), TabSizeKind::LengthPx);
        assert!((tab_size.value() - 20.0).abs() < 0.001, "inherited em/ch length must freeze at the declaring element's font size");
    }

    #[test]
    fn invalid_tab_size_cannot_override_a_prior_valid_declaration() {
        let styled = styled_paragraph("p { tab-size: 3; tab-size: -2; }");
        let indices = styled.style_for_node(paragraph_node(&styled)).expect("paragraph style");
        let tab_size = styled.text_style(indices).expect("validated text style").tab_size;
        assert_eq!(tab_size.kind(), TabSizeKind::Spaces);
        assert_eq!(tab_size.value(), 3.0);
    }

    #[test]
    fn computed_negative_calc_tab_length_clamps_to_zero() {
        let styled = styled_paragraph("p { font-size: 20px; tab-size: calc(5px - 0.5em); }");
        let indices = styled.style_for_node(paragraph_node(&styled)).expect("paragraph style");
        let tab_size = styled.text_style(indices).expect("validated text style").tab_size;
        assert_eq!(tab_size.kind(), TabSizeKind::LengthPx);
        assert_eq!(tab_size.value(), 0.0);
    }

    #[test]
    fn computed_number_calc_keeps_number_semantics() {
        let styled = styled_paragraph("p { tab-size: calc(2 + 3); }");
        let indices = styled.style_for_node(paragraph_node(&styled)).expect("paragraph style");
        let tab_size = styled.text_style(indices).expect("validated text style").tab_size;
        assert_eq!(tab_size.kind(), TabSizeKind::Spaces);
        assert_eq!(tab_size.value(), 5.0);
    }

    #[test]
    fn tab_size_marker_preserves_important_and_initial_cascade_semantics() {
        let document = html_parse::parse_dom_document("<html><body><p><span>text</span></p></body></html>").expect("valid HTML");
        let styled = style_document(document, &["p { tab-size: 2 !important; tab-size: 4; } span { tab-size: initial; }"]);
        let paragraph = paragraph_node(&styled);
        let paragraph_tab = styled.text_style(styled.style_for_node(paragraph).expect("paragraph style")).expect("paragraph text style").tab_size;
        let span = styled.document().nodes().find_map(|(id, node)| matches!(node, html_dom::NodeRef::Element(element) if element.tag() == "span").then_some(id)).expect("span node");
        let span_tab = styled.text_style(styled.style_for_node(span).expect("span style")).expect("span text style").tab_size;

        assert_eq!(paragraph_tab.value(), 2.0);
        assert_eq!(span_tab, html_style_model::TabSize::DEFAULT);
    }

    #[test]
    fn invalid_box_declarations_cannot_override_prior_valid_values() {
        let styled = styled_paragraph("p { float: inline-start; float: left right; width: 50px; width: -10px; padding-left: 12px; padding-left: auto; }");
        let paragraph = paragraph_node(&styled);
        let indices = styled.style_for_node(paragraph).expect("paragraph style");
        let box_model = styled.box_model_style(indices).expect("validated box style");

        assert_eq!(box_model.float, Float::Left);
        assert_eq!(box_model.width, PreferredSize::Px(50.0));
        assert_eq!(box_model.padding_left, LengthPct::Px(12.0));
    }

    #[test]
    fn finished_styles_are_dense_and_drop_construction_indexes() {
        let document = html_parse::parse_dom_document("<html><body><p class='same'>one</p><p class='same'>two</p></body></html>").expect("valid HTML");
        let styled = style_document(document, &[".same { color: red; margin-left: 1em; }"]);

        assert_eq!(styled.styles().node_count(), styled.document().node_count());
        let report = styled.styles().memory_usage_report();
        for entry in report.entries.iter().filter(|entry| entry.label.contains("_dedup.storage")) {
            assert_eq!(entry.count, 0, "{} retained construction entries", entry.label);
            assert_eq!(entry.bytes, 0, "{} retained construction capacity", entry.label);
        }
    }

    #[test]
    fn css_strings_belong_to_style_output_not_the_parsed_dom() {
        let document = html_parse::parse_dom_document("<html><body><p>text</p></body></html>").expect("valid HTML");
        assert!(document.lookup_string("Boundary Family").is_none());

        let styled = style_document(document, &["p { font-family: 'Boundary Family'; }"]);
        let paragraph = styled.document().nodes().find_map(|(id, node)| matches!(node, html_dom::NodeRef::Element(element) if element.tag() == "p").then_some(id)).expect("paragraph node");
        let indices = styled.style_for_node(paragraph).expect("paragraph style");
        let family = styled.font_style(indices).expect("validated style handle").font_family.expect("authored family");

        assert!(styled.document().lookup_string("Boundary Family").is_none());
        assert!(styled.styles().string(family).expect("style-owned family").contains("Boundary Family"));
    }

    fn styled_paragraph(css: &str) -> super::StyledDocument {
        let document = html_parse::parse_dom_document("<html><body><p>text</p></body></html>").expect("valid HTML");
        style_document(document, &[css])
    }

    fn paragraph_color(styled: &super::StyledDocument) -> u32 {
        let paragraph = paragraph_node(styled);
        let indices = styled.style_for_node(paragraph).expect("paragraph style");
        styled.text_style(indices).expect("validated text style").color
    }

    fn paragraph_white_space(styled: &super::StyledDocument) -> WhiteSpace {
        let paragraph = paragraph_node(styled);
        let indices = styled.style_for_node(paragraph).expect("paragraph style");
        styled.text_style(indices).expect("validated text style").white_space
    }

    fn paragraph_node(styled: &super::StyledDocument) -> html_dom::DomNodeId {
        styled.document().nodes().find_map(|(id, node)| matches!(node, html_dom::NodeRef::Element(element) if element.tag() == "p").then_some(id)).expect("paragraph node")
    }
}
