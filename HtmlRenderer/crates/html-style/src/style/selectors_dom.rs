use crate::style::selectors::AncestorFilter;
use html_dom::{Document, DomNodeId};
use lightningcss::selector::{Combinator, Component, PseudoClass, Selector};
use parcel_selectors::attr::{NamespaceConstraint, ParsedAttrSelectorOperation, ParsedCaseSensitivity};
use parcel_selectors::parser::{NthSelectorData, NthType};

// ============================================================================
// DOM-Based Selector Matching
// ============================================================================

/// Match a selector against a DOM node in the Document
pub fn selector_matches_dom_node(selector: &Selector, doc: &Document, node_idx: DomNodeId) -> bool {
    match_from_position_dom(selector.iter_raw_match_order().as_slice(), doc, node_idx)
}

/// Match selector components from a position against a DOM node
fn match_from_position_dom<'i>(components: &[Component<'i>], doc: &Document, node_idx: DomNodeId) -> bool {
    let mut pos = 0;

    // Match components in current sequence
    while pos < components.len() {
        match &components[pos] {
            Component::Combinator(combinator) => {
                // Found combinator, recurse with appropriate ancestor/sibling
                let remaining = &components[pos + 1..];
                return match combinator {
                    Combinator::Child => doc.element_ref(node_idx).and_then(|element| element.parent()).map(|parent_idx| match_from_position_dom(remaining, doc, parent_idx)).unwrap_or(false),
                    Combinator::Descendant => {
                        for ancestor_idx in doc.dom_ancestors(node_idx) {
                            if match_from_position_dom(remaining, doc, ancestor_idx) {
                                return true;
                            }
                        }
                        false
                    }
                    Combinator::NextSibling => doc.element_ref(node_idx).and_then(|element| element.previous_element_sibling()).map(|sibling| match_from_position_dom(remaining, doc, sibling.node_id())).unwrap_or(false),
                    Combinator::LaterSibling => {
                        let mut current = doc.element_ref(node_idx).and_then(|element| element.previous_element_sibling());
                        while let Some(sibling) = current {
                            if match_from_position_dom(remaining, doc, sibling.node_id()) {
                                return true;
                            }
                            current = sibling.previous_element_sibling();
                        }
                        false
                    }
                    _ => false,
                };
            }
            component => {
                if !component_matches_dom(component, doc, node_idx) {
                    return false;
                }
                pos += 1;
            }
        }
    }

    true // All components matched
}

/// Check if a single selector component matches a DOM node
fn component_matches_dom(component: &Component, doc: &Document, node_idx: DomNodeId) -> bool {
    let Some(element) = doc.element_ref(node_idx) else {
        return false;
    };
    match component {
        Component::ExplicitUniversalType => true,

        Component::LocalName(name) => element.tag() == name.lower_name.0.as_ref(),

        Component::ID(id) => element.id().is_some_and(|node_id| node_id == id.0.as_ref()),

        Component::Class(class_selector) => element.has_class(class_selector.0.as_ref()),

        Component::AttributeInNoNamespaceExists { local_name, local_name_lower } => {
            let attr_name = local_name_string(local_name, local_name_lower, element.is_html_element_in_html_document());
            element.has_attr(attr_name)
        }

        Component::AttributeInNoNamespace { local_name, operator, value, case_sensitivity, .. } => {
            let attr_name = local_name_string(local_name, local_name, element.is_html_element_in_html_document());
            let Some(attr_value) = element.attr(attr_name) else {
                return false;
            };

            operator.eval_str(attr_value, value.as_ref(), parsed_case_sensitivity(*case_sensitivity, element.is_html_element_in_html_document()))
        }

        Component::AttributeOther(other) => {
            let local_name = other.local_name.as_ref();
            match &other.operation {
                ParsedAttrSelectorOperation::Exists => attribute_exists_in_namespace(element, namespace_match(other.namespace()), local_name),
                ParsedAttrSelectorOperation::WithValue { operator, case_sensitivity, expected_value } => {
                    let Some(attr_value) = get_attribute_in_namespace(element, namespace_match(other.namespace()), local_name) else {
                        return false;
                    };

                    operator.eval_str(attr_value, expected_value.as_ref(), parsed_case_sensitivity(*case_sensitivity, element.is_html_element_in_html_document()))
                }
            }
        }

        Component::NonTSPseudoClass(pseudo) => pseudo_matches_dom(pseudo, element),
        Component::PseudoElement(_) => false,

        Component::Root => doc.dom_root() == Some(node_idx),
        // Selectors L3 `:empty`: no children at all (text nodes count as content).
        Component::Empty => element.children().next().is_none(),
        Component::Nth(nth) => nth_matches_dom(nth, doc, element),

        // Forgiving (`:is()`) and non-specificity (`:where()`) matching: the
        // node matches if any inner selector matches it. The UA stylesheet uses
        // these for nested-list markers and margins.
        Component::Is(selectors) | Component::Where(selectors) => selectors.iter().any(|selector| selector_matches_dom_node(selector, doc, node_idx)),
        // `:not()` matches when none of the inner selectors match.
        Component::Negation(selectors) => !selectors.iter().any(|selector| selector_matches_dom_node(selector, doc, node_idx)),

        _ => false,
    }
}

