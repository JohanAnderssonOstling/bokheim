//! Persistent segment data for text editing, caret navigation, and parsed
//! annotations.
//!
//! Parsing and analysis algorithms live in `segment-parser`; this crate owns
//! the data model and its invariant-preserving API:
//! - lossless runs
//! - atomic units
//! - hierarchical parsed nodes

use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::Range;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[path = "tokenizer.rs"]
mod tokenizer;
#[path = "numeric_model.rs"]
mod numeric_model;
pub use numeric_model::{CurrencySemantic, DateFact, MeasurementDegreeStyle, MeasurementDimension, MeasurementQuantityKind, MeasurementSemantic, MeasurementTypography, MeasurementValueFact, NumberFormat, NumberFormatAssessment, NumberFormatBuilder, NumberRangeSemantic, NumberRangeTypography, NumberSemantic, NumericTypographySpacing, OrdinalAssessment, OrdinalFact, OrdinalStyle, TimeFact};
use numeric_model::collect_numeric_fact_addresses;
#[path = "whitespace_model.rs"]
mod whitespace_model;
pub use whitespace_model::{PunctuationSpacingSide, WhitespaceTypographyAssessment, WhitespaceTypographyFact, WhitespaceTypographyKind, WhitespaceTypographyPreference, WhitespaceTypographyRole};
#[path = "punctuation_model.rs"]
mod punctuation_model;
pub use punctuation_model::{DelimiterAnnotation, DelimiterFactRef, DelimiterFamily, DelimiterRole, QuotationStyle};
#[path = "structured_edit.rs"]
mod structured_edit;
pub use structured_edit::{SegmentPiece, StructuredReplacement};
#[path = "tag_model.rs"]
mod tag_model;
pub use tag_model::{FormattingKind, SegmentTag, SegmentTags, TagAnchor, TagData, TagKind, TagMetadata, TagMetadataProperty, TagScope};
use tag_model::pack_formatting_counts;
#[path = "terminology.rs"]
mod terminology;
pub use terminology::TerminologyOccurrence;
#[path = "term_matcher.rs"]
mod term_matcher;
pub use term_matcher::{TermHit, TermHitCandidate, TermMatchKind};
use term_matcher::{StoredInlinePlaceable, SegmentAuxiliaryStorage};
use tokenizer::{TokenizedRun, atomize_text_into, tokenize_runs, tokenize_runs_into_parts, tokenize_runs_with_resolved_spans};
#[path = "word_model.rs"]
mod word_model;
pub use word_model::{DictionaryAnalysis, SegmentWordRef, SegmentWords};
use word_model::{SegmentWordStorage, segment_word_dictionary_annotations};

#[doc(hidden)]
#[path = "parser_api.rs"]
pub mod parser_api;
use parser_api::{Node, NodeAnnotation, NodeSemantic, TokenUnit};

mod display;
mod kind_model;
pub use kind_model::{LocalizableKind, NodeKind, PlaceableKind, ProtectedKind, UnitKind};
mod node_view;
pub use node_view::NodeRef;
use node_view::PreorderIter;
mod semantic_model;
pub use semantic_model::{PartOfSpeech, PlaceholderSemantic, VersionPrefixStyle, VersionSemantic};
#[path = "part_of_speech_model.rs"]
mod part_of_speech_model;
pub use part_of_speech_model::PosAnnotation;
use part_of_speech_model::PosAnnotations;
mod text_projection;
mod unit_model;
pub use unit_model::{Run, SegmentTreeFingerprint, SemanticUnitId, TokenUnitId, TokenizedContentWords, UnitRef};
use unit_model::{decode_optional_index, encode_optional_index};

