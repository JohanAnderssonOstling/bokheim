use std::collections::HashSet;
use std::ops::Range;

use structure::{TermHit, TermHitCandidate, TermMatchKind};

use crate::stream_util::{case_insensitive_match_spans, is_whole_word_match, surrounding_word_lengths};
use crate::terminology::{compound_alias_coverage_allowed, target_form_compound_coverage_allowed};

#[derive(Clone, Debug)]
pub struct TermMatcher {
    matcher: Option<aho_corasick::AhoCorasick>,
    canonical_terms: Box<[String]>,
    pattern_policies: Box<[TermPatternPolicy]>,
}

#[derive(Clone, Copy, Debug)]
enum TermPatternPolicy {
    Normal,
    TargetForm,
    CompoundAlias,
}

impl TermMatcher {
    pub fn new(terms: &[String]) -> Self { Self::from_patterns(terms.iter().cloned().map(|term| (term.clone(), term, TermPatternPolicy::Normal))) }
    pub fn new_with_target_forms(exact_terms: &[String], target_forms: &[(String, String)], compound_aliases: &[(String, String)]) -> Self { Self::from_patterns(target_forms.iter().cloned().map(|(surface, canonical)| (surface, canonical, TermPatternPolicy::TargetForm)).chain(compound_aliases.iter().cloned().map(|(surface, canonical)| (surface, canonical, TermPatternPolicy::CompoundAlias))).chain(exact_terms.iter().cloned().map(|term| (term.clone(), term, TermPatternPolicy::Normal)))) }
    pub fn is_empty(&self) -> bool { self.matcher.is_none() }
    pub fn hits(&self, text: &str) -> Vec<TermHit> {
        match &self.matcher {
            Some(matcher) => {
                let folded_text = crate::text_normalization::FoldedText::new(text);
                merged_term_hits(matcher.find_overlapping_iter(folded_text.text()).filter_map(|matched| {
                let span = folded_text.source_span(matched.start()..matched.end())?;
                match self.pattern_policies[matched.pattern().as_usize()] {
                    TermPatternPolicy::Normal if !term_match_coverage_allowed(text, span.clone()) => return None,
                    TermPatternPolicy::TargetForm if !term_match_coverage_allowed(text, span.clone()) && !target_form_compound_coverage_allowed(text, span.clone()) => return None,
                    TermPatternPolicy::CompoundAlias if !compound_alias_coverage_allowed(text, span.clone()) => return None,
                    _ => {}
                };
                let canonical = self.canonical_terms[matched.pattern().as_usize()].clone();
                let match_kind = term_match_is_exact_surface(text, span.clone(), &canonical).then_some(TermMatchKind::Surface).unwrap_or(TermMatchKind::Coverage);
                Some(TermHit::with_candidate(span, TermHitCandidate::new(canonical, match_kind)))
            }))
            }
            None => Vec::new(),
        }
    }
    fn from_patterns(patterns: impl IntoIterator<Item = (String, String, TermPatternPolicy)>) -> Self {
        let mut seen = HashSet::new();
        let patterns = patterns.into_iter().map(|(pattern, canonical, policy)| (crate::unicode_case_fold(&pattern), canonical, policy)).filter(|(pattern, _, _)| !pattern.is_empty() && seen.insert(pattern.clone())).collect::<Vec<_>>();
        let canonical_terms = patterns.iter().map(|(_, canonical, _)| canonical.clone()).collect::<Vec<_>>();
        let pattern_policies = patterns.iter().map(|(_, _, policy)| *policy).collect::<Vec<_>>();
        let surfaces = patterns.into_iter().map(|(pattern, _, _)| pattern).collect::<Vec<_>>();
        let matcher = (!surfaces.is_empty()).then(|| aho_corasick::AhoCorasickBuilder::new().match_kind(aho_corasick::MatchKind::Standard).build(&surfaces).expect("valid term patterns"));
        Self { matcher, canonical_terms: canonical_terms.into_boxed_slice(), pattern_policies: pattern_policies.into_boxed_slice() }
    }
}

pub fn merged_term_hits(hits: impl IntoIterator<Item = TermHit>) -> Vec<TermHit> {
    let mut hits = hits.into_iter().collect::<Vec<_>>();
    hits.sort_by(|left, right| left.span().start.cmp(&right.span().start).then_with(|| left.span().end.cmp(&right.span().end)));
    hits.into_iter().fold(Vec::<TermHit>::new(), |mut merged, hit| {
        if let Some(previous) = merged.last().filter(|previous| previous.span() == hit.span()).cloned() {
            let mut candidates = previous.candidates().to_vec();
            hit.candidates().iter().cloned().for_each(|candidate| {
                if !candidates.iter().any(|existing| crate::language_parser::language_text_eq(existing.canonical_source(), candidate.canonical_source()) && existing.match_kind() == candidate.match_kind()) {
                    candidates.push(candidate);
                }
            });
            let span = previous.span();
            *merged.last_mut().expect("previous merged hit") = TermHit::with_candidates(span, candidates);
        } else {
            merged.push(hit);
        }
        merged
    })
}