/// Match the tree-structural `:first-child`/`:last-child`/`:only-child` and
/// `:nth-*` selectors (all are `Component::Nth` with an `an+b` formula).
fn nth_matches_dom(nth: &NthSelectorData, doc: &Document, element: html_dom::ElementRef<'_>) -> bool {
    if matches!(nth.ty, NthType::Col | NthType::LastCol) {
        return false;
    }
    let of_type = matches!(nth.ty, NthType::OfType | NthType::LastOfType | NthType::OnlyOfType);
    let from_end = matches!(nth.ty, NthType::LastChild | NthType::LastOfType);

    let (sibling_count, position) = match element.parent().and_then(|parent_idx| doc.element_ref(parent_idx)) {
        Some(parent) => {
            let mut sibling_count = 0;
            let mut position = None;
            for child_idx in parent.children() {
                if sibling_counts(doc, child_idx, of_type, element.tag()) {
                    sibling_count += 1;
                    if child_idx == element.node_id() {
                        position = Some(sibling_count);
                    }
                }
            }
            (sibling_count, position)
        }
        // The root element is its parent's only element child.
        None => (1, Some(1)),
    };
    let Some(position) = position else {
        return false;
    };

    if nth.ty.is_only() {
        return sibling_count == 1;
    }
    let index = if from_end { sibling_count - position + 1 } else { position } as i32;
    nth_index_matches(nth.a, nth.b, index)
}

fn sibling_counts(doc: &Document, child_idx: DomNodeId, of_type: bool, tag: &str) -> bool {
    match doc.element_ref(child_idx) {
        Some(child) => !of_type || child.tag() == tag,
        None => false,
    }
}

/// True if the 1-based `index` is in the set `{ a*k + b | integer k >= 0 }`.
fn nth_index_matches(a: i32, b: i32, index: i32) -> bool {
    if a == 0 {
        return index == b;
    }
    let diff = index - b;
    diff % a == 0 && diff / a >= 0
}

fn local_name_string<'a>(name: &'a impl AsRef<str>, lower_name: &'a impl AsRef<str>, is_html_element_in_html_document: bool) -> &'a str {
    let name = name.as_ref();
    let lower_name = lower_name.as_ref();
    if is_html_element_in_html_document || name == lower_name { lower_name } else { name }
}

fn parsed_case_sensitivity(parsed: ParsedCaseSensitivity, is_html_element_in_html_document: bool) -> parcel_selectors::attr::CaseSensitivity {
    parsed.to_unconditional(is_html_element_in_html_document)
}

#[derive(Clone, Copy)]
enum NamespaceMatch<'a> {
    None,
    Any,
    Specific(&'a str),
}

fn namespace_match<'a>(namespace: Option<NamespaceConstraint<&'a lightningcss::values::string::CowArcStr<'a>>>) -> NamespaceMatch<'a> {
    match namespace {
        None => NamespaceMatch::None,
        Some(NamespaceConstraint::Any) => NamespaceMatch::Any,
        Some(NamespaceConstraint::Specific(url)) if url.as_ref().is_empty() => NamespaceMatch::None,
        Some(NamespaceConstraint::Specific(url)) => NamespaceMatch::Specific(url.as_ref()),
    }
}

fn get_attribute_in_namespace<'a>(element: html_dom::ElementRef<'a>, namespace: NamespaceMatch<'_>, local_name: &str) -> Option<&'a str> {
    match namespace {
        NamespaceMatch::None => element.attr(local_name),
        NamespaceMatch::Any => element.attr_in_any_namespace(local_name),
        NamespaceMatch::Specific(url) => element.attr_expanded(Some(url), local_name),
    }
}

fn attribute_exists_in_namespace(element: html_dom::ElementRef<'_>, namespace: NamespaceMatch<'_>, local_name: &str) -> bool {
    get_attribute_in_namespace(element, namespace, local_name).is_some()
}

/// Check if pseudo-class matches DOM node
fn pseudo_matches_dom(pseudo: &PseudoClass, element: html_dom::ElementRef<'_>) -> bool {
    match pseudo {
        PseudoClass::Link | PseudoClass::AnyLink(_) => element.href().is_some(),
        PseudoClass::Visited => false, // Don't match visited for privacy
        _ => false,
    }
}

// ============================================================================
// Bloom Filter Optimization for Fast Selector Rejection
// ============================================================================

/// Fast pre-check using bloom filter - can reject selectors that definitely won't match
/// This is an optimization to avoid expensive full matching for selectors targeting ancestors
pub fn selector_might_match_with_filter(selector: &Selector, ancestor_filter: &AncestorFilter) -> bool {
    let mut iter = selector.iter();

    // Skip target sequence (already checked by selector index)
    for _ in iter.by_ref() {}

    // Check ancestor sequences using bloom filter
    while iter.next_sequence().is_some() {
        for component in iter.by_ref() {
            match component {
                Component::LocalName(name) => {
                    if !ancestor_filter.might_contain(name.lower_name.0.as_ref()) {
                        return false; // Definitely no match
                    }
                }
                Component::Class(class) => {
                    if !ancestor_filter.might_contain(class.0.as_ref()) {
                        return false; // Definitely no match
                    }
                }
                Component::ID(id) => {
                    if !ancestor_filter.might_contain(id.0.as_ref()) {
                        return false; // Definitely no match
                    }
                }
                _ => {}
            }
        }
    }

    true // Might match (need full check)
}
