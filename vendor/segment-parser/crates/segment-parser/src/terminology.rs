use std::ops::Range;

use segment_lexer::word_tokens;
use structure::{SegmentTree, SegmentWords, TermMatchKind, TerminologyOccurrence};

use crate::stream_util::{is_whole_word_match, surrounding_word_lengths};
use structure::{DictionaryAnalysis, NodeKind};
use crate::{common_tag_scope, term_match_coverage_allowed};

const WHOLE_WORD_THRESHOLD: usize = 5;
const COMPOUND_COMPONENT_MINIMUM: usize = 4;
const COMPOUND_STEM_MINIMUM: usize = 5;
const COMPOUND_SUFFIX_MINIMUM: usize = 4;
const COMPOUND_ENDING_MAXIMUM: usize = 2;
const INTERNAL_COMPOUND_STEM_MINIMUM: usize = 8;
const DISCOURAGED_INFLECTION_MAXIMUM: usize = 2;

pub fn tag_scopes_equivalent(left: &structure::TagScope, right: &structure::TagScope) -> bool {
    left.tag_ids().len() == right.tag_ids().len() && left.tag_ids().iter().all(|id| left.tag_ids().iter().filter(|candidate| *candidate == id).count() == right.tag_ids().iter().filter(|candidate| *candidate == id).count())
}

pub fn normalize_terminology_text(text: &str) -> String { crate::canonical_search_text(text) }

fn term_tokens(text: &str) -> Vec<String> { word_tokens(text).map(normalize_terminology_text).collect() }

pub fn terminology_occurrence(tree: &SegmentTree, span: Range<usize>) -> TerminologyOccurrence {
    let token_unit_ids = tree.token_unit_ids_for_span(span.clone());
    let tag_scope = common_tag_scope(tree, span.clone());
    TerminologyOccurrence::new(span, token_unit_ids, tag_scope)
}

pub fn terminology_match_forms(term: &str) -> Vec<String> {
    let whole = term.trim();
    if whole.is_empty() {
        return Vec::new();
    }
    let alternatives = whole.split('/').map(|part| part.trim().trim_end_matches(crate::char_class::is_dash).trim_end()).collect::<Vec<_>>();
    if alternatives.len() < 2 || alternatives.iter().any(|part| part.is_empty()) {
        return vec![whole.to_string()];
    }
    alternatives.into_iter().fold(vec![whole.to_string()], |mut forms, form| {
        let normalized = normalize_terminology_text(form);
        if !forms.iter().any(|existing| normalize_terminology_text(existing) == normalized) {
            forms.push(form.to_string());
        }
        forms
    })
}

fn annotated_occurrences(tree: &SegmentTree, term: &str, policy: WordMatchPolicy) -> Vec<TerminologyOccurrence> {
    let normalized_term = normalize_terminology_text(term);
    let term_words = term_tokens(term);
    tree.term_hits().iter().filter(|hit| hit.candidates().iter().any(|candidate| {
        normalize_terminology_text(candidate.canonical_source()) == normalized_term && match candidate.match_kind() {
            TermMatchKind::Surface | TermMatchKind::DictionaryBaseform => true,
            TermMatchKind::Coverage => !fragment_ruled_out(tree, &hit.span(), &term_words, policy),
        }
    })).map(|hit| terminology_occurrence(tree, hit.span())).collect()
}

/// Analysis of the whole word around a match that covers only part of it, recorded on the term node by the dictionary pass.
fn fragment_analysis<'a>(tree: &'a SegmentTree, span: &Range<usize>) -> Option<&'a DictionaryAnalysis> {
    if is_whole_word_match(tree.text(), span) {
        return None;
    }
    tree.preorder_iter().filter(|node| node.kind() == NodeKind::Term && node.span().start <= span.start && span.end <= node.span().end).find_map(|node| node.dictionary_analysis())
}

/// A match inside a longer lexicon word is ruled out when that word's analysis names none of the term's words (`konto` in `kontor`).
fn fragment_ruled_out(tree: &SegmentTree, span: &Range<usize>, term_words: &[impl AsRef<str>], policy: WordMatchPolicy) -> bool {
    fragment_analysis(tree, span).filter(|analysis| analysis.is_authoritative()).is_some_and(|analysis| !term_words.iter().any(|term_word| analysis_names(analysis, term_word.as_ref(), policy)))
}

