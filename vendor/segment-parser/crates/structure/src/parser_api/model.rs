use std::ops::Range;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::super::{CurrencySemantic, DateFact, DictionaryAnalysis, DelimiterAnnotation, MeasurementSemantic, NodeKind, NumberRangeSemantic, NumberSemantic, OrdinalFact, PlaceholderSemantic, TimeFact, TokenUnitId, UnitKind, VersionSemantic, WhitespaceTypographyFact, decode_optional_index, encode_optional_index};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenUnit {
    id: TokenUnitId,
    start: u16,
    end: u16,
    encoded_kind_or_tag_index: u16,
}

impl TokenUnit {
    pub fn new(kind: UnitKind, span: Range<usize>) -> Self { Self::new_compact(kind, span.start, span.end, None) }
    pub(in super::super) fn new_compact(kind: UnitKind, start: usize, end: usize, tag_index: Option<u16>) -> Self { Self { id: TokenUnitId::new(0), start: u16::try_from(start).expect("unit start exceeds u16 range"), end: u16::try_from(end).expect("unit end exceeds u16 range"), encoded_kind_or_tag_index: encode_kind_or_tag_index(kind, tag_index) } }
    pub(in super::super) fn with_token_id(mut self, token_id: Option<TokenUnitId>) -> Self { if let Some(id) = token_id { self.id = id; } self }
    pub(in super::super) fn set_token_id(&mut self, id: TokenUnitId) { self.id = id; }
    pub(in super::super) fn stored_id(&self) -> TokenUnitId { self.id }
    pub fn kind(&self) -> UnitKind { if self.encoded_kind_or_tag_index >= TAG_WITHOUT_INDEX_CODE { UnitKind::Tag } else { UnitKind::from_compact_code(self.encoded_kind_or_tag_index as u8).expect("stored unit kind must be valid") } }
    pub fn start(&self) -> usize { self.start as usize }
    pub fn end(&self) -> usize { self.end as usize }
    pub fn span(&self) -> Range<usize> { self.start()..self.end() }
    pub fn tag_index(&self) -> Option<usize> { self.encoded_kind_or_tag_index.checked_sub(FIRST_TAG_INDEX_CODE).map(usize::from) }
    pub fn token_id(&self) -> Option<TokenUnitId> { (self.id.value() != 0).then_some(self.id) }
    pub fn is_tag(&self) -> bool { self.kind() == UnitKind::Tag }
    pub(in super::super) fn len(&self) -> usize { self.end().saturating_sub(self.start()) }
}

const TAG_WITHOUT_INDEX_CODE: u16 = 15;
const FIRST_TAG_INDEX_CODE: u16 = 16;

fn encode_kind_or_tag_index(kind: UnitKind, tag_index: Option<u16>) -> u16 {
    if kind != UnitKind::Tag { assert!(tag_index.is_none(), "only tag units may have a tag index"); return u16::from(kind.compact_code()); }
    tag_index.map(|index| FIRST_TAG_INDEX_CODE.checked_add(index).expect("tag index exceeds compact tag range")).unwrap_or(TAG_WITHOUT_INDEX_CODE)
}

#[derive(Serialize, Deserialize)]
struct SerializedTokenUnit {
    location: SerializedUnitLocation,
}

#[derive(Serialize, Deserialize)]
struct SerializedUnitLocation {
    id: TokenUnitId,
    start: u16,
    end: u16,
    unit_index: u16,
    encoded_kind_or_tag_index: u16,
}

impl Serialize for TokenUnit {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> { SerializedTokenUnit { location: SerializedUnitLocation { id: self.id, start: self.start, end: self.end, unit_index: 0, encoded_kind_or_tag_index: self.encoded_kind_or_tag_index } }.serialize(serializer) }
}

