//! General text recognition shared by segment parsing and other consumers.
//! This crate owns existing abbreviation/month catalogs and character/context rules.
//! Recognition accepts plain text and byte spans, independently of segment trees.

pub mod abbreviations;
pub mod char_class;
pub mod months;
pub use segment_lexer::{Lexeme, LexicalKind, Lexemes, lexemes, word_token_spans, word_tokens};
