use std::marker::PhantomData;
use std::ops::Range;

use structure::{NodeKind, NodeRef, NumberFormat, NumberFormatAssessment, TokenUnitId};

/// Selects which parsed nodes a `NodeFactRef` views; each kind adds its own accessors.
pub trait NodeFactKind: Copy {
    fn matches(node: NodeRef<'_>) -> bool;
}

/// Kinds whose nodes carry number formats (numbers, measurements and currencies).
pub trait NumberFormattedFactKind: NodeFactKind {}

/// A typed read-only view of one parsed node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeFactRef<'a, K> {
    node: NodeRef<'a>,
    kind: PhantomData<K>,
}

impl<'a, K: NodeFactKind> NodeFactRef<'a, K> {
    pub(crate) fn new(node: NodeRef<'a>) -> Option<Self> { K::matches(node).then_some(Self { node, kind: PhantomData }) }
    pub(crate) fn node(&self) -> NodeRef<'a> { self.node }
    pub fn text(&self) -> &'a str { self.node.text() }
    pub fn span(&self) -> Range<usize> { self.node.span() }
    pub fn token_unit_ids(&self) -> Vec<TokenUnitId> { self.node.token_unit_ids() }
}

impl<'a, K: NumberFormattedFactKind> NodeFactRef<'a, K> {
    fn first_number_format(&self) -> Option<NumberFormat> { self.node.first_number_format() }
    pub fn decimal_separator(&self) -> Option<char> { self.first_number_format().and_then(|format| format.decimal_separator()) }
    pub fn grouping_separator(&self) -> Option<char> { self.first_number_format().and_then(|format| format.grouping_separator()) }
    pub fn locale_decimal_separator(&self) -> Option<char> { self.first_number_format().and_then(|format| format.locale_decimal_separator()) }
    pub fn locale_grouping_separator(&self) -> Option<char> { self.first_number_format().and_then(|format| format.locale_grouping_separator()) }
    pub fn number_values(&self) -> &'a [i128] { self.node.number_values() }
    pub fn number_formats(&self) -> &'a [NumberFormat] { self.node.number_formats() }
    pub fn number_format_assessment(&self) -> Option<NumberFormatAssessment> { self.first_number_format().map(|format| format.assessment()) }
    pub fn number_format_requires_locale_change(&self) -> bool { self.node.number_formats().iter().any(NumberFormat::requires_locale_change) }
    pub fn decimal_separator_matches_locale(&self) -> Option<bool> { self.first_number_format().and_then(|format| format.decimal_separator_matches_locale()) }
}

macro_rules! node_fact_kinds {
    ($($kind:ident => $matches:expr;)*) => {$(
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct $kind;

        impl NodeFactKind for $kind {
            fn matches(node: NodeRef<'_>) -> bool { let matches: fn(NodeRef<'_>) -> bool = $matches; matches(node) }
        }
    )*};
}

node_fact_kinds! {
    NumberFactKind => |node| node.kind().localizable_kind() == Some(structure::LocalizableKind::Number);
    MeasurementFactKind => |node| node.measurement_fact().is_some();
    CurrencyFactKind => |node| node.currency_fact().is_some();
    DateFactKind => |node| node.date_fact().is_some();
    TimeFactKind => |node| node.time_fact().is_some();
    OrdinalFactKind => |node| node.ordinal_fact().is_some();
    NumberRangeFactKind => |node| crate::numeric_facts::node_number_range_parts(node).is_some();
    VersionFactKind => |node| node.version_semantic().is_some();
    UrlFactKind => |node| node.kind() == NodeKind::Url;
    EmailFactKind => |node| node.kind() == NodeKind::Email;
}

impl NumberFormattedFactKind for NumberFactKind {}
impl NumberFormattedFactKind for MeasurementFactKind {}
impl NumberFormattedFactKind for CurrencyFactKind {}

