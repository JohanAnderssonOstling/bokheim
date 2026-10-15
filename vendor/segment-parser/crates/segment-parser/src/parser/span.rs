use std::ops::Range;

use structure::parser_api::{Node, NodeSemantic};
use structure::{NodeKind, SegmentTree, TokenUnitId};
use super::{DetectionPriority, PhraseAmbiguity, PhraseConfidence};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Detected {
    Simple { kind: NodeKind, units: UnitRange },
    Language { kind: NodeKind, units: UnitRange, span: Range<usize> },
    Semantic { units: UnitRange, semantic: NodeSemantic },
    CompoundWord { units: UnitRange, abbreviation_units: Vec<UnitRange> },
}

impl Detected {
    pub(super) fn simple(kind: NodeKind, units: UnitRange) -> Self { Self::Simple { kind, units } }
    pub(super) fn semantic(units: UnitRange, semantic: NodeSemantic) -> Self { Self::Semantic { units, semantic } }
    pub(super) fn language(kind: NodeKind, units: UnitRange, span: Range<usize>) -> Self { Self::Language { kind, units, span } }
    pub(super) fn compound_word(units: UnitRange, abbreviation_units: Vec<UnitRange>) -> Self { Self::CompoundWord { units, abbreviation_units } }
    pub(super) fn units(&self) -> &UnitRange {
        match self {
            Self::Simple { units, .. } | Self::Language { units, .. } | Self::Semantic { units, .. } | Self::CompoundWord { units, .. } => units,
        }
    }
    pub(super) fn units_end(&self) -> usize { self.units().end_index() }
    pub(super) fn node_kind(&self) -> NodeKind {
        match self {
            Self::Simple { kind, .. } | Self::Language { kind, .. } => *kind,
            Self::Semantic { semantic, .. } => semantic.node_kind(),
            Self::CompoundWord { .. } => NodeKind::CompoundWord,
        }
    }
    pub(super) fn with_units_start(mut self, stream: &SegmentTree, start: usize) -> Self {
        match &mut self {
            Self::Simple { units, .. } | Self::Language { units, .. } | Self::Semantic { units, .. } | Self::CompoundWord { units, .. } => units.set_start(stream, start),
        }
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Detection {
    detected: Detected,
    priority: DetectionPriority,
    confidence: PhraseConfidence,
    ambiguity: PhraseAmbiguity,
}

impl Detection {
    fn new(detected: Detected, priority: DetectionPriority, confidence: PhraseConfidence, ambiguity: PhraseAmbiguity) -> Self { Self { detected, priority, confidence, ambiguity } }
    pub(super) fn protected(detected: Detected) -> Self { Self::new(detected, DetectionPriority::Protected, PhraseConfidence::High, PhraseAmbiguity::Certain) }
    pub(super) fn grounded_semantic(detected: Detected, confidence: PhraseConfidence, ambiguity: PhraseAmbiguity) -> Self { Self::new(detected, DetectionPriority::GroundedSemantic, confidence, ambiguity) }
    pub(super) fn weak_language(detected: Detected) -> Self { Self::new(detected, DetectionPriority::WeakLanguage, PhraseConfidence::Low, PhraseAmbiguity::Ambiguous) }
    pub(super) fn fallback(detected: Detected, ambiguity: PhraseAmbiguity) -> Self { Self::new(detected, DetectionPriority::Fallback, PhraseConfidence::Low, ambiguity) }
    pub(super) fn detected(&self) -> &Detected { &self.detected }
    pub(super) fn priority(&self) -> DetectionPriority { self.priority }
    pub(super) fn confidence(&self) -> PhraseConfidence { self.confidence }
    pub(super) fn ambiguity(&self) -> PhraseAmbiguity { self.ambiguity }
    pub(super) fn resolve(self) -> Detected { self.detected }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct UnitRange {
    indices: Range<usize>,
    first_id: TokenUnitId,
    last_id: TokenUnitId,
}

impl UnitRange {
    pub(super) fn new(stream: &SegmentTree, indices: Range<usize>) -> Self {
        assert!(indices.start < indices.end, "parser unit ranges must be non-empty");
        let first_id = structure::parser_api::units(stream)[indices.start].token_id().expect("tree units must have stable token ids");
        let last_id = structure::parser_api::units(stream)[indices.end - 1].token_id().expect("tree units must have stable token ids");
        Self { indices, first_id, last_id }
    }
    pub(super) fn indices(&self) -> Range<usize> { self.indices.clone() }
    pub(super) fn end_index(&self) -> usize { self.indices.end }
    pub(super) fn first_id(&self) -> TokenUnitId { self.first_id }
    pub(super) fn last_id(&self) -> TokenUnitId { self.last_id }
    pub(super) fn set_start(&mut self, stream: &SegmentTree, start: usize) {
        self.indices.start = start;
        self.first_id = structure::parser_api::units(stream)[start].token_id().expect("tree units must have stable token ids");
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ParseOutput {
    pub nodes: Vec<Node>,
}

/// Orders competing phrase candidates; derived `Ord` compares fields top to bottom.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct CandidateScore {
    priority: DetectionPriority,
    confidence: PhraseConfidence,
    ambiguity: PhraseAmbiguity,
    span_len: usize,
    order: std::cmp::Reverse<usize>,
}

impl CandidateScore {
    pub(super) fn new(priority: DetectionPriority, confidence: PhraseConfidence, ambiguity: PhraseAmbiguity, span_len: usize, order: usize) -> Self { Self { priority, confidence, ambiguity, span_len, order: std::cmp::Reverse(order) } }
}
