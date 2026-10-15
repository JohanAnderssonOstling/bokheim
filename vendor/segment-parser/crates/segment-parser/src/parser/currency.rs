use crate::language_parser::{UNIVERSAL_CURRENCY_CODES, UNIVERSAL_CURRENCY_SYMBOLS};
use super::*;

pub(super) fn parse_currency_phrase_at(context: &ParseContext<'_>, start_idx: usize, _token: &BoundedToken<'_>) -> Option<Detection> {
    let phrase = parse_prefix_currency_phrase_at(context, start_idx).or_else(|| parse_suffix_currency_phrase_at(context, start_idx))?;
    Some(Detection::grounded_semantic(phrase, PhraseConfidence::Normal, PhraseAmbiguity::Contextual))
}

fn parse_prefix_currency_phrase_at(context: &ParseContext<'_>, start_idx: usize) -> Option<Detected> {
    let unit = CurrencyUnitPhrase::parse_at(context, start_idx)?;
    let number_start = skip_layout_stream_units(context.stream(), unit.units().end_index());
    let (number_end, amount) = parse_currency_number(context, number_start, number_start)?;
    Some(resolve_currency_phrase(context, start_idx..number_end, &unit, amount))
}

fn parse_suffix_currency_phrase_at(context: &ParseContext<'_>, start_idx: usize) -> Option<Detected> {
    let stream = context.stream();
    let (span_start, number_start) = numeric_start_indices(stream, start_idx)?;
    let (number_end, amount) = parse_currency_number(context, number_start, span_start)?;
    let unit_idx = skip_layout_stream_units(context.stream(), number_end);
    let unit = CurrencyUnitPhrase::parse_at(context, unit_idx)?;
    Some(resolve_currency_phrase(context, span_start..unit.units().end_index(), &unit, amount))
}

fn parse_currency_number(context: &ParseContext<'_>, number_start: usize, span_start: usize) -> Option<(usize, ParsedNumber)> {
    let stream = context.stream();
    let policy = context.number_policy();
    let raw_end = number_span_end(stream, number_start, policy);
    let mut end_idx = trimmed_number_span_end(stream, number_start, raw_end, policy);
    while is_grouping_whitespace_continuation(stream, end_idx, policy) {
        end_idx += 2;
    }
    ParsedNumber::parse_currency_range(context, span_start, end_idx).map(|amount| (end_idx, amount))
}

fn resolve_currency_phrase(context: &ParseContext<'_>, units: Range<usize>, unit: &CurrencyUnitPhrase, amount: ParsedNumber) -> Detected {
    let stream = context.stream();
    let semantic = unit.canonical().map(|currency| CurrencySemantic::new(currency.to_string(), amount.value(), amount.format())).unwrap_or_else(|| CurrencySemantic::ambiguous(amount.value(), amount.format()));
    Detected::semantic(UnitRange::new(stream, units), NodeSemantic::Currency(semantic))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CurrencyUnitResolution {
    Resolved(&'static str),
    Ambiguous,
}

impl CurrencyUnitResolution {
    pub(super) fn canonical(self) -> Option<&'static str> { match self { Self::Resolved(canonical) => Some(canonical), Self::Ambiguous => None } }
}

pub(super) fn resolve_currency_unit_for_profile(text: &str, profile: &crate::language_parser::LanguageProfile, known_currency_units: &[&str]) -> Option<CurrencyUnitResolution> {
    let trimmed = text.trim_matches(|c: char| c.is_ascii_punctuation() && c != '$').trim();
    if let Some(canonical) = UNIVERSAL_CURRENCY_SYMBOLS.iter().find(|lexeme| lexeme.text == trimmed).map(|lexeme| lexeme.text)
        .or_else(|| UNIVERSAL_CURRENCY_CODES.iter().find(|lexeme| trimmed == lexeme.text).map(|lexeme| lexeme.canonical))
        .or_else(|| profile.currencies.words.iter().find(|lexeme| crate::language_parser::language_text_eq(trimmed, lexeme.text)).map(|lexeme| lexeme.canonical))
    {
        return Some(CurrencyUnitResolution::Resolved(canonical));
    }
    let ambiguous = profile.currencies.ambiguous_words.iter().find(|lexeme| crate::language_parser::language_text_eq(trimmed, lexeme.text))?;
    let mut grounded = ambiguous.candidates.iter().copied().filter(|candidate| known_currency_units.contains(candidate));
    if let (Some(only), None) = (grounded.next(), grounded.next()) {
        return Some(CurrencyUnitResolution::Resolved(only));
    }
    Some(profile.home_currency.filter(|home| ambiguous.candidates.contains(home)).map_or(CurrencyUnitResolution::Ambiguous, CurrencyUnitResolution::Resolved))
}

#[cfg(test)]
mod tests;