/// Analyses hold terminology-normalised values (see `dictionary_analysis_with_hunspell`), so this compares without allocating.
fn analysis_names(analysis: &DictionaryAnalysis, term_word: &str, policy: WordMatchPolicy) -> bool { analysis.lemmas().chain(analysis.components().filter(|_| policy.accepts_components())).any(|candidate| candidate == term_word) }

fn grounded_compound_match(term_word: &str, text_word: &str) -> bool {
    term_word.chars().count() >= COMPOUND_COMPONENT_MINIMUM && text_word.len() > term_word.len() && (text_word.starts_with(term_word) || text_word.ends_with(term_word))
}

pub fn compound_term_aliases(term: &str) -> Vec<String> {
    if word_tokens(term).count() != 1 || !term.chars().all(char::is_alphabetic) {
        return Vec::new();
    }
    let boundaries = term.char_indices().map(|(index, _)| index).chain(std::iter::once(term.len())).collect::<Vec<_>>();
    (1..=COMPOUND_ENDING_MAXIMUM).filter_map(|removed| boundaries.len().checked_sub(removed + 1).map(|index| &term[..boundaries[index]])).filter(|stem| stem.chars().count() >= COMPOUND_STEM_MINIMUM).map(str::to_string).collect()
}

pub(crate) fn compound_alias_coverage_allowed(text: &str, span: Range<usize>) -> bool {
    let (prefix_length, suffix_length) = surrounding_word_lengths(text, &span);
    let edge_match = prefix_length == 0 && suffix_length >= COMPOUND_SUFFIX_MINIMUM && term_match_coverage_allowed(text, span.clone());
    edge_match || internal_compound_match(text, &span, prefix_length, suffix_length)
}

pub(crate) fn target_form_compound_coverage_allowed(text: &str, span: Range<usize>) -> bool {
    let (prefix_length, suffix_length) = surrounding_word_lengths(text, &span);
    let edge_match = (prefix_length == 0 && suffix_length >= COMPOUND_COMPONENT_MINIMUM) || (suffix_length == 0 && prefix_length >= COMPOUND_COMPONENT_MINIMUM);
    edge_match || internal_compound_match(text, &span, prefix_length, suffix_length)
}

/// A match in the middle of a compound counts when both neighbouring components are substantial and the match dominates them.
fn internal_compound_match(text: &str, span: &Range<usize>, prefix_length: usize, suffix_length: usize) -> bool {
    let stem_length = text[span.clone()].chars().count();
    prefix_length >= COMPOUND_COMPONENT_MINIMUM && suffix_length >= COMPOUND_COMPONENT_MINIMUM && stem_length >= INTERNAL_COMPOUND_STEM_MINIMUM && stem_length >= prefix_length + suffix_length
}

fn grounded_elided_compound_match(term_word: &str, text_word: &str) -> bool {
    compound_term_aliases(term_word).into_iter().any(|stem| text_word.match_indices(stem.as_str()).any(|(start, matched)| compound_alias_coverage_allowed(text_word, start..start + matched.len())))
}

fn grounded_inflection_match(term_word: &str, text_word: &str) -> bool {
    let term_length = term_word.chars().count();
    let text_length = text_word.chars().count();
    let shared_stem = term_word.chars().zip(text_word.chars()).take_while(|(term, text)| term == text).count();
    shared_stem >= COMPOUND_COMPONENT_MINIMUM && term_length.saturating_sub(shared_stem) <= 2 && text_length.saturating_sub(shared_stem) <= 3
}

fn word_matches(term_word: &str, text_word: &str) -> bool {
    term_word == text_word
        || term_word.chars().count() >= WHOLE_WORD_THRESHOLD && text_word.starts_with(term_word) && term_match_coverage_allowed(text_word, 0..term_word.len())
        || grounded_compound_match(term_word, text_word)
        || grounded_elided_compound_match(term_word, text_word)
        || grounded_inflection_match(term_word, text_word)
}

/// A discouraged term counts only as the whole word or a short suffix inflection of a longer word, never as a compound component.
fn discouraged_word_matches(term_word: &str, text_word: &str) -> bool {
    term_word == text_word
        || term_word.chars().count() >= WHOLE_WORD_THRESHOLD && text_word.starts_with(term_word) && text_word.chars().count() - term_word.chars().count() <= DISCOURAGED_INFLECTION_MAXIMUM
}

/// How far a term may stretch to cover a text word.
#[derive(Clone, Copy)]
enum WordMatchPolicy {
    /// Expected terms: inflections and compound components count.
    Grounded,
    /// Forbidden and deprecated terms: inflections count, compound components never do.
    Discouraged,
}

