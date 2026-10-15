//! Bounded synthesis chunks keep logical reading items independent of audio buffers.
use segment_rules::abbreviations::ENGLISH_POLICY;
use segment_rules::char_class::is_closing_enclosure;
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
    let mut word_start = start;
    let mut last_space = None;
    let mut terminal = false;
    for (count, (offset, character)) in text[start..].char_indices().enumerate() {
        let offset = start + offset;
        if character.is_whitespace() {
            if terminal {
                return Some(start..offset);
            }
            last_space = Some(offset);
            word_start = offset + character.len_utf8();
        } else if !is_closing_enclosure(character) {
            terminal = matches!(character, '!' | '?' | '。' | '！' | '？') || (character == '.' && !ENGLISH_POLICY.recognizes(text, word_start..offset + 1));
        }
        if count + 1 >= max_chars.max(1) {
            return Some(start..last_space.filter(|space| *space > start).unwrap_or(offset + character.len_utf8()));
        }
    }
    Some(start..text.len())
}

#[cfg(test)]
mod tests {
    use super::*;

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
