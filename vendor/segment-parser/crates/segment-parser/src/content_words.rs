use crate::text_normalization::{CASE_MAPPER, unicode_case_fold};
use segment_lexer::LexicalKind;
use std::ops::Range;
use structure::{SegmentTree, SegmentWordRef, SegmentWords, TokenizedContentWords};
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

pub fn normalized_text(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    visit_normalized_text(text, |character, _, _| normalized.push(character));
    normalized
}

pub fn unicode_case_and_diacritic_fold(text: &str) -> String { unicode_case_fold(text).nfd().filter(|character| !is_combining_mark(*character)).collect() }

pub fn normalized_text_source_map(text: &str) -> (Vec<usize>, Vec<usize>) {
    let mut source_starts = Vec::with_capacity(text.len());
    let mut source_ends = Vec::with_capacity(text.len());
    visit_normalized_text(text, |character, source_start, source_end| {
        (0..character.len_utf8()).for_each(|_| {
            source_starts.push(source_start);
            source_ends.push(source_end);
        });
    });
    (source_starts, source_ends)
}

pub fn normalized_text_source_span(source_starts: &[usize], source_ends: &[usize], normalized_span: std::ops::Range<usize>) -> Option<std::ops::Range<usize>> {
    let start = *source_starts.get(normalized_span.start)?;
    let end = *source_ends.get(normalized_span.end.checked_sub(1)?)?;
    Some(start..end)
}

fn visit_normalized_text(text: &str, mut emit: impl FnMut(char, usize, usize)) {
    CASE_MAPPER.with(|case_mapper| {
        let trimmed = text.trim();
        let trim_offset = trimmed.as_ptr() as usize - text.as_ptr() as usize;
        let mut pending_space = None;
        trimmed.char_indices().for_each(|(offset, character)| {
            let source_start = trim_offset + offset;
            let source_end = source_start + character.len_utf8();
            if character.is_whitespace() {
                pending_space.get_or_insert((source_start, source_end));
                return;
            }
            if let Some((space_start, space_end)) = pending_space.take() {
                emit(' ', space_start, space_end);
            }
            let mut buffer = [0; 4];
            case_mapper.fold_string(character.encode_utf8(&mut buffer)).chars().for_each(|folded| emit(folded, source_start, source_end));
        });
    });
}

pub fn normalized_content_words<'a>(units: impl IntoIterator<Item = &'a str>) -> TokenizedContentWords {
    TokenizedContentWords::from_normalized_content_units(units.into_iter().map(unicode_case_fold))
}

pub fn normalized_content_words_with_spans<'a>(units: impl IntoIterator<Item = (Range<usize>, &'a str)>) -> (TokenizedContentWords, Vec<Range<usize>>) {
    let mut spans = Vec::new();
    let words = normalized_content_words(units.into_iter().map(|(span, text)| { spans.push(span); text }));
    (words, spans)
}

pub fn segment_content_words_with_spans(tree: &SegmentTree) -> (TokenizedContentWords, Vec<Range<usize>>) { normalized_content_words_with_spans(tree.iter_content_units().map(|unit| (unit.span(), unit.text()))) }

pub fn segment_content_words(tree: &SegmentTree) -> TokenizedContentWords { segment_content_words_with_spans(tree).0 }

pub fn segment_lexical_words_around(tree: &SegmentTree, before_offset: usize, after_offset: usize, maximum_each_side: usize) -> (TokenizedContentWords, TokenizedContentWords) {
    let lexical_words = tree.segment_words();
    let before = lexical_words.iter().filter(|word| word.span().end <= before_offset).rev().take(maximum_each_side).collect::<Vec<_>>();
    let before = normalized_content_words(before.into_iter().rev().map(|word| word.text()));
    let after = normalized_content_words(lexical_words.iter().filter(|word| word.span().start >= after_offset).take(maximum_each_side).map(|word| word.text()));
    (before, after)
}


pub fn clamp_text_offset(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

pub fn completion_token_span(text: &str, offset: usize) -> Range<usize> {
    let offset = clamp_text_offset(text, offset);
    let spans = segment_lexer::lexemes(text).fold(Vec::<Range<usize>>::new(), |mut spans, lexeme| {
        if !is_completion_lexeme(lexeme.kind(), lexeme.text()) { return spans; }
        let span = lexeme.span();
        if let Some(previous) = spans.last_mut().filter(|previous| previous.end == span.start) {
            previous.end = span.end;
        } else {
            spans.push(span);
        }
        spans
    });
    spans.iter().find(|span| span.end == offset).or_else(|| spans.iter().find(|span| span.start < offset && offset < span.end)).or_else(|| spans.iter().find(|span| span.start == offset)).cloned().unwrap_or(offset..offset)
}

pub fn caret_inside_completion_token(text: &str, offset: usize) -> bool {
    let offset = clamp_text_offset(text, offset);
    let span = completion_token_span(text, offset);
    span.start < offset && offset < span.end
}

fn is_completion_lexeme(kind: LexicalKind, text: &str) -> bool { matches!(kind, LexicalKind::Word | LexicalKind::Numeric) || kind == LexicalKind::Punct && matches!(text, "-" | "'" | "’" | "ʼ" | "<") }

pub fn adjacent_repeated_words<'a>(words: SegmentWords<'a>) -> impl Iterator<Item = (SegmentWordRef<'a>, SegmentWordRef<'a>)> + Clone + 'a {
    words.iter().zip(words.iter().skip(1)).filter(|(first, second)| first.normalized() == second.normalized() && second.whitespace_before())
}