fn leaf_from_unit(stream: &SegmentTree, idx: usize) -> Node {
    let unit = &stream.units[idx];
    Node::new(
        unit.kind().node_kind(),
        unit.span(),
        unit.tag_index().map(|tag_idx| u16::try_from(tag_idx).expect("tag index exceeds u16 range")),
        Vec::new(),
    )
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct StoredNode {
    kind: NodeKind,
    start: u16,
    end: u16,
    encoded_tag_index: u16,
    encoded_semantic_index: u16,
    encoded_annotations_index: u16,
    first_child: u16,
    child_count: u16,
}

const IMPLICIT_UNIT_CHILDREN: u16 = u16::MAX;

impl StoredNode {
    fn placeholder(kind: NodeKind, span: Range<usize>, tag_index: Option<u16>) -> Self { let start = u16::try_from(span.start).expect("stored node start exceeds u16 range"); let end = u16::try_from(span.end).expect("stored node end exceeds u16 range"); Self { kind, start, end, encoded_tag_index: encode_optional_index(tag_index), encoded_semantic_index: 0, encoded_annotations_index: 0, first_child: 0, child_count: 0 } }
    fn start(&self) -> usize { self.start as usize }
    fn end(&self) -> usize { self.end as usize }
    fn span(&self) -> Range<usize> { self.start()..self.end() }
    fn len(&self) -> usize { self.end().saturating_sub(self.start()) }
    fn tag_index(&self) -> Option<usize> { decode_optional_index(self.encoded_tag_index).map(|index| index as usize) }
    fn semantic_index(&self) -> Option<usize> { decode_optional_index(self.encoded_semantic_index).map(|index| index as usize) }
    fn annotations_index(&self) -> Option<usize> { decode_optional_index(self.encoded_annotations_index).map(|index| index as usize) }
    fn child_range(&self) -> Range<usize> { debug_assert_ne!(self.first_child, IMPLICIT_UNIT_CHILDREN); self.first_child as usize..self.first_child as usize + self.child_count as usize }
}

const TOKEN_LOOKUP_INDEX_THRESHOLD: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct UnitSemanticEntry {
    encoded: [u8; 10],
}

impl UnitSemanticEntry {
    fn new(unit_index: u16, semantic_id: SemanticUnitId) -> Self { let mut encoded = [0; 10]; encoded[..2].copy_from_slice(&unit_index.to_le_bytes()); encoded[2..].copy_from_slice(&semantic_id.value().to_le_bytes()); Self { encoded } }
    fn unit_index(self) -> u16 { u16::from_le_bytes([self.encoded[0], self.encoded[1]]) }
    fn semantic_id(self) -> SemanticUnitId { SemanticUnitId::from_raw(u64::from_le_bytes([self.encoded[2], self.encoded[3], self.encoded[4], self.encoded[5], self.encoded[6], self.encoded[7], self.encoded[8], self.encoded[9]])) }
}

fn token_lookup_index(units: &[TokenUnit]) -> Option<Box<[u16]>> {
    (units.len() > TOKEN_LOOKUP_INDEX_THRESHOLD).then(|| {
        let mut indices = (0..units.len()).map(|unit_index| u16::try_from(unit_index).expect("unit index exceeds u16 range")).collect::<Vec<_>>();
        indices.sort_unstable_by_key(|unit_index| units[*unit_index as usize].stored_id().value());
        indices.into_boxed_slice()
    })
}

/// Per-tree allocator for [`TokenUnitId`]s, live only while a tree is being
/// built.
///
/// Ids are only ever resolved against the tree that minted them — every path
/// bringing content in from elsewhere sets `token_id: None` so the destination
/// tree mints afresh — so they need no process-wide uniqueness. Deriving the
/// next id from the units already in hand keeps a cache-reloaded tree safe: its
/// ids come back from disk, and anything minted afterwards starts above them.
/// A process-global counter could not do this, because it restarts at 1 each
/// launch and would re-mint ids the reloaded tree already holds.
struct TokenIdCounter(u64);

impl TokenIdCounter {
    /// Starts past the highest id already carried by `units`, so ids preserved
    /// across a retokenization are never handed out a second time.
    fn starting_after(units: &[TokenUnit]) -> Self { Self(units.iter().filter_map(|unit| unit.token_id()).map(|id| id.value()).max().map_or(1, |highest| highest.saturating_add(1))) }
    fn mint(&mut self) -> TokenUnitId { let id = TokenUnitId::new(self.0); self.0 = self.0.saturating_add(1); id }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SegmentTree {
    text: String,
    units: Box<[TokenUnit]>,
    tags: Box<[TagData]>,
    #[serde(default)]
    inherited_formatting_counts: u32,
    nodes: Box<[StoredNode]>,
    node_children: Box<[u16]>,
    #[serde(serialize_with = "serialize_numeric_fact_addresses")]
    numeric_fact_addresses: Box<[u16]>,
    node_semantics: Box<[NodeSemantic]>,
    node_annotations: Box<[Box<[NodeAnnotation]>]>,
    #[serde(default)]
    pos_annotations: Option<Box<PosAnnotations>>,
    #[serde(default, rename = "term_hits")]
    auxiliary: Option<Box<SegmentAuxiliaryStorage>>,
    unit_semantics: Box<[UnitSemanticEntry]>,
    token_lookup: Option<Box<[u16]>>,
    segment_words: Box<SegmentWordStorage>,
}

fn serialize_numeric_fact_addresses<S: Serializer>(addresses: &Box<[u16]>, serializer: S) -> Result<S::Ok, S::Error> { Some(addresses).serialize(serializer) }

#[derive(Deserialize)]
struct SerializedSegmentTree {
    text: String,
    units: Box<[TokenUnit]>,
    tags: Box<[TagData]>,
    #[serde(default)]
    inherited_formatting_counts: u32,
    nodes: Box<[StoredNode]>,
    node_children: Box<[u16]>,
    #[serde(default)]
    numeric_fact_addresses: Option<Box<[u16]>>,
    node_semantics: Box<[NodeSemantic]>,
    node_annotations: Box<[Box<[NodeAnnotation]>]>,
    #[serde(default)]
    pos_annotations: Option<Box<PosAnnotations>>,
    #[serde(default, rename = "term_hits")]
    auxiliary: Option<Box<SegmentAuxiliaryStorage>>,
    unit_semantics: Box<[UnitSemanticEntry]>,
    token_lookup: Option<Box<[u16]>>,
    segment_words: Box<SegmentWordStorage>,
}

impl<'de> Deserialize<'de> for SegmentTree {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let stored = SerializedSegmentTree::deserialize(deserializer)?;
        let rebuild_numeric_index = stored.numeric_fact_addresses.is_none();
        let mut tree = Self { text: stored.text, units: stored.units, tags: stored.tags, inherited_formatting_counts: stored.inherited_formatting_counts, nodes: stored.nodes, node_children: stored.node_children, numeric_fact_addresses: stored.numeric_fact_addresses.unwrap_or_default(), node_semantics: stored.node_semantics, node_annotations: stored.node_annotations, pos_annotations: stored.pos_annotations, auxiliary: stored.auxiliary, unit_semantics: stored.unit_semantics, token_lookup: stored.token_lookup, segment_words: stored.segment_words };
        if rebuild_numeric_index { tree.numeric_fact_addresses = collect_numeric_fact_addresses(&tree); }
        let (has_dictionary_annotations, dictionary_known_words) = segment_word_dictionary_annotations(&tree);
        tree.segment_words.set_dictionary_annotations(has_dictionary_annotations, dictionary_known_words);
        Ok(tree)
    }
}

fn intern_annotations(node_annotations: Box<[NodeAnnotation]>, annotations: &mut Vec<Box<[NodeAnnotation]>>) -> u16 {
    if let Some(index) = annotations.iter().position(|stored| stored.as_ref() == node_annotations.as_ref()) {
        return u16::try_from(index).expect("annotation set index exceeds u16 range");
    }
    let index = u16::try_from(annotations.len()).expect("annotation set count exceeds u16 range");
    annotations.push(node_annotations);
    index
}

#[derive(Clone, Copy)]
enum PendingNodeAddress {
    Stored(u16),
    Unit(u16),
}

fn virtual_leaf_unit_index(node: &Node, units: &[TokenUnit]) -> Option<u16> {
    if !node.children.is_empty() || node.semantic.is_some() || !node.annotations.is_empty() {
        return None;
    }
    let unit_kind = node.kind.unit_kind()?;
    let first = units.partition_point(|unit| unit.start() < node.start());
    units[first..].iter().enumerate().take_while(|(_, unit)| unit.start() == node.start()).find(|(_, unit)| unit.kind() == unit_kind && unit.span() == node.span() && unit.tag_index() == node.tag_index()).map(|(offset, _)| u16::try_from(first + offset).expect("virtual unit index exceeds u16 range"))
}

fn store_node(node: Node, units: &[TokenUnit], nodes: &mut Vec<StoredNode>, child_addresses: &mut Vec<PendingNodeAddress>, semantics: &mut Vec<NodeSemantic>, annotations: &mut Vec<Box<[NodeAnnotation]>>) -> PendingNodeAddress {
    if let Some(unit_index) = virtual_leaf_unit_index(&node, units) {
        return PendingNodeAddress::Unit(unit_index);
    }
    let node_index = u16::try_from(nodes.len()).expect("node count exceeds u16 range");
    nodes.push(StoredNode::placeholder(node.kind, node.span(), node.tag_index().map(|index| u16::try_from(index).expect("tag index exceeds u16 range"))));
    let Node { kind, start, end, encoded_tag_index, semantic, annotations: node_annotations, children } = node;
    let semantic_index = semantic.map(|semantic| {
        let index = u16::try_from(semantics.len()).expect("semantic count exceeds u16 range");
        semantics.push(*semantic);
        index
    });
    let annotations_index = (!node_annotations.is_empty()).then(|| intern_annotations(node_annotations, annotations));
    let children = children.into_vec().into_iter().map(|child| store_node(child, units, nodes, child_addresses, semantics, annotations)).collect::<Vec<_>>();
    let first_child = u16::try_from(child_addresses.len()).expect("node child-address count exceeds u16 range");
    let child_count = u16::try_from(children.len()).expect("node child count exceeds u16 range");
    child_addresses.extend(children);
    let stored = &mut nodes[node_index as usize];
    stored.kind = kind;
    stored.start = start;
    stored.end = end;
    stored.encoded_tag_index = encoded_tag_index;
    stored.encoded_semantic_index = encode_optional_index(semantic_index);
    stored.encoded_annotations_index = encode_optional_index(annotations_index);
    stored.first_child = first_child;
    stored.child_count = child_count;
    PendingNodeAddress::Stored(node_index)
}

fn encode_node_address(address: PendingNodeAddress, stored_node_count: usize) -> u16 {
    match address {
        PendingNodeAddress::Stored(index) => index,
        PendingNodeAddress::Unit(index) => u16::try_from(stored_node_count.checked_add(index as usize).expect("node address overflow")).expect("combined stored-node and virtual-unit count exceeds u16 range"),
    }
}

fn load_node(tree: &SegmentTree, node_address: u16) -> Option<Node> {
    if let Some(unit_index) = tree.virtual_unit_index(node_address) {
        return Some(leaf_from_unit(tree, unit_index));
    }
    let stored = tree.nodes.get(node_address as usize)?;
    let children = tree.node_children.get(stored.child_range())?.iter().filter_map(|address| load_node(tree, *address)).collect::<Vec<_>>();
    let semantic = stored.semantic_index().and_then(|index| tree.node_semantics.get(index)).cloned();
    let mut node = Node::from_parts(stored.kind, stored.span(), stored.tag_index().map(|index| u16::try_from(index).expect("tag index exceeds u16 range")), semantic, children);
    node.annotations = stored.annotations_index().and_then(|index| tree.node_annotations.get(index)).cloned().unwrap_or_default();
    Some(node)
}

impl SegmentTree {
    fn from_tokenized_parts_with_semantic_ids(text: String, mut units: Vec<TokenUnit>, tags: Vec<TagData>, inherited_formatting: Vec<FormattingKind>, semantic_ids: Option<Vec<Option<SemanticUnitId>>>) -> Self {
        if let Some(semantic_ids) = &semantic_ids {
            assert_eq!(semantic_ids.len(), units.len(), "semantic ID count must match unit count");
        }
        let mut next_semantic_id = semantic_ids.as_deref().unwrap_or_default().iter().flatten().map(|id| id.value()).max().map_or(1, |highest| highest.saturating_add(1));
        let unit_semantics = units.iter().enumerate().filter_map(|(unit_index, unit)| {
            if !unit.kind().has_semantic_identity() {
                return None;
            }
            let semantic_id = semantic_ids.as_ref().and_then(|ids| ids[unit_index]).filter(|id| id.value() != 0).unwrap_or_else(|| { let id = SemanticUnitId::from_raw(next_semantic_id); next_semantic_id = next_semantic_id.saturating_add(1); id });
            Some(UnitSemanticEntry::new(u16::try_from(unit_index).expect("unit index exceeds u16 range"), semantic_id))
        }).collect::<Vec<_>>().into_boxed_slice();
        let mut next_token_id = TokenIdCounter::starting_after(&units);
        units.iter_mut().for_each(|unit| {
            if unit.token_id().is_none() {
                unit.set_token_id(next_token_id.mint());
            }
        });
        let token_lookup = token_lookup_index(&units);
        let units = units.into_boxed_slice();
        let segment_words = Box::new(SegmentWordStorage::new(&text, &units));
        let len = text.len();
        let mut tree = Self { text, units, tags: tags.into_boxed_slice(), inherited_formatting_counts: pack_formatting_counts(inherited_formatting), nodes: Box::default(), node_children: Box::default(), numeric_fact_addresses: Box::default(), node_semantics: Box::default(), node_annotations: Box::default(), pos_annotations: None, auxiliary: None, unit_semantics, token_lookup, segment_words };
        tree.set_root(Node::new(NodeKind::Root, 0..len, None, Vec::new()));
        tree
    }

    fn set_root(&mut self, root: Node) {
        let mut nodes = Vec::new();
        let mut node_children = Vec::new();
        let mut node_semantics = Vec::new();
        let mut node_annotations = Vec::new();
        let root_address = store_node(root, &self.units, &mut nodes, &mut node_children, &mut node_semantics, &mut node_annotations);
        debug_assert!(matches!(root_address, PendingNodeAddress::Stored(0)));
        let stored_node_count = nodes.len();
        self.nodes = nodes.into_boxed_slice();
        self.node_children = node_children.into_iter().map(|address| encode_node_address(address, stored_node_count)).collect::<Vec<_>>().into_boxed_slice();
        self.node_semantics = node_semantics.into_boxed_slice();
        self.node_annotations = node_annotations.into_boxed_slice();
        self.numeric_fact_addresses = collect_numeric_fact_addresses(self);
        let (has_dictionary_annotations, dictionary_known_words) = segment_word_dictionary_annotations(self);
        self.segment_words.set_dictionary_annotations(has_dictionary_annotations, dictionary_known_words);
    }

    fn loaded_root(&self) -> Node { load_node(self, 0).unwrap_or_else(|| Node::new(NodeKind::Root, 0..self.text.len(), None, Vec::new())) }

    #[cfg(test)]
    pub(crate) fn from_text(text: impl Into<String>) -> Self {
        Self::from_runs([Run::new(text)])
    }

    pub(crate) fn from_runs(runs: impl IntoIterator<Item = Run>) -> Self {
        Self::from_runs_with_resolved_spans(runs, &[])
    }

    pub(crate) fn from_runs_with_resolved_spans(runs: impl IntoIterator<Item = Run>, spans: &[(UnitKind, Range<usize>)]) -> Self {
        let runs = runs.into_iter().collect();
        let tree = if spans.is_empty() { tokenize_runs(runs) } else { tokenize_runs_with_resolved_spans(runs, spans) };
        Self::finish_build(tree)
    }

    fn finish_build(mut tree: Self) -> Self {
        let nodes = (0..tree.units.len()).map(|index| leaf_from_unit(&tree, index)).collect::<Vec<_>>();
        tree.set_root(Node::new(NodeKind::Root, 0..tree.text.len(), None, nodes));
        tree
    }

    pub fn iter_units(&self) -> impl DoubleEndedIterator<Item = UnitRef<'_>> + ExactSizeIterator { (0..self.units.len()).map(|index| UnitRef::new(self, index)) }

    pub(crate) fn stored_whitespace_typography_facts_internal(&self) -> impl Iterator<Item = WhitespaceTypographyFact> + '_ {
        self.iter_nodes().flat_map(|node| node.annotations().iter().filter_map(|annotation| match annotation {
            NodeAnnotation::WhitespaceTypography(fact) => Some(*fact),
            _ => None,
        }))
    }

    pub(crate) fn apply_parse_output_internal(&mut self, nodes: Vec<Node>) {
        self.set_root(Node::new(NodeKind::Root, 0..self.text.len(), None, nodes));
    }

    pub fn iter_tag_units(&self) -> impl Iterator<Item = UnitRef<'_>> { self.iter_units().filter(|unit| unit.is_tag()) }

    pub fn iter_content_units(&self) -> impl DoubleEndedIterator<Item = UnitRef<'_>> + '_ { self.iter_units().filter(|unit| unit.kind().is_text_content()) }

    pub fn protected_nodes(&self) -> impl Iterator<Item = NodeRef<'_>> + Clone + '_ { self.preorder_iter().filter(NodeRef::is_protected) }
    pub fn protected_non_translatable_nodes(&self) -> impl Iterator<Item = NodeRef<'_>> + Clone + '_ { self.protected_nodes().filter(|node| node.protected_kind().is_some_and(ProtectedKind::participates_in_non_translatable_matching)) }
    pub fn inherited_formatting_count(&self, kind: FormattingKind) -> usize { ((self.inherited_formatting_counts >> kind.packed_shift()) & 0x3f) as usize }
    pub fn is_structurally_empty(&self) -> bool { self.units.is_empty() }
    pub fn iter_nodes(&self) -> impl DoubleEndedIterator<Item = NodeRef<'_>> + Clone + '_ { self.node_child_addresses(0).iter().copied().map(move |node_address| NodeRef::new(self, node_address)) }

    pub fn iter_leaf_nodes(&self) -> impl Iterator<Item = NodeRef<'_>> {
        self.preorder_iter().filter(NodeRef::is_leaf)
    }

    pub fn root(&self) -> NodeRef<'_> { NodeRef::new(self, 0) }

    pub fn text(&self) -> &str { &self.text }
    pub fn fingerprint(&self) -> SegmentTreeFingerprint {
        let mut hasher = DefaultHasher::new();
        self.text.hash(&mut hasher);
        self.iter_units().for_each(|unit| {
            hasher.write_u8(unit.kind() as u8);
            unit.text().hash(&mut hasher);
        });
        self.term_hits().iter().for_each(|hit| {
            hit.span().hash(&mut hasher);
            hit.candidates().hash(&mut hasher);
        });
        SegmentTreeFingerprint::new(hasher.finish())
    }

    pub fn unit_count(&self) -> usize { self.units.len() }

    pub fn unit_kind_count(&self, kind: UnitKind) -> usize { self.units.iter().filter(|unit| unit.kind() == kind).count() }

    #[cfg(test)]
    fn punctuation_units_are_atomic(&self) -> bool { self.iter_units().all(|unit| unit.kind() != UnitKind::Punct || unit.text().chars().count() == 1) }

    pub fn node_kind_count(&self, kind: NodeKind) -> usize { self.preorder_iter().filter(|node| node.kind() == kind).count() }
    pub fn has_term_units(&self) -> bool { self.units.iter().any(|unit| unit.kind() == UnitKind::Term) }
    pub fn term_hits(&self) -> &[TermHit] { self.auxiliary.as_deref().map(SegmentAuxiliaryStorage::hits).unwrap_or_default() }

    fn set_term_hits(&mut self, hits: Vec<TermHit>) {
        match self.auxiliary.as_mut() {
            Some(storage) => storage.set_hits(hits),
            None if !hits.is_empty() => self.auxiliary = Some(Box::new(SegmentAuxiliaryStorage::new(hits))),
            None => {}
        }
        if self.auxiliary.as_deref().is_some_and(SegmentAuxiliaryStorage::is_empty) { self.auxiliary = None; }
    }

    pub fn span(&self) -> Range<usize> { 0..self.text.len() }

    pub fn len(&self) -> usize { self.text.len() }
    pub fn is_empty(&self) -> bool { self.text.is_empty() }

    pub fn unit(&self, index: usize) -> Option<UnitRef<'_>> { (index < self.units.len()).then_some(UnitRef::new(self, index)) }

    fn unit_tag(&self, index: usize) -> Option<&TagData> { self.units.get(index)?.tag_index().and_then(|idx| self.tags.get(idx)) }

    pub fn token_index(&self, id: TokenUnitId) -> Option<usize> {
        match &self.token_lookup {
            Some(indices) => indices.binary_search_by_key(&id.value(), |unit_index| self.units[*unit_index as usize].stored_id().value()).ok().map(|lookup_index| indices[lookup_index] as usize),
            None => self.units.iter().position(|unit| unit.stored_id() == id),
        }
    }

    pub fn token_unit_ids_for_span(&self, span: Range<usize>) -> Vec<TokenUnitId> {
        self.iter_units()
            .filter(|unit| unit.kind() != UnitKind::Whitespace)
            .filter(|unit| {
                let unit_span = unit.span();
                span.start < unit_span.end && unit_span.start < span.end
            })
            .map(|unit| unit.id())
            .collect()
    }

    pub fn pos_annotations(&self) -> &[PosAnnotation] { self.pos_annotations.as_deref().map(PosAnnotations::annotations).unwrap_or_default() }

    pub fn replace_pos_annotations(&mut self, annotations: impl IntoIterator<Item = PosAnnotation>) {
        self.pos_annotations = PosAnnotations::new(annotations.into_iter().filter(|annotation| self.token_index(annotation.word_id()).is_some())).map(Box::new);
    }

    pub fn part_of_speech_for_word_id(&self, word_id: TokenUnitId) -> Option<PartOfSpeech> { self.pos_annotations.as_deref().and_then(|annotations| annotations.part_of_speech(word_id)) }

    fn unit_semantic_id(&self, unit_index: usize) -> Option<SemanticUnitId> {
        let unit_index = u16::try_from(unit_index).ok()?;
        self.unit_semantics.binary_search_by_key(&unit_index, |entry| entry.unit_index()).ok().map(|index| self.unit_semantics[index].semantic_id())
    }

    pub(crate) fn attach_inline_placeable(&mut self, span: Range<usize>, external_id: Option<String>) {
        assert!(span.start <= span.end && span.end <= self.text.len() && self.text.is_char_boundary(span.start) && self.text.is_char_boundary(span.end), "inline placeable span must address valid text");
        let semantic_id = self.iter_units().find(|unit| unit.span() == span && unit.kind().is_semantic_text()).and_then(|unit| unit.semantic_id());
        let placeables = self.inline_placeables().iter().filter(|placeable| placeable.span() != span && semantic_id.is_none_or(|id| placeable.semantic_id() != Some(id))).cloned().chain([StoredInlinePlaceable::new(span.clone(), semantic_id, external_id)]).collect();
        self.set_inline_placeables(placeables);
    }

    fn inline_placeable_id(&self, unit_index: usize) -> Option<Option<&str>> {
        let unit = self.units.get(unit_index)?;
        let semantic_id = self.unit_semantic_id(unit_index);
        self.inline_placeables().iter().find(|placeable| placeable.span() == unit.span() || semantic_id.is_some() && placeable.semantic_id() == semantic_id).map(|placeable| placeable.external_id())
    }

    fn inline_placeables(&self) -> &[StoredInlinePlaceable] { self.auxiliary.as_deref().map(SegmentAuxiliaryStorage::inline_placeables).unwrap_or_default() }

    pub fn inline_placeable_count(&self) -> usize { self.inline_placeables().len() }

    fn set_inline_placeables(&mut self, placeables: Vec<StoredInlinePlaceable>) {
        match self.auxiliary.as_mut() {
            Some(storage) => storage.set_inline_placeables(placeables),
            None if !placeables.is_empty() => { let mut storage = SegmentAuxiliaryStorage::default(); storage.set_inline_placeables(placeables); self.auxiliary = Some(Box::new(storage)); }
            None => {}
        }
        if self.auxiliary.as_deref().is_some_and(SegmentAuxiliaryStorage::is_empty) { self.auxiliary = None; }
    }

    fn copy_inline_placeables_preserved_by(&mut self, source: &Self) {
        let placeables = source.inline_placeables().iter().filter_map(|placeable| {
            let semantic_id = placeable.semantic_id()?;
            let span = self.iter_units().find(|unit| unit.semantic_id() == Some(semantic_id)).map(|unit| unit.span())?;
            Some(StoredInlinePlaceable::new(span, Some(semantic_id), placeable.external_id().map(str::to_string)))
        }).collect();
        self.set_inline_placeables(placeables);
    }

    fn virtual_unit_index(&self, node_address: u16) -> Option<usize> { (node_address as usize).checked_sub(self.nodes.len()).filter(|index| *index < self.units.len()) }

    fn node_child_addresses(&self, node_address: u16) -> &[u16] { self.nodes.get(node_address as usize).and_then(|stored| self.node_children.get(stored.child_range())).unwrap_or(&[]) }

    pub fn preorder_iter(&self) -> impl Iterator<Item = NodeRef<'_>> + Clone { PreorderIter::new(self) }

    pub fn rebuilt_with_resolved_spans(&self, spans: &[(UnitKind, Range<usize>)]) -> Self {
        let inline_placeables = self.inline_placeables().iter().map(|placeable| (placeable.span(), placeable.external_id().map(str::to_string))).collect::<Vec<_>>();
        let mut run = Run::new(self.text.clone());
        self.iter_tag_units().filter_map(|unit| unit.tag().map(|tag| (unit.span().start, tag.clone()))).for_each(|(offset, tag)| run.push_tag_at(offset, tag));
        let mut tree = SegmentTree::from_runs_with_resolved_spans([run], spans);
        let inline_placeables = inline_placeables.into_iter().filter(|(span, _)| tree.iter_units().any(|unit| unit.span() == *span && unit.kind().is_semantic_text())).collect::<Vec<_>>();
        inline_placeables.into_iter().for_each(|(span, id)| tree.attach_inline_placeable(span, id));
        tree.inherited_formatting_counts = self.inherited_formatting_counts;
        tree
    }

    pub fn rebuilt_with_resolved_term_hits(&self, primary_hits: &[TermHit], hits: Vec<TermHit>) -> Self {
        let previous_hit_spans = self.term_hits().iter().map(TermHit::span).collect::<Vec<_>>();
        let primary_spans = primary_hits.iter().map(TermHit::span).collect::<Vec<_>>();
        let explicit_spans = self.iter_units().filter(|unit| unit.kind() == UnitKind::Term).map(|unit| unit.span()).filter(|span| !previous_hit_spans.iter().any(|previous| ranges_overlap(previous, span)) && !primary_spans.iter().any(|primary| ranges_overlap(primary, span))).collect::<Vec<_>>();
        let spans = primary_spans.into_iter().chain(explicit_spans).map(|span| (UnitKind::Term, span)).collect::<Vec<_>>();
        let mut tree = self.rebuilt_with_resolved_spans(&spans);
        tree.set_term_hits(hits);
        tree
    }

}

