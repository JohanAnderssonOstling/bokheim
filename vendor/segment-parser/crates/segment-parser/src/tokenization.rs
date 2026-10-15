use std::ops::Range;

#[cfg(feature = "terminology")]
use aho_corasick::{AhoCorasickBuilder, MatchKind};
use structure::{SegmentTree, UnitKind};
#[cfg(feature = "terminology")]
use structure::Run;

#[cfg(feature = "terminology")]
#[derive(Clone, Debug, PartialEq, Eq)]
struct ResolvedPatternSpan {
    kind: UnitKind,
    span: Range<usize>,
}

// Automaton construction dominates matching for small pattern sets.
#[cfg(feature = "terminology")]
const SCAN_PATTERN_THRESHOLD: usize = 8;

#[cfg(feature = "terminology")]
fn build_run_vec_with_semantic_unit_patterns(runs: Vec<Run>, patterns: &[(UnitKind, &str)]) -> SegmentTree {
    let text = runs.iter().map(Run::text).collect::<String>();
    let spans = resolved_pattern_spans(&text, patterns);
    let spans = spans.into_iter().map(|resolved| (resolved.kind, resolved.span)).collect::<Vec<_>>();
    crate::tokenized_runs_with_resolved_spans_tree(runs, &spans)
}

#[cfg(feature = "terminology")]
pub fn build_run_vec_with_semantic_unit_ranges(runs: Vec<Run>, pattern_text: &str, patterns: &[(UnitKind, Range<usize>)]) -> SegmentTree {
    let patterns = patterns.iter().filter_map(|(kind, range)| pattern_text.get(range.clone()).map(|pattern| (*kind, pattern))).collect::<Vec<_>>();
    build_run_vec_with_semantic_unit_patterns(runs, &patterns)
}

pub fn semantic_unit_ranges(tree: &SegmentTree) -> Vec<(UnitKind, Range<usize>)> {
    tree.preorder_iter().filter_map(|node| node.kind().semantic_text_unit_kind().map(|kind| (kind, node.span()))).collect()
}

pub(crate) fn semantic_unit_ranges_from_nodes(nodes: &[structure::parser_api::Node]) -> Vec<(UnitKind, Range<usize>)> {
    nodes.iter().flat_map(|node| {
        node.kind().semantic_text_unit_kind().map(|kind| (kind, node.span())).into_iter().chain(semantic_unit_ranges_from_nodes(&node.iter_children().cloned().collect::<Vec<_>>()))
    }).collect()
}

#[cfg(feature = "terminology")]
fn resolved_pattern_spans(text: &str, patterns: &[(UnitKind, &str)]) -> Vec<ResolvedPatternSpan> {
    let patterns = patterns.iter().copied().filter(|(_, pattern)| !pattern.is_empty()).collect::<Vec<_>>();
    if patterns.len() <= SCAN_PATTERN_THRESHOLD { spans_by_scan(text, &patterns) } else { spans_by_automaton(text, &patterns) }
}

#[cfg(feature = "terminology")]
fn spans_by_scan(text: &str, patterns: &[(UnitKind, &str)]) -> Vec<ResolvedPatternSpan> {
    let candidates = patterns.iter().flat_map(|&(kind, pattern)| text.match_indices(pattern).map(move |(start, _)| ResolvedPatternSpan { kind, span: start..start + pattern.len() })).collect();
    crate::stream_util::select_non_overlapping(candidates, |candidate| candidate.span.clone())
}

#[cfg(feature = "terminology")]
fn spans_by_automaton(text: &str, patterns: &[(UnitKind, &str)]) -> Vec<ResolvedPatternSpan> {
    if patterns.is_empty() {
        return Vec::new();
    }
    let matcher = AhoCorasickBuilder::new().match_kind(MatchKind::LeftmostLongest).build(patterns.iter().map(|(_, pattern)| *pattern)).expect("valid semantic-unit patterns");
    matcher.find_iter(text).map(|matched| {
        let (kind, _) = patterns[matched.pattern().as_usize()];
        ResolvedPatternSpan { kind, span: matched.start()..matched.end() }
    }).collect()
}

#[cfg(all(test, feature = "terminology"))]
mod tests {
    use super::*;

    #[test]
    fn longest_overlapping_pattern_wins() {
        let tree = build_run_vec_with_semantic_unit_patterns(vec![Run::new("blood pressure")], &[(UnitKind::Term, "blood"), (UnitKind::Term, "blood pressure")]);
        assert_eq!(tree.iter_units().map(|unit| (unit.kind(), unit.text())).collect::<Vec<_>>(), vec![(UnitKind::Term, "blood pressure")]);
    }

    #[test]
    fn scan_and_automaton_strategies_agree() {
        let cases = [
            ("abc", vec![(UnitKind::Term, "abc"), (UnitKind::Word, "b")]),
            ("abcd", vec![(UnitKind::Term, "ab"), (UnitKind::Word, "bcd")]),
            ("abc abc", vec![(UnitKind::Term, "abc"), (UnitKind::NonTranslatable, "abc")]),
            ("abc abc", vec![(UnitKind::Term, "abc")]),
            ("", Vec::new()),
        ];
        cases.into_iter().for_each(|(text, patterns)| assert_eq!(spans_by_scan(text, &patterns), spans_by_automaton(text, &patterns)));
        let many = (0..50).map(|index| (UnitKind::Term, if index % 2 == 0 { "abc" } else { "b" })).collect::<Vec<_>>();
        assert_eq!(spans_by_scan("abc abc", &many), spans_by_automaton("abc abc", &many));
    }

    #[test]
    fn semantic_unit_ranges_are_resolved_against_target_text() {
        let source = "Write to help@acme.com";
        let target = Run::new("Skriv till help@acme.com");
        let tree = build_run_vec_with_semantic_unit_ranges(vec![target], source, &[(UnitKind::Email, 9..22)]);
        assert!(tree.iter_units().any(|unit| unit.kind() == UnitKind::Email && unit.text() == "help@acme.com" && unit.span() == (11..24)));
    }

    #[test]
    fn semantic_unit_ranges_are_derived_from_semantic_nodes() {
        let text = "Visit https://example.com and email a@test.com about Yellowstone SkyShowtime";
        let patterns = [(UnitKind::Term, "Yellowstone"), (UnitKind::Url, "https://example.com"), (UnitKind::Email, "a@test.com"), (UnitKind::NonTranslatable, "SkyShowtime")];
        let mut spans = patterns.into_iter().filter_map(|(kind, pattern)| text.find(pattern).map(|start| (kind, start..start + pattern.len()))).collect::<Vec<_>>();
        spans.sort_by_key(|(_, span)| span.start);
        let tree = crate::tokenized_runs_with_resolved_spans_tree([Run::new(text)], &spans);
        let semantic = semantic_unit_ranges(&tree);
        spans.into_iter().for_each(|expected| assert!(semantic.contains(&expected)));
    }
}