pub fn primary_term_hits(hits: &[TermHit]) -> Vec<TermHit> {
    crate::stream_util::select_non_overlapping(hits.to_vec(), TermHit::span)
}

fn term_match_is_exact_surface(text: &str, span: Range<usize>, canonical: &str) -> bool { text.get(span.clone()).is_some_and(|surface| crate::language_parser::language_text_eq(surface, canonical)) && is_whole_word_match(text, &span) }

pub fn term_match_coverage_allowed(text: &str, span: Range<usize>) -> bool {
    let Some(matched) = text.get(span.clone()) else { return false; };
    let runs = alphanumeric_runs(matched);
    let Some(first) = runs.first() else { return true; };
    let last = runs.last().expect("first alphanumeric run exists");
    let (outer_prefix, outer_suffix) = surrounding_word_lengths(text, &span);
    let prefix = if first.start == 0 { outer_prefix } else { 0 };
    let suffix = if last.end == matched.len() { outer_suffix } else { 0 };
    if runs.len() == 1 {
        return dominant_word_coverage(matched[first.clone()].chars().count(), prefix + matched[first.clone()].chars().count() + suffix);
    }
    dominant_word_coverage(matched[first.clone()].chars().count(), prefix + matched[first.clone()].chars().count())
        && dominant_word_coverage(matched[last.clone()].chars().count(), matched[last.clone()].chars().count() + suffix)
}

pub fn first_case_insensitive_match_span(text: &str, term: &str) -> Option<Range<usize>> { case_insensitive_match_spans(text, term).into_iter().next() }

pub fn first_text_match_span(text: &str, term: &str, case_sensitive: bool) -> Option<Range<usize>> {
    if case_sensitive { text.find(term).map(|start| start..start + term.len()) } else { first_case_insensitive_match_span(text, term) }
}

fn alphanumeric_runs(text: &str) -> Vec<Range<usize>> {
    text.char_indices().filter(|(_, character)| character.is_alphanumeric()).fold(Vec::<Range<usize>>::new(), |mut runs, (index, character)| {
        let end = index + character.len_utf8();
        match runs.last_mut() {
            Some(run) if run.end == index => run.end = end,
            _ => runs.push(index..end),
        }
        runs
    })
}

fn dominant_word_coverage(matched: usize, containing: usize) -> bool { matched.saturating_mul(5) > containing.saturating_mul(3) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_insensitive_match_preserves_unicode_offsets() {
        let first_case_insensitive_term_match_span = |text: &str, term: &str| case_insensitive_match_spans(text, term).into_iter().find(|span| term_match_coverage_allowed(text, span.clone()));
        assert_eq!(first_case_insensitive_term_match_span("Bitte ÖFFNEN", "öffnen"), Some(6..13));
        assert_eq!(first_case_insensitive_term_match_span("Bitte LOGGENS vermeiden", "loggen"), Some(6..12));
        assert_eq!(first_case_insensitive_term_match_span("Siehe den KATALOG", "log"), None);
        assert_eq!(first_case_insensitive_match_span("İstanbul", "i\u{307}s"), Some(0..3));
        assert_eq!(first_case_insensitive_match_span("Die Straße", "STRASSE"), Some(4..11));
        assert_eq!(first_text_match_span("Die Straße", "STRASSE", false), Some(4..11));
        assert_eq!(first_text_match_span("Die Straße", "STRASSE", true), None);
    }

    #[test]
    fn matcher_preserves_overlapping_hits_and_selects_primary_longest_first() {
        let matcher = TermMatcher::new(&["New York City".to_string(), "York City Hall".to_string()]);
        let hits = matcher.hits("New York City Hall");
        assert_eq!(hits.len(), 2);
        assert_eq!(primary_term_hits(&hits).len(), 1);
        assert_eq!(primary_term_hits(&hits)[0].span(), 0..13);
    }

    #[test]
    fn matcher_registers_non_ascii_case_variants_without_changing_offsets() {
        let matcher = TermMatcher::new(&["hårddisk".to_string(), "öffnen".to_string()]);
        let hits = matcher.hits("HÅRDDISK ÖFFNEN");

        assert_eq!(hits.iter().map(TermHit::span).collect::<Vec<_>>(), vec![0..9, 10..17]);
        assert_eq!(hits.iter().map(|hit| hit.candidates()[0].canonical_source()).collect::<Vec<_>>(), vec!["hårddisk", "öffnen"]);
    }

    #[test]
    fn matcher_uses_full_case_folding_with_original_spans() {
        let matcher = TermMatcher::new(&["STRASSE".to_string(), "οδυσσεύς".to_string()]);
        let hits = matcher.hits("Straße ΟΔΥΣΣΕΎΣ");

        assert_eq!(hits.iter().map(TermHit::span).collect::<Vec<_>>(), vec![0..7, 8..24]);
    }
}