fn ranges_overlap(left: &Range<usize>, right: &Range<usize>) -> bool { left.start < right.end && right.start < left.end }

#[cfg(test)]
mod tests {
    use super::*;
    use segment_lexer::LexicalKind;
    use std::hint::black_box;
    use std::time::Instant;

    #[test]
    fn typed_node_facts_are_read_from_node_semantic() {
        let format = NumberFormat::builder().decimal_separator(Some('.')).build();
        let semantic = NodeSemantic::Measurement(MeasurementSemantic::new(
            "kg".to_string(),
            vec![MeasurementValueFact::new(MeasurementDimension::Mass, 2_000_000_000)],
            vec![2_000_000_000],
            vec![format],
        ));
        let node = Node::with_semantic(0..4, None, semantic, Vec::new());

        let measurement = node.measurement_fact().expect("measurement fact");
        assert_eq!(measurement.unit(), "kg");
        assert_eq!(measurement.values(), &[MeasurementValueFact::new(MeasurementDimension::Mass, 2_000_000_000)]);
        assert_eq!(measurement.number_values(), &[2_000_000_000]);
        assert_eq!(measurement.number_formats(), &[format]);
        assert_eq!(node.first_number_format().and_then(|format| format.decimal_separator()), Some('.'));
    }

    #[test]
    fn placeable_hierarchy_exposes_broad_and_exact_classification() {
        assert_eq!(NodeKind::Version.placeable_kind(), Some(PlaceableKind::Protected(ProtectedKind::Version)));
        assert_eq!(NodeKind::Measurement.localizable_kind(), Some(LocalizableKind::Measurement));
        assert!(NodeKind::Placeholder.is_protected());
        assert!(NodeKind::Date.localizable_kind().is_some());
        assert!(NodeKind::Word.placeable_kind().is_none());
    }

