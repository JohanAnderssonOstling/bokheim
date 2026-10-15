use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use crate::{Run, SegmentTree, SegmentWordRef, TagData, TagKind, TagMetadata, UnitKind, UnitRef};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructuredSegment {
    elements: Vec<StructuredElement>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StructuredElement {
    Text(String),
    Tag(TagData),
    Placeable(InlinePlaceable),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlinePlaceable {
    id: Option<String>,
    text: String,
    kind: Option<UnitKind>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Lossless lexical stage produced from raw text or runs.
///
/// Semantic parsing consumes this value and returns a `SegmentTree`, keeping
/// an unparsed tree from escaping through the supported construction API.
pub struct TokenizedSegment {
    tree: SegmentTree,
}

impl StructuredSegment {
    pub fn new(elements: Vec<StructuredElement>) -> Self { Self { elements: coalesced_elements(elements) } }
    pub fn from_text(text: impl Into<String>) -> Self { let text = text.into(); Self::new((!text.is_empty()).then_some(StructuredElement::Text(text)).into_iter().collect()) }
    pub fn from_segment_tree(tree: &SegmentTree) -> Self {
        let elements = tree.iter_units().filter_map(|unit| {
            if let Some(tag) = unit.tag() { return Some(StructuredElement::Tag(tag_from_tree(tag))); }
            if unit.text().is_empty() { return None; }
            if let Some(id) = unit.inline_placeable_id() { return Some(StructuredElement::Placeable(InlinePlaceable::with_kind(id.map(str::to_string), unit.text(), unit.kind()))); }
            if unit.kind().is_numeric_fact() { return Some(StructuredElement::Placeable(InlinePlaceable::with_kind(unit.semantic_id().map(|id| format!("{id:?}")), unit.text(), unit.kind()))); }
            Some(StructuredElement::Text(unit.text().to_string()))
        }).collect();
        Self::new(elements)
    }
    pub fn elements(&self) -> &[StructuredElement] { &self.elements }
    pub fn display_text(&self) -> String { self.elements.iter().fold(String::new(), |mut text, element| { text.push_str(&element.display_text()); text }) }
    pub fn display_span_for_text_span(&self, span: Range<usize>) -> Range<usize> { self.display_pos_for_text_pos(span.start, false)..self.display_pos_for_text_pos(span.end, true) }
    fn display_pos_for_text_pos(&self, text_position: usize, end_boundary: bool) -> usize {
        let mut text_offset = 0usize;
        let mut display_offset = 0usize;
        self.elements.iter().find_map(|element| {
            let display_length = element.display_text().len();
            if matches!(element, StructuredElement::Tag(_)) { display_offset += display_length; return None; }
            let text_end = text_offset + display_length;
            let contains = if end_boundary { text_offset < text_position && text_position <= text_end } else { text_offset <= text_position && text_position < text_end };
            if contains { return Some(display_offset + text_position.saturating_sub(text_offset)); }
            text_offset = text_end;
            display_offset += display_length;
            None
        }).unwrap_or(display_offset)
    }
    pub fn matching_text(&self) -> String {
        self.elements.iter().fold(String::new(), |mut text, element| {
            match element {
                StructuredElement::Text(value) => text.push_str(value),
                StructuredElement::Placeable(placeable) => text.push_str(placeable.text()),
                StructuredElement::Tag(tag) => tag.equiv_text().filter(|value| !value.is_empty()).into_iter().for_each(|value| text.push_str(value)),
            }
            text
        })
    }
    pub fn display_runs(&self) -> Vec<(usize, bool)> { self.elements.iter().filter_map(|element| { let len = element.display_text().len(); (len > 0).then_some((len, matches!(element, StructuredElement::Tag(_)))) }).collect() }
    pub fn slice_display(&self, range: Range<usize>) -> Self {
        let display = self.display_text();
        if range.is_empty() || range.end > display.len() || !display.is_char_boundary(range.start) || !display.is_char_boundary(range.end) { return Self::new(Vec::new()); }
        let (elements, _) = self.elements.iter().fold((Vec::new(), 0usize), |(mut elements, offset), element| {
            let element_range = offset..offset + element.display_text().len();
            match element {
                StructuredElement::Tag(tag) if element_range.is_empty() && range.start <= offset && offset <= range.end => elements.push(StructuredElement::Tag(tag.clone())),
                StructuredElement::Tag(tag) if range.start <= element_range.start && element_range.end <= range.end => elements.push(StructuredElement::Tag(tag.clone())),
                StructuredElement::Text(text) => push_text_overlap(&mut elements, text, &element_range, &range),
                StructuredElement::Placeable(placeable) => push_placeable_overlap(&mut elements, placeable, &element_range, &range),
                StructuredElement::Tag(_) => {}
            }
            (elements, element_range.end)
        });
        Self::new(remove_unmatched_boundary_tags(elements))
    }
    pub fn into_tokenized(self) -> TokenizedSegment {
        let (run, placeables) = self.elements.into_iter().fold((Run::empty(), Vec::new()), |(mut run, mut placeables), element| {
            match element {
                StructuredElement::Text(text) => run.push_text(text),
                StructuredElement::Tag(tag) => run.push_tag(tag),
                StructuredElement::Placeable(placeable) => {
                    let start = run.text().len();
                    let (id, text, kind) = placeable.into_parts();
                    run.push_text(&text);
                    placeables.push((start..start + text.len(), id, kind));
                }
            }
            (run, placeables)
        });
        let spans = placeables.iter().filter_map(|(span, _, kind)| kind.map(|kind| (kind, span.clone()))).collect::<Vec<_>>();
        let mut tree = SegmentTree::from_runs_with_resolved_spans([run], &spans);
        placeables.into_iter().for_each(|(span, id, _)| tree.attach_inline_placeable(span, id));
        TokenizedSegment { tree }
    }
}

impl StructuredElement {
    pub fn text(text: impl Into<String>) -> Self { Self::Text(text.into()) }
    pub fn tag(tag: TagData) -> Self { Self::Tag(tag) }
    pub fn placeable(placeable: InlinePlaceable) -> Self { Self::Placeable(placeable) }
    pub fn display_text(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Tag(tag) => tag.equiv_text().map(str::to_string).unwrap_or_else(|| format!("{{{}}}", tag.id())),
            Self::Placeable(placeable) => placeable.text().to_string(),
        }
    }
}

impl InlinePlaceable {
    pub fn new(id: Option<String>, text: impl Into<String>) -> Self { Self { id, text: text.into(), kind: None } }
    pub fn with_kind(id: Option<String>, text: impl Into<String>, kind: UnitKind) -> Self { assert!(kind.is_semantic_text(), "inline placeables require a semantic text unit kind"); Self { id, text: text.into(), kind: Some(kind) } }
    pub fn id(&self) -> Option<&str> { self.id.as_deref() }
    pub fn text(&self) -> &str { &self.text }
    pub fn kind(&self) -> Option<UnitKind> { self.kind }
    pub fn into_parts(self) -> (Option<String>, String, Option<UnitKind>) { (self.id, self.text, self.kind) }
}

impl TokenizedSegment {
    pub fn from_text(text: impl Into<String>) -> Self { Self::from_runs([Run::new(text)]) }
    pub fn from_runs(runs: impl IntoIterator<Item = Run>) -> Self { Self { tree: SegmentTree::from_runs(runs) } }
    pub fn from_runs_with_resolved_spans(runs: impl IntoIterator<Item = Run>, spans: &[(UnitKind, Range<usize>)]) -> Self { Self { tree: SegmentTree::from_runs_with_resolved_spans(runs, spans) } }
    /// Copies one unit into a standalone lexical segment with fresh tree-local identities.
    pub fn from_unit(unit: UnitRef<'_>) -> Self {
        if let Some(tag) = unit.tag() { return Self::from_runs([Run::tag(tag.clone())]); }
        let text = unit.text().to_string();
        let mut tree = if unit.kind().is_semantic_text() { SegmentTree::from_runs_with_resolved_spans([Run::new(&text)], &[(unit.kind(), 0..text.len())]) } else { SegmentTree::from_runs([Run::new(&text)]) };
        if let Some(external_id) = unit.inline_placeable_id() { tree.attach_inline_placeable(0..tree.text().len(), external_id.map(str::to_string)); }
        Self { tree }
    }
    /// Copies one parsed lexical word into a standalone lexical segment without
    /// requiring consumers to downgrade it to raw text first.
    pub fn from_word(word: SegmentWordRef<'_>) -> Self {
        let tree = word.tree();
        if let Some(unit) = tree.token_index(word.token_unit_id()).and_then(|index| tree.unit(index)).filter(|unit| unit.span() == word.span()) { return Self::from_unit(unit); }
        StructuredSegment::from_segment_tree(tree).slice_display(tree.display_span_for_text_span(word.span())).into_tokenized()
    }
    /// Adopts a tree that has already crossed the parser boundary for a
    /// consumer that only needs its lexical projection.
    pub fn from_segment_tree(tree: SegmentTree) -> Self { Self { tree } }
    pub fn tree(&self) -> &SegmentTree { &self.tree }
    pub fn to_structured(&self) -> StructuredSegment { StructuredSegment::from_segment_tree(&self.tree) }
    pub fn display_text(&self) -> String {
        self.tree.iter_units().fold(String::new(), |mut text, unit| {
            match unit.tag() {
                Some(tag) => text.push_str(&tag.equiv_text().map(str::to_string).unwrap_or_else(|| format!("{{{}}}", tag.id()))),
                None => text.push_str(unit.text()),
            }
            text
        })
    }
    pub fn matching_text(&self) -> String {
        self.tree.iter_units().fold(String::new(), |mut text, unit| {
            match unit.tag() {
                Some(tag) => tag.equiv_text().filter(|value| !value.is_empty()).into_iter().for_each(|value| text.push_str(value)),
                None => text.push_str(unit.text()),
            }
            text
        })
    }
    #[doc(hidden)]
    pub fn into_tree_for_parser(self) -> SegmentTree { self.tree }
}

pub fn structured_tag(tag_type: Option<String>, id: Option<String>, text_equivalent: Option<String>) -> TagData {
    let tag_type = tag_type.unwrap_or_else(|| "Standalone".to_string());
    let external_id = id;
    let id = external_id.clone().unwrap_or_else(|| "structured-tag".to_string());
    let kind = match tag_type.to_ascii_lowercase().as_str() {
        "start" | "open" | "unmatchedstart" => TagKind::Open,
        "end" | "close" | "unmatchedend" => TagKind::Close,
        _ => TagKind::Standalone,
    };
    let equivalent = text_equivalent;
    let mut metadata = TagMetadata::default();
    metadata.set_tag_type(&tag_type);
    metadata.set_original_id(external_id.unwrap_or_default());
    metadata.set_equiv_text_if_empty(equivalent.clone());
    metadata.set_sdl_emoji(tag_type.eq_ignore_ascii_case("TextPlaceholder") && equivalent.is_some());
    let raw = match kind { TagKind::Open => format!("<{tag_type}>"), TagKind::Close => format!("</{tag_type}>"), TagKind::Standalone => format!("<{tag_type}/>") };
    TagData::new(id, tag_type, kind, raw, equivalent, Some(Arc::new(metadata)))
}

fn coalesced_elements(elements: Vec<StructuredElement>) -> Vec<StructuredElement> {
    elements.into_iter().fold(Vec::new(), |mut coalesced, element| {
        match (coalesced.last_mut(), element) {
            (Some(StructuredElement::Text(current)), StructuredElement::Text(next)) => current.push_str(&next),
            (_, element) => coalesced.push(element),
        }
        coalesced
    })
}

fn tag_from_tree(tag: &TagData) -> TagData {
    let (id, name, kind, raw, equivalent, metadata) = tag.clone().into_parts();
    TagData::new(id, name, kind, raw, Some(equivalent.unwrap_or_default()), metadata)
}

fn push_text_overlap(elements: &mut Vec<StructuredElement>, text: &str, element_range: &Range<usize>, requested: &Range<usize>) {
    let overlap = requested.start.max(element_range.start)..requested.end.min(element_range.end);
    if overlap.start >= overlap.end { return; }
    let local = overlap.start - element_range.start..overlap.end - element_range.start;
    elements.push(StructuredElement::Text(text[local].to_string()));
}

fn push_placeable_overlap(elements: &mut Vec<StructuredElement>, placeable: &InlinePlaceable, element_range: &Range<usize>, requested: &Range<usize>) {
    let overlap = requested.start.max(element_range.start)..requested.end.min(element_range.end);
    if overlap.start >= overlap.end { return; }
    let local = overlap.start - element_range.start..overlap.end - element_range.start;
    if local.start == 0 && local.end == placeable.text().len() { elements.push(StructuredElement::Placeable(placeable.clone())); } else { elements.push(StructuredElement::Text(placeable.text()[local].to_string())); }
}

fn remove_unmatched_boundary_tags(elements: Vec<StructuredElement>) -> Vec<StructuredElement> {
    let paired_ids = elements.iter().filter_map(|element| {
        let StructuredElement::Tag(tag) = element else { return None; };
        let tag_type = tag.metadata().and_then(|metadata| metadata.tag_type()).unwrap_or(tag.name());
        let start = matches!(tag.kind(), TagKind::Open);
        let end = matches!(tag.kind(), TagKind::Close);
        (start || end).then_some((tag.id().to_string(), start, tag_type))
    }).fold(HashMap::<String, (bool, bool)>::new(), |mut pairs, (id, start, _)| {
        let pair = pairs.entry(id).or_default();
        if start { pair.0 = true; } else { pair.1 = true; }
        pairs
    });
    elements.into_iter().filter(|element| {
        let StructuredElement::Tag(tag) = element else { return true; };
        !matches!(tag.kind(), TagKind::Open | TagKind::Close) || paired_ids.get(tag.id()).is_some_and(|(start, end)| *start && *end)
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsed_words_cross_the_lexical_boundary_without_raw_text_conversion() {
        let segment = TokenizedSegment::from_runs_with_resolved_spans([Run::new("hard disk")], &[(UnitKind::Term, 0..9)]);
        let word = segment.tree().segment_words().iter().nth(1).expect("second term word");
        let extracted = TokenizedSegment::from_word(word);

        assert_eq!(extracted.tree().text(), "disk");
        assert_eq!(extracted.tree().segment_words().iter().next().map(|word| word.text()), Some("disk"));
    }

    #[test]
    fn text_spans_map_around_zero_width_display_tags() {
        let tag = |kind| StructuredElement::tag(TagData::new("1".to_string(), "b".to_string(), kind, String::new(), Some(String::new()), None));
        let segment = StructuredSegment::new(vec![StructuredElement::text("before "), tag(TagKind::Open), StructuredElement::text("inside"), tag(TagKind::Close), StructuredElement::text(" after")]);

        assert_eq!(segment.display_span_for_text_span(7..13), 7..13);
        assert_eq!(&segment.display_text()[segment.display_span_for_text_span(7..13)], "inside");
    }
}
