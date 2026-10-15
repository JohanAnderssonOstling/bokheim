use std::ops::Range;

use serde::{Deserialize, Serialize};

use crate::{SegmentTree, TokenUnitId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PunctuationSpacingSide {
    Before,
    After,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WhitespaceTypographyKind {
    Absent,
    Regular,
    NonBreaking,
    NarrowNonBreaking,
    Thin,
    Tab,
    LineBreak,
    Other,
}

impl WhitespaceTypographyKind {
    pub fn from_text(text: &str) -> Self {
        if text.is_empty() {
            return Self::Absent;
        }
        if text.chars().all(|character| matches!(character, '\r' | '\n')) {
            return Self::LineBreak;
        }
        let mut characters = text.chars();
        let Some(character) = characters.next().filter(|_| characters.next().is_none()) else { return Self::Other; };
        match character {
            ' ' => Self::Regular,
            '\u{00A0}' => Self::NonBreaking,
            '\u{202F}' => Self::NarrowNonBreaking,
            '\u{2009}' => Self::Thin,
            '\t' => Self::Tab,
            _ => Self::Other,
        }
    }
    pub fn text(self) -> Option<&'static str> {
        match self {
            Self::Absent => Some(""),
            Self::Regular => Some(" "),
            Self::NonBreaking => Some("\u{00A0}"),
            Self::NarrowNonBreaking => Some("\u{202F}"),
            Self::Thin => Some("\u{2009}"),
            Self::Tab => Some("\t"),
            Self::LineBreak => Some("\n"),
            Self::Other => None,
        }
    }
    pub fn description(self) -> &'static str {
        match self {
            Self::Absent => "none",
            Self::Regular => "ordinary space",
            Self::NonBreaking => "non-breaking space",
            Self::NarrowNonBreaking => "narrow non-breaking space",
            Self::Thin => "thin space",
            Self::Tab => "tab",
            Self::LineBreak => "line break",
            Self::Other => "other whitespace",
        }
    }
    pub fn is_present(self) -> bool { self != Self::Absent }
    pub fn is_non_breaking(self) -> bool { matches!(self, Self::NonBreaking | Self::NarrowNonBreaking) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
enum WhitespaceTypographyAcceptance {
    Exact,
    AnyNonBreaking,
    AnyPresent,
}

impl WhitespaceTypographyAcceptance {
    fn accepts_alternative(self, observed: WhitespaceTypographyKind) -> bool {
        match self {
            Self::Exact => false,
            Self::AnyNonBreaking => observed.is_non_breaking(),
            Self::AnyPresent => observed.is_present(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WhitespaceTypographyPreference {
    preferred: WhitespaceTypographyKind,
    acceptance: WhitespaceTypographyAcceptance,
}

impl WhitespaceTypographyPreference {
    const fn new(preferred: WhitespaceTypographyKind, acceptance: WhitespaceTypographyAcceptance) -> Self { Self { preferred, acceptance } }
    pub const fn exact(preferred: WhitespaceTypographyKind) -> Self { Self::new(preferred, WhitespaceTypographyAcceptance::Exact) }
    pub const fn any_non_breaking(preferred: WhitespaceTypographyKind) -> Self { Self::new(preferred, WhitespaceTypographyAcceptance::AnyNonBreaking) }
    pub const fn any_present(preferred: WhitespaceTypographyKind) -> Self { Self::new(preferred, WhitespaceTypographyAcceptance::AnyPresent) }
    pub fn preferred(self) -> WhitespaceTypographyKind { self.preferred }
    fn assess(self, observed: WhitespaceTypographyKind) -> WhitespaceTypographyAssessment {
        if observed == self.preferred {
            return WhitespaceTypographyAssessment::Canonical;
        }
        if self.acceptance.accepts_alternative(observed) { WhitespaceTypographyAssessment::Accepted } else { WhitespaceTypographyAssessment::Foreign }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WhitespaceTypographyRole {
    LexicalSeparator,
    BeforePunctuation,
    AfterPunctuation,
    MeasurementSeparator,
    PercentSeparator,
    NumericGrouping,
    DashSpacing,
    QuoteInterior,
    Layout,
    Unknown,
    BracketInterior,
    Boundary,
}

impl WhitespaceTypographyRole {
    pub fn punctuation_side(self) -> Option<PunctuationSpacingSide> {
        match self {
            Self::BeforePunctuation => Some(PunctuationSpacingSide::Before),
            Self::AfterPunctuation => Some(PunctuationSpacingSide::After),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WhitespaceTypographyAssessment {
    Canonical,
    Accepted,
    Foreign,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WhitespaceTypographyFact {
    observed: WhitespaceTypographyKind,
    role: WhitespaceTypographyRole,
    preferred: Option<WhitespaceTypographyKind>,
    assessment: WhitespaceTypographyAssessment,
    span_start: u16,
    span_end: u16,
    subject_start: u16,
    subject_end: u16,
}

impl Default for WhitespaceTypographyFact {
    fn default() -> Self { Self { observed: WhitespaceTypographyKind::Absent, role: WhitespaceTypographyRole::Unknown, preferred: None, assessment: WhitespaceTypographyAssessment::Unknown, span_start: 0, span_end: 0, subject_start: 0, subject_end: 0 } }
}

impl WhitespaceTypographyFact {
    pub fn new(observed: WhitespaceTypographyKind, role: WhitespaceTypographyRole, preferred: Option<WhitespaceTypographyKind>, assessment: WhitespaceTypographyAssessment, span: Range<usize>, subject_span: Range<usize>) -> Self {
        Self {
            observed,
            role,
            preferred,
            assessment,
            span_start: u16::try_from(span.start).expect("whitespace span start exceeds u16 range"),
            span_end: u16::try_from(span.end).expect("whitespace span end exceeds u16 range"),
            subject_start: u16::try_from(subject_span.start).expect("whitespace subject start exceeds u16 range"),
            subject_end: u16::try_from(subject_span.end).expect("whitespace subject end exceeds u16 range"),
        }
    }
    pub fn from_preference(observed: WhitespaceTypographyKind, role: WhitespaceTypographyRole, preference: WhitespaceTypographyPreference, span: Range<usize>, subject_span: Range<usize>) -> Self { Self::new(observed, role, Some(preference.preferred()), preference.assess(observed), span, subject_span) }
    pub fn observed(&self) -> WhitespaceTypographyKind { self.observed }
    pub fn observed_text<'a>(&self, tree: &'a SegmentTree) -> &'a str { tree.text.get(self.span()).unwrap_or("") }
    pub fn role(&self) -> WhitespaceTypographyRole { self.role }
    pub fn preferred(&self) -> Option<WhitespaceTypographyKind> { self.preferred }
    pub(in crate::tree) fn preferred_text(&self) -> Option<&'static str> { self.preferred.and_then(WhitespaceTypographyKind::text) }
    pub fn assessment(&self) -> WhitespaceTypographyAssessment { self.assessment }
    pub fn span(&self) -> Range<usize> { self.span_start as usize..self.span_end as usize }
    pub fn subject_span(&self) -> Range<usize> { self.subject_start as usize..self.subject_end as usize }
    pub fn token_unit_ids(&self, tree: &SegmentTree) -> Vec<TokenUnitId> {
        let ids = tree.token_unit_ids_for_span(self.span());
        if ids.is_empty() { tree.token_unit_ids_for_span(self.subject_span()) } else { ids }
    }
    pub fn punctuation_side(&self) -> Option<PunctuationSpacingSide> { self.role.punctuation_side() }
}

impl SegmentTree {
    pub fn punctuation_whitespace_facts(&self) -> impl Iterator<Item = WhitespaceTypographyFact> + '_ { self.stored_whitespace_typography_facts_internal().filter(|fact| matches!(fact.role(), WhitespaceTypographyRole::BeforePunctuation | WhitespaceTypographyRole::AfterPunctuation)) }
    pub fn quote_interior_whitespace_facts(&self) -> impl Iterator<Item = WhitespaceTypographyFact> + '_ { self.stored_whitespace_typography_facts_internal().filter(|fact| fact.role() == WhitespaceTypographyRole::QuoteInterior) }
    pub fn bracket_interior_whitespace_facts(&self) -> impl Iterator<Item = WhitespaceTypographyFact> + '_ { self.stored_whitespace_typography_facts_internal().filter(|fact| fact.role() == WhitespaceTypographyRole::BracketInterior) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_acceptance_only_canonicalizes_the_preferred_kind() {
        let preference = WhitespaceTypographyPreference::exact(WhitespaceTypographyKind::Absent);

        assert_eq!(preference.assess(WhitespaceTypographyKind::Absent), WhitespaceTypographyAssessment::Canonical);
        assert_eq!(preference.assess(WhitespaceTypographyKind::Regular), WhitespaceTypographyAssessment::Foreign);
    }

    #[test]
    fn non_breaking_acceptance_rejects_breaking_whitespace() {
        let non_breaking = WhitespaceTypographyPreference::any_non_breaking(WhitespaceTypographyKind::NonBreaking);

        assert_eq!(non_breaking.assess(WhitespaceTypographyKind::NarrowNonBreaking), WhitespaceTypographyAssessment::Accepted);
        assert_eq!(non_breaking.assess(WhitespaceTypographyKind::Regular), WhitespaceTypographyAssessment::Foreign);
    }

    #[test]
    fn any_present_accepts_variants_but_not_omission() {
        let preference = WhitespaceTypographyPreference::any_present(WhitespaceTypographyKind::NonBreaking);

        assert_eq!(preference.assess(WhitespaceTypographyKind::NonBreaking), WhitespaceTypographyAssessment::Canonical);
        assert_eq!(preference.assess(WhitespaceTypographyKind::Regular), WhitespaceTypographyAssessment::Accepted);
        assert_eq!(preference.assess(WhitespaceTypographyKind::Absent), WhitespaceTypographyAssessment::Foreign);
    }
}