    #[test]
    fn flat_unit_kinds_round_trip_through_hierarchical_node_kinds() {
        let kinds = UnitKind::ALL;
        kinds.into_iter().for_each(|kind| assert_eq!(kind.node_kind().unit_kind(), Some(kind), "{kind:?}"));
        [NodeKind::Root, NodeKind::CompoundWord, NodeKind::NumberRange, NodeKind::Ordinal, NodeKind::Ellipsis, NodeKind::Placeholder, NodeKind::Version].into_iter().for_each(|kind| assert_eq!(kind.unit_kind(), None, "{kind:?}"));
    }

    #[test]
    fn lexical_and_semantic_text_unit_boundaries_are_exhaustive() {
        let lexical = [(LexicalKind::Word, UnitKind::Word), (LexicalKind::Numeric, UnitKind::Numeric), (LexicalKind::Whitespace, UnitKind::Whitespace), (LexicalKind::Punct, UnitKind::Punct)];
        lexical.into_iter().for_each(|(lexical, unit)| assert_eq!(UnitKind::from(lexical), unit));
        let semantic_text = [UnitKind::Number, UnitKind::Date, UnitKind::Measurement, UnitKind::Time, UnitKind::Currency, UnitKind::IpAddress, UnitKind::Url, UnitKind::Email, UnitKind::Term, UnitKind::NonTranslatable, UnitKind::Abbreviation];
        semantic_text.into_iter().for_each(|kind| {
            assert!(kind.is_semantic_text(), "{kind:?}");
            assert_eq!(kind.node_kind().semantic_text_unit_kind(), Some(kind));
        });
        assert!(!UnitKind::Tag.is_semantic_text());
        assert_eq!(NodeKind::Tag.semantic_text_unit_kind(), None);
    }

