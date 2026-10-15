use serde::{Deserialize, Serialize};

#[path = "punctuation_model/view.rs"]
mod view;
pub use view::DelimiterFactRef;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DelimiterFamily {
    DoubleQuote,
    SingleQuote,
    RoundBracket,
    SquareBracket,
    CurlyBracket,
    Question,
    Exclamation,
}

impl DelimiterFamily {
    pub fn is_quote(self) -> bool { matches!(self, Self::DoubleQuote | Self::SingleQuote) }
    pub fn is_bracket(self) -> bool { matches!(self, Self::RoundBracket | Self::SquareBracket | Self::CurlyBracket) }
    pub fn is_language_punctuation(self) -> bool { matches!(self, Self::Question | Self::Exclamation) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DelimiterRole {
    Open,
    Close,
    Unmatched,
    Ignored,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum QuotationStyle {
    Straight,
    #[serde(alias = "EnglishCurly")]
    CurlyOpenClose,
    #[serde(alias = "SwedishCurly")]
    CurlyHighHigh,
    #[serde(alias = "GermanLowHigh")]
    CurlyLowHigh,
    Guillemets,
    ReversedGuillemets,
    Other,
}

impl QuotationStyle {
    pub fn marks(self, family: DelimiterFamily) -> Option<(char, char)> {
        match (self, family) {
            (Self::Straight, DelimiterFamily::DoubleQuote) => Some(('"', '"')),
            (Self::Straight, DelimiterFamily::SingleQuote) => Some(('\'', '\'')),
            (Self::CurlyOpenClose, DelimiterFamily::DoubleQuote) => Some(('“', '”')),
            (Self::CurlyOpenClose, DelimiterFamily::SingleQuote) => Some(('‘', '’')),
            (Self::CurlyHighHigh, DelimiterFamily::DoubleQuote) => Some(('”', '”')),
            (Self::CurlyHighHigh, DelimiterFamily::SingleQuote) => Some(('’', '’')),
            (Self::CurlyLowHigh, DelimiterFamily::DoubleQuote) => Some(('„', '“')),
            (Self::CurlyLowHigh, DelimiterFamily::SingleQuote) => Some(('‚', '‘')),
            (Self::Guillemets, DelimiterFamily::DoubleQuote) => Some(('«', '»')),
            (Self::Guillemets, DelimiterFamily::SingleQuote) => Some(('‹', '›')),
            (Self::ReversedGuillemets, DelimiterFamily::DoubleQuote) => Some(('»', '«')),
            (Self::ReversedGuillemets, DelimiterFamily::SingleQuote) => Some(('›', '‹')),
            _ => None,
        }
    }
    pub fn from_marks(family: DelimiterFamily, open: char, close: char) -> Self { [Self::Straight, Self::CurlyOpenClose, Self::CurlyHighHigh, Self::CurlyLowHigh, Self::Guillemets, Self::ReversedGuillemets].into_iter().find(|style| style.marks(family) == Some((open, close))).unwrap_or(Self::Other) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DelimiterAnnotation {
    family: DelimiterFamily,
    role: DelimiterRole,
    #[serde(default)]
    preferred_quotation_style: Option<QuotationStyle>,
}

impl DelimiterAnnotation {
    pub fn new(family: DelimiterFamily, role: DelimiterRole) -> Self { Self { family, role, preferred_quotation_style: None } }
    pub fn with_preferred_quotation_style(mut self, style: Option<QuotationStyle>) -> Self { self.preferred_quotation_style = style; self }
    pub fn family(self) -> DelimiterFamily { self.family }
    pub fn role(self) -> DelimiterRole { self.role }
    pub fn preferred_quotation_style(self) -> Option<QuotationStyle> { self.preferred_quotation_style }
}