impl WordMatchPolicy {
    fn accepts_components(self) -> bool { matches!(self, Self::Grounded) }
    fn heuristic_match(self, term_word: &str, text_word: &str) -> bool {
        match self {
            Self::Grounded => word_matches(term_word, text_word),
            Self::Discouraged => discouraged_word_matches(term_word, text_word),
        }
    }
}

/// A discouraged term matches a piece of a word split by a termbase hit only when the whole word is an inflection of it.
/// Dictionary analysis first: a lemma or (for grounded terms) a compound component equal to the term matches.
/// When the word is a full lexicon entry its analysis is complete, so the look-alike heuristics may not add
/// matches it rules out (`konto` in `kontor`, `port` in `rapport`).
fn word_at_matches(tree: &SegmentTree, words: &SegmentWords<'_>, index: usize, term_word: &str, policy: WordMatchPolicy) -> bool {
    let Some(word) = words.get(index) else { return false; };
    if !policy.accepts_components() && !is_whole_word_match(tree.text(), &word.span()) {
        return fragment_analysis(tree, &word.span()).is_some_and(|analysis| analysis.lemmas().any(|lemma| lemma == term_word));
    }
    if fragment_ruled_out(tree, &word.span(), &[term_word], policy) {
        return false;
    }
    let surface = normalize_terminology_text(word.text());
    let baseform = word.baseform().map(normalize_terminology_text);
    let analysis = word.dictionary_analysis();
    let analysed = analysis.is_some_and(|analysis| analysis_names(analysis, term_word, policy));
    if surface == term_word || analysed {
        return true;
    }
    if analysis.is_some_and(structure::DictionaryAnalysis::is_authoritative) {
        return false;
    }
    policy.heuristic_match(term_word, &surface) || baseform.is_some_and(|baseform| policy.heuristic_match(term_word, &baseform))
}

fn occurrence_for_window(tree: &SegmentTree, words: &SegmentWords<'_>, start: usize, length: usize) -> Option<TerminologyOccurrence> {
    let first = words.get(start)?.span();
    let last = words.get(start + length.checked_sub(1)?)?.span();
    Some(terminology_occurrence(tree, first.start..last.end))
}

fn lexical_occurrences(tree: &SegmentTree, pattern: &[String], policy: WordMatchPolicy) -> Vec<TerminologyOccurrence> {
    let words = tree.segment_words();
    (pattern.len() <= words.len()).then(|| (0..=words.len() - pattern.len())
        .filter(|start| pattern.iter().enumerate().all(|(offset, term_word)| word_at_matches(tree, &words, start + offset, term_word, policy)))
        .filter_map(|start| occurrence_for_window(tree, &words, start, pattern.len()))
        .collect()).unwrap_or_default()
}

pub fn grounded_target_terminology_occurrences(tree: &SegmentTree, term: &str) -> Vec<TerminologyOccurrence> {
    let forms = terminology_match_forms(term);
    if forms.is_empty() {
        return Vec::new();
    }
    let lexical = forms.iter().flat_map(|form| lexical_occurrences(tree, &term_tokens(form), WordMatchPolicy::Grounded));
    distinct_occurrences(annotated_occurrences(tree, term, WordMatchPolicy::Grounded).into_iter().chain(lexical).collect())
}

/// Strict counterpart of [`grounded_target_terminology_occurrences`] for terms the target must not contain.
/// A false hit there is a spurious error, so compound edges and loose stem sharing do not count: `port` must not fire on `rapport`.
pub fn discouraged_target_terminology_occurrences(tree: &SegmentTree, term: &str) -> Vec<TerminologyOccurrence> {
    let forms = terminology_match_forms(term);
    if forms.is_empty() {
        return Vec::new();
    }
    let annotated = annotated_occurrences(tree, term, WordMatchPolicy::Discouraged).into_iter().filter(|occurrence| is_whole_word_match(tree.text(), occurrence.span()));
    let lexical = forms.iter().flat_map(|form| lexical_occurrences(tree, &term_tokens(form), WordMatchPolicy::Discouraged));
    distinct_occurrences(annotated.chain(lexical).collect())
}

