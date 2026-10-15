use std::iter::FusedIterator;
use std::ops::Range;

use unicode_properties::{GeneralCategoryGroup, UnicodeEmoji, UnicodeGeneralCategory};

const ASCII_KINDS: [LexicalKind; 256] = ascii_kind_table();

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LexicalKind {
    Word,
    Numeric,
    Whitespace,
    Punct,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lexeme<'a> {
    text: &'a str,
    kind: LexicalKind,
    start: usize,
    end: usize,
}

impl<'a> Lexeme<'a> {
    fn new(text: &'a str, kind: LexicalKind, start: usize, end: usize) -> Self { Self { text, kind, start, end } }
    pub fn kind(self) -> LexicalKind { self.kind }
    pub fn text(self) -> &'a str { &self.text[self.start..self.end] }
    pub fn span(self) -> Range<usize> { self.start..self.end }
}

#[derive(Clone, Debug)]
pub struct Lexemes<'a> {
    text: &'a str,
    cursor: usize,
}

impl<'a> Lexemes<'a> {
    fn new(text: &'a str) -> Self { Self { text, cursor: 0 } }
}

impl<'a> Iterator for Lexemes<'a> {
    type Item = Lexeme<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let start = self.cursor;
        let (kind, mut end) = kind_at(self.text, start)?;
        if kind == LexicalKind::Punct {
            self.cursor = end;
            return Some(Lexeme::new(self.text, kind, start, end));
        }
        while let Some((next_kind, next_end)) = kind_at(self.text, end) {
            if is_joined_combining_mark(self.text, end, start) {
                end = next_end;
                continue;
            }
            if next_kind != kind || next_kind == LexicalKind::Punct {
                break;
            }
            end = next_end;
        }
        self.cursor = end;
        Some(Lexeme::new(self.text, kind, start, end))
    }

    fn size_hint(&self) -> (usize, Option<usize>) { (usize::from(self.cursor < self.text.len()), Some(self.text.len().saturating_sub(self.cursor))) }
}

impl FusedIterator for Lexemes<'_> {}

pub fn lexemes(text: &str) -> Lexemes<'_> { Lexemes::new(text) }

pub fn lexical_token_spans(text: &str) -> impl Iterator<Item = (LexicalKind, Range<usize>, &str)> + Clone { lexemes(text).map(|lexeme| (lexeme.kind(), lexeme.span(), lexeme.text())) }

pub fn word_token_spans(text: &str) -> impl Iterator<Item = (Range<usize>, &str)> + Clone { lexemes(text).filter_map(|lexeme| (lexeme.kind() == LexicalKind::Word).then(|| (lexeme.span(), lexeme.text()))) }

pub fn word_tokens(text: &str) -> impl Iterator<Item = &str> + Clone { lexemes(text).filter_map(|lexeme| (lexeme.kind() == LexicalKind::Word).then(|| lexeme.text())) }

pub fn is_unicode_punctuation(character: char) -> bool { character.general_category_group() == GeneralCategoryGroup::Punctuation }

fn is_joined_combining_mark(text: &str, index: usize, run_start: usize) -> bool {
    index > run_start
        && text[index..].chars().next().is_some_and(|character| character.general_category_group() == GeneralCategoryGroup::Mark && !matches!(character, '\u{FE00}'..='\u{FE0F}'))
}

fn kind_at(text: &str, index: usize) -> Option<(LexicalKind, usize)> {
    let byte = *text.as_bytes().get(index)?;
    if byte.is_ascii() {
        return Some((ASCII_KINDS[byte as usize], index + 1));
    }
    let character = text[index..].chars().next()?;
    Some((kind_for(character), index + character.len_utf8()))
}

fn kind_for(character: char) -> LexicalKind {
    if character.is_alphabetic() || character.is_emoji_char() {
        LexicalKind::Word
    } else if character.is_ascii_digit() {
        LexicalKind::Numeric
    } else if character.is_whitespace() {
        LexicalKind::Whitespace
    } else {
        LexicalKind::Punct
    }
}

const fn ascii_kind_table() -> [LexicalKind; 256] {
    let mut table = [LexicalKind::Punct; 256];
    let mut index = b'A' as usize;
    while index <= b'Z' as usize {
        table[index] = LexicalKind::Word;
        index += 1;
    }
    index = b'a' as usize;
    while index <= b'z' as usize {
        table[index] = LexicalKind::Word;
        index += 1;
    }
    index = b'0' as usize;
    while index <= b'9' as usize {
        table[index] = LexicalKind::Numeric;
        index += 1;
    }
    table[b' ' as usize] = LexicalKind::Whitespace;
    table[b'\t' as usize] = LexicalKind::Whitespace;
    table[b'\n' as usize] = LexicalKind::Whitespace;
    table[b'\r' as usize] = LexicalKind::Whitespace;
    table[0x0B] = LexicalKind::Whitespace;
    table[0x0C] = LexicalKind::Whitespace;
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_word_byte_spans() {
        let text = " Ångström-42 café! ";
        assert_eq!(word_token_spans(text).map(|(span, token)| (span, token.to_string())).collect::<Vec<_>>(), vec![(1..11, "Ångström".to_string()), (15..20, "café".to_string())]);
        assert_eq!(word_tokens(text).collect::<Vec<_>>(), vec!["Ångström", "café"]);
    }

    #[test]
    fn exposes_every_scanner_class() {
        assert_eq!(lexical_token_spans("Word42 !").map(|(kind, _, token)| (kind, token)).collect::<Vec<_>>(), vec![(LexicalKind::Word, "Word"), (LexicalKind::Numeric, "42"), (LexicalKind::Whitespace, " "), (LexicalKind::Punct, "!")]);
    }

    #[test]
    fn emits_each_punctuation_character_separately() {
        assert_eq!(lexical_token_spans("a...b").map(|(_, span, token)| (span, token)).collect::<Vec<_>>(), vec![(0..1, "a"), (1..2, "."), (2..3, "."), (3..4, "."), (4..5, "b")]);
    }

    #[test]
    fn distinguishes_unicode_punctuation_from_symbols_in_the_punctuation_lexical_class() {
        assert!(is_unicode_punctuation('…'));
        assert!(is_unicode_punctuation('”'));
        assert!(!is_unicode_punctuation('€'));
        assert!(!is_unicode_punctuation('™'));
    }

    #[test]
    fn keeps_combining_marks_with_their_base_character() {
        assert_eq!(word_tokens("Cafe\u{301} noir").collect::<Vec<_>>(), vec!["Cafe\u{301}", "noir"]);
    }

    #[test]
    fn treats_emoji_as_words() {
        assert_eq!(lexical_token_spans("Hi 😳!").map(|(kind, _, token)| (kind, token)).collect::<Vec<_>>(), vec![(LexicalKind::Word, "Hi"), (LexicalKind::Whitespace, " "), (LexicalKind::Word, "😳"), (LexicalKind::Punct, "!")]);
    }

    #[test]
    fn iteration_is_lossless_and_cloneable() {
        let text = "Ångström 42...";
        let lexemes = lexemes(text);
        assert_eq!(lexemes.clone().map(Lexeme::text).collect::<String>(), text);
        assert_eq!(lexemes.count(), 6);
    }
}
