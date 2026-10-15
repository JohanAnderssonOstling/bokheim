use super::*;

#[derive(Clone, Copy, Default)]
pub(super) struct ParseGrounding<'a> {
    known_units: &'a [&'a str],
    known_number_values: &'a [i128],
    known_currency_units: &'a [&'a str],
    known_non_translatables: Option<&'a [&'a str]>,
}

impl<'a> ParseGrounding<'a> {
    pub(super) fn new() -> Self { Self::default() }
    pub(super) fn with_known_units(mut self, known_units: &'a [&'a str]) -> Self { self.known_units = known_units; self }
    pub(super) fn with_known_number_values(mut self, known_number_values: &'a [i128]) -> Self { self.known_number_values = known_number_values; self }
    pub(super) fn with_known_currency_units(mut self, known_currency_units: &'a [&'a str]) -> Self { self.known_currency_units = known_currency_units; self }
    pub(super) fn with_known_non_translatables(mut self, known_non_translatables: &'a [&'a str]) -> Self { self.known_non_translatables = Some(known_non_translatables); self }
}

pub(super) struct ParseContext<'a> {
    stream: &'a SegmentTree,
    locale: ParseLocale,
    known_units: &'a [&'a str],
    known_number_values: &'a [i128],
    known_currency_units: &'a [&'a str],
    known_non_translatables: Option<&'a [&'a str]>,
}

impl<'a> ParseContext<'a> {
    #[cfg(test)]
    pub(super) fn for_language(stream: &'a SegmentTree, language: ParseLanguage, known_units: &'a [&'a str]) -> Self { Self::new(stream, ParseLocale::from_language(language), ParseGrounding::new().with_known_units(known_units)) }
    pub(super) fn new(stream: &'a SegmentTree, locale: ParseLocale, grounding: ParseGrounding<'a>) -> Self { Self { stream, locale, known_units: grounding.known_units, known_number_values: grounding.known_number_values, known_currency_units: grounding.known_currency_units, known_non_translatables: grounding.known_non_translatables } }
    pub(super) fn stream(&self) -> &'a SegmentTree { self.stream }
    pub(super) fn locale(&self) -> ParseLocale { self.locale }
    pub(super) fn language(&self) -> ParseLanguage { self.locale.language() }
    pub(super) fn known_units(&self) -> &'a [&'a str] { self.known_units }
    pub(super) fn known_number_values(&self) -> &'a [i128] { self.known_number_values }
    pub(super) fn known_currency_units(&self) -> &'a [&'a str] { self.known_currency_units }
    pub(super) fn knows_non_translatable(&self, text: &str) -> bool { self.known_non_translatables.is_some_and(|known| known.contains(&text)) }
    pub(super) fn allows_non_translatable(&self, text: &str) -> bool { self.known_non_translatables.is_none_or(|known| known.contains(&text)) }
    pub(super) fn profile(&self) -> &'static crate::language_parser::LanguageProfile { self.locale.language().profile() }
    pub(super) fn number_policy(&self) -> NumberParsingPolicy { self.locale.number_policy() }
    pub(super) fn time_format(&self) -> crate::language_parser::TimeFormatPolicy { self.locale.time_format() }
    pub(super) fn number_format(&self, text: &str, decimal_separator: Option<char>) -> NumberFormat { observed_number_format(text, decimal_separator, self.number_policy()) }
    pub(super) fn punctuation_policy(&self) -> &'static crate::language_parser::PunctuationPolicy { self.locale.punctuation_policy() }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ContextPair {
    close_idx: usize,
    family: DelimiterFamily,
}

#[derive(Clone, Copy)]
struct BracketDefinition {
    open: char,
    close: char,
    family: DelimiterFamily,
}

impl BracketDefinition {
    const fn new(open: char, close: char, family: DelimiterFamily) -> Self { Self { open, close, family } }
}

const BRACKETS: &[BracketDefinition] = &[
    BracketDefinition::new('(', ')', DelimiterFamily::RoundBracket),
    BracketDefinition::new('[', ']', DelimiterFamily::SquareBracket),
    BracketDefinition::new('{', '}', DelimiterFamily::CurlyBracket),
];
const CONTEXT_ONLY_OPENERS: &[char] = &['<', '「'];
const CONTEXT_ONLY_CLOSERS: &[char] = &['>', '」', '?', '!'];

impl ContextPair {
    fn new(close_idx: usize, family: DelimiterFamily) -> Self { Self { close_idx, family } }
    pub(super) fn close_idx(self) -> usize { self.close_idx }
    pub(super) fn family(self) -> DelimiterFamily { self.family }
}

