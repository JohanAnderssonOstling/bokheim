use std::ops::Range;

use serde::{Deserialize, Serialize};

use crate::SemanticUnitId;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum TermMatchKind {
    Surface,
    DictionaryBaseform,
    Coverage,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct TermHitCandidate {
    canonical_source: String,
    match_kind: TermMatchKind,
}

impl TermHitCandidate {
    pub fn new(canonical_source: String, match_kind: TermMatchKind) -> Self { Self { canonical_source, match_kind } }
    pub fn canonical_source(&self) -> &str { &self.canonical_source }
    pub fn match_kind(&self) -> TermMatchKind { self.match_kind }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TermHit {
    span: Range<usize>,
    candidates: Box<[TermHitCandidate]>,
}

impl TermHit {
    pub fn with_candidate(span: Range<usize>, candidate: TermHitCandidate) -> Self { Self { span, candidates: vec![candidate].into_boxed_slice() } }
    pub fn with_candidates(span: Range<usize>, candidates: Vec<TermHitCandidate>) -> Self { Self { span, candidates: candidates.into_boxed_slice() } }
    pub fn span(&self) -> Range<usize> { self.span.clone() }
    pub fn candidates(&self) -> &[TermHitCandidate] { &self.candidates }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct SegmentAuxiliaryStorage {
    hits: Box<[TermHit]>,
    #[serde(default)]
    inline_placeables: Box<[StoredInlinePlaceable]>,
}

impl SegmentAuxiliaryStorage {
    pub(super) fn new(hits: Vec<TermHit>) -> Self { Self { hits: hits.into_boxed_slice(), inline_placeables: Box::default() } }
    pub(super) fn hits(&self) -> &[TermHit] { &self.hits }
    pub(super) fn set_hits(&mut self, hits: Vec<TermHit>) { self.hits = hits.into_boxed_slice(); }
    pub(super) fn inline_placeables(&self) -> &[StoredInlinePlaceable] { &self.inline_placeables }
    pub(super) fn set_inline_placeables(&mut self, placeables: Vec<StoredInlinePlaceable>) { self.inline_placeables = placeables.into_boxed_slice(); }
    pub(super) fn is_empty(&self) -> bool { self.hits.is_empty() && self.inline_placeables.is_empty() }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct StoredInlinePlaceable {
    start: u16,
    end: u16,
    semantic_id: Option<SemanticUnitId>,
    external_id: Option<String>,
}

impl StoredInlinePlaceable {
    pub(super) fn new(span: Range<usize>, semantic_id: Option<SemanticUnitId>, external_id: Option<String>) -> Self { Self { start: u16::try_from(span.start).expect("inline placeable start exceeds u16 range"), end: u16::try_from(span.end).expect("inline placeable end exceeds u16 range"), semantic_id, external_id } }
    pub(super) fn span(&self) -> Range<usize> { self.start as usize..self.end as usize }
    pub(super) fn semantic_id(&self) -> Option<SemanticUnitId> { self.semantic_id }
    pub(super) fn external_id(&self) -> Option<&str> { self.external_id.as_deref() }
}
