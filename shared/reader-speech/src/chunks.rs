//! Bounded synthesis chunks keep logical reading items independent of audio buffers.
use segment_parser::{NodeKind, ParseLanguage, SegmentTree, parse_text_with_language, terminal_compatible_abbreviation_periods};
use segment_rules::char_class::{is_closing_enclosure, is_sentence_terminal};
use std::ops::Range;

pub const MAX_CHUNK_CHARS: usize = 400;
pub const FIRST_CHUNK_CHARS: usize = 120;

/// Select a sentence or bounded word span from original, unspoken-normalized text.
/// A reader may supply only `MAX_CHUNK_CHARS + 1` characters of lookahead.
pub fn next_chunk(text: &str, start: usize) -> Option<Range<usize>> {
    next_chunk_with_limit(text, start, MAX_CHUNK_CHARS)
}

/// Select a shorter opening chunk without changing source offsets or splitting
/// a UTF-8 character. Later chunks use the normal sentence/word limit.
pub fn next_chunk_with_limit(text: &str, start: usize, max_chars: usize) -> Option<Range<usize>> {
    let tail = text.get(start..)?;
    let start = start + tail.len() - tail.trim_start().len();
    if start == text.len() {
        return None;
    }
    // Bound parser work even when the caller supplies an entire chapter. One
    // extra character lets selection distinguish a complete node from overflow.
    let tail = &text[start..];
    let end = tail.char_indices().nth(max_chars.max(1).saturating_add(1)).map_or(tail.len(), |(offset, _)| offset);
    let tree = parse_text_with_language(&tail[..end], ParseLanguage::English);
    let span = next_chunk_in_tree(&tree, 0, max_chars)?;
    Some(start + span.start..start + span.end)
}