pub(super) fn matching_context_pair(context: &ParseContext<'_>, open_idx: usize, end_idx: usize) -> Option<ContextPair> {
    let stream = context.stream();
    let open = delimiter_character(stream, open_idx)?;
    bracket_definition(open).and_then(|bracket| matching_nested_close(stream, open_idx, end_idx, bracket.open, bracket.close).map(|close_idx| ContextPair::new(close_idx, bracket.family)))
        .or_else(|| delimiter_pairs(context).filter(|pair| pair.open == open).find_map(|pair| matching_quote_close(stream, open_idx, end_idx, pair.close).map(|close_idx| ContextPair::new(close_idx, pair.family))))
}

pub(super) fn unmatched_delimiter_annotation(context: &ParseContext<'_>, index: usize) -> Option<DelimiterAnnotation> {
    let character = delimiter_character(context.stream(), index)?;
    let family = delimiter_family(context, character)?;
    let role = if likely_unpaired_apostrophe(context, index, character) { DelimiterRole::Ignored } else { DelimiterRole::Unmatched };
    Some(delimiter_annotation(context, family, role))
}

pub(super) fn delimiter_annotation(context: &ParseContext<'_>, family: DelimiterFamily, role: DelimiterRole) -> DelimiterAnnotation {
    let preferred = family.is_quote().then_some(context.profile().preferred_quotation_style).flatten();
    DelimiterAnnotation::new(family, role).with_preferred_quotation_style(preferred)
}

fn delimiter_character(stream: &SegmentTree, index: usize) -> Option<char> {
    let unit = structure::parser_api::units(stream).get(index)?;
    let text = structure::parser_api::unit_text(stream, index)?;
    (unit.kind() == UnitKind::Punct && text.chars().count() == 1).then(|| text.chars().next()).flatten()
}

fn bracket_definition(character: char) -> Option<BracketDefinition> { BRACKETS.iter().copied().find(|bracket| bracket.open == character || bracket.close == character) }

fn delimiter_pairs<'a>(context: &'a ParseContext<'_>) -> impl Iterator<Item = &'a crate::language_parser::QuotePairLexeme> { context.punctuation_policy().paired_marks.iter().chain(context.profile().quote_pairs.iter()) }

fn delimiter_family(context: &ParseContext<'_>, character: char) -> Option<DelimiterFamily> {
    bracket_definition(character).map(|bracket| bracket.family)
        .or_else(|| delimiter_pairs(context).find(|pair| pair.open == character || pair.close == character).map(|pair| pair.family))
}

pub(super) fn punctuation_whitespace_facts(context: &ParseContext<'_>, index: usize) -> Vec<WhitespaceTypographyFact> {
    let Some(character) = delimiter_character(context.stream(), index) else { return Vec::new(); };
    context.punctuation_policy().spacing_rules()
        .copied()
        .filter(|rule| rule.applies_to_character(character) && spacing_rule_applies(context.stream(), index..index + 1, rule.side(), rule.preference().preferred()))
        .filter_map(|rule| punctuation_whitespace_fact(context.stream(), index..index + 1, rule.side(), rule.preference()))
        .collect()
}

pub(super) fn ellipsis_whitespace_facts(context: &ParseContext<'_>, units: Range<usize>) -> Vec<WhitespaceTypographyFact> {
    context.punctuation_policy().spacing_rules()
        .copied()
        .filter(|rule| rule.applies_to_ellipsis() && spacing_rule_applies(context.stream(), units.clone(), rule.side(), rule.preference().preferred()))
        .filter_map(|rule| punctuation_whitespace_fact(context.stream(), units.clone(), rule.side(), rule.preference()))
        .collect()
}

pub(super) fn delimiter_interior_whitespace_fact(context: &ParseContext<'_>, index: usize, family: DelimiterFamily, role: DelimiterRole) -> Option<WhitespaceTypographyFact> {
    let (preference, whitespace_role) = match family {
        DelimiterFamily::DoubleQuote | DelimiterFamily::SingleQuote => (context.punctuation_policy().quote_interior_whitespace()?, WhitespaceTypographyRole::QuoteInterior),
        DelimiterFamily::RoundBracket | DelimiterFamily::SquareBracket => (context.punctuation_policy().bracket_interior_whitespace()?, WhitespaceTypographyRole::BracketInterior),
        DelimiterFamily::CurlyBracket | DelimiterFamily::Question | DelimiterFamily::Exclamation => return None,
    };
    let side = match role {
        DelimiterRole::Open => PunctuationSpacingSide::After,
        DelimiterRole::Close => PunctuationSpacingSide::Before,
        DelimiterRole::Unmatched | DelimiterRole::Ignored => return None,
    };
    let subject_span = structure::parser_api::unit_span(context.stream(), index)?;
    let span = crate::stream_util::adjacent_whitespace_span(context.stream(), subject_span.clone(), side);
    let observed = WhitespaceTypographyKind::from_text(context.stream().text().get(span.clone()).unwrap_or(""));
    Some(WhitespaceTypographyFact::from_preference(observed, whitespace_role, preference, span, subject_span))
}

