//! Language catalogs and plain-text evidence for abbreviation recognition.
//! These rules classify text; consumers decide how to render or pronounce it.
use std::ops::Range;

mod catalogs;
mod context;
pub use catalogs::{ENGLISH_POLICY, GERMAN_POLICY, SWEDISH_POLICY};
pub use context::{has_continuation_evidence, is_dotted_initialism, is_proper_name_token, is_reference_token, next_lexical_token, previous_lexical_token};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbbreviationCase {
    CaseInsensitive,
    CaseInsensitiveExceptAllCaps,
    ExactCase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbbreviationEvidence {
    Listed,
    Continuation,
    FollowingProperName,
    PrecedingProperName,
    PrecedingReference,
    TerminalCompatible,
}

impl AbbreviationEvidence {
    /// Checks the original parser's evidence rules against a UTF-8 byte span.
    pub fn is_satisfied_by(self, text: &str, span: Range<usize>) -> bool {
        if text.get(span.clone()).is_none() { return false; }
        match self {
            Self::Listed | Self::TerminalCompatible => true,
            Self::Continuation => has_continuation_evidence(text, span.end),
            Self::FollowingProperName => next_lexical_token(text, span.end).is_some_and(is_proper_name_token),
            Self::PrecedingProperName => previous_lexical_token(text, span.start).is_some_and(is_proper_name_token),
            Self::PrecedingReference => previous_lexical_token(text, span.start).is_some_and(is_reference_token),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AbbreviationRule {
    spellings: &'static [&'static str],
    case: AbbreviationCase,
    evidence: AbbreviationEvidence,
}

impl AbbreviationRule {
    pub const fn new(spellings: &'static [&'static str], case: AbbreviationCase, evidence: AbbreviationEvidence) -> Self { Self { spellings, case, evidence } }
    pub const fn spellings(self) -> &'static [&'static str] { self.spellings }
    pub const fn case(self) -> AbbreviationCase { self.case }
    pub const fn evidence(self) -> AbbreviationEvidence { self.evidence }

    fn matches(self, exact: &str, normalized: &str) -> bool {
        match self.case {
            AbbreviationCase::CaseInsensitive => self.spellings.contains(&normalized),
            AbbreviationCase::CaseInsensitiveExceptAllCaps => !is_all_caps(exact) && self.spellings.contains(&normalized),
            AbbreviationCase::ExactCase => self.spellings.contains(&exact),
        }
    }

    fn matches_prefix(self, exact: &str, normalized: &str) -> bool {
        match self.case {
            AbbreviationCase::CaseInsensitive => self.spellings.iter().any(|spelling| spelling.starts_with(normalized)),
            AbbreviationCase::CaseInsensitiveExceptAllCaps => !is_all_caps(exact) && self.spellings.iter().any(|spelling| spelling.starts_with(normalized)),
            AbbreviationCase::ExactCase => self.spellings.iter().any(|spelling| spelling.starts_with(exact)),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct AbbreviationPolicy {
    catalogs: &'static [&'static [AbbreviationRule]],
    allow_dotted_initialism: bool,
}

impl AbbreviationPolicy {
    pub const fn new(catalogs: &'static [&'static [AbbreviationRule]], allow_dotted_initialism: bool) -> Self { Self { catalogs, allow_dotted_initialism } }
    pub const fn catalogs(self) -> &'static [&'static [AbbreviationRule]] { self.catalogs }
    pub const fn allows_dotted_initialism(self) -> bool { self.allow_dotted_initialism }
    pub fn contains(self, candidate: &str) -> bool { self.contains_normalized(candidate, &candidate.to_lowercase()) }
    pub fn has_prefix(self, candidate: &str) -> bool { self.has_prefix_normalized(candidate, &candidate.to_lowercase()) }
    pub fn evidence(self, candidate: &str) -> Option<AbbreviationEvidence> { self.evidence_normalized(candidate, &candidate.to_lowercase()) }

    /// Checks a candidate span without allocating a segment tree.
    pub fn recognizes(self, text: &str, span: Range<usize>) -> bool {
        let Some(candidate) = text.get(span.clone()) else { return false; };
        self.allow_dotted_initialism && is_dotted_initialism(candidate) || self.evidence(candidate).is_some_and(|evidence| evidence.is_satisfied_by(text, span))
    }

    /// Accepts the original spelling and its precomputed Unicode lowercase form.
    pub fn contains_normalized(self, exact: &str, normalized: &str) -> bool { self.rules().any(|rule| rule.matches(exact, normalized)) }
    pub fn has_prefix_normalized(self, exact: &str, normalized: &str) -> bool { self.rules().any(|rule| rule.matches_prefix(exact, normalized)) }
    pub fn evidence_normalized(self, exact: &str, normalized: &str) -> Option<AbbreviationEvidence> { self.rules().find_map(|rule| rule.matches(exact, normalized).then_some(rule.evidence)) }
    fn rules(self) -> impl Iterator<Item = &'static AbbreviationRule> { self.catalogs.iter().flat_map(|catalog| catalog.iter()) }
}

fn is_all_caps(candidate: &str) -> bool { candidate.chars().any(char::is_alphabetic) && candidate.chars().filter(|character| character.is_alphabetic()).all(|character| character.is_uppercase()) }