/// Select from parsed source nodes, retaining semantic spans as chunk boundaries.
/// A single node longer than the limit is split at a UTF-8 character boundary.
pub fn next_chunk_in_tree(tree: &SegmentTree, start: usize, max_chars: usize) -> Option<Range<usize>> {
    let text = tree.text();
    let tail = text.get(start..)?;
    let start = start + tail.len() - tail.trim_start().len();
    if start == text.len() {
        return None;
    }
    let limit = text[start..].char_indices().nth(max_chars.max(1)).map_or(text.len(), |(offset, _)| start + offset);
    let abbreviation_periods = terminal_compatible_abbreviation_periods(tree, ParseLanguage::English);
    let mut last_space = None;
    let mut terminal = false;
    // Walk top-level nodes: their children include the individual periods of
    // ellipses and numbers, which must not become sentence boundaries.
    for node in tree.iter_nodes().filter(|node| node.span().end > start) {
        let span = node.span();
        if span.start >= limit {
            break;
        }
        if node.kind() == NodeKind::Whitespace {
            if terminal {
                return Some(start..span.start);
            }
            last_space = Some(span.start);
        } else if span.end > limit {
            return Some(start..last_space.filter(|space| *space > start).unwrap_or_else(|| if span.start > start { span.start } else { limit }));
        } else if node.kind() == NodeKind::Punct {
            if !node.text().chars().all(is_closing_enclosure) {
                terminal = node.text().chars().any(is_sentence_terminal);
            }
        } else if node.kind() != NodeKind::Tag {
            terminal = abbreviation_periods.iter().any(|period| period.end == span.end);
        }
    }
    if limit < text.len() {
        return Some(start..last_space.filter(|space| *space > start).unwrap_or(limit));
    }
    Some(start..text.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ellipses_stay_with_the_continuing_sentence() {
        for ellipsis in ["...", "…"] {
            let text = format!("“Well{ellipsis}” she said, “perhaps{ellipsis} tomorrow.” Next.");
            let first = next_chunk(&text, 0).unwrap();
            assert_eq!(&text[first.clone()], format!("“Well{ellipsis}” she said, “perhaps{ellipsis} tomorrow.”"));
            assert_eq!(&text[next_chunk(&text, first.end).unwrap()], "Next.");
        }
    }

    #[test]
    fn parsed_semantics_keep_internal_whitespace_and_punctuation_together() {
        for sentence in ["Use e. g. apples and pears.", "At 12:30 p.m. we left.", "Visit https://example.com/a...b for details.", "He paid 3.14 dollars."] {
            let text = format!("{sentence} Next.");
            let tree = parse_text_with_language(&text, ParseLanguage::English);
            let first = next_chunk_in_tree(&tree, 0, MAX_CHUNK_CHARS).unwrap();
            assert_eq!(&text[first.clone()], sentence);
            assert_eq!(&text[next_chunk_in_tree(&tree, first.end, MAX_CHUNK_CHARS).unwrap()], "Next.");
        }
    }

    #[test]
    fn limits_prefer_the_start_of_a_semantic_node() {
        let text = "Wait at 12:30 p.m. tomorrow.";
        let tree = parse_text_with_language(text, ParseLanguage::English);
        let span = next_chunk_in_tree(&tree, 0, 16).unwrap();
        assert_eq!(&text[span], "Wait at");
    }

    #[test]
    fn terminal_abbreviations_can_still_end_sentences() {
        for sentence in ["The prices apply, resp.", "This is art."] {
            let text = format!("{sentence} Next.");
            let first = next_chunk(&text, 0).unwrap();
            assert_eq!(&text[first], sentence);
        }
    }

    #[test]
    fn parsed_chunk_limits_preserve_source_and_always_advance() {
        let text = "  ångström... at 12:30 p.m. https://example.com/a...b 界界界  ";
        let tree = parse_text_with_language(text, ParseLanguage::English);
        for limit in [0, 1, 2, 8, 16, 32, 400] {
            let mut offset = 0;
            let mut recovered = String::new();
            while let Some(span) = next_chunk_in_tree(&tree, offset, limit) {
                assert!(span.end > offset);
                assert!(text[span.clone()].chars().count() <= limit.max(1));
                offset = span.end;
                recovered.push_str(&text[span]);
            }
            assert_eq!(recovered.chars().filter(|c| !c.is_whitespace()).collect::<String>(), text.chars().filter(|c| !c.is_whitespace()).collect::<String>());
        }
        assert_eq!(next_chunk_in_tree(&tree, text.len() + 1, 400), None);
        assert_eq!(next_chunk_in_tree(&tree, 3, 400), None); // Inside å's UTF-8 bytes.
    }

    #[test]
    fn sentence_chunks_preserve_abbreviations_numbers_and_quotes() {
        let text = "Dr. Smith paid 3.14 dollars. \"Really?\" Yes!";
        let first = next_chunk(text, 0).unwrap();
        assert_eq!(&text[first.clone()], "Dr. Smith paid 3.14 dollars.");
        let second = next_chunk(text, first.end).unwrap();
        assert_eq!(&text[second.clone()], "\"Really?\"");
        assert_eq!(&text[next_chunk(text, second.end).unwrap()], "Yes!");
    }

    #[test]
    fn oversized_items_are_bounded_without_losing_unicode_text() {
        for text in ["ångström ".repeat(300), "界".repeat(1000)] {
            let mut offset = 0;
            let mut chunks = Vec::new();
            while let Some(span) = next_chunk(&text, offset) {
                assert!(text[span.clone()].chars().count() <= MAX_CHUNK_CHARS);
                offset = span.end;
                chunks.push(text[span].to_owned());
            }
            assert_eq!(chunks.concat().chars().filter(|c| !c.is_whitespace()).collect::<String>(), text.chars().filter(|c| !c.is_whitespace()).collect::<String>());
        }
    }

    #[test]
    fn shorter_opening_keeps_the_remaining_sentence_and_unicode_intact() {
        let text = format!("Dr. Smith said {} before leaving.", "ångström ".repeat(60));
        let first = next_chunk_with_limit(&text, 0, FIRST_CHUNK_CHARS).unwrap();
        assert!(text[first.clone()].starts_with("Dr. Smith"));
        assert!(text[first.clone()].chars().count() <= FIRST_CHUNK_CHARS);
        let second = next_chunk(&text, first.end).unwrap();
        assert!(text[second.clone()].chars().count() > FIRST_CHUNK_CHARS);
        let mut recovered = text[first].to_owned();
        let mut offset = second.start;
        while let Some(span) = next_chunk(&text, offset) {
            offset = span.end;
            recovered.push_str(&text[span]);
        }
        assert_eq!(recovered.chars().filter(|c| !c.is_whitespace()).collect::<String>(), text.chars().filter(|c| !c.is_whitespace()).collect::<String>());
    }
}