fn spacing_rule_applies(stream: &SegmentTree, units: Range<usize>, side: PunctuationSpacingSide, preferred: WhitespaceTypographyKind) -> bool {
    if preferred == WhitespaceTypographyKind::Absent {
        return true;
    }
    let adjacent = adjacent_unit_index(stream, units, side);
    adjacent.and_then(|index| structure::parser_api::units(stream).get(index)).is_some_and(|unit| unit.kind() == UnitKind::Whitespace || unit.kind().is_text_content())
}

fn adjacent_unit_index(stream: &SegmentTree, units: Range<usize>, side: PunctuationSpacingSide) -> Option<usize> {
    match side {
        PunctuationSpacingSide::Before => units.start.checked_sub(1),
        PunctuationSpacingSide::After => (units.end < structure::parser_api::units(stream).len()).then_some(units.end),
    }
}

fn punctuation_whitespace_fact(stream: &SegmentTree, units: Range<usize>, side: PunctuationSpacingSide, preference: WhitespaceTypographyPreference) -> Option<WhitespaceTypographyFact> {
    let subject_span = structure::parser_api::unit_span(stream, units.start)?.start..structure::parser_api::unit_span(stream, units.end.checked_sub(1)?)?.end;
    let boundary = match side {
        PunctuationSpacingSide::Before => subject_span.start,
        PunctuationSpacingSide::After => subject_span.end,
    };
    let span = adjacent_unit_index(stream, units, side).and_then(|index| structure::parser_api::units(stream).get(index).filter(|unit| unit.kind() == UnitKind::Whitespace).map(|unit| unit.span())).unwrap_or(boundary..boundary);
    let observed = WhitespaceTypographyKind::from_text(stream.text().get(span.clone()).unwrap_or(""));
    let role = match side {
        PunctuationSpacingSide::Before => WhitespaceTypographyRole::BeforePunctuation,
        PunctuationSpacingSide::After => WhitespaceTypographyRole::AfterPunctuation,
    };
    Some(WhitespaceTypographyFact::from_preference(observed, role, preference, span, subject_span))
}

fn matching_nested_close(stream: &SegmentTree, open_idx: usize, end_idx: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 1usize;
    (open_idx + 1..end_idx).find(|index| {
        let Some(character) = delimiter_character(stream, *index) else { return false; };
        if character == open {
            depth += 1;
            return false;
        }
        if character != close {
            return false;
        }
        depth -= 1;
        depth == 0
    })
}

fn matching_quote_close(stream: &SegmentTree, open_idx: usize, end_idx: usize, close: char) -> Option<usize> {
    (open_idx + 1..end_idx).find(|index| delimiter_character(stream, *index) == Some(close))
}

fn likely_unpaired_apostrophe(context: &ParseContext<'_>, index: usize, character: char) -> bool {
    if !crate::char_class::is_apostrophe(character) {
        return false;
    }
    let Some(span) = structure::parser_api::unit_span(context.stream(), index) else { return false; };
    let previous = context.stream().text().get(..span.start).and_then(|text| text.chars().next_back());
    let following = context.stream().text().get(span.end..).and_then(|text| text.chars().next());
    previous.is_some_and(char::is_alphanumeric)
        || following.is_some_and(char::is_numeric)
        || context.profile().join_apostrophe_words && previous.is_none() && following.is_some_and(char::is_uppercase)
}

pub(super) fn is_context_open_char(context: &ParseContext<'_>, character: char) -> bool { BRACKETS.iter().any(|bracket| bracket.open == character) || delimiter_pairs(context).any(|pair| pair.open == character) || CONTEXT_ONLY_OPENERS.contains(&character) }

pub(super) fn is_context_close_char(context: &ParseContext<'_>, character: char) -> bool { BRACKETS.iter().any(|bracket| bracket.close == character) || delimiter_pairs(context).any(|pair| pair.close == character) || CONTEXT_ONLY_CLOSERS.contains(&character) }

pub(super) fn is_context_punct_unit(context: &ParseContext<'_>, text: &str) -> bool { !text.is_empty() && text.chars().all(|character| is_context_punct_char(context, character)) }

pub(super) fn is_context_open_unit(context: &ParseContext<'_>, text: &str) -> bool { is_context_punct_unit(context, text) && text.chars().any(|character| is_context_open_char(context, character)) }

pub(super) fn is_context_close_unit(context: &ParseContext<'_>, text: &str) -> bool { is_context_punct_unit(context, text) && text.chars().any(|character| is_context_close_char(context, character)) }

fn is_context_punct_char(context: &ParseContext<'_>, character: char) -> bool { character.is_ascii_punctuation() || delimiter_pairs(context).any(|pair| pair.open == character || pair.close == character) || CONTEXT_ONLY_OPENERS.contains(&character) || CONTEXT_ONLY_CLOSERS.contains(&character) }

#[cfg(test)]
mod tests;