pub fn exact_source_terminology_occurrences(tree: &SegmentTree, term: &str) -> Vec<TerminologyOccurrence> {
    let term = term.trim();
    if term.is_empty() {
        return Vec::new();
    }
    let occurrences = crate::stream_util::case_insensitive_match_spans(tree.text(), term).into_iter().filter(|span| exact_source_match_allowed(tree.text(), span.clone())).map(|span| terminology_occurrence(tree, span)).collect();
    distinct_occurrences(occurrences)
}

fn exact_source_match_allowed(text: &str, span: Range<usize>) -> bool {
    if is_whole_word_match(text, &span) {
        return true;
    }
    let matched = &text[span];
    !matched.chars().any(char::is_whitespace) && matched.chars().filter(|character| character.is_alphanumeric()).count() >= COMPOUND_COMPONENT_MINIMUM
}

fn distinct_occurrences(occurrences: Vec<TerminologyOccurrence>) -> Vec<TerminologyOccurrence> { crate::stream_util::select_non_overlapping(occurrences, |occurrence| occurrence.span().clone()) }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{rebuilt_with_term_matcher, ParseLanguage, TermMatcher};
    use structure::{Run, TagData, TagKind};

    fn nested_tag_scope(ids: &[&str]) -> structure::TagScope {
        let mut run = Run::new("term");
        ids.iter().for_each(|id| run.push_tag_at(0, TagData::new((*id).to_string(), "g".to_string(), TagKind::Open, "<g>".to_string(), None, None)));
        ids.iter().rev().for_each(|id| run.push_tag_at(4, TagData::new((*id).to_string(), "g".to_string(), TagKind::Close, "</g>".to_string(), None, None)));
        common_tag_scope(&crate::tokenized_runs_tree([run]), 0..4)
    }

    #[test]
    fn matching_is_case_insensitive_and_canonically_normalized_but_preserves_diacritics() {
        let tree = crate::tokenized_text_tree("RÉSUMÉ re\u{301}sume\u{301} resume får far");

        assert_eq!(grounded_target_terminology_occurrences(&tree, "résumé").len(), 2);
        assert_eq!(grounded_target_terminology_occurrences(&tree, "resume").len(), 1);
        assert_eq!(grounded_target_terminology_occurrences(&tree, "får").len(), 1);
        assert_eq!(grounded_target_terminology_occurrences(&tree, "far").len(), 1);
    }

    #[test]
    fn grounded_terms_match_compound_edges_but_not_incidental_internal_substrings() {
        assert_eq!(grounded_target_terminology_occurrences(&crate::tokenized_text_tree("hårddisk"), "disk").len(), 1);
        assert_eq!(grounded_target_terminology_occurrences(&crate::tokenized_text_tree("akuttandläkarbesök"), "tandläkare").len(), 1);
        assert!(grounded_target_terminology_occurrences(&crate::tokenized_text_tree("Geschwindigkeit"), "wind").is_empty());
        assert!(grounded_target_terminology_occurrences(&crate::tokenized_text_tree("brunch"), "run").is_empty());
    }

    #[test]
    fn discouraged_terms_match_whole_words_and_short_inflections_but_not_compound_edges() {
        assert!(discouraged_target_terminology_occurrences(&crate::tokenized_text_tree("rapport transport"), "port").is_empty());
        assert!(discouraged_target_terminology_occurrences(&crate::tokenized_text_tree("ändring styrning"), "ring").is_empty());
        assert!(discouraged_target_terminology_occurrences(&crate::tokenized_text_tree("planet"), "plan").is_empty());
        assert!(discouraged_target_terminology_occurrences(&crate::tokenized_text_tree("hårddisk"), "disk").is_empty());
        assert_eq!(discouraged_target_terminology_occurrences(&crate::tokenized_text_tree("Port och PORT"), "port").len(), 2);
        assert_eq!(discouraged_target_terminology_occurrences(&crate::tokenized_text_tree("Bitte loggens vermeiden"), "loggen").len(), 1);
    }

    #[cfg(feature = "hunspell")]
    fn swedish_tree(text: &str) -> SegmentTree {
        let mut tree = crate::parse_text(text);
        crate::apply_dictionary(&mut tree, &crate::HunspellDictionary::for_language("sv"));
        tree
    }

    #[cfg(feature = "hunspell")]
    #[test]
    fn lexicon_entries_veto_look_alike_term_matches() {
        assert!(grounded_target_terminology_occurrences(&swedish_tree("rapporten"), "port").is_empty());
        assert!(grounded_target_terminology_occurrences(&swedish_tree("kontoret"), "konto").is_empty());
        assert!(grounded_target_terminology_occurrences(&swedish_tree("terminalen"), "termin").is_empty());
        assert_eq!(grounded_target_terminology_occurrences(&swedish_tree("planet"), "plan").len(), 1);
        assert_eq!(grounded_target_terminology_occurrences(&swedish_tree("kontoret"), "kontor").len(), 1);
    }

    #[cfg(feature = "hunspell")]
    #[test]
    fn dictionary_compound_parts_satisfy_expected_but_not_discouraged_terms() {
        assert_eq!(grounded_target_terminology_occurrences(&swedish_tree("kontorsstolen"), "stol").len(), 1);
        assert_eq!(grounded_target_terminology_occurrences(&swedish_tree("kontorsstolen"), "kontor").len(), 1);
        assert_eq!(grounded_target_terminology_occurrences(&swedish_tree("akuttandläkarbesök"), "tandläkare").len(), 1);
        assert!(discouraged_target_terminology_occurrences(&swedish_tree("kontorsstolen"), "stol").is_empty());
        assert_eq!(discouraged_target_terminology_occurrences(&swedish_tree("kontoret"), "kontor").len(), 1);
    }

    /// The termbase hit splits the word before the dictionary pass runs, as at document load.
    #[cfg(feature = "hunspell")]
    fn swedish_tree_with_term(text: &str, term: &str) -> SegmentTree {
        let mut tree = rebuilt_with_term_matcher(&crate::tokenized_text_tree(text), &TermMatcher::new(&[term.to_string()]), ParseLanguage::Swedish);
        crate::apply_dictionary(&mut tree, &crate::HunspellDictionary::for_language("sv"));
        tree
    }

    #[cfg(feature = "hunspell")]
    #[test]
    fn termbase_hits_inside_lexicon_words_need_the_dictionary_to_agree() {
        let account = swedish_tree_with_term("kontor", "konto");
        assert_eq!(account.term_hits().len(), 1, "coverage rule lets the termbase matcher hit konto inside kontor");
        assert!(grounded_target_terminology_occurrences(&account, "konto").is_empty());
        assert_eq!(grounded_target_terminology_occurrences(&swedish_tree_with_term("planet", "plan"), "plan").len(), 1);
    }

    #[cfg(feature = "hunspell")]
    #[test]
    fn discouraged_terms_ignore_compound_pieces_split_off_by_termbase_target_forms() {
        let mut tree = rebuilt_with_term_matcher(&crate::tokenized_text_tree("kontorsstol"), &TermMatcher::new_with_target_forms(&[], &[("stol".to_string(), "stol".to_string())], &[]), ParseLanguage::Swedish);
        crate::apply_dictionary(&mut tree, &crate::HunspellDictionary::for_language("sv"));
        assert_eq!(tree.term_hits().len(), 1, "target-form coverage splits stol off kontorsstol");
        assert!(discouraged_target_terminology_occurrences(&tree, "stol").is_empty());
        assert_eq!(grounded_target_terminology_occurrences(&tree, "stol").len(), 1);
    }

    #[test]
    fn exact_source_terms_match_standalone_and_inside_compound_words() {
        assert_eq!(exact_source_terminology_occurrences(&crate::tokenized_text_tree("clear aligner system2"), "aligner").len(), 1);
        assert_eq!(exact_source_terminology_occurrences(&crate::tokenized_text_tree("clearalignersystem2"), "aligner").len(), 1);
        assert!(exact_source_terminology_occurrences(&crate::tokenized_text_tree("paid"), "AI").is_empty());
    }

    #[test]
    fn annotated_trademark_term_before_hyphen_is_grounded() {
        let tree = rebuilt_with_term_matcher(&crate::tokenized_text_tree("INVISALIGN®-tandskenor"), &TermMatcher::new(&["Invisalign".to_string()]), ParseLanguage::English);
        assert_eq!(tree.term_hits().len(), 1);
        assert_eq!(grounded_target_terminology_occurrences(&tree, "Invisalign").len(), 1);
    }

    #[test]
    fn tag_scope_equivalence_compares_scope_id_multiplicity_without_order_dependence() {
        let repeated = nested_tag_scope(&["1", "1"]);
        let different = nested_tag_scope(&["1", "2"]);
        let reordered = nested_tag_scope(&["2", "1"]);

        assert!(!tag_scopes_equivalent(&repeated, &different));
        assert!(tag_scopes_equivalent(&different, &reordered));
    }
}
