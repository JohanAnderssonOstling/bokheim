use std::ops::Range;

use segment_lexer::lexemes;

use super::{FormattingKind, Run, SegmentTree, TagData, TokenUnit, UnitKind};

pub(super) struct TokenizedRun {
    text: String,
    units: Vec<TokenUnit>,
    tags: Vec<TagData>,
    inherited_formatting: Vec<FormattingKind>,
}

impl TokenizedRun {
    fn new(text: String, units: Vec<TokenUnit>, tags: Vec<TagData>, inherited_formatting: Vec<FormattingKind>) -> Self { Self { text, units, tags, inherited_formatting } }
    fn into_tree(self) -> SegmentTree { SegmentTree::from_tokenized_parts_with_semantic_ids(self.text, self.units, self.tags, self.inherited_formatting, None) }
    pub(super) fn into_parts(self) -> (String, Vec<TokenUnit>, Vec<TagData>, Vec<FormattingKind>) { (self.text, self.units, self.tags, self.inherited_formatting) }
}

/// Lowercase text and collapse each whitespace run to one ASCII space.
/// Tokenize lossless runs into a flat unit stream.
///
/// Text runs become contiguous lexical units. Tag runs become zero-width tag
/// units anchored at the current text position.
pub(super) fn tokenize_runs(runs: Vec<Run>) -> SegmentTree { tokenize_runs_into_parts(runs).into_tree() }

pub(super) fn tokenize_runs_into_parts(runs: Vec<Run>) -> TokenizedRun { tokenize_run_with_semantic_spans(into_single_run(runs), &[]) }

pub(super) fn tokenize_runs_with_resolved_spans(runs: Vec<Run>, spans: &[(UnitKind, Range<usize>)]) -> SegmentTree {
    let run = into_single_run(runs);
    let spans = spans.iter().map(|(kind, span)| SemanticSpan { start: span.start, end: span.end, kind: *kind }).collect::<Vec<_>>();
    tokenize_run_with_semantic_spans(run, &spans).into_tree()
}

fn into_single_run(mut runs: Vec<Run>) -> Run { if runs.len() == 1 { runs.pop().expect("one run") } else { merge_runs(runs) } }

fn merge_runs(runs: Vec<Run>) -> Run {
    let text_len: usize = runs.iter().map(|run| run.text().len()).sum();
    runs.into_iter().fold(Run::new(String::with_capacity(text_len)), |mut acc, run| { acc.append(run); acc })
}