    #[test]
    fn every_node_semantic_derives_its_exact_kind() {
        let range_typography = NumberRangeTypography::new(1, 2, '-', NumericTypographySpacing::Attached, NumericTypographySpacing::Attached, '–');
        let semantics = vec![
            (NodeSemantic::Number(NumberSemantic::new(1, NumberFormat::default())), NodeKind::Number),
            (NodeSemantic::NumberRange(NumberRangeSemantic::new(1, 2, range_typography)), NodeKind::NumberRange),
            (NodeSemantic::Date(DateFact::new(2024, 1, 1)), NodeKind::Date),
            (NodeSemantic::Time(TimeFact::new(12, 30, None)), NodeKind::Time),
            (NodeSemantic::Measurement(MeasurementSemantic::new("kg".to_string(), Vec::new(), vec![1], vec![NumberFormat::default()])), NodeKind::Measurement),
            (NodeSemantic::Currency(CurrencySemantic::new("EUR".to_string(), 1, NumberFormat::default())), NodeKind::Currency),
            (NodeSemantic::Ordinal(OrdinalFact::new(1, OrdinalStyle::EnglishSuffix, OrdinalStyle::EnglishSuffix)), NodeKind::Ordinal),
            (NodeSemantic::Placeholder(Box::new(PlaceholderSemantic::new("name".to_string(), "{name}".to_string()))), NodeKind::Placeholder),
            (NodeSemantic::Version(Box::new(VersionSemantic::new(VersionPrefixStyle::LowerV, vec!["1".to_string(), "0".to_string()]))), NodeKind::Version),
        ];
        semantics.into_iter().for_each(|(semantic, expected)| assert_eq!(semantic.node_kind(), expected));
    }

    #[test]
    fn localizable_and_protected_subtypes_have_stable_broad_classification() {
        [LocalizableKind::Number, LocalizableKind::NumberRange, LocalizableKind::Ordinal, LocalizableKind::Date, LocalizableKind::Time, LocalizableKind::Measurement, LocalizableKind::Currency].into_iter().for_each(|kind| {
            let node = NodeKind::Placeable(PlaceableKind::Localizable(kind));
            assert_eq!(node.localizable_kind(), Some(kind));
            assert!(node.localizable_kind().is_some() && !node.is_protected());
        });
        [ProtectedKind::Opaque, ProtectedKind::IpAddress, ProtectedKind::Url, ProtectedKind::Email, ProtectedKind::Placeholder, ProtectedKind::Version].into_iter().for_each(|kind| {
            let node = NodeKind::Placeable(PlaceableKind::Protected(kind));
            assert_eq!(node.protected_kind(), Some(kind));
            assert!(node.is_protected() && node.localizable_kind().is_none());
        });
    }

