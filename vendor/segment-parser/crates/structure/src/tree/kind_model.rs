use serde::{Deserialize, Serialize};
use segment_lexer::LexicalKind;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnitKind {
    Word,
    Numeric,
    Number,
    Date,
    Measurement,
    Time,
    Currency,
    IpAddress,
    Url,
    Email,
    Term,
    NonTranslatable,
    Abbreviation,
    Whitespace,
    Punct,
    Tag,
}

impl From<LexicalKind> for UnitKind {
    fn from(kind: LexicalKind) -> Self {
        match kind {
            LexicalKind::Word => Self::Word,
            LexicalKind::Numeric => Self::Numeric,
            LexicalKind::Whitespace => Self::Whitespace,
            LexicalKind::Punct => Self::Punct,
        }
    }
}

impl UnitKind {
    pub(super) const ALL: [Self; 16] = [Self::Word, Self::Numeric, Self::Number, Self::Date, Self::Measurement, Self::Time, Self::Currency, Self::IpAddress, Self::Url, Self::Email, Self::Term, Self::NonTranslatable, Self::Abbreviation, Self::Whitespace, Self::Punct, Self::Tag];
    pub(super) fn compact_code(self) -> u8 { self as u8 }
    pub(super) fn from_compact_code(code: u8) -> Option<Self> { Self::ALL.get(code as usize).copied() }

    pub fn is_text_content(self) -> bool { matches!(self, Self::Word | Self::Numeric | Self::Number | Self::Date | Self::Measurement | Self::Time | Self::Currency | Self::IpAddress | Self::Url | Self::Email | Self::Term | Self::NonTranslatable | Self::Abbreviation) }
    pub fn is_numeric_fact(self) -> bool { self.node_kind().is_numeric_fact() }
    pub fn has_semantic_identity(self) -> bool { matches!(self, Self::Number | Self::Date | Self::Measurement | Self::Time | Self::Currency | Self::IpAddress | Self::Url | Self::Email | Self::Term | Self::NonTranslatable | Self::Abbreviation | Self::Tag) }
    pub fn is_semantic_text(self) -> bool { self.is_text_content() && self.has_semantic_identity() }
    pub fn node_kind(self) -> NodeKind {
        match self {
            Self::Word => NodeKind::Word,
            Self::Numeric => NodeKind::Numeric,
            Self::Number => NodeKind::Number,
            Self::Date => NodeKind::Date,
            Self::Measurement => NodeKind::Measurement,
            Self::Time => NodeKind::Time,
            Self::Currency => NodeKind::Currency,
            Self::IpAddress => NodeKind::IpAddress,
            Self::Url => NodeKind::Url,
            Self::Email => NodeKind::Email,
            Self::Term => NodeKind::Term,
            Self::NonTranslatable => NodeKind::Opaque,
            Self::Abbreviation => NodeKind::Abbreviation,
            Self::Whitespace => NodeKind::Whitespace,
            Self::Punct => NodeKind::Punct,
            Self::Tag => NodeKind::Tag,
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LocalizableKind {
    Number,
    NumberRange,
    Ordinal,
    Date,
    Time,
    Measurement,
    Currency,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProtectedKind {
    Opaque,
    IpAddress,
    Url,
    Email,
    Placeholder,
    Version,
}

impl ProtectedKind {
    pub(in crate::tree) fn participates_in_non_translatable_matching(self) -> bool { matches!(self, Self::Opaque | Self::IpAddress | Self::Placeholder | Self::Version) }
}

impl LocalizableKind {
    pub fn is_numeric_fact(self) -> bool { matches!(self, Self::Number | Self::Date | Self::Time | Self::Measurement | Self::Currency) }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlaceableKind {
    Localizable(LocalizableKind),
    Protected(ProtectedKind),
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeKind {
    Root,
    Word,
    Numeric,
    Whitespace,
    Punct,
    Tag,
    Term,
    CompoundWord,
    Abbreviation,
    Ellipsis,
    Placeable(PlaceableKind),
}

#[allow(non_upper_case_globals)]
impl NodeKind {
    pub const Number: Self = Self::Placeable(PlaceableKind::Localizable(LocalizableKind::Number));
    pub const NumberRange: Self = Self::Placeable(PlaceableKind::Localizable(LocalizableKind::NumberRange));
    pub const Ordinal: Self = Self::Placeable(PlaceableKind::Localizable(LocalizableKind::Ordinal));
    pub const Date: Self = Self::Placeable(PlaceableKind::Localizable(LocalizableKind::Date));
    pub const Time: Self = Self::Placeable(PlaceableKind::Localizable(LocalizableKind::Time));
    pub const Measurement: Self = Self::Placeable(PlaceableKind::Localizable(LocalizableKind::Measurement));
    pub const Currency: Self = Self::Placeable(PlaceableKind::Localizable(LocalizableKind::Currency));
    pub const Opaque: Self = Self::Placeable(PlaceableKind::Protected(ProtectedKind::Opaque));
    pub const NonTranslatable: Self = Self::Opaque;
    pub const IpAddress: Self = Self::Placeable(PlaceableKind::Protected(ProtectedKind::IpAddress));
    pub const Url: Self = Self::Placeable(PlaceableKind::Protected(ProtectedKind::Url));
    pub const Email: Self = Self::Placeable(PlaceableKind::Protected(ProtectedKind::Email));
    pub const Placeholder: Self = Self::Placeable(PlaceableKind::Protected(ProtectedKind::Placeholder));
    pub const Version: Self = Self::Placeable(PlaceableKind::Protected(ProtectedKind::Version));

    pub fn placeable_kind(self) -> Option<PlaceableKind> { match self { Self::Placeable(kind) => Some(kind), _ => None } }
    pub fn localizable_kind(self) -> Option<LocalizableKind> { match self.placeable_kind() { Some(PlaceableKind::Localizable(kind)) => Some(kind), _ => None } }
    pub fn protected_kind(self) -> Option<ProtectedKind> { match self.placeable_kind() { Some(PlaceableKind::Protected(kind)) => Some(kind), _ => None } }
    pub fn is_protected(self) -> bool { self.protected_kind().is_some() }
    pub fn is_numeric_fact(self) -> bool { self.localizable_kind().is_some_and(LocalizableKind::is_numeric_fact) }
    pub fn semantic_text_unit_kind(self) -> Option<UnitKind> { self.unit_kind().filter(|kind| kind.is_semantic_text()) }
    pub fn unit_kind(self) -> Option<UnitKind> {
        match self {
            Self::Word => Some(UnitKind::Word),
            Self::Numeric => Some(UnitKind::Numeric),
            Self::Whitespace => Some(UnitKind::Whitespace),
            Self::Punct => Some(UnitKind::Punct),
            Self::Tag => Some(UnitKind::Tag),
            Self::Url => Some(UnitKind::Url),
            Self::Email => Some(UnitKind::Email),
            Self::Term => Some(UnitKind::Term),
            Self::Opaque => Some(UnitKind::NonTranslatable),
            Self::Abbreviation => Some(UnitKind::Abbreviation),
            Self::Date => Some(UnitKind::Date),
            Self::Measurement => Some(UnitKind::Measurement),
            Self::Number => Some(UnitKind::Number),
            Self::Time => Some(UnitKind::Time),
            Self::Currency => Some(UnitKind::Currency),
            Self::IpAddress => Some(UnitKind::IpAddress),
            Self::Root | Self::CompoundWord | Self::NumberRange | Self::Ordinal | Self::Ellipsis | Self::Placeholder | Self::Version => None,
        }
    }
}