pub fn contains_same_normalized_with_at_least_alphabetic(words: SegmentWords<'_>, candidate: SegmentWordRef<'_>, minimum: u16) -> bool {
    words.iter().any(|word| word.has_at_least_alphabetic(minimum) && word.normalized() == candidate.normalized())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical_search_text;

    fn tokenize_content_words(text: impl Into<String>) -> TokenizedContentWords { segment_content_words(&crate::tokenized_text_tree(text)) }

    #[test]
    fn tokenized_content_words_keep_compact_unicode_lowercase_units() {
        let words = tokenize_content_words("ÖPPNA 20 kg. İSTANBUL");
        assert_eq!(words.iter().collect::<Vec<_>>(), vec!["öppna", "20", "kg", "i̇stanbul"]);
        assert_eq!(words.iter().collect::<Vec<_>>(), vec!["öppna", "20", "kg", "i̇stanbul"]);
    }

    #[test]
    fn tokenized_content_words_use_full_unicode_case_folding() {
        let words = tokenize_content_words("Straße STRASSE");
        assert_eq!(words.iter().collect::<Vec<_>>(), vec!["strasse", "strasse"]);
    }

    #[test]
    fn content_projection_keeps_normalized_words_aligned_with_source_spans() {
        let tree = crate::tokenized_text_tree("Straße 20");
        let (words, spans) = segment_content_words_with_spans(&tree);

        assert_eq!(words.iter().collect::<Vec<_>>(), vec!["strasse", "20"]);
        assert_eq!(spans.iter().map(|span| &tree.text()[span.clone()]).collect::<Vec<_>>(), vec!["Straße", "20"]);
    }

    #[test]
    fn lexical_context_projection_excludes_the_current_caret_range() {
        let tree = crate::tokenized_text_tree("one two three four");
        let (before, after) = segment_lexical_words_around(&tree, 10, 13, 2);

        assert_eq!(before.iter().collect::<Vec<_>>(), vec!["one", "two"]);
        assert_eq!(after.iter().collect::<Vec<_>>(), vec!["four"]);
    }

    #[test]
    fn completion_token_boundaries_come_from_lexer_spans() {
        assert_eq!(completion_token_span("hard-disk", 7), 0..9);
        assert_eq!(completion_token_span("l’affaire", 5), 0..11);
        assert_eq!(completion_token_span("Cafe\u{301} noir", 6), 0..6);
        assert_eq!(completion_token_span("hello world", 6), 6..11);
        assert!(caret_inside_completion_token("hard-disk", 7));
        assert!(!caret_inside_completion_token("hello world", 6));
    }

    #[test]
    fn normalization_collapses_unicode_whitespace_and_lowercases() {
        assert_eq!(normalized_text("  Keep\tHIS\u{2003}Cool  "), "keep his cool");
    }

    #[test]
    fn normalization_source_map_tracks_collapsed_and_expanding_characters() {
        let text = "  A\u{2003}İ  ";
        let normalized = normalized_text(text);
        let (starts, ends) = normalized_text_source_map(text);

        assert_eq!(normalized, "a i\u{307}");
        assert_eq!(starts.len(), normalized.len());
        assert_eq!(ends.len(), normalized.len());
        assert_eq!((&text[starts[0]..ends[0]], &text[starts[1]..ends[1]], &text[starts[2]..ends[2]]), ("A", "\u{2003}", "İ"));
    }

    #[test]
    fn normalized_words_use_full_unicode_case_folding() {
        assert_eq!(normalized_text("Straße Σς"), "strasse σσ");
    }

    #[test]
    fn canonical_search_normalization_unifies_unicode_representation_and_whitespace() {
        assert_eq!(canonical_search_text("  CAFÉ\tﬁle  "), canonical_search_text("cafe\u{301} file"));
        assert_eq!(canonical_search_text("  CAFÉ\tﬁle  "), "café file");
    }

    #[test]
    fn canonical_search_normalization_preserves_language_significant_diacritics() {
        assert_ne!(canonical_search_text("far"), canonical_search_text("får"));
        assert_ne!(canonical_search_text("bar"), canonical_search_text("bär"));
        assert_ne!(canonical_search_text("ost"), canonical_search_text("öst"));
    }

    #[test]
    fn canonical_search_normalization_uses_full_unicode_case_folding() {
        assert_eq!(canonical_search_text("Straße"), canonical_search_text("STRASSE"));
        assert_eq!(canonical_search_text("οδυσσεύς"), canonical_search_text("ΟΔΥΣΣΕΎΣ"));
        assert_eq!(unicode_case_fold("ς"), unicode_case_fold("σ"));
    }

    #[test]
    fn scoring_fold_can_ignore_case_and_diacritics_without_changing_search_keys() {
        assert_eq!(unicode_case_and_diacritic_fold("RÉSUMÉ"), unicode_case_and_diacritic_fold("resume"));
        assert_ne!(canonical_search_text("résumé"), canonical_search_text("resume"));
    }

    #[test]
    fn repeated_word_matching_uses_normalized_words_and_whitespace_boundaries() {
        let tree = crate::tokenized_text_tree("Go go-go");
        let words = tree.segment_words();
        assert_eq!(adjacent_repeated_words(words).map(|(left, right)| (left.text(), right.text())).collect::<Vec<_>>(), vec![("Go", "go")]);
        assert!(contains_same_normalized_with_at_least_alphabetic(words, words.iter().nth(1).expect("second word"), 2));
    }
}