    #[test]
    fn semantic_construction_derives_the_exact_placeable_kind() {
        let semantic = NodeSemantic::Placeholder(Box::new(PlaceholderSemantic::new("name".to_string(), "{name}".to_string())));
        let node = Node::with_semantic(0..6, None, semantic, Vec::new());
        assert_eq!(node.kind(), NodeKind::Placeholder);
        assert_eq!(node.kind().protected_kind(), Some(ProtectedKind::Placeholder));
    }

    #[test]
    fn tree_global_term_hits_survive_node_tree_replacement() {
        let mut tree = SegmentTree::from_text("network");
        tree.set_term_hits(vec![TermHit::with_candidates(0..7, Vec::new())]);
        tree.apply_parse_output_internal(vec![Node::new(NodeKind::Term, 0..7, None, Vec::new())]);
        assert_eq!(tree.term_hits(), &[TermHit::with_candidates(0..7, Vec::new())]);
        assert!(tree.node_semantics.iter().all(|semantic| semantic.node_kind() != NodeKind::Term));
    }

    #[test]
    #[ignore = "profiling test; run explicitly with --ignored --nocapture"]
    fn measure_full_rebuild_cost() {
        let mut tree = SegmentTree::from_text(
            "Dr. Smith bought 1.234,56 kg of apples on 2024-06-29 and sent mail@test.com.",
        );
        let start = Instant::now();
        let mut checksum = 0usize;

        for _ in 0..10_000 {
            let inserted = tree.splice_unit_pieces(4..4, [SegmentPiece::text(" ")]);
            checksum = checksum.wrapping_add(inserted);
            let removed = tree.splice_unit_pieces(4..5, [SegmentPiece::text("")]);
            checksum = checksum.wrapping_add(removed);
            black_box(&tree);
        }

        let elapsed = start.elapsed();
        black_box((elapsed, checksum));
    }

    #[test]
    fn replace_text_updates_unit_lengths() {
        let mut tree = SegmentTree::from_text("hello world");

        assert_eq!(tree.replace_text("hello", "greetings"), 1);
        assert_eq!(tree.display_text(), "greetings world");
        assert_eq!(tree.text(), "greetings world");
        assert_eq!(tree.display_len(), "greetings world".len());
    }

    #[test]
    fn build_tokenizes_exact_terms_before_lexical_units() {
        let tree = SegmentTree::from_runs_with_resolved_spans([Run::new("Yellowstones")], &[(UnitKind::Term, 0..11)]);
        let units = tree.iter_units().map(|unit| (unit.kind(), unit.text().to_string())).collect::<Vec<_>>();
        assert_eq!(units, vec![(UnitKind::Term, "Yellowstone".to_string()), (UnitKind::Word, "s".to_string())]);
        assert!(tree.root().iter_children().any(|node| node.kind() == NodeKind::Term && node.text() == "Yellowstone"));
    }

}

#[cfg(test)]
mod tokenized_content_words_tests {
    use super::*;

