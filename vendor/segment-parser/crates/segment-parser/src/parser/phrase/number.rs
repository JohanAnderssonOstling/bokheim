use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::parser) struct ParsedNumber {
    value: i128,
    format: NumberFormat,
    units: UnitRange,
}

impl ParsedNumber {
    pub(in crate::parser) fn parse_range(context: &ParseContext<'_>, span_start: usize, end_idx: usize) -> Option<Self> {
        let text = unit_range_text(context.stream(), span_start, end_idx)?;
        let (value, decimal_separator) = parse_fixed_number(text, context.number_policy())?;
        Some(Self { value, format: context.number_format(text, decimal_separator), units: UnitRange::new(context.stream(), span_start..end_idx) })
    }

    pub(in crate::parser) fn parse_currency_range(context: &ParseContext<'_>, span_start: usize, end_idx: usize) -> Option<Self> {
        Self::parse_range(context, span_start, end_idx).or_else(|| {
            let text = unit_range_text(context.stream(), span_start, end_idx)?;
            let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
            let (value, decimal_separator) = parse_fixed_number(&compact, context.number_policy())?;
            Some(Self { value, format: context.number_format(text, decimal_separator), units: UnitRange::new(context.stream(), span_start..end_idx) })
        })
    }

    pub(in crate::parser) fn value(&self) -> i128 { self.value }
    pub(in crate::parser) fn format(&self) -> NumberFormat { self.format }
    pub(in crate::parser) fn value_as_f64(&self) -> f64 { self.value as f64 / NORMALIZED_VALUE_SCALE }
    pub(in crate::parser) fn units(&self) -> &UnitRange { &self.units }
    pub(in crate::parser) fn into_parts(self) -> (i128, NumberFormat, UnitRange) { (self.value, self.format, self.units) }
}