fn tokenize_run_with_semantic_spans(run: Run, semantic_spans: &[SemanticSpan]) -> TokenizedRun {
    let (text, anchors, inherited_formatting) = run.into_parts();
    assert_tokenizable_len(text.len());
    let mut units = Vec::with_capacity(anchors.len().saturating_add(text.len() / 3).saturating_add(1));
    let mut tags = Vec::with_capacity(anchors.len());
    if anchors.is_empty() && semantic_spans.is_empty() {
        atomize_text_into(&text, 0, &mut units);
        return TokenizedRun::new(text, units, tags, inherited_formatting);
    }
    let mut pos = 0usize;
    let mut tag_idx = 0usize;
    for span in semantic_spans {
        assert!(span.start >= pos && span.end <= text.len() && text.is_char_boundary(span.start) && text.is_char_boundary(span.end), "semantic span is out of order or not a char boundary");
        if span.kind == UnitKind::Abbreviation && anchors.iter().any(|anchor| anchor.offset() > span.start && anchor.offset() < span.end) {
            continue;
        }
        emit_text_and_tags_until(&text, &anchors, &mut tag_idx, span.start, &mut pos, &mut units, &mut tags);
        units.push(TokenUnit::new_compact(span.kind, span.start, span.end, None));
        while anchors.get(tag_idx).is_some_and(|anchor| anchor.offset() < span.end) {
            emit_tag(&anchors[tag_idx], &mut units, &mut tags);
            tag_idx += 1;
        }
        pos = span.end;
    }
    emit_text_and_tags_until(&text, &anchors, &mut tag_idx, text.len(), &mut pos, &mut units, &mut tags);
    atomize_text_into(&text[pos..], pos, &mut units);
    TokenizedRun::new(text, units, tags, inherited_formatting)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SemanticSpan {
    start: usize,
    end: usize,
    kind: UnitKind,
}

fn emit_text_and_tags_until(text: &str, anchors: &[super::TagAnchor], tag_idx: &mut usize, until: usize, pos: &mut usize, units: &mut Vec<TokenUnit>, tags: &mut Vec<super::TagData>) {
    while anchors.get(*tag_idx).is_some_and(|anchor| anchor.offset() <= until) {
        let anchor = &anchors[*tag_idx];
        assert!(anchor.offset() >= *pos && anchor.offset() <= text.len() && text.is_char_boundary(anchor.offset()), "tag anchor offset is out of order or not a char boundary");
        atomize_text_into(&text[*pos..anchor.offset()], *pos, units);
        emit_tag(anchor, units, tags);
        *pos = anchor.offset();
        *tag_idx += 1;
    }
    atomize_text_into(&text[*pos..until], *pos, units);
    *pos = until;
}

fn emit_tag(anchor: &super::TagAnchor, units: &mut Vec<TokenUnit>, tags: &mut Vec<super::TagData>) {
    let tag_index = u16::try_from(tags.len()).expect("tag count exceeds u16 range");
    tags.push(anchor.tag().clone());
    units.push(TokenUnit::new_compact(UnitKind::Tag, anchor.offset(), anchor.offset(), Some(tag_index)));
}

pub(super) fn atomize_text_into(text: &str, base: usize, out: &mut Vec<TokenUnit>) { lexemes(text).for_each(|lexeme| { let span = lexeme.span(); out.push(TokenUnit::new_compact(lexeme.kind().into(), base + span.start, base + span.end, None)); }); }

fn assert_tokenizable_len(len: usize) { assert!(u16::try_from(len).is_ok(), "tokenized text exceeds u16 span range"); }

#[cfg(test)]
mod tests {
    use super::*;
    use std::hint::black_box;
    use std::time::Instant;

    fn kinds(text: &str) -> Vec<(UnitKind, String)> {
        tokenize_runs(vec![Run::new(text)])
            .iter_units()
            .map(|u| (u.kind(), u.text().to_string()))
            .collect()
    }

    #[test]
    fn tokenizes_lexically_without_locale_semantics() {
        assert_eq!(
            kinds("1.234,56 2024-06-29 10,5 kg 10% https://example.com mail@test.com 123."),
            vec![
                (UnitKind::Numeric, "1".to_string()),
                (UnitKind::Punct, ".".to_string()),
                (UnitKind::Numeric, "234".to_string()),
                (UnitKind::Punct, ",".to_string()),
                (UnitKind::Numeric, "56".to_string()),
                (UnitKind::Whitespace, " ".to_string()),
                (UnitKind::Numeric, "2024".to_string()),
                (UnitKind::Punct, "-".to_string()),
                (UnitKind::Numeric, "06".to_string()),
                (UnitKind::Punct, "-".to_string()),
                (UnitKind::Numeric, "29".to_string()),
                (UnitKind::Whitespace, " ".to_string()),
                (UnitKind::Numeric, "10".to_string()),
                (UnitKind::Punct, ",".to_string()),
                (UnitKind::Numeric, "5".to_string()),
                (UnitKind::Whitespace, " ".to_string()),
                (UnitKind::Word, "kg".to_string()),
                (UnitKind::Whitespace, " ".to_string()),
                (UnitKind::Numeric, "10".to_string()),
                (UnitKind::Punct, "%".to_string()),
                (UnitKind::Whitespace, " ".to_string()),
                (UnitKind::Word, "https".to_string()),
                (UnitKind::Punct, ":".to_string()),
                (UnitKind::Punct, "/".to_string()),
                (UnitKind::Punct, "/".to_string()),
                (UnitKind::Word, "example".to_string()),
                (UnitKind::Punct, ".".to_string()),
                (UnitKind::Word, "com".to_string()),
                (UnitKind::Whitespace, " ".to_string()),
                (UnitKind::Word, "mail".to_string()),
                (UnitKind::Punct, "@".to_string()),
                (UnitKind::Word, "test".to_string()),
                (UnitKind::Punct, ".".to_string()),
                (UnitKind::Word, "com".to_string()),
                (UnitKind::Whitespace, " ".to_string()),
                (UnitKind::Numeric, "123".to_string()),
                (UnitKind::Punct, ".".to_string()),
            ]
        );
    }

    #[test]
    fn tokenizes_term_spanning_tag_as_atomic_term_unit() {
        let mut run = Run::new("blood pressure");
        run.push_tag_at(6, super::super::TagData::new("1".to_string(), "g".to_string(), super::super::TagKind::Open, "<g id=\"1\">".to_string(), None, None));
        let stream = tokenize_runs_with_resolved_spans(vec![run], &[(UnitKind::Term, 0..14)]);
        assert_eq!(stream.units[0].kind(), UnitKind::Term);
        assert_eq!(&stream.text()[stream.units[0].span()], "blood pressure");
        assert!(stream.units.iter().any(|unit| unit.kind() == UnitKind::Tag));
    }

    #[test]
    fn protected_abbreviation_does_not_cross_a_tag_boundary() {
        let mut run = Run::new("App.");
        run.push_tag_at(3, super::super::TagData::new("1".to_string(), "mrk".to_string(), super::super::TagKind::Close, "</mrk>".to_string(), None, None));
        let stream = tokenize_runs_with_resolved_spans(vec![run], &[(UnitKind::Abbreviation, 0..4)]);
        let kinds = stream.units.iter().map(TokenUnit::kind).collect::<Vec<_>>();

        assert_eq!(kinds, vec![UnitKind::Word, UnitKind::Tag, UnitKind::Punct]);
        assert_eq!(stream.to_xml(), "App</mrk>.");
    }

    #[test]
    #[ignore = "tokenization profiling; run explicitly with --ignored --nocapture"]
    fn profile_tokenize_runs_only() {
        let samples = [
            "Dr. Smith bought 1.234,56 kg of apples on 2024-06-29 and sent mail@test.com.",
            "Visit https://example.com/path?q=1 and compare 10,5 kg with 10% before 2026-07-01.",
            "The C-Med Alpha IFU_UserManualV23 contains ABC_123 and nontranslatable identifiers.",
            "Plain sentence text with repeated words, punctuation, and no obvious entities.",
        ];
        profile_samples("mixed-existing", &samples, |sample| Run::new(sample));
    }

    #[test]
    #[ignore = "tokenization profiling; run explicitly with --ignored --nocapture"]
    fn profile_ascii_no_tags() {
        let samples = [
            "Plain sentence text with repeated words, punctuation, and no obvious entities.",
            "Another regular source segment that should resemble translation memory content.",
            "Short UI label text with words and spaces.",
            "A longer paragraph-like segment has commas, periods, and repeated common words for scanning.",
        ];
        profile_samples("ascii-no-tags", &samples, |sample| Run::new(sample));
    }

    #[test]
    #[ignore = "tokenization profiling; run explicitly with --ignored --nocapture"]
    fn profile_ascii_with_tags() {
        let samples = [
            "Hello world from tagged content.",
            "Keep bold words and placeholders in order.",
            "Segment with two inline tags around text.",
            "A longer tagged sentence with punctuation, numbers 123, and words.",
        ];
        profile_samples("ascii-with-tags", &samples, tagged_run);
    }

    #[test]
    #[ignore = "tokenization profiling; run explicitly with --ignored --nocapture"]
    fn profile_mixed_unicode() {
        let samples = [
            "Översätt följande mening med åäö och några svenska ord.",
            "Résumé envoyé à François avec naïve façade and coöperate.",
            "Mått: 10,5 kg, längd 42 cm, temperatur −5 °C.",
            "日本語 mixed with svenska åäö and English words.",
        ];
        profile_samples("mixed-unicode", &samples, |sample| Run::new(sample));
    }

    #[test]
    #[ignore = "tokenization profiling; run explicitly with --ignored --nocapture"]
    fn profile_punctuation_heavy() {
        let samples = [
            "A/B-C_D.E:F;G,H(I)[J]{K}<L>|M\\N+O=P*Q&R%S#T@U!",
            "123,456.789-00/11:22;33%44+55=66 and ABC_DEF-123.",
            "https://example.com/path?q=1&x=2 mail@test.example.com",
            "((alpha)) -- [[beta]] :: {{gamma}} && delta != epsilon;",
        ];
        profile_samples("punctuation-heavy", &samples, |sample| Run::new(sample));
    }

    fn profile_samples(name: &str, samples: &[&str], make_run: impl Fn(&str) -> Run) {
        let iterations = std::env::var("RUSTTRADOS_TOKENIZE_PROFILE_ITERS").ok().and_then(|s| s.parse::<usize>().ok()).unwrap_or(100_000);
        let start = Instant::now();
        let mut bytes = 0usize;
        let mut units = 0usize;
        let mut checksum = 0usize;
        for i in 0..iterations {
            let sample = black_box(samples[i % samples.len()]);
            let stream = tokenize_runs(vec![make_run(sample)]);
            bytes = bytes.wrapping_add(stream.text().len());
            units = units.wrapping_add(stream.units.len());
            checksum = checksum.wrapping_add(stream.text().len()).wrapping_add(stream.units.len());
            black_box(stream);
        }
        let semantic_units = samples.iter().map(|sample| tokenize_runs(vec![make_run(sample)]).iter_units().filter(|u| matches!(u.kind(), UnitKind::Number | UnitKind::Date | UnitKind::Measurement | UnitKind::Url | UnitKind::Email | UnitKind::Term | UnitKind::NonTranslatable | UnitKind::Abbreviation)).count()).sum::<usize>();
        let elapsed = start.elapsed();
        let runs_per_sec = iterations as f64 / elapsed.as_secs_f64();
        let mb_per_sec = bytes as f64 / elapsed.as_secs_f64() / (1024.0 * 1024.0);
        println!("{name}: {iterations} runs in {elapsed:?}; {runs_per_sec:.0} runs/s; {mb_per_sec:.2} MiB/s; units={units}; semantic_units={semantic_units}; checksum={checksum}");
        black_box((name, iterations, bytes, units, semantic_units, elapsed, checksum));
    }

    fn tagged_run(sample: &str) -> Run {
        let mid = sample.len() / 2;
        let split = sample[..mid].char_indices().last().map(|(i, c)| i + c.len_utf8()).unwrap_or(0);
        let mut run = Run::default();
        run.push_text(&sample[..split]);
        run.push_tag(super::super::TagData::new("1".to_string(), "g".to_string(), super::super::TagKind::Open, "<g id=\"1\">".to_string(), None, None));
        run.push_text(&sample[split..]);
        run.push_tag(super::super::TagData::new("1".to_string(), "g".to_string(), super::super::TagKind::Close, "</g>".to_string(), None, None));
        run
    }
}