    #[test]
    fn compact_core_layout_is_smaller_than_the_previous_inline_layout() {
        #[allow(dead_code)]
        struct PreviousTokenUnit {
            kind: UnitKind,
            start: u32,
            end: u32,
            tag_index: Option<u32>,
            token_id: Option<TokenUnitId>,
            semantic_id: Option<SemanticUnitId>,
        }

        #[allow(dead_code)]
        struct PreviousNode {
            kind: NodeKind,
            start: u32,
            end: u32,
            tag_index: Option<u32>,
            semantic: Option<NodeSemantic>,
            annotations: Vec<NodeAnnotation>,
            children: Vec<PreviousNode>,
        }

        assert!(std::mem::size_of::<TokenUnit>() < std::mem::size_of::<PreviousTokenUnit>());
        assert_eq!(std::mem::size_of::<TokenUnit>(), 16);
        assert!(std::mem::size_of::<Node>() < std::mem::size_of::<PreviousNode>());
        assert_eq!(std::mem::size_of::<StoredNode>(), 18);
        assert_eq!(word_model::stored_segment_word_size(), 40);
        assert_eq!(std::mem::size_of::<SegmentTree>(), 200);
        assert_eq!(std::mem::size_of::<UnitSemanticEntry>(), 10);
        assert_eq!(std::mem::size_of::<NodeSemantic>(), 32);
        assert_eq!(std::mem::size_of::<NodeRef<'static>>(), 16);
    }

    #[test]
    fn optional_pos_sidecar_is_sorted_and_resolved_by_stable_word_id() {
        let mut tree = SegmentTree::from_text("Close door");
        let words = tree.segment_words().iter().collect::<Vec<_>>();
        let close_id = words[0].token_unit_id();
        let door_id = words[1].token_unit_id();

        tree.replace_pos_annotations([PosAnnotation::new(door_id, PartOfSpeech::Noun), PosAnnotation::new(close_id, PartOfSpeech::Verb)]);

        assert_eq!(tree.pos_annotations().iter().map(|annotation| annotation.word_id()).collect::<Vec<_>>(), vec![close_id, door_id]);
        assert_eq!(tree.segment_words().iter().map(|word| word.part_of_speech()).collect::<Vec<_>>(), vec![Some(PartOfSpeech::Verb), Some(PartOfSpeech::Noun)]);
        assert_eq!(tree.preorder_iter().filter(|node| node.kind() == NodeKind::Word).map(|node| node.part_of_speech()).collect::<Vec<_>>(), vec![Some(PartOfSpeech::Verb), Some(PartOfSpeech::Noun)]);
    }

    #[test]
    fn flat_node_arena_preserves_paths_children_annotations_and_semantics() {
        let mut tree = SegmentTree::from_text("Alpha 2024-01-15");
        let mut word = Node::new(NodeKind::Word, 0..5, None, Vec::new());
        word = word.with_annotations(vec![NodeAnnotation::DictionaryKnown]);
        let term = Node::new(NodeKind::Term, 0..5, None, vec![word]);
        let date = Node::with_semantic(6..16, None, NodeSemantic::Date(DateFact::new(2024, 1, 15)), Vec::new());
        tree.apply_parse_output_internal(vec![term, date]);

        assert_eq!(&tree.node_children[tree.nodes[0].child_range()], &[1, 3]);
        assert_eq!((tree.nodes[1].kind, tree.nodes[3].kind), (NodeKind::Term, NodeKind::Date));
        assert_eq!(&tree.node_children[tree.nodes[1].child_range()], &[2]);
        assert_eq!(tree.nodes[2].kind, NodeKind::Word);
        assert_eq!(tree.preorder_iter().map(|node| node.kind()).collect::<Vec<_>>(), vec![NodeKind::Root, NodeKind::Term, NodeKind::Word, NodeKind::Date]);
        assert_eq!(tree.root().child(0).and_then(|node| node.child(0)).map(|node| node.annotations().to_vec()), Some(vec![NodeAnnotation::DictionaryKnown]));
        assert_eq!(tree.root().child(1).and_then(|node| node.date_fact()), Some(DateFact::new(2024, 1, 15)));
    }

    #[test]
    fn node_resolves_its_non_whitespace_token_unit_ids() {
        let mut tree = SegmentTree::from_text("12 kg");
        tree.apply_parse_output_internal(vec![Node::new(NodeKind::Measurement, 0..5, None, Vec::new())]);
        let node = tree.iter_nodes().next().expect("measurement node");
        let expected = tree.iter_units().filter(|unit| unit.kind() != UnitKind::Whitespace).map(|unit| unit.id()).collect::<Vec<_>>();

        assert_eq!(node.token_unit_ids(), expected);
    }

    #[test]
    fn number_format_builder_carries_parser_locale_interpretation() {
        let legacy = NumberFormat::builder().decimal_separator(Some('.')).build();
        let conforming = NumberFormat::builder().decimal_separator(Some(',')).locale_decimal_separator(Some(',')).assessment(NumberFormatAssessment::Canonical).build();
        let nonconforming = NumberFormat::builder().decimal_separator(Some('.')).locale_decimal_separator(Some(',')).assessment(NumberFormatAssessment::Foreign).build();
        let grouped = NumberFormat::builder()
            .decimal_separator(Some(','))
            .grouping_separator(Some('\u{00A0}'))
            .locale_decimal_separator(Some(','))
            .locale_grouping_separator(Some('\u{00A0}'))
            .fraction_digits(Some(2))
            .assessment(NumberFormatAssessment::Canonical)
            .build();

        assert_eq!(legacy.decimal_separator_matches_locale(), None);
        assert_eq!(conforming.decimal_separator_matches_locale(), Some(true));
        assert_eq!(nonconforming.decimal_separator_matches_locale(), Some(false));
        assert_eq!(legacy.decimal_separator(), nonconforming.decimal_separator());
        assert_eq!(grouped.grouping_separator(), Some('\u{00A0}'));
        assert_eq!(grouped.locale_grouping_separator(), Some('\u{00A0}'));
        assert_eq!(grouped.fraction_digits(), Some(2));
        assert_eq!(grouped.assessment(), NumberFormatAssessment::Canonical);
        assert_eq!(std::mem::size_of::<NumberFormat>(), 8);
    }

    #[test]
    fn numeric_fact_index_tracks_outermost_facts_across_tree_mutation() {
        let mut tree = SegmentTree::from_text("12 kg");
        let number = Node::with_semantic(0..2, None, NodeSemantic::Number(NumberSemantic::new(12, NumberFormat::default())), Vec::new());
        let measurement = Node::with_semantic(
            0..5,
            None,
            NodeSemantic::Measurement(MeasurementSemantic::new("kg".to_string(), Vec::new(), vec![12], vec![NumberFormat::default()])),
            vec![number],
        );
        tree.apply_parse_output_internal(vec![measurement]);

        assert_eq!(tree.numeric_fact_addresses.len(), 1);
        assert_eq!(tree.iter_nodes().next().map(|node| node.kind()), Some(NodeKind::Measurement));

        let number = Node::with_semantic(0..2, None, NodeSemantic::Number(NumberSemantic::new(12, NumberFormat::default())), Vec::new());
        tree.apply_parse_output_internal(vec![number]);
        assert_eq!(tree.numeric_fact_addresses.len(), 1);
        assert_eq!(tree.iter_nodes().next().map(|node| node.kind()), Some(NodeKind::Number));

        tree.apply_parse_output_internal(Vec::new());
        assert!(tree.numeric_fact_addresses.is_empty());
    }

    #[test]
    fn structure_fingerprint_tracks_tree_content() {
        let first = SegmentTree::from_text("Alpha");
        let same = SegmentTree::from_text("Alpha");
        let different = SegmentTree::from_text("Beta");

        assert_eq!(first.fingerprint(), same.fingerprint());
        assert_ne!(first.fingerprint(), different.fingerprint());
    }

    #[test]
    fn structured_edit_preserves_tags_outside_the_changed_text() {
        let tag = TagData::new("1".to_string(), "g".to_string(), TagKind::Standalone, "<g/>".to_string(), None, None);
        let base = SegmentTree::from_runs([Run::new("A "), Run::tag(tag), Run::new(" B")]);
        let replacement = base.replacement_for_text_span(0..1, "C").expect("text replacement");
        let tree = replacement.apply_to(&base).expect("fresh edit");

        assert_eq!(tree.text(), "C  B");
        assert_eq!(tree.iter_tag_units().filter_map(|unit| unit.tag()).map(TagData::id).collect::<Vec<_>>(), vec!["1"]);
    }

    #[test]
    fn structured_replacement_can_edit_inside_atomic_semantic_text() {
        let protected = [(UnitKind::Measurement, 4..8)];
        let tree = SegmentTree::from_runs_with_resolved_spans([Run::new("Use 5 lb")], &protected);
        let replacement = tree.replacement_for_text_span(6..8, "kg").expect("interior replacement");

        assert_eq!(replacement.apply_to(&tree).expect("fresh edit").text(), "Use 5 kg");
    }

    #[test]
    fn flat_node_arena_interns_identical_annotation_sets() {
        let mut first = Node::new(NodeKind::Word, 0..5, None, Vec::new());
        let mut second = Node::new(NodeKind::Word, 6..10, None, Vec::new());
        first = first.with_annotations(vec![NodeAnnotation::DictionaryKnown, NodeAnnotation::DictionaryUnknown]);
        second = second.with_annotations(vec![NodeAnnotation::DictionaryKnown, NodeAnnotation::DictionaryUnknown]);
        let mut tree = SegmentTree::from_text("Alpha beta");
        tree.apply_parse_output_internal(vec![first, second]);

        assert_eq!(tree.node_annotations.len(), 1);
        assert_eq!(tree.nodes[1].encoded_annotations_index, tree.nodes[2].encoded_annotations_index);
        assert_eq!(tree.iter_nodes().map(|node| node.annotations().to_vec()).collect::<Vec<_>>(), vec![
            vec![NodeAnnotation::DictionaryKnown, NodeAnnotation::DictionaryUnknown],
            vec![NodeAnnotation::DictionaryKnown, NodeAnnotation::DictionaryUnknown],
        ]);
    }

    #[test]
    fn plain_whitespace_leaf_is_virtual_and_keeps_its_stable_unit_id() {
        let mut tree = SegmentTree::from_text("Hello world!");
        let whitespace_id = tree.iter_units().find(|unit| unit.kind() == UnitKind::Whitespace).map(|unit| unit.id()).expect("whitespace unit");

        assert_eq!(tree.nodes.len(), 1);
        assert_eq!(tree.node_children.len(), tree.unit_count());
        assert_eq!(tree.preorder_iter().find(|node| node.kind() == NodeKind::Whitespace).map(|node| node.text().to_string()), Some(" ".to_string()));

        tree.replace_text("Hello", "Hi");

        assert_eq!(tree.iter_units().find(|unit| unit.kind() == UnitKind::Whitespace).map(|unit| unit.id()), Some(whitespace_id));
        assert_eq!(tree.preorder_iter().filter(|node| node.kind() == NodeKind::Whitespace).count(), 1);
    }

    #[test]
    fn compact_location_indexes_preserve_id_lookup() {
        let tree = SegmentTree::from_text("Alpha 20 beta");
        let expected = tree.iter_units().map(|unit| (unit.id(), unit.kind(), unit.span())).collect::<Vec<_>>();

        expected.iter().for_each(|(id, kind, span)| {
            let unit = tree.unit(tree.token_index(*id).expect("token index")).expect("token unit");
            assert_eq!((unit.kind(), unit.span()), (*kind, span.clone()));
        });
    }

    #[test]
    fn semantic_unit_ids_are_stored_per_token_unit() {
        let semantic_id = SemanticUnitId::new(81);
        let units = vec![
            TokenUnit::new_compact(UnitKind::Term, 0, 5, None).with_token_id(Some(TokenUnitId::new(91))),
            TokenUnit::new_compact(UnitKind::Whitespace, 5, 6, None).with_token_id(Some(TokenUnitId::new(92))),
            TokenUnit::new_compact(UnitKind::Term, 6, 10, None).with_token_id(Some(TokenUnitId::new(93))),
            TokenUnit::new_compact(UnitKind::Whitespace, 10, 11, None).with_token_id(Some(TokenUnitId::new(94))),
            TokenUnit::new_compact(UnitKind::Term, 11, 16, None).with_token_id(Some(TokenUnitId::new(95))),
        ];
        let tree = SegmentTree::from_tokenized_parts_with_semantic_ids("Alpha beta gamma".to_string(), units, Vec::new(), Vec::new(), Some(vec![Some(semantic_id), None, Some(semantic_id), None, None]));
        assert_eq!(tree.iter_units().map(|unit| unit.semantic_id()).collect::<Vec<_>>(), vec![Some(semantic_id), None, Some(semantic_id), None, Some(SemanticUnitId::new(82))]);
        assert_eq!(std::mem::size_of_val(tree.unit_semantics.as_ref()), 30);
    }

    #[test]
    fn compact_unit_kind_encoding_covers_every_kind() {
        let kinds = UnitKind::ALL;

        kinds.into_iter().enumerate().for_each(|(offset, kind)| {
            let unit = TokenUnit::new(kind, 0..0);
            assert_eq!(unit.kind(), kind, "{offset}");
        });
    }

    #[test]
    fn token_id_lookup_allocates_an_index_only_for_long_segments() {
        let short = SegmentTree::from_text("Alpha 20 beta");
        let long = SegmentTree::from_text("word ".repeat(80));
        let long_ids = long.iter_units().map(|unit| unit.id()).collect::<Vec<_>>();

        assert!(short.token_lookup.is_none());
        assert!(long.unit_count() > TOKEN_LOOKUP_INDEX_THRESHOLD);
        assert_eq!(long.token_lookup.as_ref().map(|entries| entries.len()), Some(long.unit_count()));
        assert_eq!(long.token_lookup.as_deref().map(std::mem::size_of_val), Some(long.unit_count() * std::mem::size_of::<u16>()));
        [0, long_ids.len() / 2, long_ids.len() - 1].into_iter().for_each(|index| {
            assert_eq!(long.token_index(long_ids[index]), Some(index));
        });
        assert!(long.token_index(TokenUnitId::new(u64::MAX)).is_none());
    }

    #[test]
    fn atomic_punctuation_check_rejects_legacy_combined_ellipsis_unit() {
        let current = SegmentTree::from_text("...");
        let legacy = SegmentTree::from_tokenized_parts_with_semantic_ids("...".to_string(), vec![TokenUnit::new(UnitKind::Punct, 0..3)], Vec::new(), Vec::new(), None);

        assert!(current.punctuation_units_are_atomic());
        assert!(!legacy.punctuation_units_are_atomic());
    }

    #[test]
    fn segment_tag_iterator_borrows_order_occurrence_and_spacing_from_tree() {
        let mut run = Run::new("A B C");
        run.push_tag_at(1, TagData::new("g".to_string(), "g".to_string(), TagKind::Standalone, "<g/>".to_string(), None, None));
        run.push_tag_at(3, TagData::new("g".to_string(), "g".to_string(), TagKind::Standalone, "<g/>".to_string(), None, None));
        run.push_tag_at(5, TagData::new("x".to_string(), "x".to_string(), TagKind::Standalone, "<x/>".to_string(), None, None));
        let tree = SegmentTree::from_runs([run]);
        let tags = tree.segment_tags();
        let iterated = tags.iter().collect::<Vec<_>>();

        assert_eq!(tags.len(), 3);
        assert_eq!(iterated.iter().map(SegmentTag::id).collect::<Vec<_>>(), vec!["g", "g", "x"]);
        assert_eq!(iterated.iter().map(SegmentTag::occurrence).collect::<Vec<_>>(), vec![0, 1, 0]);
        assert_eq!(iterated.iter().map(SegmentTag::label).collect::<Vec<_>>(), vec!["<1/>", "<1/>", "<2/>"]);
        assert_eq!(iterated[0].whitespace_after(), WhitespaceTypographyKind::Regular);
        assert_eq!(iterated[0].whitespace_before(), WhitespaceTypographyKind::Absent);
    }

    #[test]
    fn segment_words_own_normalization_attributes_and_separators() {
        let tree = SegmentTree::from_text("Alpha \t beta, BETA");
        let words = tree.segment_words();

        assert_eq!(words.iter().map(SegmentWordRef::text).collect::<Vec<_>>(), vec!["Alpha", "beta", "BETA"]);
        assert_eq!(words.iter().map(SegmentWordRef::normalized).collect::<Vec<_>>(), vec!["alpha", "beta", "beta"]);
        assert!(words.iter().next().is_some_and(|word| word.is_capitalized() && word.has_at_least_alphabetic(5)));
    }

    #[test]
    fn segment_words_are_stored_once_and_rebuilt_by_tree_mutation() {
        let mut tree = SegmentTree::from_text("Alpha beta");
        let first = tree.segment_words();
        let second = tree.segment_words();

        assert!(first.shares_storage_with(second));

        tree.replace_text("Alpha", "Gamma");

        assert_eq!(tree.segment_words().iter().next().map(SegmentWordRef::normalized), Some("gamma"));
    }

    #[test]
    fn segment_words_project_dictionary_annotations_from_word_nodes() {
        let mut known = Node::new(NodeKind::Word, 0..5, None, Vec::new());
        let mut unknown = Node::new(NodeKind::Word, 6..10, None, Vec::new());
        known = known.with_annotations(vec![NodeAnnotation::DictionaryKnown]);
        unknown = unknown.with_annotations(vec![NodeAnnotation::DictionaryUnknown]);
        let mut tree = SegmentTree::from_text("Alpha beta");
        tree.apply_parse_output_internal(vec![known, unknown]);
        let words = tree.segment_words();

        assert!(words.has_dictionary_annotations());
        let projected = words.iter().collect::<Vec<_>>();
        assert!(projected[0].dictionary_known());
        assert!(!projected[1].dictionary_known());
    }

    #[test]
    fn content_unit_iteration_exposes_absolute_token_spans() {
        let tree = SegmentTree::from_text("Hi, 20 kg!");
        let units = tree.iter_content_units().collect::<Vec<_>>();

        assert_eq!(units.iter().map(|unit| unit.text()).collect::<Vec<_>>(), vec!["Hi", "20", "kg"]);
        assert_eq!(units.iter().map(UnitRef::span).collect::<Vec<_>>(), vec![0..2, 4..6, 7..9]);
    }

    #[test]
    fn content_unit_iteration_excludes_layout_units_and_keeps_semantic_units_atomic() {
        let mut run = Run::new("Term, 20 kg ");
        run.push_tag(TagData::new("1".to_string(), "x".to_string(), TagKind::Standalone, "<x/>".to_string(), None, None));
        run.push_text(" 42");
        let tree = SegmentTree::from_runs_with_resolved_spans([run], &[(UnitKind::Term, 0..4), (UnitKind::Measurement, 6..11)]);
        let units = tree.iter_content_units().collect::<Vec<_>>();

        assert_eq!(units.iter().map(|unit| (unit.kind(), unit.text(), unit.span())).collect::<Vec<_>>(), vec![(UnitKind::Term, "Term", 0..4), (UnitKind::Measurement, "20 kg", 6..11), (UnitKind::Numeric, "42", 13..15)]);
        assert!(tree.iter_units().any(|unit| matches!(unit.kind(), UnitKind::Punct | UnitKind::Whitespace | UnitKind::Tag)));
    }

}