impl<'de> Deserialize<'de> for TokenUnit {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> { let stored = SerializedTokenUnit::deserialize(deserializer)?.location; Ok(Self { id: stored.id, start: stored.start, end: stored.end, encoded_kind_or_tag_index: stored.encoded_kind_or_tag_index }) }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeAnnotation {
    DictionaryKnown,
    DictionaryUnknown,
    DictionaryBaseform(String),
    Delimiter(DelimiterAnnotation),
    WhitespaceTypography(WhitespaceTypographyFact),
    DictionaryAnalysis(Box<DictionaryAnalysis>),
}

impl NodeAnnotation {
    pub(in crate::tree) fn is_dictionary_known(&self) -> bool { matches!(self, Self::DictionaryKnown) }
    pub(in crate::tree) fn is_dictionary_unknown(&self) -> bool { matches!(self, Self::DictionaryUnknown) }
    pub(in crate::tree) fn dictionary_baseform(&self) -> Option<&str> { match self { Self::DictionaryBaseform(baseform) => Some(baseform), _ => None } }
    pub(in crate::tree) fn dictionary_analysis(&self) -> Option<&DictionaryAnalysis> { match self { Self::DictionaryAnalysis(analysis) => Some(analysis), _ => None } }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeSemantic {
    Number(NumberSemantic),
    NumberRange(NumberRangeSemantic),
    Date(DateFact),
    Time(TimeFact),
    Measurement(MeasurementSemantic),
    Currency(CurrencySemantic),
    Ordinal(OrdinalFact),
    Placeholder(Box<PlaceholderSemantic>),
    Version(Box<VersionSemantic>),
}

impl NodeSemantic {
    pub fn node_kind(&self) -> NodeKind {
        match self {
            Self::Number(_) => NodeKind::Number,
            Self::NumberRange(_) => NodeKind::NumberRange,
            Self::Date(_) => NodeKind::Date,
            Self::Time(_) => NodeKind::Time,
            Self::Measurement(_) => NodeKind::Measurement,
            Self::Currency(_) => NodeKind::Currency,
            Self::Ordinal(_) => NodeKind::Ordinal,
            Self::Placeholder(_) => NodeKind::Placeholder,
            Self::Version(_) => NodeKind::Version,
        }
    }
    pub(in crate::tree) fn placeholder(&self) -> Option<&PlaceholderSemantic> { match self { Self::Placeholder(placeholder) => Some(placeholder), _ => None } }
    pub(in crate::tree) fn version(&self) -> Option<&VersionSemantic> { match self { Self::Version(version) => Some(version), _ => None } }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub(in super::super) kind: NodeKind,
    pub(in super::super) start: u16,
    pub(in super::super) end: u16,
    pub(in super::super) encoded_tag_index: u16,
    pub(in super::super) semantic: Option<Box<NodeSemantic>>,
    pub(in super::super) annotations: Box<[NodeAnnotation]>,
    pub(in super::super) children: Box<[Node]>,
}

impl Node {
    pub fn new(kind: NodeKind, span: Range<usize>, tag_index: Option<u16>, children: Vec<Node>) -> Self { Self::from_parts(kind, span, tag_index, None, children) }
    pub fn with_semantic(span: Range<usize>, tag_index: Option<u16>, semantic: NodeSemantic, children: Vec<Node>) -> Self { let kind = semantic.node_kind(); Self::from_parts(kind, span, tag_index, Some(semantic), children) }
    pub(in super::super) fn from_parts(kind: NodeKind, span: Range<usize>, tag_index: Option<u16>, semantic: Option<NodeSemantic>, children: Vec<Node>) -> Self { assert!(semantic.as_ref().map(NodeSemantic::node_kind).is_none_or(|semantic_kind| semantic_kind == kind), "node kind must agree with semantic payload"); let start = u16::try_from(span.start).expect("node start exceeds u16 range"); let end = u16::try_from(span.end).expect("node end exceeds u16 range"); Self { kind, start, end, encoded_tag_index: encode_optional_index(tag_index), semantic: semantic.map(Box::new), annotations: Box::default(), children: children.into_boxed_slice() } }
    pub fn kind(&self) -> NodeKind { self.kind }
    pub fn annotations(&self) -> &[NodeAnnotation] { &self.annotations }
    pub fn with_annotation(mut self, annotation: NodeAnnotation) -> Self { self.annotations = self.annotations.into_vec().into_iter().chain([annotation]).collect(); self }
    pub fn semantic(&self) -> Option<&NodeSemantic> { self.semantic.as_deref() }
    pub fn iter_children(&self) -> std::slice::Iter<'_, Node> { self.children.iter() }
    pub fn with_children(mut self, children: Vec<Node>) -> Self { self.children = children.into_boxed_slice(); self }
    pub fn with_kind(mut self, kind: NodeKind) -> Self { assert!(self.semantic.as_ref().map(|semantic| semantic.node_kind()).is_none_or(|semantic_kind| semantic_kind == kind), "node kind must agree with semantic payload"); self.kind = kind; self }
    pub fn with_annotations(mut self, annotations: Vec<NodeAnnotation>) -> Self { self.annotations = annotations.into_boxed_slice(); self }
    pub fn start(&self) -> usize { self.start as usize }
    pub fn end(&self) -> usize { self.end as usize }
    pub fn tag_index(&self) -> Option<usize> { decode_optional_index(self.encoded_tag_index).map(|index| index as usize) }
    pub fn span(&self) -> Range<usize> { self.start()..self.end() }
}
