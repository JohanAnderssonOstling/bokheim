//! Character classes shared by the parser and the analysis facts. Quote marks
//! are direction-neutral on purpose: Swedish uses ”…”, German »…« and French
//! «…», so any quote mark can open or close depending on the language.

pub fn is_quote_mark(character: char) -> bool { matches!(character, '"' | '\'' | '“' | '”' | '„' | '‘' | '’' | '‚' | '«' | '»' | '‹' | '›') }

pub fn is_opening_bracket(character: char) -> bool { matches!(character, '(' | '[' | '{') }

pub fn is_closing_bracket(character: char) -> bool { matches!(character, ')' | ']' | '}') }

/// Marks that may precede a sentence's or token's first content character.
pub fn is_opening_enclosure(character: char) -> bool { is_opening_bracket(character) || is_quote_mark(character) }

/// Marks that may follow a sentence's or token's last content character.
pub fn is_closing_enclosure(character: char) -> bool { is_closing_bracket(character) || is_quote_mark(character) }

pub fn is_apostrophe(character: char) -> bool { matches!(character, '\'' | '’') }

pub fn is_hyphen(character: char) -> bool { matches!(character, '-' | '\u{2010}' | '\u{2011}') }

/// Hyphens and typographic dashes, excluding the minus sign.
pub fn is_dash(character: char) -> bool { is_hyphen(character) || matches!(character, '\u{2012}' | '\u{2013}' | '\u{2014}') }

pub fn is_dash_or_minus(character: char) -> bool { is_dash(character) || character == '\u{2212}' }

/// Characters that join the parts of one word, as in "state-of-the-art" or "don't".
pub fn is_word_joiner(character: char) -> bool { is_hyphen(character) || is_apostrophe(character) }

pub fn is_sentence_terminal(character: char) -> bool { matches!(character, '.' | '!' | '?' | '…' | '。' | '！' | '？' | '؟' | '։' | '՜' | '՞' | '\u{037E}') }

pub fn is_list_separator(character: char) -> bool { matches!(character, ',' | ';' | ':' | '،' | '؛' | '，' | '；' | '：') }

pub fn is_punctuation(character: char) -> bool { character.is_ascii_punctuation() || segment_lexer::is_unicode_punctuation(character) || character == '\u{2212}' }

pub fn is_roman_numeral_letter(character: char) -> bool { matches!(character, 'I' | 'V' | 'X' | 'L' | 'C' | 'D' | 'M') }

pub fn contains_line_break(text: &str) -> bool { text.contains(['\r', '\n']) }
