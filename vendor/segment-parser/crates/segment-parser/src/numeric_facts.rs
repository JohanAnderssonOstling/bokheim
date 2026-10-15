use std::ops::Range;

use structure::parser_api::{node_semantic, numeric_nodes, NodeSemantic};

use crate::node_facts::{CurrencyFactKind, DateFactKind, MeasurementFactKind, NodeFactRef, NumberFactKind, NumberRangeFactKind, OrdinalFactKind, TimeFactKind};
use structure::{
    CurrencySemantic, DateFact, LocalizableKind, MeasurementSemantic, NodeKind, NodeRef, NumberFormat,
    NumberFormatAssessment, NumberRangeTypography, OrdinalAssessment, OrdinalFact, OrdinalStyle,
    SegmentTree, TimeFact, TokenUnitId, WhitespaceTypographyFact,
};

pub type SegmentOrdinalFactRef<'a> = NodeFactRef<'a, OrdinalFactKind>;
pub type SegmentNumberFactRef<'a> = NodeFactRef<'a, NumberFactKind>;
pub type SegmentMeasurementFactRef<'a> = NodeFactRef<'a, MeasurementFactKind>;
pub type SegmentDateFactRef<'a> = NodeFactRef<'a, DateFactKind>;
pub type SegmentTimeFactRef<'a> = NodeFactRef<'a, TimeFactKind>;
pub type SegmentCurrencyFactRef<'a> = NodeFactRef<'a, CurrencyFactKind>;
pub type SegmentNumberRangeFactRef<'a> = NodeFactRef<'a, NumberRangeFactKind>;

impl NodeFactRef<'_, OrdinalFactKind> {
    fn ordinal(&self) -> OrdinalFact { self.node().ordinal_fact().expect("typed ordinal fact must retain ordinal semantics") }
    pub fn value(&self) -> u32 { self.ordinal().value() }
    pub fn style(&self) -> OrdinalStyle { self.ordinal().style() }
    pub fn preferred_style(&self) -> OrdinalStyle { self.ordinal().preferred_style() }
    pub fn assessment(&self) -> OrdinalAssessment { self.ordinal().assessment() }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentNumericKind {
    Number,
    Measurement,
    Date,
    Time,
    Currency,
}

impl SegmentNumericKind {
    fn from_node_kind(kind: NodeKind) -> Option<Self> {
        match kind.localizable_kind() {
            Some(LocalizableKind::Number) => Some(Self::Number),
            Some(LocalizableKind::Measurement) => Some(Self::Measurement),
            Some(LocalizableKind::Date) => Some(Self::Date),
            Some(LocalizableKind::Time) => Some(Self::Time),
            Some(LocalizableKind::Currency) => Some(Self::Currency),
            Some(LocalizableKind::NumberRange | LocalizableKind::Ordinal) | None => None,
        }
    }
}

impl<'a> NodeFactRef<'a, MeasurementFactKind> {
    pub fn measurement_fact(&self) -> &'a MeasurementSemantic { self.node().measurement_fact().expect("typed measurement fact must retain measurement semantics") }
    pub fn measurement_unit_span(&self) -> Range<usize> {
        let typography = self.measurement_fact().typography();
        let start = self.span().start;
        start + typography.unit_start()..start + typography.unit_end()
    }
    pub fn measurement_separator_span(&self) -> Range<usize> {
        let typography = self.measurement_fact().typography();
        let start = self.span().start;
        start + typography.separator_start()..start + typography.unit_start()
    }
    pub fn whitespace_typography_fact(&self) -> WhitespaceTypographyFact { self.measurement_fact().typography().whitespace_fact() }
    pub fn measurement_unit_ids(&self) -> Vec<TokenUnitId> { self.node().tree().token_unit_ids_for_span(self.measurement_unit_span()) }
}

impl NodeFactRef<'_, DateFactKind> {
    pub fn date_fact(&self) -> DateFact { self.node().date_fact().expect("typed date fact must retain date semantics") }
}

impl NodeFactRef<'_, TimeFactKind> {
    pub fn time_fact(&self) -> TimeFact { self.node().time_fact().expect("typed time fact must retain time semantics") }
}

impl<'a> NodeFactRef<'a, CurrencyFactKind> {
    pub fn currency_fact(&self) -> &'a CurrencySemantic { self.node().currency_fact().expect("typed currency fact must retain currency semantics") }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentQuantityFactRef<'a> {
    Measurement(SegmentMeasurementFactRef<'a>),
    Currency(SegmentCurrencyFactRef<'a>),
}

impl<'a> SegmentQuantityFactRef<'a> {
    fn new(node: NodeRef<'a>) -> Option<Self> {
        NodeFactRef::new(node).map(Self::Measurement).or_else(|| NodeFactRef::new(node).map(Self::Currency))
    }
    fn node(&self) -> NodeRef<'a> { match self { Self::Measurement(fact) => fact.node(), Self::Currency(fact) => fact.node() } }
    pub fn text(&self) -> &'a str { self.node().text() }
    pub fn span(&self) -> Range<usize> { self.node().span() }
    pub fn token_unit_ids(&self) -> Vec<TokenUnitId> { self.node().token_unit_ids() }
    pub fn number_formats(&self) -> &'a [NumberFormat] { self.node().number_formats() }
    pub fn number_format_requires_locale_change(&self) -> bool { self.number_formats().iter().any(NumberFormat::requires_locale_change) }
    pub fn as_measurement(self) -> Option<SegmentMeasurementFactRef<'a>> { match self { Self::Measurement(fact) => Some(fact), Self::Currency(_) => None } }
}

impl<'a> From<SegmentMeasurementFactRef<'a>> for SegmentQuantityFactRef<'a> {
    fn from(fact: SegmentMeasurementFactRef<'a>) -> Self { Self::Measurement(fact) }
}

impl<'a> From<SegmentCurrencyFactRef<'a>> for SegmentQuantityFactRef<'a> {
    fn from(fact: SegmentCurrencyFactRef<'a>) -> Self { Self::Currency(fact) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentNumericFactRef<'a> {
    Number(SegmentNumberFactRef<'a>),
    Measurement(SegmentMeasurementFactRef<'a>),
    Date(SegmentDateFactRef<'a>),
    Time(SegmentTimeFactRef<'a>),
    Currency(SegmentCurrencyFactRef<'a>),
}

impl<'a> SegmentNumericFactRef<'a> {
    fn new(node: NodeRef<'a>) -> Option<Self> {
        match SegmentNumericKind::from_node_kind(node.kind())? {
            SegmentNumericKind::Number => NodeFactRef::new(node).map(Self::Number),
            SegmentNumericKind::Measurement => NodeFactRef::new(node).map(Self::Measurement),
            SegmentNumericKind::Date => NodeFactRef::new(node).map(Self::Date),
            SegmentNumericKind::Time => NodeFactRef::new(node).map(Self::Time),
            SegmentNumericKind::Currency => NodeFactRef::new(node).map(Self::Currency),
        }
    }
    fn node(&self) -> NodeRef<'a> { match self { Self::Number(fact) => fact.node(), Self::Measurement(fact) => fact.node(), Self::Date(fact) => fact.node(), Self::Time(fact) => fact.node(), Self::Currency(fact) => fact.node() } }
    fn number_formatted(&self) -> Option<NodeRef<'a>> { (!matches!(self, Self::Date(_) | Self::Time(_))).then(|| self.node()) }
    pub fn text(&self) -> &'a str { self.node().text() }
    pub fn kind(&self) -> SegmentNumericKind { match self { Self::Number(_) => SegmentNumericKind::Number, Self::Measurement(_) => SegmentNumericKind::Measurement, Self::Date(_) => SegmentNumericKind::Date, Self::Time(_) => SegmentNumericKind::Time, Self::Currency(_) => SegmentNumericKind::Currency } }
    pub fn span(&self) -> Range<usize> { self.node().span() }
    pub fn token_unit_ids(&self) -> Vec<TokenUnitId> { self.node().token_unit_ids() }
    pub fn decimal_separator(&self) -> Option<char> { self.number_formats().first().and_then(|format| format.decimal_separator()) }
    pub fn grouping_separator(&self) -> Option<char> { self.number_formats().first().and_then(|format| format.grouping_separator()) }
    pub fn locale_decimal_separator(&self) -> Option<char> { self.number_formats().first().and_then(|format| format.locale_decimal_separator()) }
    pub fn locale_grouping_separator(&self) -> Option<char> { self.number_formats().first().and_then(|format| format.locale_grouping_separator()) }
    pub fn number_values(&self) -> &'a [i128] { self.number_formatted().map_or(&[], |node| node.number_values()) }
    pub fn number_formats(&self) -> &'a [NumberFormat] { self.number_formatted().map_or(&[], |node| node.number_formats()) }
    pub fn number_format_assessment(&self) -> Option<NumberFormatAssessment> { self.number_formats().first().map(|format| format.assessment()) }
    pub fn number_format_requires_locale_change(&self) -> bool { self.number_formats().iter().any(NumberFormat::requires_locale_change) }
    pub fn decimal_separator_matches_locale(&self) -> Option<bool> { self.number_formats().first().and_then(|format| format.decimal_separator_matches_locale()) }
    pub fn as_measurement(self) -> Option<SegmentMeasurementFactRef<'a>> { match self { Self::Measurement(fact) => Some(fact), _ => None } }
    pub fn date_fact(&self) -> Option<DateFact> { match self { Self::Date(fact) => Some(fact.date_fact()), _ => None } }
    pub fn time_fact(&self) -> Option<TimeFact> { match self { Self::Time(fact) => Some(fact.time_fact()), _ => None } }
    pub fn currency_fact(&self) -> Option<&'a CurrencySemantic> { match self { Self::Currency(fact) => Some(fact.currency_fact()), _ => None } }
    pub fn measurement_fact(&self) -> Option<&'a MeasurementSemantic> { match self { Self::Measurement(fact) => Some(fact.measurement_fact()), _ => None } }
    pub fn measurement_unit_span(&self) -> Option<Range<usize>> { match self { Self::Measurement(fact) => Some(fact.measurement_unit_span()), _ => None } }
    pub fn measurement_separator_span(&self) -> Option<Range<usize>> { match self { Self::Measurement(fact) => Some(fact.measurement_separator_span()), _ => None } }
    pub fn whitespace_typography_fact(&self) -> Option<WhitespaceTypographyFact> { match self { Self::Measurement(fact) => Some(fact.whitespace_typography_fact()), _ => None } }
    pub fn measurement_unit_ids(&self) -> Vec<TokenUnitId> { match self { Self::Measurement(fact) => fact.measurement_unit_ids(), _ => Vec::new() } }
}

impl<'a> From<SegmentNumberFactRef<'a>> for SegmentNumericFactRef<'a> {
    fn from(fact: SegmentNumberFactRef<'a>) -> Self { Self::Number(fact) }
}

impl<'a> From<SegmentMeasurementFactRef<'a>> for SegmentNumericFactRef<'a> {
    fn from(fact: SegmentMeasurementFactRef<'a>) -> Self { Self::Measurement(fact) }
}

impl<'a> From<SegmentDateFactRef<'a>> for SegmentNumericFactRef<'a> {
    fn from(fact: SegmentDateFactRef<'a>) -> Self { Self::Date(fact) }
}

impl<'a> From<SegmentTimeFactRef<'a>> for SegmentNumericFactRef<'a> {
    fn from(fact: SegmentTimeFactRef<'a>) -> Self { Self::Time(fact) }
}

impl<'a> From<SegmentCurrencyFactRef<'a>> for SegmentNumericFactRef<'a> {
    fn from(fact: SegmentCurrencyFactRef<'a>) -> Self { Self::Currency(fact) }
}

impl<'a> From<SegmentQuantityFactRef<'a>> for SegmentNumericFactRef<'a> {
    fn from(fact: SegmentQuantityFactRef<'a>) -> Self { match fact { SegmentQuantityFactRef::Measurement(fact) => Self::Measurement(fact), SegmentQuantityFactRef::Currency(fact) => Self::Currency(fact) } }
}

impl NodeFactRef<'_, NumberRangeFactKind> {
    fn parts(&self) -> (i128, i128, NumberRangeTypography) { node_number_range_parts(self.node()).expect("typed number range fact must retain range semantics") }
    pub fn values(&self) -> Option<(i128, i128)> { let (start, end, _) = self.parts(); Some((start, end)) }
    pub fn typography(&self) -> Option<NumberRangeTypography> { Some(self.parts().2) }
    pub fn typography_is_canonical(&self) -> bool { self.typography().is_some_and(NumberRangeTypography::is_canonical) }
    pub fn separator_region_span(&self) -> Option<Range<usize>> {
        let typography = self.typography()?;
        let start = self.span().start;
        Some(start + typography.separator_region_start()..start + typography.separator_region_end())
    }
    pub fn canonical_separator(&self) -> Option<String> { self.typography().map(|typography| typography.preferred_separator().to_string()) }
}

pub(crate) fn node_number_range_parts(node: NodeRef<'_>) -> Option<(i128, i128, NumberRangeTypography)> {
    match node_semantic(node)? {
        NodeSemantic::NumberRange(range) => Some((range.start_value(), range.end_value(), range.typography())),
        NodeSemantic::Measurement(measurement) => {
            let normalized = measurement.values();
            let values = measurement.number_values();
            if normalized.len() == 2 {
                return Some((normalized[0].amount(), normalized[1].amount(), measurement.number_range_typography()?));
            }
            (values.len() == 2).then(|| measurement.number_range_typography().map(|typography| (values[0], values[1], typography))).flatten()
        }
        _ => None,
    }
}

pub fn all_numeric_facts(tree: &SegmentTree) -> impl Iterator<Item = SegmentNumericFactRef<'_>> + Clone { numeric_nodes(tree).filter_map(SegmentNumericFactRef::new) }
pub fn numeric_facts(tree: &SegmentTree) -> impl Iterator<Item = SegmentNumericFactRef<'_>> + Clone { all_numeric_facts(tree).filter(|fact| matches!(fact.kind(), SegmentNumericKind::Number | SegmentNumericKind::Measurement | SegmentNumericKind::Date)) }
pub fn number_facts(tree: &SegmentTree) -> impl Iterator<Item = SegmentNumberFactRef<'_>> + Clone { numeric_nodes(tree).filter_map(NodeFactRef::new) }
pub fn date_facts(tree: &SegmentTree) -> impl Iterator<Item = SegmentDateFactRef<'_>> + Clone { numeric_nodes(tree).filter_map(NodeFactRef::new) }
pub fn time_facts(tree: &SegmentTree) -> impl Iterator<Item = SegmentTimeFactRef<'_>> + Clone { numeric_nodes(tree).filter_map(NodeFactRef::new) }
pub fn currency_facts(tree: &SegmentTree) -> impl Iterator<Item = SegmentCurrencyFactRef<'_>> + Clone { numeric_nodes(tree).filter_map(NodeFactRef::new) }
pub fn quantity_facts(tree: &SegmentTree) -> impl Iterator<Item = SegmentQuantityFactRef<'_>> + Clone { numeric_nodes(tree).filter_map(SegmentQuantityFactRef::new) }
pub fn measurement_facts(tree: &SegmentTree) -> impl Iterator<Item = SegmentMeasurementFactRef<'_>> + Clone { numeric_nodes(tree).filter_map(NodeFactRef::new) }
pub fn number_range_facts(tree: &SegmentTree) -> impl Iterator<Item = SegmentNumberRangeFactRef<'_>> + Clone { tree.preorder_iter().filter_map(NodeFactRef::new) }
pub fn ordinal_facts(tree: &SegmentTree) -> impl Iterator<Item = SegmentOrdinalFactRef<'_>> + Clone { tree.preorder_iter().filter_map(NodeFactRef::new) }
/// Spans of operators written between two numeric operands, as in `5 × 4 = 20` or `3 ≤ 5`.
pub fn mathematical_operator_spans(tree: &SegmentTree) -> Vec<Range<usize>> {
    let tokens = tree.root().iter_children().filter(|node| !matches!(node.kind(), NodeKind::Whitespace | NodeKind::Tag)).fold(Vec::<(Range<usize>, String, NodeKind)>::new(), |mut tokens, node| {
        match tokens.last_mut() {
            Some((span, text, _)) if span.end == node.span().start && matches!(text.as_str(), "!" | "<" | ">") && node.text() == "=" => { span.end = node.span().end; text.push('='); }
            _ => tokens.push((node.span(), node.text().to_string(), node.kind())),
        }
        tokens
    });
    tokens.windows(3).filter(|window| is_numeric_operand(window[0].2) && MATHEMATICAL_OPERATORS.contains(&window[1].1.as_str()) && is_numeric_operand(window[2].2)).map(|window| window[1].0.clone()).collect()
}

const MATHEMATICAL_OPERATORS: &[&str] = &["+", "-", "‐", "‑", "‒", "–", "—", "―", "−", "*", "×", "x", "X", "х", "Х", "/", "÷", "=", "±", "≈", "≃", "≅", "≠", "<", ">", "≤", "≥", "!=", "<=", ">="];

fn is_numeric_operand(kind: NodeKind) -> bool { matches!(kind, NodeKind::Number | NodeKind::Measurement | NodeKind::Currency | NodeKind::NumberRange) }

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(text: &str) -> SegmentTree {
        let mut tree = crate::tokenized_text_tree(text);
        crate::parse(&mut tree, "en");
        tree
    }

    #[test]
    fn category_iterators_return_guaranteed_typed_facts() {
        let number = parsed("12 items");
        let measurement = parsed("5 kg");
        let date = parsed("12 June 2024");
        let time = parsed("14:30");
        let currency = parsed("EUR 20");

        assert_eq!(number_facts(&number).next().expect("number").number_values(), &[120_000_000_000]);
        assert_eq!(measurement_facts(&measurement).next().expect("measurement").measurement_fact().unit(), "kg");
        assert_eq!(date_facts(&date).next().expect("date").date_fact(), DateFact::new(2024, 6, 12));
        assert_eq!(time_facts(&time).next().expect("time").time_fact(), TimeFact::new(14, 30, None));
        assert_eq!(currency_facts(&currency).next().expect("currency").currency_fact().currency(), Some("EUR"));
    }

    #[test]
    fn heterogeneous_iteration_preserves_the_typed_variant() {
        let measurement = parsed("5 kg");
        let fact = all_numeric_facts(&measurement).next().expect("numeric fact");

        assert!(matches!(fact, SegmentNumericFactRef::Measurement(_)));
        assert_eq!(fact.as_measurement().expect("measurement variant").measurement_fact().unit(), "kg");
    }

    #[test]
    fn quantity_iteration_excludes_non_quantity_numeric_variants() {
        let tree = parsed("12 items cost EUR 20 and weigh 5 kg at 14:30");
        let facts = quantity_facts(&tree).collect::<Vec<_>>();

        assert_eq!(facts.len(), 2);
        assert!(matches!(facts[0], SegmentQuantityFactRef::Currency(_)));
        assert!(matches!(facts[1], SegmentQuantityFactRef::Measurement(_)));
    }
}
