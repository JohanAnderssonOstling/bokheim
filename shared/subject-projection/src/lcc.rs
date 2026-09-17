use super::compact_whitespace;
use std::collections::{BTreeSet, HashMap};
use std::sync::OnceLock;

#[derive(Default)]
struct LccOutline {
    ranges_by_main: HashMap<char, Vec<LccRange>>,
}

struct LccRange {
    start_letters: String,
    start_number: f64,
    end_letters: String,
    end_number: f64,
    path: String,
}

pub(super) struct LccCall<'a> {
    pub(super) letters: String,
    pub(super) number: f64,
    display: &'a str,
    pub(super) remainder: &'a str,
}

fn outline() -> &'static LccOutline {
    static OUTLINE: OnceLock<LccOutline> = OnceLock::new();
    OUTLINE.get_or_init(|| {
        let mut outline = LccOutline::default();
        let mut reader = csv::Reader::from_reader(include_str!("../data/lcc-2024-outline.csv").as_bytes());
        for record in reader.records() {
            let record = record.expect("the embedded LCC outline CSV must be valid");
            let start_letters = record.get(0).unwrap_or_default().to_owned();
            let range = LccRange {
                start_number: record.get(1).unwrap_or_default().parse().expect("numeric LCC range start"),
                end_letters: record.get(2).unwrap_or_default().to_owned(),
                end_number: record.get(3).unwrap_or_default().parse().expect("numeric LCC range end"),
                path: record.get(4).unwrap_or_default().to_owned(),
                start_letters: start_letters.clone(),
            };
            outline.ranges_by_main.entry(start_letters.chars().next().expect("LCC class letter")).or_default().push(range);
        }
        outline
    })
}

pub(super) fn outline_subclasses() -> BTreeSet<String> {
    outline().ranges_by_main.values().flatten().flat_map(|r| [r.start_letters.clone(),r.end_letters.clone()]).collect()
}

// Imported preliminary calls may carry a schedule table reference. The
// explicit preceding class/span is usable without constructing a finer call.
fn registered_table_base(value: &str) -> Option<&str> {
    let tail = value.strip_suffix('+').unwrap_or(value).strip_suffix(')')?;
    let (base, table) = tail.rsplit_once(" (")?;
    if base.contains(['(', ')']) || base.starts_with('K') { return None; }
    static TABLES: OnceLock<BTreeSet<&'static str>> = OnceLock::new();
    let tables = TABLES.get_or_init(|| include_str!("../data/lcc-table-identifiers.txt").lines().collect());
    let mut family = None;
    for reference in table.split([',', ';']).map(str::trim) {
        if reference.as_bytes().first().is_some_and(u8::is_ascii_digit) {
            let expanded = format!("{}{reference}", family?);
            if !tables.contains(expanded.as_str()) { return None; }
        } else {
            if !tables.contains(reference) { return None; }
            let number = reference.find(|c: char| c.is_ascii_digit())?;
            family = Some(&reference[..number]);
        }
    }
    Some(base)
}

pub(super) fn schedule_table_base(value: &str) -> Option<&str> {
    let base = registered_table_base(value)?;
    canonical_lcc_notation(base)?;
    Some(base)
}

// Historical DS7/VII biography displays are not aliases of DS-DX7,
// which is a different current table. Accept only an explicitly stated
// G320 biography operation within a published first-Cutter biography scope.
fn historical_biography_operation(endpoint: &LccEndpoint, table: &str, lower: &str, upper: &str) -> bool {
    if !matches!(table, "DS7" | "DSVII" | "DS-DXVII" | "VII" | "TABLE VII")
        || !matches!((lower, upper), ("A2", "A2") | ("A25", "A25") | ("A3", "A3") | ("A4", "A4") | ("A5", "A5") | ("A6", "Z"))
        || endpoint.letters != "DS" { return false; }
    let cutters = super::runtime::cutter_components(&endpoint.remainder);
    if cutters.len() != 1 || cutters[0].0.len() != 1 { return false; }
    static SCOPES: OnceLock<Vec<(LccEndpoint, LccEndpoint)>> = OnceLock::new();
    SCOPES.get_or_init(|| include_str!("../data/lcc-ds-biography-scopes.tsv").lines().map(|line| {
        let (a, b) = line.split_once('\t').expect("biography scope columns");
        (lcc_endpoint(a, false).expect("biography start"), lcc_endpoint(b, true).expect("biography end"))
    }).collect()).iter().any(|(a, b)| {
        endpoint.number == a.number && endpoint.number == b.number
            && !super::runtime::compare_cutter_endpoint(&cutters, &super::runtime::cutter_components(&a.remainder)).is_lt()
            && !super::runtime::compare_cutter_endpoint(&cutters, &super::runtime::cutter_components(&b.remainder)).is_gt()
    })
}

// A qualified table note can state the entire relative Cutter interval.
// Apply only documented intervals to an already complete Cutter prefix;
// .x represents that prefix, not an additional invented Cutter.
fn qualified_table_parts(value: &str) -> Option<(LccEndpoint, &str, &str)> {
    let tail = value.strip_suffix('+').unwrap_or(value).strip_suffix(')')?;
    let (base, note) = tail.rsplit_once(" (")?;
    // Imports may place the same relative interval before the table name:
    // AUTHOR.xZ5-Z999 (P-PZ40). Both forms use the exact documented registry.
    let (base, table, relative) = if let Some((table, relative)) = note.split_once(':') {
        (base, table, relative)
    } else {
        let at = base.find(".X")?;
        (base[..at].trim_end(), note, &base[at..])
    };
    if base.starts_with('K') || base.contains(['(', ')', '-', '[', ']']) { return None; }
    let relative = relative.trim();
    let (lower, upper) = relative.split_once('-').unwrap_or((relative, relative));
    let lower = lower.trim().strip_prefix(".X")?;
    let upper = upper.trim();
    let upper = upper.strip_prefix(".X").unwrap_or_else(|| upper.strip_prefix('.').unwrap_or(upper));
    static SPANS: OnceLock<BTreeSet<&'static str>> = OnceLock::new();
    let spans = SPANS.get_or_init(|| include_str!("../data/lcc-relative-table-spans.tsv").lines()
        .chain(include_str!("../data/lcc-numeric-relative-spans.tsv").lines()).collect());
    let endpoint = lcc_endpoint(base, false)?;
    if !spans.contains(format!("{}\t.X{lower}\t.X{upper}", table.trim()).as_str())
        && !historical_biography_operation(&endpoint, table.trim(), lower, upper) { return None; }
    Some((endpoint, lower, upper))
}

pub(super) fn qualified_table_span(value: &str) -> Option<String> {
    let (endpoint, lower, upper) = qualified_table_parts(value)?;
    let cutters = super::runtime::cutter_components(&endpoint.remainder);
    if cutters.is_empty() || cutters.iter().any(|(letters, digits)| letters.len() != 1 || digits.is_empty()) { return None; }
    // A numeric relative prefix continues the last complete Cutter's
    // digits; an alphabetic relative prefix starts another Cutter. Keep
    // the original digits, including trailing zeroes, during construction.
    let separator = |suffix: &str| if suffix.as_bytes().first().is_some_and(u8::is_ascii_digit) { "" } else { "." };
    canonical_printed_range(&format!("{}{}{lower}-{}{}{upper}", endpoint.code, separator(lower), endpoint.code, separator(upper)))
}

// A documented table operation cannot complete a missing author Cutter.
// Retain only the stated filing family and require coverage of that whole
// family; appending the table suffix to a lone initial would invent a call.
pub(super) fn qualified_table_family(value: &str) -> Option<String> {
    let (endpoint, _, _) = qualified_table_parts(value)?;
    let mut cutters = super::runtime::cutter_components(&endpoint.remainder);
    let (initial, digits) = cutters.pop()?;
    if initial.len() != 1 || !digits.is_empty()
        || cutters.iter().any(|(letters, digits)| letters.len() != 1 || digits.is_empty()) { return None; }
    Some(endpoint.code)
}

fn reference_scopes() -> &'static HashMap<&'static str, Vec<&'static str>> {
    static SCOPES: OnceLock<HashMap<&'static str, Vec<&'static str>>> = OnceLock::new();
    SCOPES.get_or_init(|| {
        let mut map: HashMap<&str, Vec<&str>> = HashMap::new();
        for line in include_str!("../data/lcc-reference-scopes.tsv").lines() {
            let (start, end) = line.split_once('\t').expect("reference scope TSV");
            map.entry(start).or_default().push(end);
        }
        map
    })
}

// Plus-marked preliminary numbers retain every documented scope beginning
// at that number. A single-number entry must not hide a broader range.
pub(super) fn reference_plus_span(value: &str) -> Option<(LccEndpoint, LccEndpoint)> {
    let value = if let Some(rest) = value.strip_prefix("IN PROCESS ") {
        rest.strip_prefix('[')?.strip_suffix(']')?
    } else { value };
    let base = value.strip_suffix('+')?.trim_end();
    if base.starts_with('K') || base.contains(['+', '-', '(', ')', '[', ']']) { return None; }
    let start = lcc_endpoint(base, false)?;
    let scopes = reference_scopes();
    let mut upper: Option<LccEndpoint> = None;
    for code in scopes.get(start.code.as_str())? {
        let end = lcc_endpoint(code, true)?;
        let order = compare_lcc_key(&start.letters, start.number, &end.letters, end.number);
        if order.is_gt() || (order.is_eq() && super::runtime::compare_cutter_endpoint(
            &super::runtime::cutter_components(&start.remainder),
            &super::runtime::cutter_components(&end.remainder)).is_gt()) { return None; }
        if upper.as_ref().is_none_or(|upper| compare_lcc_key(&upper.letters, upper.number, &end.letters, end.number)
            .then_with(|| super::runtime::compare_upper_cutters(
                &super::runtime::cutter_components(&upper.remainder),
                &super::runtime::cutter_components(&end.remainder))).is_lt()) { upper = Some(end); }
    }
    Some((start, upper?))
}

// In table-annotated Cutter ranges, a shortened upper Cutter retains the
// lower endpoint's class and preceding Cutters: B33Z5-Z999 means B33.Z5
// through B33.Z999. Repeated preceding Cutters and final alphabetic bounds
// retain that same scope. Never treat Z999 as a separate bibliography class.
pub(super) fn table_cutter_span(value: &str) -> Option<String> {
    let base = registered_table_base(value)?;
    let base = if let Some((stem, year)) = base.rsplit_once(' ') {
        if year.len() == 4 && year.bytes().all(|b| b.is_ascii_digit()) { stem.trim_end() } else { base }
    } else { base };
    let (lower, upper) = base.split_once('-')?;
    let endpoint = lcc_endpoint(lower, false)?;
    let mut cutters = super::runtime::cutter_components(&endpoint.remainder);
    let (last_letter, _) = cutters.pop()?;
    if last_letter.len() != 1 || cutters.iter().any(|(letters, digits)| letters.len() != 1 || digits.is_empty()) { return None; }
    let call = parse_lcc_call(lower)?;
    let class = &lower[..lower.len() - call.remainder.len()];
    let upper = upper.strip_prefix('.').unwrap_or(upper);
    // Simple A6-Z already has a stable path through ordinary range parsing.
    if cutters.is_empty() && upper.len() == 1 { return None; }
    let upper_endpoint = lcc_endpoint(&format!("{class}.{upper}"), true)?;
    let mut upper_cutters = super::runtime::cutter_components(&upper_endpoint.remainder);
    let (upper_letter, upper_digits) = upper_cutters.pop()?;
    if upper_letter.len() != 1 || (upper_letter != "Z" && upper_letter != last_letter) { return None; }
    // An upper endpoint may repeat all preceding Cutters (S5A6-S5Z),
    // or omit them (R36A6-Z). Partial or different prefixes are ambiguous.
    if !upper_cutters.is_empty() && upper_cutters != cutters { return None; }
    let prefix = cutters.iter().map(|(letters, digits)| format!(".{letters}{digits}")).collect::<String>();
    canonical_printed_range(&format!("{}-{class}{prefix}.{upper_letter}{upper_digits}", endpoint.code))
}

// Numeric relative suffixes are validated here without appending their
// digits to a class number. The caller retains the whole outer Cutter span.
fn registered_nested_operation(table: &str, relative: &str) -> bool {
    let (lower, upper) = relative.split_once('-').unwrap_or((relative, relative));
    let upper = upper.strip_prefix(".X").unwrap_or_else(|| upper.strip_prefix('.').unwrap_or(upper));
    static SPANS: OnceLock<BTreeSet<&'static str>> = OnceLock::new();
    let spans = SPANS.get_or_init(|| include_str!("../data/lcc-relative-table-spans.tsv").lines()
        .chain(include_str!("../data/lcc-numeric-relative-spans.tsv").lines()).collect());
    if spans.contains(format!("{table}\t{lower}\t.X{upper}").as_str()) { return true; }
    // A preliminary final initial still leaves the full documented inner
    // A-Z range unresolved. It must not replace that range with a point.
    let Some(at) = upper.rfind(|c: char| c.is_ascii_uppercase()) else { return false; };
    let (bound, initial) = upper.split_at(at);
    if !bound.ends_with('Z') || !initial.as_bytes()[1..].iter().all(u8::is_ascii_digit) { return false; }
    let Some(lower) = lower.strip_prefix(".X") else { return false; };
    let lower_initial = lower.bytes().find(u8::is_ascii_uppercase);
    if lower_initial.is_none_or(|letter| initial.as_bytes()[0] < letter) { return false; }
    spans.contains(format!("{table}\t.X{lower}\t.X{bound}").as_str())
}

// An explicit single-Cutter interval ending at Z already supplies its
// complete upper family. Honor that exact documented interval even when
// another, broader heading begins at the same lower endpoint.
fn documented_closed_plus_span(value: &str) -> Option<String> {
    let base = value.strip_suffix('+')?;
    let (start, end) = lcc_span(base)?;
    if start.letters.starts_with('K') || start.letters != end.letters
        || start.number != end.number || end.remainder != ".Z"
        || super::runtime::cutter_components(&start.remainder).len() != 1 { return None; }
    if !reference_scopes().get(start.code.as_str())?.contains(&end.code.as_str()) { return None; }
    canonical_printed_range(base)
}

// A preliminary initial retains the exact published outer
// interval. Require the widest reference scope at its start, not a point
// or an arbitrarily completed country/person Cutter.
fn documented_preliminary_plus_span(value: &str) -> Option<String> {
    let base = value.strip_suffix('+').unwrap_or(value);
    if base.starts_with('K') || base.contains(['+', '(', ')', '[', ']']) { return None; }
    let (lower, upper) = base.split_once('-')?;
    let initial = upper.strip_prefix('.').unwrap_or(upper).strip_prefix('Z')?;
    if initial.is_empty() && !value.ends_with('+') { return None; }
    if !initial.is_empty() && (!initial.as_bytes()[0].is_ascii_uppercase()
        || !initial.as_bytes()[1..].iter().all(u8::is_ascii_digit)) { return None; }
    let start = lcc_endpoint(lower, false)?;
    let cutters = super::runtime::cutter_components(&start.remainder);
    if cutters.len() != 1 || cutters[0].0.len() != 1 || (!initial.is_empty() && initial.as_bytes()[0] < cutters[0].0.as_bytes()[0]) { return None; }
    let (_, end) = reference_plus_span(&format!("{}+", start.code))?;
    if end.code != format!("{}{}.Z", start.letters, start.number) { return None; }
    canonical_printed_range(&format!("{}-{}", start.code, end.code))
}

// An unqualified .x position is usable only where the published outer
// interval explicitly defines that position, inline or by a named table.
fn documented_placeholder_span(value: &str) -> Option<String> {
    if value.starts_with('K') { return None; }
    let (stem, position) = value.strip_suffix('+').unwrap_or(value).rsplit_once(".X")?;
    if !position.bytes().all(|b| b.is_ascii_digit()) { return None; }
    let (lower, upper) = stem.trim_end().split_once('-')?;
    let initial = upper.strip_prefix('.').unwrap_or(upper).strip_prefix('Z')?;
    if !initial.is_empty() && (!initial.as_bytes()[0].is_ascii_uppercase()
        || !initial.as_bytes()[1..].iter().all(u8::is_ascii_digit)) { return None; }
    let start = lcc_endpoint(lower, false)?;
    let cutters = super::runtime::cutter_components(&start.remainder);
    if cutters.len() != 1 || cutters[0].0.len() != 1 || (!initial.is_empty() && initial.as_bytes()[0] < cutters[0].0.as_bytes()[0]) { return None; }
    let end = format!("{}{}.Z", start.letters, start.number);
    static POSITIONS: OnceLock<BTreeSet<String>> = OnceLock::new();
    let positions = POSITIONS.get_or_init(|| include_str!("../data/lcc-placeholder-positions.tsv").lines().map(|line| {
        let parts = line.split('\t').collect::<Vec<_>>();
        let a = lcc_endpoint(parts[0], false).expect("placeholder scope start");
        let b = lcc_endpoint(parts[1], true).expect("placeholder scope end");
        format!("{}\t{}\t{}", a.code, b.code, parts[2])
    }).collect());
    if !positions.contains(&format!("{}\t{end}\t.X{position}", start.code)) { return None; }
    canonical_printed_range(&format!("{}-{end}", start.code))
}

// These unmarked preliminary forms occur verbatim in linked MARC 050 fields.
// Retain the complete explicit range; the trailing initial cannot supply a
// finished language/author Cutter. Only source-reviewed spellings are accepted.
fn reviewed_preliminary_span(value: &str) -> Option<String> {
    static SPANS: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    let spans = SPANS.get_or_init(|| include_str!("../data/lcc-reviewed-preliminary-spans.tsv")
        .lines().map(|line| line.split_once('\t').expect("reviewed preliminary span pair")).collect());
    canonical_printed_range(spans.get(value)?)
}

// A preliminary construction states a Cutter span followed by an
// initial (optionally numbered) and sometimes a variable .x position. Keep
// the full stated span; do not invent a completed geographic/author Cutter.
pub(super) fn variable_cutter_span(value: &str) -> Option<String> {
    if let Some(span) = reviewed_preliminary_span(value) { return Some(span); }
    let Some(base) = registered_table_base(value) else {
        return documented_placeholder_span(value).or_else(|| documented_closed_plus_span(value))
            .or_else(|| documented_preliminary_plus_span(value));
    };
    // Table arrangements may end with a publication year. The year does
    // not narrow the stated subject span; validate it before detaching it.
    let base = if let Some((stem, year)) = base.rsplit_once(' ') {
        if year.len() == 4 && year.bytes().all(|b| b.is_ascii_digit()) {
            stem.trim_end()
        } else { base }
    } else { base };
    let mut nested_operation = false;
    let stem = if let Some((stem, position)) = base.rsplit_once(".X") {
        if position.bytes().all(|b| b.is_ascii_digit()) { stem } else {
            // A documented relative operation on an unresolved outer Cutter
            // cannot complete it. Keep the whole outer interval, after
            // validating every character of the nested operation.
            let table = value.strip_suffix('+').unwrap_or(value).strip_suffix(')')?.rsplit_once(" (")?.1;
            nested_operation = true;
            base.match_indices(".X").find_map(|(at, _)| {
                registered_nested_operation(table, &base[at..]).then_some(base[..at].trim_end())
            })?
        }
    } else { base };
    if nested_operation {
        if let Some((a, b)) = lcc_span(stem) {
            if a.letters == b.letters && a.number == b.number && !a.remainder.is_empty() && !b.remainder.is_empty() {
                return canonical_printed_range(stem);
            }
        }
    }
    let (lower, upper) = stem.split_once('-')?;
    let upper = upper.strip_prefix('.').unwrap_or(upper);
    let initial_at = upper.rfind(|c: char| c.is_ascii_uppercase())?;
    let initial_cutter = &upper[initial_at..];
    let bound = &upper[..initial_at];
    let z_at = bound.rfind('Z')?;
    let upper_digits = &bound[z_at + 1..];
    if !upper_digits.bytes().all(|b| b.is_ascii_digit()) { return None; }
    let repeated_prefix = bound[..z_at].trim_end_matches('.');
    let initial = *initial_cutter.as_bytes().first()?;
    if !initial_cutter.as_bytes()[1..].iter().all(u8::is_ascii_digit) { return None; }
    let endpoint = lcc_endpoint(lower, false)?;
    let mut cutters = super::runtime::cutter_components(&endpoint.remainder);
    let (letter, lower_digits) = cutters.pop()?;
    if letter.len() != 1 || initial < letter.as_bytes()[0]
        || cutters.iter().any(|(letters, digits)| letters.len() != 1 || digits.is_empty()) { return None; }
    // A numbered upper bound is usable only when the explicitly named
    // table defines this exact relative interval. Keep Z458 as Z458;
    // a following preliminary initial cannot turn it into the whole Z family.
    if !upper_digits.is_empty() {
        let table = value.strip_suffix('+').unwrap_or(value).strip_suffix(')')?.rsplit_once(" (")?.1;
        if !registered_nested_operation(table, &format!(".X{letter}{lower_digits}-.XZ{upper_digits}")) { return None; }
    }
    // Inherit every completed preceding Cutter when expanding the upper
    // endpoint. B432A-ZE states B432.A through B432.Z, not class Z.
    let call = parse_lcc_call(lower)?;
    let class = &lower[..lower.len() - call.remainder.len()];
    let prefix = cutters.iter().map(|(letters, digits)| format!(".{letters}{digits}")).collect::<String>();
    if !repeated_prefix.is_empty() {
        let repeated = lcc_endpoint(&format!("{class}.{repeated_prefix}"), false)?;
        if super::runtime::cutter_components(&repeated.remainder) != cutters { return None; }
    }
    canonical_printed_range(&format!("{}-{class}{prefix}.Z{upper_digits}", endpoint.code))
}

// Imported fields concatenate complete calls and documented outline spans.
// Consume the entire input and require explicit numeric/Cutter boundaries;
// ordinary adjacent Cutters must not turn a single book into several subjects.
fn joined_lcc_codes(value: &str) -> Option<Vec<String>> {
    if value.len() > 512 || !value.is_ascii() { return None; }
    // A second multi-letter class prefix distinguishes joined calls from
    // ordinary single-letter item Cutters. Avoid work on ordinary shelfmarks.
    if !value.contains('-') {
        let bytes = value.as_bytes();
        let multi_letter = bytes.windows(3).skip(1)
            .any(|w| w[0].is_ascii_alphabetic() && w[1].is_ascii_alphabetic() && w[2].is_ascii_digit());
        // A single-letter class with its own numeric decimal is also
        // unambiguous here (RC466.8R726.7). A dotted item Cutter is not.
        let single_letter_decimal = bytes.iter().enumerate().skip(1).any(|(at, b)| {
            if !b.is_ascii_alphabetic() || !bytes[at - 1].is_ascii_digit() { return false; }
            let digits = bytes[at + 1..].iter().take_while(|b| b.is_ascii_digit()).count();
            let end = at + 1 + digits;
            digits > 0 && bytes.get(end) == Some(&b'.')
                && bytes.get(end + 1).is_some_and(u8::is_ascii_digit)
        });
        if !multi_letter && (!single_letter_decimal || period_year_call_base(value).is_some()) { return None; }
    }
    let value = value.trim().to_ascii_uppercase();
    if value.bytes().any(|b| !(b.is_ascii_alphanumeric() || b == b'.' || b == b'-')) { return None; }
    static SPANS: OnceLock<BTreeSet<String>> = OnceLock::new();
    static CLASSES: OnceLock<BTreeSet<String>> = OnceLock::new();
    let spans = SPANS.get_or_init(|| {
        // Detailed LCC reference: HF4999.2-HF6182 is Business; the
        // abbreviated outline starts at HF5001 and omits this lower bound.
        let mut spans = BTreeSet::from(["R-RZ".to_owned(), "D1-DX301".to_owned(),
            "HF4999.2-HF6182".to_owned(), "HF4999.2-6182".to_owned()]);
        for range in outline().ranges_by_main.values().flatten() {
            if range.start_letters == range.end_letters && range.start_number == range.end_number { continue; }
            spans.insert(format!("{}{}-{}{}", range.start_letters, range.start_number, range.end_letters, range.end_number));
            if range.start_letters == range.end_letters {
                spans.insert(format!("{}{}-{}", range.start_letters, range.start_number, range.end_number));
            }
        }
        // Complete intervals verified in the detailed LCC reference. These
        // retain historical bounds omitted by the condensed outline.
        for line in include_str!("../data/lcc-joined-reference-spans.tsv").lines() {
            let (start, end) = line.split_once('\t').expect("joined reference span columns");
            let full = format!("{start}-{end}");
            spans.insert(canonical_printed_range(&full).expect("valid documented joined span"));
            spans.insert(full);
        }
        spans
    });
    let classes = CLASSES.get_or_init(outline_subclasses);
    let mut rest = value.as_str();
    let mut parts = Vec::new();
    let mut anchored = false;
    let mut range_keys = BTreeSet::new();
    let mut repeated_range = false;
    while !rest.is_empty() {
        if parts.len() == 32 { return None; }
        // Probe input boundaries instead of scanning every outline span.
        let mut anchors = (1..=rest.len()).filter(|&end|
            rest.as_bytes().get(end).is_none_or(u8::is_ascii_alphabetic)
                && (spans.contains(&rest[..end]) || canonical_printed_range(&rest[..end])
                    .is_some_and(|key| spans.contains(&key))));
        if let Some(end) = anchors.next() {
            let span = &rest[..end];
            if anchors.next().is_some() { return None; }
            let key = canonical_printed_range(span)?;
            repeated_range |= !range_keys.insert(key);
            parts.push(span.to_owned());
            rest = &rest[span.len()..];
            anchored = true;
            continue;
        }
        let letters = rest.bytes().take_while(u8::is_ascii_alphabetic).count();
        if !classes.contains(&rest[..letters]) { return None; }
        // An undotted single-letter suffix can be an item Cutter. Do not
        // reinterpret it as a second classification without a separator.
        let mut end = letters;
        while rest.as_bytes().get(end).is_some_and(u8::is_ascii_digit) { end += 1; }
        if end == letters { return None; }
        let decimal_class = rest.as_bytes().get(end) == Some(&b'.') && rest.as_bytes().get(end + 1).is_some_and(u8::is_ascii_digit);
        if decimal_class {
            end += 1;
            while rest.as_bytes().get(end).is_some_and(u8::is_ascii_digit) { end += 1; }
        }
        if letters == 1 && !parts.is_empty() && !decimal_class { return None; }
        // Only explicitly dotted, single-letter numeric Cutters are safe here.
        while rest.as_bytes().get(end) == Some(&b'.') && rest.as_bytes().get(end + 1).is_some_and(u8::is_ascii_alphabetic) {
            end += 2;
            let digits = end;
            while rest.as_bytes().get(end).is_some_and(u8::is_ascii_digit) { end += 1; }
            if end == digits { return None; }
        }
        parts.push(rest[..end].to_owned());
        rest = &rest[end..];
    }
    // Exact repetition already has its own interpretation. A repeated range
    // plus an extra fragment may be a cut-off copy, not a new classification.
    if parts.len() < 2 { return None; }
    // Original editions retain complete separate components proving these
    // fields end inside a classification. See the source-record evidence in
    // data/reports/20260907-confirmed-clipped-joined-components.json.
    // Length alone cannot establish truncation for other imported fields.
    if matches!(value.as_str(), "T385TA1637-1638TK788" | "QB1-991QB460-466QB98"
        | "QA315-316QA402.3QA40" | "TJ1075-1081TA467TA41" | "QC173.45-173.458TA40"
        | "SB123-123.5S494.5.B5" | "Q334-342TA329-348TA6" | "QC138-168.86QA930QC1"
        | "QA639.5-640.7QA640.7" | "QC173.45-173.458TA41" | "QA76.9.C66QA76.76.A6") { return None; }
    let classes: BTreeSet<_> = parts.iter().map(|part| {
        let n = part.bytes().take_while(u8::is_ascii_alphabetic).count();
        &part[..n]
    }).collect();
    if repeated_range && classes.len() == 1 { return None; }
    if !anchored {
        // Require a decimal class or a dotted numeric Cutter in every suffix
        // call, rather than treating possible undotted item marks as classes.
        if parts.iter().skip(1).any(|part| !part.contains('.')) { return None; }
        if parts.iter().all(|part| part == &parts[0]) { return None; }
    }
    Some(parts)
}

// Explicit semicolons separate complete classifications. Every component
// must validate; punctuation inside table notes must not become a code list.
pub(super) fn lcc_code_list(value: &str) -> Option<Vec<String>> {
    if !value.contains(';') { return joined_lcc_codes(value); }
    let value = value.trim();
    let value = value.strip_prefix('[').and_then(|v| v.strip_suffix(']'))
        .filter(|v| !v.contains(['[', ']'])).unwrap_or(value);
    static SUBCLASSES: OnceLock<BTreeSet<String>> = OnceLock::new();
    let mut codes = Vec::new();
    for part in value.split(';') {
        let canonical = canonical_lcc_notation(part.trim())?;
        let unwrapped = display_bracket_base(&canonical).unwrap_or(&canonical);
        let marc = marc_subfield_call_base(unwrapped);
        let call = parse_lcc_call(marc.as_deref().unwrap_or(unwrapped))?;
        if call.letters.starts_with('K') || !SUBCLASSES.get_or_init(outline_subclasses).contains(&call.letters) { return None; }
        // An unmarked single-letter integer such as B43 can be an item
        // Cutter, not a second class (BF175; B43). Require evidence that
        // distinguishes it: a class decimal, $a marker, or alternate brackets.
        let base = marc.as_deref().unwrap_or(unwrapped);
        let number_prefix = &base[..base.len() - call.remainder.len()];
        if !codes.is_empty() && call.letters.len() == 1 && !number_prefix.contains('.')
            && !["$A", "ǂA", "‡A"].iter().any(|marker| canonical.starts_with(marker))
            && !canonical.starts_with('[') { return None; }
        codes.push(canonical);
    }
    (codes.len() > 1).then_some(codes)
}

/// Outline paths for each explicitly stated classification, with duplicates removed.
pub fn lcc_subject_paths(value: &str) -> Vec<String> {
    if let Some(codes) = lcc_code_list(value) {
        let paths: Option<BTreeSet<String>> = codes.iter().map(|code| lcc_subject_path(code)).collect();
        return paths.unwrap_or_default().into_iter().collect();
    }
    lcc_subject_path(value).into_iter().collect()
}

// MARC 050 $a supplies the class and $b the item portion. Imported display
// markers must not hide either portion or concatenate alternative $a fields.
pub(super) fn marc_subfield_call_base(value: &str) -> Option<String> {
    if !value.contains(['$', 'ǂ', '‡']) { return None; }
    // OCLC documents these as equivalent displays of the MARC delimiter.
    // Keep the original notation outside this temporary match base.
    let normalized = value.replace(['ǂ', '‡'], "$");
    let value = normalized.as_str();
    let (value, explicit_class) = if let Some(base) = value.strip_prefix("$A") {
        (base.trim_start(), true)
    } else { (value, false) };
    let (class, item) = if let Some((class, item)) = value.split_once("$B") {
        (class.trim_end(), Some(item.trim()))
    } else if explicit_class { (value, None) } else { return None; };
    if class.contains(['$', '-']) { return None; }
    let call = parse_lcc_call(class)?;
    if !valid_lcc_remainder(call.remainder) && !valid_literature_work_mark(&call) { return None; }
    let base = if let Some(item) = item {
        if item.is_empty() || item.contains('$') { return None; }
        format!("{class} {item}")
    } else { class.to_owned() };
    canonical_lcc_notation(&base)?;
    Some(base)
}

// Repeating a numeric interval with and without its upper subclass letters
// adds no information either. Every copy must be complete and equivalent.
fn repeated_numeric_range_base(value: &str) -> Option<&str> {
    static SUBCLASSES: OnceLock<BTreeSet<String>> = OnceLock::new();
    let call = parse_lcc_call(value)?;
    if call.letters.starts_with('K') { return None; }
    for (length, _) in value.match_indices(&call.letters).skip(1) {
        let base = &value[..length];
        let Some((start, end)) = lcc_span(base) else { continue; };
        if start.letters != end.letters || !start.remainder.is_empty() || !end.remainder.is_empty()
            || !SUBCLASSES.get_or_init(outline_subclasses).contains(&start.letters) { continue; }
        let full = format!("{}-{}", start.code, end.code);
        let short = format!("{}-{}", start.code, end.code.strip_prefix(&end.letters)?);
        let mut rest = value;
        let mut copies = 0;
        while let Some(tail) = rest.strip_prefix(&full).or_else(|| rest.strip_prefix(&short)) {
            rest = tail;copies += 1;
        }
        if rest.is_empty() && copies > 1 { return Some(base); }
    }
    None
}

// Exact repetition adds no classification information. Only collapse whole
// copies of an independently valid class/Cutter or span with known subclasses;
// concatenated different codes and truncated copies are not repetitions.
pub(super) fn repeated_lcc_base(value: &str) -> Option<&str> {
    if let Some(base) = repeated_numeric_range_base(value) { return Some(base); }
    let call = parse_lcc_call(value)?;
    if call.letters.starts_with('K') { return None; }
    static SUBCLASSES: OnceLock<BTreeSet<String>> = OnceLock::new();
    for (length, _) in value.match_indices(&call.letters).skip(1) {
        if length > value.len() / 2 { break; }
        if value.len() % length != 0 || !value.as_bytes().chunks(length).all(|part| part == &value.as_bytes()[..length]) { continue; }
        let base = &value[..length];
        let known = SUBCLASSES.get_or_init(outline_subclasses);
        let valid = |endpoint: &LccEndpoint, allow_initial: bool| {
            known.contains(&endpoint.letters)
                && super::runtime::cutter_components(&endpoint.remainder).iter()
                    .all(|(letters, digits)| letters.len() == 1 && (allow_initial || !digits.is_empty()))
        };
        if base.contains('-') {
            if lcc_span(base).is_some_and(|(start, end)| valid(&start, true) && valid(&end, true)) { return Some(base); }
        } else if lcc_endpoint(base, false).is_some_and(|endpoint| valid(&endpoint, false)) { return Some(base); }
    }
    None
}

// Published British CIP can end a class/Cutter prefix with a slash. Preserve
// the notation and interpret the prefix as a filing family, never as a
// completed shelf number or permission to discard an arbitrary remainder.
pub(super) fn terminal_slash_base(value: &str) -> Option<&str> {
    let base = value.strip_suffix('/')?.trim_end();
    if base.starts_with('K') || base.contains(['/', '-', '(', ')', '[', ']']) { return None; }
    let call = parse_lcc_call(base)?;
    let mut rest = call.remainder;
    while !rest.is_empty() {
        rest = rest.trim_start();
        if let Some(after) = rest.strip_prefix('.') { rest = after.trim_start(); }
        let initial = *rest.as_bytes().first()?;
        if !initial.is_ascii_uppercase() { return None; }
        rest = &rest[1..];
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 && !rest.is_empty() { return None; }
        rest = &rest[digits..];
    }
    Some(base)
}

// The LC H outline names the whole Sociology subclass HM(1)-1281; the
// parentheses flag obsolete numbers. This is a documented heading alias,
// not a general permission to erase parentheses or fill retired intervals.
pub(super) fn documented_outline_heading(value: &str) -> Option<&'static str> {
    match value { "HM(1)-1281" => Some("HM"), _ => None }
}

// Unicode hyphens and their numeric character references still delimit a
// whole range. The ampersand-less #8209; form occurs in imported data.
// Decode only these separators, then validate both complete endpoints.
fn encoded_hyphen_range(value: &str) -> Option<String> {
    if !value.contains(['‐', '‑', '#']) { return None; }
    let value = compact_whitespace(value).to_ascii_uppercase();
    let decoded = value.replace(['‐', '‑'], "-")
        .replace("&#8208;", "-").replace("&#X2010;", "-")
        .replace("&#8209;", "-").replace("&#X2011;", "-")
        .replace("#8209;", "-");
    if decoded == value { return None; }
    let (start, end) = lcc_span(&decoded)?;
    if start.letters.starts_with('K') || end.letters.starts_with('K') { return None; }
    canonical_printed_range(&decoded)
}

// The pinned BX p.277 arranges BX870 by papal accession date; DC p.181
// arranges DC240 by event date within 1815. Dates are classification detail,
// not disposable publication years. Book suffixes use the ordinary parser.
pub(super) fn documented_date_call(value: &str) -> Option<(&'static str, String, String)> {
    fn month(value: &str) -> Option<(u32, &'static str)> {
        Some(match value.trim_end_matches('.') {
            "JAN" | "JANUARY" => (1, "JAN."), "FEB" | "FEBRUARY" => (2, "FEB."),
            "MAR" | "MARCH" => (3, "MARCH"), "APR" | "APRIL" => (4, "APRIL"),
            "MAY" => (5, "MAY"), "JUN" | "JUNE" => (6, "JUNE"),
            "JUL" | "JULY" => (7, "JULY"), "AUG" | "AUGUST" => (8, "AUG."),
            "SEP" | "SEPT" | "SEPTEMBER" => (9, "SEPT."), "OCT" | "OCTOBER" => (10, "OCT."),
            "NOV" | "NOVEMBER" => (11, "NOV."), "DEC" | "DECEMBER" => (12, "DEC."), _ => return None,
        })
    }
    let (class, rest) = if let Some(rest) = value.strip_prefix("BX870 ") { ("BX870", rest) }
        else { ("DC240", value.strip_prefix("DC240 ")?) };
    let mut parts = rest.split_whitespace().peekable();
    let year = if class == "BX870" {
        let y = parts.next()?;
        if y.len() != 4 || !y.bytes().all(|b| b.is_ascii_digit()) { return None; }
        let y = y.parse::<u32>().ok()?; if y == 0 { return None; } y
    } else { 1815 };
    let raw_month = parts.next()?;
    if raw_month.ends_with("..") { return None; }
    let (m, label) = month(raw_month)?;
    let mut heading = if class == "BX870" { format!("{class} {year:04} {label}") } else { format!("{class} {label}") };
    if class == "DC240" || parts.peek().is_some_and(|x| x.len() <= 2 && x.bytes().all(|b| b.is_ascii_digit())) {
        let raw_day = parts.next()?;
        if raw_day.len() > 2 || !raw_day.bytes().all(|b| b.is_ascii_digit()) { return None; }
        let day = raw_day.parse::<u32>().ok()?;
        let max = match m { 4 | 6 | 9 | 11 => 30, 2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29, 2 => 28, _ => 31 };
        if day == 0 || day > max { return None; }
        heading.push_str(&format!(" {day}"));
    }
    let suffix = parts.collect::<Vec<_>>().join(" ");
    if suffix.is_empty() { return Some((class, heading, String::new())); }
    // Require an ordinary complete item Cutter/year, not another class,
    // range, arbitrary text or a second chronological qualification.
    if suffix.contains(['-', '+', ';']) || suffix.split_whitespace().any(|x| month(x).is_some()) { return None; }
    if super::runtime::cutter_components(&suffix).iter().any(|(letters, digits)| letters.len() != 1 || digits.is_empty()) { return None; }
    let item = canonical_lcc_notation(&format!("{class} {suffix}"))?;
    let tail = item.strip_prefix(class)?.trim_start().to_owned();
    Some((class, heading, tail))
}

// Literal DATE is an unfilled item-date position only at documented
// reference templates or unmodified BS2 text positions. Never invent a year.
pub(super) fn documented_literal_date_base(value: &str) -> Option<String> {
    let base = value.strip_suffix(" DATE")?;
    let endpoint = lcc_endpoint(base, false)?;
    static SCOPES: OnceLock<BTreeSet<&'static str>> = OnceLock::new();
    let scopes = SCOPES.get_or_init(|| include_str!("../data/lcc-literal-date-scopes.tsv").lines().collect());
    scopes.contains(endpoint.code.as_str()).then_some(endpoint.code)
}

// Exact source-reviewed corrections retain the original string as evidence.
// The exporter verifies the preserved Open Library edition and linked MARC
// record. No publication year or neighboring call number is inferred.
pub(super) fn reviewed_call_base(value: &str) -> Option<&'static str> {
    static BASES: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    BASES.get_or_init(|| include_str!("../data/lcc-reviewed-call-bases.tsv").lines().map(|line| {
        line.split_once('\t').expect("reviewed call base row")
    }).collect()).get(value).copied()
}

pub fn lcc_subject_path(notation: &str) -> Option<String> {
    if let Some(range) = encoded_hyphen_range(notation) { return lcc_subject_path(&range); }
    if lcc_code_list(notation).is_some() { return None; }
    if notation.contains(';') && canonical_lcc_notation(notation).is_none() { return None; }
    let normalized = compact_whitespace(notation).to_ascii_uppercase().replace(['–', '—'], "-");
    // Reviewed whole-class spans also occur inside joined imported fields.
    if normalized == "R-RZ" { return letters_only_path("R"); }
    if normalized == "D1-DX301" { return letters_only_path("D"); }
    if let Some(base) = reference_collection_base(&normalized) { return lcc_subject_path(base); }
    if let Some(base) = local_cutter_x_base(&normalized) { return lcc_subject_path(&base); }
    if let Some(base) = reviewed_call_base(&normalized) { return lcc_subject_path(base); }
    if let Some(base) = documented_literal_date_base(&normalized) { return lcc_subject_path(&base); }
    if let Some((base, _, _)) = documented_date_call(&normalized) { return lcc_subject_path(base); }
    if let Some(base) = range_item_base(&normalized) { return lcc_subject_path(base); }
    if let Some(base) = marc_subfield_call_base(&normalized) { return lcc_subject_path(&base); }
    if let Some((start, end)) = reference_plus_span(&normalized) { return lcc_subject_path(&format!("{}-{}", start.code, end.code)); }
    if let Some(base) = repeated_lcc_base(&normalized) { return lcc_subject_path(base); }
    if let Some(base) = terminal_slash_base(&normalized) { return lcc_subject_path(base); }
    if let Some(base) = qualified_table_family(&normalized) { return lcc_subject_path(&base); }
    if let Some(base) = qualified_table_span(&normalized) { return lcc_subject_path(&base); }
    if let Some(base) = table_cutter_span(&normalized) { return lcc_subject_path(&base); }
    if let Some(base) = variable_cutter_span(&normalized) { return lcc_subject_path(&base); }
    if let Some(class) = minimal_subclass_assignment(&normalized) { return lcc_subject_path(class); }
    if let Some(base) = schedule_table_base(&normalized) { return lcc_subject_path(base); }
    if let Some(base) = documented_outline_heading(&normalized) { return lcc_subject_path(base); }
    if let Some(base) = display_bracket_base(&normalized) { return lcc_subject_path(base); }
    if let Some(assignment) = minimal_class_assignment(&normalized) {
        let main = outline().ranges_by_main.get(&assignment.chars().next()?)?
            .first()?.path.split(" / ").next()?;
        return Some(build_subject_path(main, &normalized));
    }
    if is_catalog_or_storage_marker(&normalized) { return None; }
    if let Some((start, end)) = lcc_span(&normalized) {
        let ranges = outline().ranges_by_main.get(&start.letters.chars().next()?)?;
        let best = ranges.iter().filter(|r|
            compare_lcc_key(&r.start_letters,r.start_number,&start.letters,start.number).is_le()
            && compare_lcc_key(&end.letters,end.number,&r.end_letters,r.end_number).is_le())
            .max_by_key(|r| r.path.matches(" / ").count())?;
        return Some(build_subject_path(&best.path, &normalized));
    }
    if normalized.contains('-') { return None; }
    if let Some(path) = letters_only_path(notation) {
        return Some(path);
    }
    let call = parse_lcc_call(notation)?;
    let ranges = outline().ranges_by_main.get(&call.letters.chars().next()?)?;
    let best = ranges.iter().filter(|range| range_contains(range, &call)).max_by(|left, right| compare_matches(left, right, &call))?;
    Some(build_subject_path(&best.path, call.display))
}

/// Normalizes a syntactically valid LCC class, call number, or shelf number
/// containing an explicit LC classification assignment, without
/// requiring that the currently installed outline already contains it.
/// This keeps collection of external identifiers independent from local
/// taxonomy coverage so finer server-side schedules can be requested later.
pub fn canonical_lcc_notation(notation: &str) -> Option<String> {
    if let Some(range) = encoded_hyphen_range(notation) { return Some(range); }
    let notation = compact_whitespace(notation).to_ascii_uppercase().replace(['–', '—'], "-");
    // The published LC outline prints K(520)-5582 for comparative law.
    // Imports also lose the separator after the parenthesized lower bound.
    // https://www.loc.gov/catdir/cpso/lcco/lcco_k.pdf
    if matches!(notation.as_str(), "K(520) 5582" | "K(520)-5582" | "K(520) - 5582") {
        return Some("K520-5582".to_owned());
    }
    if lcc_code_list(&notation).is_some() { return Some(notation); }
    if reference_collection_base(&notation).is_some() { return Some(notation); }
    if local_cutter_x_base(&notation).is_some() { return Some(notation); }
    if reviewed_call_base(&notation).is_some() { return Some(notation); }
    if documented_literal_date_base(&notation).is_some() { return Some(notation); }
    if let Some((_, heading, suffix)) = documented_date_call(&notation) {
        return Some(if suffix.is_empty() { heading } else { format!("{heading} {suffix}") });
    }
    if range_item_base(&notation).is_some() { return Some(notation); }
    if marc_subfield_call_base(&notation).is_some() { return Some(notation); }
    if reference_plus_span(&notation).is_some() { return Some(notation); }
    if repeated_lcc_base(&notation).is_some() { return Some(notation); }
    if terminal_slash_base(&notation).is_some() { return Some(notation); }
    if qualified_table_family(&notation).is_some() { return Some(notation); }
    if qualified_table_span(&notation).is_some() { return Some(notation); }
    if table_cutter_span(&notation).is_some() { return Some(notation); }
    if variable_cutter_span(&notation).is_some() { return Some(notation); }
    if minimal_subclass_assignment(&notation).is_some() { return Some(notation); }
    if schedule_table_base(&notation).is_some() { return Some(notation); }
    if documented_outline_heading(&notation).is_some() { return Some(notation); }
    if display_bracket_base(&notation).is_some() { return Some(notation); }
    if minimal_class_assignment(&notation).is_some() { return Some(notation); }
    if is_catalog_or_storage_marker(&notation) {
        return None;
    }
    if let Some(range) = canonical_printed_range(&notation) { return Some(range); }
    if letters_only_syntax(&notation) || bracketed_year_call_base(&notation).is_some() || doubled_cutter_period_base(&notation).is_some() || period_year_call_base(&notation).is_some() || apostrophe_call_base(&notation).is_some() || lcc_item_base(&notation).is_some()
        || parse_lcc_call(&notation).is_some_and(|call| valid_lcc_remainder(call.remainder) || valid_literature_work_mark(&call)) {
        Some(notation)
    } else {
        None
    }
}

// MARC 050 display brackets surround alternate calls or calls for items not
// held by LC. Accept one complete wrapper, retaining the original evidence.
pub(super) fn display_bracket_base(value: &str) -> Option<&str> {
    let inner = value.strip_prefix('[')?.strip_suffix(']')?.trim();
    if inner.contains(['[', ']']) { return None; }
    canonical_lcc_notation(inner)?;
    Some(inner)
}

// Printed spans stay distinct from individual call numbers and can also be
// queried as complete numeric intervals by the taxonomy matcher.
#[derive(Clone, Debug)]
pub(super) struct LccEndpoint {
    pub letters: String,
    pub number: f64,
    pub remainder: String,
    pub code: String,
}

pub(super) fn lcc_endpoint(value: &str, upper: bool) -> Option<LccEndpoint> {
    let value = value.trim().to_ascii_uppercase();
    if letters_only_syntax(&value) {
        return Some(LccEndpoint { letters: value.clone(), number: if upper { f64::INFINITY } else { 0.0 }, remainder: String::new(), code: value });
    }
    let call = parse_lcc_call(&value)?;
    let mut rest = call.remainder.trim();
    let mut remainder = String::new();
    while !rest.is_empty() {
        rest = rest.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '.');
        let letters = rest.bytes().take_while(u8::is_ascii_alphabetic).count();
        let digits = rest[letters..].bytes().take_while(u8::is_ascii_digit).count();
        if letters == 0 || (digits == 0 && (letters != 1 || letters != rest.len())) { return None; }
        remainder.push('.');
        remainder.push_str(&rest[..letters + digits]);
        rest = &rest[letters + digits..];
    }
    let code = format!("{}{}{}", call.letters, call.number, remainder);
    Some(LccEndpoint { letters: call.letters, number: call.number, remainder, code })
}

pub(super) fn lcc_span(value: &str) -> Option<(LccEndpoint, LccEndpoint)> {
    let (start, end) = value.split_once('-')?;
    let start = lcc_endpoint(start, false)?;
    let end = end.trim();
    let expanded = if end.starts_with(|c: char| c.is_ascii_digit()) {
        format!("{}{end}", start.letters)
    } else if end.strip_prefix('.').is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit())) {
        // A decimal-only end inherits the whole-number part, not the start's
        // existing fraction. Cutter-only ends below inherit the entire number.
        if !start.number.is_finite() || start.code == start.letters { return None; }
        format!("{}{:.0}{end}", start.letters, start.number.trunc())
    } else if start.remainder.len() >= 2
        && start.remainder.starts_with('.')
        && start.remainder.as_bytes()[1].is_ascii_alphabetic()
        && start.remainder.as_bytes()[2..].iter().all(u8::is_ascii_digit)
        && end.len() == 1 && end.as_bytes()[0].is_ascii_alphabetic() {
        // Printed alphabetic Cutter intervals (B829.5.A-Z) share a class
        // number. The lower Cutter may include digits (A3-Z); a numbered
        // upper class endpoint remains explicit.
        format!("{}{}.{end}", start.letters, start.number)
    } else if start.letters.starts_with('P') && start.remainder.ends_with(".Z5")
        && matches!(end, "Z999" | ".Z999")
        && super::runtime::cutter_components(&start.remainder).len() >= 2 {
        // LC literature tables use an author's second Cutter Z5-Z999 for
        // general biography and criticism. Inherit the completed author
        // prefix, rather than interpreting Z999 as a different class.
        // https://loc.gov/catworkshop/lcc/PDFs%20of%20slides/12-5%20handout.pdf
        format!("{}{}{}.Z999", start.letters, start.number,
            start.remainder.strip_suffix(".Z5")?)
    } else if end.starts_with('.') {
        format!("{}{}{end}", start.letters, start.number)
    } else { end.to_owned() };
    let end = lcc_endpoint(&expanded, true)?;
    let order = compare_lcc_key(&start.letters, start.number, &end.letters, end.number);
    if order.is_gt() { return None; }
    if order.is_eq() && !end.remainder.is_empty() {
        let a = super::runtime::cutter_components(&start.remainder);
        let b = super::runtime::cutter_components(&end.remainder);
        if super::runtime::compare_cutter_endpoint(&a, &b).is_gt() { return None; }
    }
    Some((start, end))
}

// A whitespace-separated item Cutter and publication year after a complete
// numeric span describe the book, not a narrower upper classification bound.
// Require a final year and validate every suffix token; never discard prose.
pub(super) fn range_item_base(value: &str) -> Option<&str> {
    if !value.contains('-') || value.len() > 512 { return None; }
    for (at, _) in value.match_indices(char::is_whitespace) {
        let base = &value[..at];
        let Some((start, end)) = lcc_span(base) else { continue; };
        let suffix: Vec<_> = value[at..].split_whitespace().collect();
        let Some(year) = suffix.last() else { return None; };
        let bytes = year.as_bytes();
        if !(bytes.len() == 4 || (bytes.len() == 5 && bytes[4].is_ascii_alphabetic()))
            || !bytes[..4].iter().all(u8::is_ascii_digit) { return None; }
        if !suffix[..suffix.len() - 1].iter().all(|part| {
            let part = part.strip_prefix('.').unwrap_or(part).as_bytes();
            part.len() > 1 && part[0].is_ascii_alphabetic()
                && part[1..].iter().all(u8::is_ascii_digit)
        }) { return None; }
        if start.code != start.letters && end.code != end.letters { return Some(base); }
    }
    None
}

fn canonical_printed_range(value: &str) -> Option<String> {
    let (start, end) = lcc_span(value)?;
    let end_code = if start.letters == end.letters && start.number.is_finite() && end.number.is_finite() {
        end.code.strip_prefix(&end.letters)?.to_owned()
    } else { end.code };
    Some(format!("{}-{end_code}", start.code))
}

// G140 uses the numeric publication year without its display brackets.
// Retain imported brackets as evidence, but match the complete dated call.
pub(super) fn bracketed_year_call_base(value: &str) -> Option<String> {
    let (prefix, year) = value.strip_suffix(']')?.rsplit_once(" [")?;
    if prefix.contains(['[', ']']) { return None; }
    let bytes = year.as_bytes();
    if !(bytes.len() == 4 || (bytes.len() == 5 && bytes[4].is_ascii_alphabetic()))
        || !bytes[..4].iter().all(u8::is_ascii_digit) { return None; }
    parse_lcc_call(prefix)?;
    let base = format!("{prefix} {year}");
    canonical_lcc_notation(&base)?;
    Some(base)
}

// Imported MARC records can duplicate the period before a numeric Cutter.
// Collapse exactly two separators only at a digit-to-Cutter boundary, then
// validate the entire resulting call. Preserve the original as evidence.
pub(super) fn doubled_cutter_period_base(value: &str) -> Option<String> {
    if value.contains('-') { return None; }
    let mut base = String::new();
    let mut copied = 0;
    for (at, _) in value.match_indices('.') {
        if at < copied || !value[..at].trim_end().as_bytes().last().is_some_and(u8::is_ascii_digit) { continue; }
        let tail = value[at + 1..].trim_start();
        let Some(tail) = tail.strip_prefix('.') else { continue; };
        let tail = tail.trim_start();
        if !tail.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
            || !tail.as_bytes().get(1).is_some_and(u8::is_ascii_digit) { continue; }
        base.push_str(&value[copied..at]);
        base.push('.');
        copied = value.len() - tail.len();
    }
    if copied == 0 { return None; }
    base.push_str(&value[copied..]);
    let call = parse_lcc_call(&base)?;
    if !valid_lcc_remainder(call.remainder) && !valid_literature_work_mark(&call)
        && lcc_item_base(&base).is_none() { return None; }
    Some(base)
}

// REF is a reference-collection location label (UGA Libraries). OCLC
// permits location stamps above or below a call number. Remove only the
// exact label around an independently complete LCC call, never local prose.
// https://libraries.uga.edu/user-services/locating/callnumbers
pub(super) fn reference_collection_base(value: &str) -> Option<&str> {
    let candidate = value.strip_prefix("REF ").or_else(|| value.strip_suffix(" REF"))?;
    let candidate = candidate.trim();
    let base = lcc_enumeration_base(candidate).unwrap_or(candidate);
    if base.contains('-') { return None; }
    let call = parse_lcc_call(base)?;
    if !outline().ranges_by_main.get(&call.letters.chars().next()?)?.iter()
        .any(|range| range.start_letters == call.letters) { return None; }
    let complete_parts = call.remainder.split_whitespace().all(|part| {
        let part = part.trim_start_matches('.');
        let bytes = part.as_bytes();
        if bytes.first().is_some_and(u8::is_ascii_digit) {
            return (bytes.len() == 4 || (bytes.len() == 5 && bytes[4].is_ascii_alphabetic()))
                && bytes[..4].iter().all(u8::is_ascii_digit);
        }
        let cutters = super::runtime::cutter_components(part);
        !cutters.is_empty() && cutters.iter().all(|(letters, digits)| letters.len() == 1 && !digits.is_empty())
    });
    ((valid_lcc_remainder(call.remainder) && complete_parts) || valid_literature_work_mark(&call)).then_some(base)
}

// UCLA's Exceptional Call Numbers for Rapid Cataloging, section 2,
// documents Cutter-final x as a removable local suffix. Do not confuse it
// with a Cutter beginning X or a separated literary title work mark.
// https://unitproj.library.ucla.edu/cataloging/procedures/Exceptional%20Call%20Numbers%20for%20Rapid%20Cataloging.pdf
pub(super) fn local_cutter_x_base(value: &str) -> Option<String> {
    let at = value.rfind('X')?;
    let prefix = value[..at].trim_end();
    let tail = &value[at + 1..];
    if !tail.is_empty() && !tail.starts_with(|c: char| c.is_ascii_whitespace() || c == ',') { return None; }
    let year = tail.trim_start().strip_prefix(',').unwrap_or(tail.trim_start()).trim_start();
    if !year.is_empty() {
        let bytes = year.as_bytes();
        if !(bytes.len() == 4 || (bytes.len() == 5 && bytes[4].is_ascii_alphabetic()))
            || !bytes[..4].iter().all(u8::is_ascii_digit) { return None; }
    } else if tail.contains(',') { return None; }
    if prefix.contains('-') { return None; }
    let call = parse_lcc_call(prefix)?;
    if !outline().ranges_by_main.get(&call.letters.chars().next()?)?.iter()
        .any(|range| range.start_letters == call.letters) { return None; }
    let mut rest = call.remainder;
    let mut cutters = 0;
    let mut ends_with_cutter = false;
    while !rest.is_empty() {
        rest = rest.trim_start();
        rest = rest.strip_prefix('.').unwrap_or(rest).trim_start();
        let bytes = rest.as_bytes();
        if bytes.first().is_some_and(u8::is_ascii_digit) {
            let digits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
            if digits != 4 { return None; }
            rest = &rest[digits..];
            ends_with_cutter = false;
            continue;
        }
        if !bytes.first().is_some_and(u8::is_ascii_alphabetic) { return None; }
        let digits = bytes[1..].iter().take_while(|b| b.is_ascii_digit()).count();
        if digits == 0 { return None; }
        cutters += 1;
        rest = &rest[1 + digits..];
        ends_with_cutter = true;
    }
    if cutters == 0 || !ends_with_cutter { return None; }
    if cutters == 1 && matches!(call.letters.as_str(), "PR" | "PS" | "PZ") && value[..at].ends_with(' ') { return None; }
    Some(if year.is_empty() { prefix.to_owned() } else { format!("{prefix} {year}") })
}

// Some imported calls separate the final year with a period. Only interpret
// this after a class and complete numeric Cutters; never join decimal digits.
pub(super) fn period_year_call_base(value: &str) -> Option<String> {
    let (prefix, year) = value.rsplit_once('.')?;
    let bytes = year.as_bytes();
    if !(bytes.len() == 4 || (bytes.len() == 5 && bytes[4].is_ascii_alphabetic()))
        || !bytes[..4].iter().all(u8::is_ascii_digit) { return None; }
    let call = parse_lcc_call(prefix)?;
    let mut rest = call.remainder;
    let mut cutters = 0;
    while !rest.is_empty() {
        rest = rest.trim_start();
        rest = rest.strip_prefix('.').unwrap_or(rest).trim_start();
        let bytes = rest.as_bytes();
        if !bytes.first().is_some_and(u8::is_ascii_alphabetic) { return None; }
        let digits = bytes[1..].iter().take_while(|b| b.is_ascii_digit()).count();
        if digits == 0 { return None; }
        cutters += 1;
        rest = &rest[1 + digits..];
        if !rest.is_empty() && !rest.starts_with(|c: char| c.is_ascii_whitespace() || c == '.') { return None; }
    }
    if cutters == 0 { return None; }
    Some(format!("{prefix} {year}"))
}

// Imported records sometimes use apostrophes between complete call-number
// components. Keep the original notation, and interpret only unambiguous
// class + Cutter components with an optional final publication year.
pub(super) fn apostrophe_call_base(value: &str) -> Option<String> {
    if !value.contains('\'') { return None; }
    for (at, _) in value.match_indices('\'') {
        if at == 0 || !value.as_bytes()[at - 1].is_ascii_alphanumeric()
            || !value.as_bytes().get(at + 1).is_some_and(u8::is_ascii_alphanumeric) { return None; }
    }
    let base = value.replace('\'', " ");
    // Compose two already supported display forms: apostrophe separators
    // and item enumeration. The enumeration never changes classification.
    let call = parse_lcc_call(lcc_enumeration_base(&base).unwrap_or(&base))?;
    let mut cutters = 0;
    let mut parts = call.remainder.split_whitespace().peekable();
    while let Some(part) = parts.next() {
        let part = part.strip_prefix('.').unwrap_or(part);
        let bytes = part.as_bytes();
        if bytes.first().is_some_and(u8::is_ascii_alphabetic)
            && bytes.len() > 1 && bytes[1..].iter().all(u8::is_ascii_digit) {
            cutters += 1;
        } else if cutters > 0 && parts.peek().is_none()
            && (bytes.len() == 4 || (bytes.len() == 5 && bytes[4].is_ascii_alphabetic()))
            && bytes[..4].iter().all(u8::is_ascii_digit) {
            break;
        } else { return None; }
    }
    Some(base)
}

fn is_catalog_or_storage_marker(value: &str) -> bool {
    value.starts_with("CPB") || value.starts_with("MLC") || value.starts_with("LAW") || matches!(value, "LCC" | "COMIC" | "MICROFICHE" | "MICROFILM" | "UNCLASSIFIED" | "ACQUIRED" | "ACQUISITION" | "NONE" | "N/A")
}

// Later LC MLC records also carry explicit subclass assignments, e.g.
// MLCF 2013/40001 (PQ). Validate the completed shelf number with the same
// rules as main-class assignments, and accept only known non-law subclasses.
fn minimal_subclass_assignment(value: &str) -> Option<&str> {
    if !value.starts_with("MLC") { return None; }
    let at = value.rfind('(')?;
    let class = value[at..].strip_prefix('(')?.strip_suffix(')')?.trim();
    if !(2..=3).contains(&class.len()) || class.starts_with('K')
        || !value[..at].contains('/') { return None; }
    static SUBCLASSES: OnceLock<BTreeSet<String>> = OnceLock::new();
    if !SUBCLASSES.get_or_init(outline_subclasses).contains(class) { return None; }
    let main = format!("{}({})", &value[..at], &class[..1]);
    minimal_class_assignment(&main)?;
    Some(class)
}

// DCM B11 and CSB 36: size/custody letters identify shelving, while the
// optional final parenthesized letter carries a broad LC classification.
fn minimal_class_assignment(value: &str) -> Option<&str> {
    // Parentheses already delimit the assignment; imported shelf numbers
    // sometimes omit the space before it. Keep the entire original notation.
    let at = value.rfind('(')?;
    let assignment = &value[at..];
    let class = assignment.strip_prefix('(')?.strip_suffix(')')?.trim();
    if class.len() != 1 || !b"ABCDEFGHJKLMNPQRSTUVZ".contains(&class.as_bytes()[0]) { return None; }
    let placeholder = |s: &str| (3..=5).contains(&s.len()) && s.bytes().all(|b| b == b'X');
    // LC preliminary records can use MLCxxx (P) before a shelf sequence is
    // assigned. Only the explicit parenthesized class supplies a subject.
    let shelving_prefix = |s: &str| {
        let Some(suffix) = s.strip_prefix("MLC") else { return false; };
        let suffix = suffix.as_bytes();
        matches!(suffix.len(), 1 | 2) && b"SMLF".contains(&suffix[0])
            && (suffix.len() == 1 || b"ACEHJKMNRT".contains(&suffix[1]))
    };
    let shelf = value[..at].trim();
    if let Some(x) = shelf.find('X') {
        let prefix = shelf[..x].trim();
        if (prefix == "MLC" || shelving_prefix(prefix)) && placeholder(&shelf[x..]) { return Some(class); }
    }
    let mut parts = value[..at].split_whitespace().peekable();
    let prefix = parts.next()?;
    if matches!(prefix, "MICROFILM" | "MICROFICHE" | "MICROOPAQUE") {
        if parts.peek().is_some_and(|part| matches!(*part, "(O)" | "(W)")) { parts.next(); }
    } else if prefix == "MLC" {
        if parts.next()? != "R" { return None; }
    } else if !shelving_prefix(prefix) { return None; }
    // Whitespace around the delimiter is harmless; whitespace within either
    // digit component remains invalid. Do not concatenate split digits.
    let number = parts.collect::<Vec<_>>().join(" ");
    if prefix.starts_with("MLC") && placeholder(&number) { return Some(class); }
    let (year, sequence) = number.split_once('/')?;
    let (year, sequence) = (year.trim(), sequence.trim());
    if !matches!(year.len(), 2 | 4) || !year.bytes().all(|b| b.is_ascii_digit())
        || sequence.is_empty() || sequence.len() > 5 || !sequence.bytes().all(|b| b.is_ascii_digit()) { return None; }
    Some(class)
}

// G610/G615 volume, part and number designations describe an item, not a
// different subject. Only detach a complete numbered suffix from a valid call.
// Complete coverage chronology or a year plus a two-digit serial number.
// LC MARC 050 for OL3222788M records A33 1979-80; OL2329047M records
// .M23 1985-02, also identified as a series number in MARC 490/830.
// Both forms retain the common call base; no century or endpoint is inferred.
fn complete_item_year_suffix(value: &str) -> bool {
    // LC MARC 050/490/830 for OL1648155M records A13 1990/1.
    // Slash chronology identifies a numbered series issue, not a class range.
    if let Some((year, issue)) = value.split_once('/') {
        if year.len() != 4 || !year.bytes().all(|b| b.is_ascii_digit())
            || !(1000..=2999).contains(&year.parse::<u32>().unwrap()) { return false; }
        // LC 050 for OL4232369M: S37 1978/1979:5. The coverage
        // chronology and issue number jointly describe the series item.
        let issue = if let Some((end, number)) = issue.split_once(':') {
            if !matches!(end.len(), 2 | 4) || !end.bytes().all(|b| b.is_ascii_digit())
                || (end.len() == 4 && !(year.parse::<u32>().unwrap()..=2999)
                    .contains(&end.parse::<u32>().unwrap())) { return false; }
            number
        } else { issue };
        return !issue.is_empty() && issue.len() <= 5 && issue.bytes().all(|b| b.is_ascii_digit());
    }
    let Some((start, end)) = value.split_once('-') else { return false; };
    if start.len() != 4 || !start.bytes().all(|b| b.is_ascii_digit())
        || !matches!(end.len(), 2 | 4) || !end.bytes().all(|b| b.is_ascii_digit()) { return false; }
    let start = start.parse::<u32>().unwrap();
    if !(1000..=2999).contains(&start) { return false; }
    if end.len() == 2 { return true; }
    (start..=2999).contains(&end.parse::<u32>().unwrap())
}

pub(super) fn lcc_enumeration_base(value: &str) -> Option<&str> {
    fn number(rest: &str) -> Option<&str> {
        // A following Bd caption is not series B, Roman volume D.
        if rest.starts_with("BD ") { return None; }
        // Holdings enclose cataloger-supplied enumeration in square brackets.
        // Accept only a complete numeric item value, never a missing value or
        // arbitrary bracketed text (and never a replacement class number).
        if let Some(rest) = rest.strip_prefix('[') {
            let (digits, tail) = rest.split_once(']')?;
            return (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())).then_some(tail);
        }
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            // Series enumeration can be alphanumeric (LC's series-numbering
            // guidance), e.g. the observed "no. M5". This only runs after a
            // recognized item caption; it never creates a class or Cutter.
            if rest.as_bytes().first().is_some_and(u8::is_ascii_alphabetic) {
                let count = rest.as_bytes()[1..].iter().take_while(|b| b.is_ascii_digit()).count();
                if count > 0 { return Some(&rest[1 + count..]); }
                if rest.len() == 1 || rest[1..].starts_with(|c: char| c.is_ascii_whitespace() || matches!(c, '-' | '/' | ':' | ',' | '=')) {
                    return Some(&rest[1..]);
                }
            }
            // MARC 050/830 for OL5370894M identifies AI as series A,
            // volume I; OL491237M records BIV. Validate the Roman portion
            // with the same grammar as an ordinary Roman volume.
            let roman_rest = rest.strip_prefix(['A', 'B']).unwrap_or(rest);
            let count = roman_rest.bytes().take_while(|b| b"IVXLCDM".contains(b)).count();
            if count == 0 { return None; }
            let mut roman = &roman_rest[..count];
            for _ in 0..3 { if let Some(tail) = roman.strip_prefix('M') { roman = tail; } }
            for (nine, four, five, one) in [("CM", "CD", 'D', 'C'), ("XC", "XL", 'L', 'X'), ("IX", "IV", 'V', 'I')] {
                if let Some(tail) = roman.strip_prefix(nine).or_else(|| roman.strip_prefix(four)) { roman = tail; }
                else {
                    roman = roman.strip_prefix(five).unwrap_or(roman);
                    for _ in 0..3 { if let Some(tail) = roman.strip_prefix(one) { roman = tail; } }
                }
            }
            let tail = &roman_rest[count..];
            if !roman.is_empty() || (!tail.is_empty() && !tail.starts_with(|c: char| c.is_ascii_whitespace() || matches!(c, '-' | '/' | ':' | ',' | '='))) { return None; }
            return Some(tail);
        }
        let end = digits + usize::from(rest.as_bytes().get(digits).is_some_and(u8::is_ascii_alphabetic));
        let mut tail = &rest[end..];
        // Columbia 050/490/830 for OL27425628M records Fasz.5a(1).
        if let Some(part) = tail.strip_prefix('(') {
            let (part, after) = part.split_once(')')?;
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) { return None; }
            tail = after;
        }
        // MARC 050 for OL4202742M records Bd. 2. fasz. 3b. A single
        // separating/full-stop period is distinct from a decimal subdivision.
        if let Some(after) = tail.strip_prefix('.') {
            if after.is_empty() || after.starts_with(char::is_whitespace) { tail = after; }
        }
        Some(tail)
    }
    fn new_series_tail(rest: &str) -> Option<&str> {
        ["NEW SER.", "NEW SERIES", "N. S.", "N. S", "N.S.", "N.S",
            "N. F.", "N. F", "N.F.", "N.F", "NEUE FOLGE",
            "N. R.", "N. R", "N.R.", "N.R", "NEUE REIHE", "NY SER.",
            "NOUV. SER.", "NOUV. SéR.", "NOUV. SÉR.", "NOUV. SE\u{301}R.",
            "NOUV. PéRIODE", "NOUV. PÉRIODE", "NOUV. PE\u{301}RIODE"]
            .iter().find_map(|caption| rest.strip_prefix(caption))
    }
    fn complete_year(value: &str) -> bool {
        let year = value.strip_prefix('(').and_then(|s| s.strip_suffix(')')).unwrap_or(value).as_bytes();
        (year.len() == 4 || (year.len() == 5 && year[4].is_ascii_alphabetic()))
            && year[..4].iter().all(u8::is_ascii_digit)
    }
    fn ordinal_caption_tail(rest: &str) -> Option<&str> {
        fn ordinal(rest: &str) -> Option<&str> {
            let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
            if digits == 0 {
                let (roman, tail) = rest.split_once('.')?;
                if roman.is_empty() || !roman.bytes().all(|b| b"IVXLCDM".contains(&b))
                    || number(roman) != Some("") { return None; }
                return Some(tail.trim_start());
            }
            let n = rest[..digits].parse::<u32>().ok()?;
            if n == 0 { return None; }
            let tail = &rest[digits..];
            let tail = if let Some(tail) = tail.strip_prefix('.') { tail } else {
                let ending = if (11..=13).contains(&(n % 100)) { "TH" }
                    else { match n % 10 { 1 => "ST", 2 => "ND", 3 => "RD", _ => "TH" } };
                let tail = tail.strip_prefix(ending).or_else(|| matches!(n, 2 | 3).then(|| tail.strip_prefix('D')).flatten())?;
                tail.strip_prefix('.').unwrap_or(tail)
            };
            Some(tail.trim_start())
        }
        let mut tail = ordinal(rest)?;
        // A combined numbered part can use ordinal punctuation on both
        // numbers, e.g. 21./22. Hft. Never supply a missing endpoint.
        while let Some(next) = tail.strip_prefix(['/','-']) { tail = ordinal(next.trim_start())?; }
        ["SERIES", "SER.", "JAHRG.", "EDITION", "ED.", "AUFL.",
            "REIHE", "BD.", "HFT.", "HEFT", "ABT.", "T.", "SESS."]
            .iter().find_map(|caption| tail.strip_prefix(caption))
    }
    fn hungarian_volume_tail(rest: &str) -> Option<&str> {
        // LC-PCC Hungarian abbreviations: kötet -> köt. Preserve both
        // composed and decomposed accents in imported evidence.
        let tail = ["KöT", "KÖT", "KO\u{308}T"].iter()
            .find_map(|caption| rest.strip_prefix(caption))?;
        let tail = tail.strip_prefix('.').unwrap_or(tail);
        (tail.is_empty() || tail.starts_with(|c: char| c.is_ascii_whitespace()
            || c.is_ascii_digit() || matches!(c, ',' | ':'))).then_some(tail)
    }
    fn numbered_caption_tail(rest: &str) -> Option<&str> {
        // MARC 050 source records confirm Fasz. (OL4683326M), Fas.
        // (OL22424244M), Abh (OL4818026M) and Bd (OL316030M).
        if let Some(tail) = hungarian_volume_tail(rest) {
            let tail = tail.trim_start();
            return Some(tail.strip_prefix(',').unwrap_or(tail).trim_start());
        }
        new_series_tail(rest).map(|tail| {
            let tail = tail.trim_start();
            tail.strip_prefix([',', ':']).unwrap_or(tail).trim_start()
        }).or_else(|| ["SER.", "SERIES ", "VOLUME ", "VOLS.", "VOL .", "VOL ", "VOL.", "V .", "V.",
                "PART ", "PTS.", "PT ", "PT.", "NUMBER ", "NOS.", "NO .", "NO ", "NO.", "N:O", "ISSUE ", "ISS.", "SECT.", "SESS.", "SUPPL.", "SUPPL ", "SUPP.", "SUPL.",
                "BD.", "BD ", "BDE.", "BDCHN.", "NR.", "T.", "FASC.", "FASZ.", "FAS.", "ABT.", "LFG.", "LIVR.",
                "BK.", "JAHRG.", "JAARG.", "KN.", "KNJ.", "CZ.", "D.", "SV.", "ZESZ.", "VYP.",
                "HEFT.", "HEFT ", "H.", "HFT.", "HBD.", "HALBBD.", "DEEL", "DL.", "SZ.", "NIDE ", "OSA ",
                "ABH.", "ABH ", "BEIHEFT ", "BEIH.", "FOLGE ", "AR.", "ZV.", "REIHE ", "BOOK ", "UNIT ", "COPY ", "COP.", "BAND ", "TEIL ", "TEILBD.", "CH.", "SEC.", "CAHIER ",
                "RéSZ ", "RÉSZ ", "RE\u{301}SZ ", "FüZET ", "FÜZET ", "FU\u{308}ZET ",
                "SéR.", "SÉR.", "SE\u{301}R.", "AFL.", "PTIES ", "PTIE ",
                "ROč.", "ROČ.", "ROC\u{30c}.", "SEš.", "SEŠ.", "SES\u{30c}.", "LVL.", "LVL ", "LEVEL "]
                .iter().find_map(|label| rest.strip_prefix(label)))
    }
    fn suffix(mut rest: &str, allow_unfinished: bool) -> bool {
        // Supplied volume enumeration may enclose both caption and number,
        // e.g. [v.2]. Require a complete numbered caption inside one pair;
        // this does not unwrap arbitrary shelving notes or nested brackets.
        if let Some(inner) = rest.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            if inner.contains(['[', ']'])
                || numbered_caption_tail(inner).and_then(|s| number(s.trim_start())).is_none() {
                return false;
            }
            return suffix(inner, false);
        }
        let mut numbered_level = false;
        loop {
            // Hungarian also places the volume caption after its number.
            let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
            if digits > 0 && rest[digits..].starts_with(char::is_whitespace) {
                if let Some(tail) = hungarian_volume_tail(rest[digits..].trim_start()) {
                    rest = tail.trim_start();
                    if rest.is_empty() { return true; }
                    rest = rest.strip_prefix([',', ':']).unwrap_or(rest).trim_start();
                    if rest.is_empty() { return false; }
                    continue;
                }
            }
            // Ordinal numbering may precede its caption (3rd ser., 38.Jahrg.).
            if let Some(tail) = ordinal_caption_tail(rest) {
                numbered_level = true;
                rest = tail.trim_start();
                if rest.is_empty() || complete_year(rest) { return true; }
                rest = rest.strip_prefix([',', ':']).unwrap_or(rest).trim_start();
                if rest.is_empty() { return false; }
                continue;
            }
            // LC 050 for OL439736M records vol. 90, commentary.
            // Only recognize this designation after a numbered item level.
            if numbered_level {
                if let Some(tail) = rest.strip_prefix("COMMENTARY") {
                    if tail.is_empty() { return true; }
                    if !tail.starts_with(|c: char| c.is_ascii_whitespace() || c == ',') { return false; }
                    rest = tail.trim_start().strip_prefix(',').unwrap_or(tail.trim_start()).trim_start();
                    if rest.is_empty() { return false; }
                    numbered_level = false;
                    continue;
                }
            }
            // Imported item enumeration can stop at its final caption.
            // The caller validates the complete preceding call; no missing
            // volume/part number is reconstructed or used as classification.
            // Bare V is ambiguous with an unfinished Cutter unless it
            // follows an already numbered enumeration level.
            if allow_unfinished && (matches!(rest, "VOL." | "VOL" | "V." | "PT." | "PT" | "NO." | "NO")
                || (rest == "V" && numbered_level)) { return true; }
            // G155 uses Suppl. for the first supplement, with numbers for
            // subsequent supplements. Retain the common unpunctuated form.
            // G140/G230 also append Index to the original call number.
            if matches!(rest, "SUPPL." | "SUPPL" | "SUPP." | "SUPL." | "INDEX") { return true; }
            // Although G140 prescribes no date after Index, LC-source
            // imports do contain this form (OL3543081M, MARC 050 00).
            // Keep the original call's date; the following year identifies
            // the index item and never supplies a missing classification.
            if let Some(year) = rest.strip_prefix("INDEX ") {
                return year.len() == 4 && year.bytes().all(|b| b.is_ascii_digit());
            }
            // An unnumbered supplement may itself have numbered volumes:
            // Suppl. v.61, Suppl., v.62, or the joined German Suppl.Bd.14.
            // Require a recognized next caption, whose number is validated
            // on the next iteration; unknown prose is never discarded.
            if let Some(tail) = ["SUPPL.", "SUPP.", "SUPL.", "SUPPL "]
                .iter().find_map(|caption| rest.strip_prefix(caption)) {
                let tail = tail.trim_start();
                let tail = tail.strip_prefix([',', ':']).unwrap_or(tail).trim_start();
                if numbered_caption_tail(tail).and_then(|n| number(n.trim_start())).is_some() { rest = tail; continue; }
            }
            // MARC 853-855 allows a numbered series or an unnumbered
            // "new ser." caption preceding the ordinary volume levels.
            // Legacy holdings also use n.s. (Harvard Library item-description
            // guidance and OCLC's caption abbreviation list).
            if let Some(tail) = new_series_tail(rest) {
                if tail.is_empty() { return true; }
                if tail.starts_with(|c: char| c.is_ascii_whitespace() || matches!(c, ':' | ',')) {
                    let tail = tail.trim_start();
                    let tail = tail.strip_prefix([':', ',']).unwrap_or(tail).trim_start();
                    if tail.is_empty() { return false; }
                    if number(tail).is_none() { rest = tail; continue; }
                }
            }
            // G615 retains established vernacular designations. The PCC
            // caption abbreviation list documents these numbered labels.
            // Annual-volume captions can be followed by chronology. Do not
            // reinterpret a shortened year there as another volume number.
            let comma_numbers = !["JAHRG.", "JAARG.", "AR.", "ROč.", "ROČ.", "ROC\u{30c}."].iter().any(|caption| rest.starts_with(caption));
            let Some(tail) = numbered_caption_tail(rest) else { return false; };
            let Some(tail) = number(tail.trim_start()) else { return false; };
            numbered_level = true;
            rest = tail;
            loop {
                // Holdings lists repeat the enumeration without repeating its
                // caption (v.5,6,7 or no.88-89,94). A following caption still
                // takes the normal path below; never discard unknown text.
                if comma_numbers {
                    if let Some(next) = rest.trim_start().strip_prefix(',') {
                        let next = next.trim_start();
                        // Preserve an ordinal caption such as 2nd ed.; its
                        // leading digits are not another volume number.
                        if ordinal_caption_tail(next).is_some() { break; }
                        if let Some(tail) = number(next) { rest = tail; continue; }
                    }
                }
                if !rest.starts_with(['-', ':', '/']) { break; }
                // A colon can introduce the next captioned level (v.1:no.2)
                // as well as the existing numeric shorthand (v.1:2).
                if rest.starts_with(':') && number(&rest[1..]).is_none() { break; }
                let Some(tail) = number(&rest[1..]) else { return false; };
                rest = tail;
            }
            if rest.is_empty() { return true; }
            if !rest.starts_with(|c: char| c.is_ascii_whitespace() || matches!(c, ',' | ':' | '=')) { return false; }
            rest = rest.trim_start();
            if hungarian_volume_tail(rest) == Some("") { return true; }
            if rest.starts_with([',', ':', '=']) { rest = rest[1..].trim_start(); }
            if rest.is_empty() { return false; }
            // "v. 82, etc." records further item volumes, without changing
            // the classification. Require the complete numbered level above.
            if let Some(tail) = rest.strip_prefix("ETC.") {
                if tail.is_empty() { return true; }
                if !tail.starts_with(char::is_whitespace) { return false; }
                rest = tail.trim_start();
            }
            // Serial/item call numbers may end with chronology after their
            // enumeration (e.g. v.1 (1999)). Only a complete year is detached.
            if complete_year(rest) || complete_item_year_suffix(rest) { return true; }
            if let Some(year) = rest.strip_prefix("OF ") {
                return year.len() == 4 && year.bytes().all(|b| b.is_ascii_digit());
            }
            // Chronology may separate two numbered levels, e.g.
            // Jahrg. 44, 1976, Heft 1. The following caption must still parse.
            if let Some((year, tail)) = rest.split_once(',') {
                if complete_year(year.trim()) { rest = tail.trim_start(); }
            }
        }
    }
    for (at, _) in value.match_indices(' ') {
        if !suffix(value[at..].trim_start(), !value.starts_with('K')) { continue; }
        let base = value[..at].trim_end().strip_suffix(',').unwrap_or(value[..at].trim_end()).trim_end();
        // A storage phrase must not become a PZ title mark plus issue number.
        if base.ends_with(" BOX") && value[at..].trim_start().starts_with("NO") { continue; }
        let call = parse_lcc_call(base)?;
        if !base.contains('-') && (valid_lcc_remainder(call.remainder) || valid_literature_work_mark(&call)) {
            return Some(base);
        }
    }
    None
}

// CSB 24 and G622 distinguish custodial assignments from classification.
// Preserve the full notation; only known assignment forms yield a match base.
pub(super) fn lcc_item_base(value: &str) -> Option<&str> {
    let mut base = value;
    // Coverage years and year/serial identifiers follow numeric Cutters.
    // Match their common call base without inventing a year or selecting
    // an endpoint. The original suffix remains in the evidence.
    // https://openlibrary.org/show-records/marc_loc_2016/BooksAll.2016.part15.utf8:18301670:981
    for (at, _) in value.match_indices(' ').rev() {
        let prefix = &value[..at];
        let (years, tail) = value[at + 1..].split_once(' ').unwrap_or((&value[at + 1..], ""));
        if !complete_item_year_suffix(years) { continue; }
        // LC 050/490/830 for OL4156375M: .H4 1979/80 abh. 5.
        // Validate a following numbered caption independently of chronology.
        let complete_tail = tail.is_empty() || (tail.bytes().any(|b| b.is_ascii_digit())
            && lcc_enumeration_base(&format!("{prefix} {tail}")) == Some(prefix));
        if complete_tail && !prefix.contains('-')
            && parse_lcc_call(prefix).is_some_and(|call| {
                if call.letters.starts_with('K') { return false; }
                let mut rest = call.remainder;
                let mut count = 0;
                while !rest.is_empty() {
                    rest = rest.trim_start();
                    rest = rest.strip_prefix('.').unwrap_or(rest).trim_start();
                    let bytes = rest.as_bytes();
                    if !bytes.first().is_some_and(u8::is_ascii_alphabetic) { return false; }
                    let digits = bytes[1..].iter().take_while(|b| b.is_ascii_digit()).count();
                    if digits == 0 { return false; }
                    count += 1;
                    rest = &rest[1 + digits..];
                }
                count > 0 && outline().ranges_by_main.get(&call.letters.chars().next().unwrap()).is_some_and(|ranges|
                    ranges.iter().any(|range| range.start_letters == call.letters))
            }) { base = prefix; break; }
    }
    // Columbia CPM-605 records F, FF and FFF in MARC 852 $m as
    // oversize suffixes on LC calls, also after volume enumeration.
    // Require a complete dated call or explicit enumeration so an
    // unfinished Cutter or literary title letter cannot become a size mark.
    // https://www1.columbia.edu/sec/cu/libraries/inside/clio/docs/bcd/cpm/cpmspe/cpm605.html
    if let Some(prefix) = [" FFF", " FF", " F"].iter().find_map(|suffix| value.strip_suffix(suffix)) {
        let enumerated = lcc_enumeration_base(prefix);
        let candidate = enumerated.unwrap_or(prefix);
        let year = candidate.split_whitespace().last().unwrap_or_default().as_bytes();
        let dated = (year.len() == 4 || (year.len() == 5 && year[4].is_ascii_alphabetic()))
            && year[..4].iter().all(u8::is_ascii_digit);
        let numbered = enumerated.is_some() && prefix[candidate.len()..].bytes().any(|b| b.is_ascii_digit());
        if (dated || numbered) && !candidate.contains('-')
            && parse_lcc_call(candidate).is_some_and(|call| {
                let complete_parts = call.remainder.split_whitespace().all(|part| {
                    let part = part.trim_start_matches('.');
                    let bytes = part.as_bytes();
                    if bytes.first().is_some_and(u8::is_ascii_digit) {
                        return (bytes.len() == 4 || (bytes.len() == 5 && bytes[4].is_ascii_alphabetic()))
                            && bytes[..4].iter().all(u8::is_ascii_digit);
                    }
                    let cutters = super::runtime::cutter_components(part);
                    !cutters.is_empty() && cutters.iter().all(|(letters, digits)| letters.len() == 1 && !digits.is_empty())
                });
                ((valid_lcc_remainder(call.remainder) && complete_parts) || valid_literature_work_mark(&call))
                    && outline().ranges_by_main.get(&call.letters.chars().next().unwrap()).is_some_and(|ranges|
                        ranges.iter().any(|range| range.start_letters == call.letters))
            }) { base = candidate; }
    }
    // A single terminal period after a complete publication year is export
    // punctuation, not a decimal or an unfinished Cutter. Keep the dated
    // call intact and preserve the original notation in the match keys.
    if let Some(prefix) = value.strip_suffix('.') {
        if let Some((undated, year)) = prefix.rsplit_once(' ') {
            let year = year.as_bytes();
            if (year.len() == 4 || (year.len() == 5 && year[4].is_ascii_alphabetic()))
                && year[..4].iter().all(u8::is_ascii_digit) && !prefix.contains('-')
                && parse_lcc_call(undated).is_some_and(|call| {
                    let known_class = outline().ranges_by_main.get(&call.letters.chars().next().unwrap()).is_some_and(|ranges|
                        ranges.iter().any(|range| range.start_letters == call.letters));
                    if !known_class { return false; }
                    if valid_literature_work_mark(&call) { return true; }
                    let mut rest = call.remainder;
                    while !rest.is_empty() {
                        rest = rest.trim_start();
                        rest = rest.strip_prefix('.').unwrap_or(rest).trim_start();
                        let bytes = rest.as_bytes();
                        if !bytes.first().is_some_and(u8::is_ascii_alphabetic) { return false; }
                        let digits = bytes[1..].iter().take_while(|b| b.is_ascii_digit()).count();
                        if digits == 0 { return false; }
                        rest = &rest[1 + digits..];
                        if !rest.is_empty() && !rest.starts_with(|c: char| c.is_ascii_whitespace() || c == '.') { return false; }
                    }
                    true
                }) { base = prefix; }
        }
    }
    // Libraries append eb (often directly to the date) or ebook to identify
    // electronic copies. The remaining call must independently be complete.
    if let Some(prefix) = value.strip_suffix(" EBOOK").or_else(|| value.strip_suffix("EB")) {
        let prefix = prefix.trim_end();
        let mut candidate = lcc_enumeration_base(prefix).unwrap_or(prefix);
        // Imported ebook records sometimes repeat the marker, or attach it
        // both to the call and to its volume. Only collapse repeated markers
        // after a digit; a literary title mark ending in EB stays intact.
        let mut repeated_base = candidate;
        while let Some(rest) = repeated_base.strip_suffix("EB") { repeated_base = rest.trim_end(); }
        if repeated_base.as_bytes().last().is_some_and(u8::is_ascii_digit) { candidate = repeated_base; }
        if !candidate.contains('-') && parse_lcc_call(candidate).is_some_and(|call| {
            let complete_parts = call.remainder.split_whitespace().all(|part| {
                let part = part.trim_start_matches('.');
                if part.is_empty() { return true; }
                if part.as_bytes()[0].is_ascii_digit() {
                    let (year, cutter) = part.split_once('.').map_or((part, None), |(year, cutter)| (year, Some(cutter)));
                    let year = year.as_bytes();
                    return (year.len() == 4 || (year.len() == 5 && year[4].is_ascii_alphabetic()))
                        && year[..4].iter().all(u8::is_ascii_digit)
                        && cutter.is_none_or(|part| {
                            let cutters = super::runtime::cutter_components(part);
                            !cutters.is_empty() && cutters.iter().all(|(letters, digits)| letters.len() == 1 && !digits.is_empty())
                        });
                }
                let cutters = super::runtime::cutter_components(part);
                !cutters.is_empty() && cutters.iter().all(|(letters, digits)| letters.len() == 1 && !digits.is_empty())
            });
            // LC's MARC 050 for LCCN 75313994 records TX553.A3 W67a.
            // This verified legacy work mark is not a general license to
            // accept arbitrary trailing letters on numeric Cutters.
            // https://openlibrary.org/show-records/marc_loc_2016/BooksAll.2016.part09.utf8:65764277:1235
            // Columbia's MARC 050 4 for OL12773809M independently records
            // QP905 .H236s v.175, corroborating this second legacy base.
            // https://openlibrary.org/show-records/marc_columbia/Columbia-extract-20221130-012.mrc:204912902:1888
            // LC's MARC 050 10 for OL4886228M records PZ3.M8346 Lo5.
            // https://openlibrary.org/show-records/marc_loc_2016/BooksAll.2016.part09.utf8:176896691:989
            let reviewed_legacy_work_mark = matches!(candidate, "TX553.A3 W67A" | "QP905 .H236S" | "PZ3.M8346 LO5");
            ((valid_lcc_remainder(call.remainder) && complete_parts) || valid_literature_work_mark(&call)
                || reviewed_legacy_work_mark)
                && outline().ranges_by_main.get(&call.letters.chars().next().unwrap()).is_some_and(|ranges|
                    ranges.iter().any(|range| range.start_letters == call.letters))
        }) { base = candidate; }
    }
    // Textbook call numbers can append a K–12 grade designation (Tarleton
    // Library). Interpret it only after a complete dated call, so GR as an
    // LCC class or a label inside an unfinished call is not discarded.
    if let Some((prefix, grade)) = value.rsplit_once(" GR.").or_else(|| value.rsplit_once(" GRADE ")) {
        let grade = grade.trim_start();
        let grade_number = |part: &str| -> Option<u8> {
            if part == "K" { return Some(0); }
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) { return None; }
            part.parse::<u8>().ok().filter(|n| (1..=12).contains(n))
        };
        let valid_grade = if let Some((start, end)) = grade.split_once('-') {
            grade_number(start).zip(grade_number(end)).is_some_and(|(a, b)| a <= b)
        } else { grade_number(grade).is_some() };
        let year = prefix.split_whitespace().last().unwrap_or_default().as_bytes();
        if valid_grade && (year.len() == 4 || (year.len() == 5 && year[4].is_ascii_alphabetic()))
            && year[..4].iter().all(u8::is_ascii_digit) && !prefix.contains('-')
            && parse_lcc_call(prefix).is_some_and(|call| {
                let cutters = super::runtime::cutter_components(call.remainder);
                valid_lcc_remainder(call.remainder) && !cutters.is_empty()
                    && cutters.iter().all(|(letters, digits)| letters.len() == 1 && !digits.is_empty())
                    && outline().ranges_by_main.get(&call.letters.chars().next().unwrap()).is_some_and(|ranges|
                        ranges.iter().any(|range| range.start_letters == call.letters))
            }) { base = prefix; }
    }
    // Serial enumeration can use year:issue. Keep the complete dated call
    // as the match base; never turn a bare number or truncated issue into LCC.
    if let Some((prefix, issue)) = value.rsplit_once(':') {
        let year = prefix.split_whitespace().last().unwrap_or_default();
        if year.len() == 4 && year.bytes().all(|b| b.is_ascii_digit())
            && !issue.is_empty() && issue.bytes().all(|b| b.is_ascii_digit())
            && !prefix.contains(['-', ':'])
            && parse_lcc_call(prefix).is_some_and(|call| {
                let cutters = super::runtime::cutter_components(call.remainder);
                !call.letters.starts_with('K') && valid_lcc_remainder(call.remainder)
                    && !cutters.is_empty()
                    && cutters.iter().all(|(letters, digits)| letters.len() == 1 && !digits.is_empty())
                    && outline().ranges_by_main.get(&call.letters.chars().next().unwrap()).is_some_and(|ranges|
                        ranges.iter().any(|range| range.start_letters == call.letters))
            }) { base = prefix; }
    }
    // UBC's standard print suffix list includes workbook. A year is not
    // required when the preceding classification and Cutter are complete.
    let workbook_prefix = [" STUDENT WORKBOOK", " WORKBOOK"].iter()
        .find_map(|suffix| value.strip_suffix(suffix)).or_else(|| {
            let (prefix, tail) = value.rsplit_once(" WORKBOOK ")?;
            let year = tail.as_bytes();
            let complete_year = (year.len() == 4 || (year.len() == 5 && year[4].is_ascii_alphabetic()))
                && year[..4].iter().all(u8::is_ascii_digit);
            let numbered_part = tail.starts_with("PART ")
                && lcc_enumeration_base(&format!("{prefix} {tail}")) == Some(prefix);
            (complete_year || tail == "TEACHER'S EDITION" || numbered_part).then_some(prefix)
        });
    if let Some(prefix) = workbook_prefix {
        let prefix = prefix.trim_end().strip_suffix(',').unwrap_or(prefix.trim_end()).trim_end();
        let candidate = lcc_enumeration_base(prefix).unwrap_or(prefix);
        if parse_lcc_call(candidate).is_some_and(|call| {
            let cutters = super::runtime::cutter_components(call.remainder);
            !candidate.contains('-') && valid_lcc_remainder(call.remainder)
                && !cutters.is_empty()
                && cutters.iter().all(|(letters, digits)| letters.len() == 1 && !digits.is_empty())
                && outline().ranges_by_main.get(&call.letters.chars().next().unwrap()).is_some_and(|ranges|
                    ranges.iter().any(|range| range.start_letters == call.letters))
        }) { base = candidate; }
    }
    // Accompanying print material and explicit folio-size labels keep the
    // original dated call (Yale Accompanying Material and folio guidance).
    // LC 050 for OL3795881M records <fol>. Imported edition records also
    // display fol and <fol.>; recognize only these exact folio spellings.
    // Recognize only these documented descriptors, not arbitrary prose.
    if let Some(prefix) = [" INSTRUCTOR'S RESOURCE MANUAL", " INSTRUCTOR'S MANUAL", " INSTRUCTOR MANUAL",
        " INSTRUCTOR'S GUIDE", " TEACHER'S MANUAL", " TEACHERS GUIDE", " TEACHER GUIDE", " TEACHER'S GUIDE",
        " STUDY GUIDE", " STUDENT GUIDE", " STUDENT MANUAL", " SOLUTIONS MANUAL", " SOLUTION MANUAL", " LAB MANUAL",
        " USER GUIDE", " GUIDE", " MANUAL", " WORKBOOK", " FOLIO", " FOL.", " FOL", " <FOL>", " <FOL.>"]
        .iter().find_map(|suffix| value.strip_suffix(suffix)) {
        let prefix = prefix.trim_end().strip_suffix(',').unwrap_or(prefix.trim_end()).trim_end();
        let year = prefix.split_whitespace().last().unwrap_or_default().as_bytes();
        if (year.len() == 4 || (year.len() == 5 && year[4].is_ascii_alphabetic()))
            && year[..4].iter().all(u8::is_ascii_digit)
            && parse_lcc_call(prefix).is_some_and(|call| valid_lcc_remainder(call.remainder)
                && outline().ranges_by_main.get(&call.letters.chars().next().unwrap()).is_some_and(|ranges|
                    ranges.iter().any(|range| range.start_letters == call.letters))) {
            base = prefix;
        }
    }
    // Folio markers describe physical size, including after existing volume
    // enumeration. Angle brackets supply their own token boundary. Retain
    // literary work letters only when a date or numbered caption disambiguates
    // the size label from an undated title mark.
    if let Some(prefix) = ["<FOL.>", "<FOL>", " FOLIO", " FOL.", " FOL"]
        .iter().find_map(|suffix| value.strip_suffix(suffix)) {
        let prefix = prefix.trim_end().strip_suffix(',').unwrap_or(prefix.trim_end()).trim_end();
        let enumerated = lcc_enumeration_base(prefix);
        let candidate = enumerated.unwrap_or(prefix);
        let year = candidate.split_whitespace().last().unwrap_or_default().as_bytes();
        let dated = (year.len() == 4 || (year.len() == 5 && year[4].is_ascii_alphabetic()))
            && year[..4].iter().all(u8::is_ascii_digit);
        let numbered = enumerated.is_some() && prefix[candidate.len()..].bytes().any(|b| b.is_ascii_digit());
        if !candidate.contains('-') && parse_lcc_call(candidate).is_some_and(|call| {
            let literary = matches!(call.letters.as_str(), "PR" | "PS" | "PZ");
            let cutters = super::runtime::cutter_components(call.remainder);
            let numeric_cutters = !cutters.is_empty()
                && cutters.iter().all(|(letters, digits)| letters.len() == 1 && !digits.is_empty());
            (dated || numbered || (!literary && numeric_cutters))
                && (valid_lcc_remainder(call.remainder) || valid_literature_work_mark(&call))
                && outline().ranges_by_main.get(&call.letters.chars().next().unwrap())
                    .is_some_and(|ranges| ranges.iter().any(|range| range.start_letters == call.letters))
        }) { base = candidate; }
    }
    // Some imported calls separate the publication year with a semicolon.
    // Only a complete four-digit year can be detached; a second call or an
    // unfinished classification list must still use the list parser.
    if let Some((prefix, year)) = value.split_once(';') {
        let year = year.trim();
        let prefix = prefix.trim_end();
        if year.len() == 4 && year.bytes().all(|b| b.is_ascii_digit())
            && !prefix.contains('-')
            && parse_lcc_call(prefix).is_some_and(|call|
                valid_lcc_remainder(call.remainder)
                    && call.remainder.split(|c: char| c.is_ascii_whitespace() || c == '.')
                        .filter(|part| !part.is_empty()).all(|part| {
                            let bytes = part.as_bytes();
                            bytes.len() >= 2 && bytes[0].is_ascii_alphabetic()
                                && bytes[1..].iter().all(u8::is_ascii_digit)
                        })
                    && outline().ranges_by_main
                    .get(&call.letters.chars().next().unwrap()).is_some_and(|ranges|
                        ranges.iter().any(|range| range.start_letters == call.letters))) {
            base = prefix;
        }
    }
    // A complete call followed by the physical-item qualifier (set) keeps
    // its classification. Do not interpret other parenthesized text here.
    if let Some(prefix) = value.strip_suffix("(SET)") {
        let prefix = prefix.trim_end();
        if parse_lcc_call(prefix).is_some_and(|call| outline().ranges_by_main
            .get(&call.letters.chars().next().unwrap()).is_some_and(|ranges|
                ranges.iter().any(|range| range.start_letters == call.letters))) {
            base = prefix;
        }
    }
    // CSB 36 (Spring 1987), p. 43: minimally cataloged maps retain a
    // regular LC call number followed by a space and MLC. This marker
    // supplies no subject of its own; the complete map call must validate.
    if let Some(prefix) = value.strip_suffix(" MLC") {
        let prefix = prefix.trim_end();
        if parse_lcc_call(prefix).is_some_and(|call| call.letters == "G" && valid_lcc_remainder(call.remainder)) {
            base = prefix;
        }
    }
    let opener = match value.as_bytes().last() { Some(b'>') => '<', Some(b')') => '(', _ => '\0' };
    if opener != '\0' {
        if let Some((prefix, note)) = value[..value.len() - 1].rsplit_once(opener) {
            let parts = note.split_whitespace().collect::<Vec<_>>();
            let language = |s: &str| matches!(s, "CHINA" | "JAPAN" | "KOREA" | "ARAB" | "HEBR" | "HIND" | "PERS" | "AMHAR" | "TIGR" | "CEBUANO");
            let recognized = match parts.as_slice() {
                ["HEBR"] => true,
                ["ORIEN" | "ASIAN" | "AMED", lang] => language(lang),
                ["ORIEN" | "ASIAN" | "AMED", lang, "CAGE"] => language(lang),
                _ => false,
            };
            if recognized && prefix.ends_with(char::is_whitespace) { base = prefix.trim_end(); }
        }
    }
    // Yale appends (LC) to call numbers in designated shelving runs.
    // It carries no additional subject information. Require a complete call
    // with a known LCC class; the marker alone never creates a match.
    if let Some(prefix) = base.strip_suffix(" (LC)") {
        let call_base = lcc_enumeration_base(prefix).unwrap_or(prefix);
        if parse_lcc_call(call_base).is_some_and(|call|
            (valid_lcc_remainder(call.remainder) || valid_literature_work_mark(&call))
                && outline().ranges_by_main.get(&call.letters.chars().next().unwrap())
                    .is_some_and(|ranges| ranges.iter().any(|range| range.start_letters == call.letters))) {
            base = prefix;
        }
    }
    // Yale's historical oversize notation appends + (or ++ for scores).
    // Recognize it only after a complete dated call with numeric Cutters;
    // a plus on a class/range remains a classification-scope expression.
    if let Some(prefix) = base.strip_suffix("++").or_else(|| base.strip_suffix('+')) {
        if let Some((call_base, year)) = prefix.rsplit_once(' ') {
            let year = year.as_bytes();
            if (year.len() == 4 || (year.len() == 5 && year[4].is_ascii_alphabetic()))
                && year[..4].iter().all(u8::is_ascii_digit) && !call_base.contains('-')
                && parse_lcc_call(call_base).is_some_and(|call| {
                    let cutters = super::runtime::cutter_components(call.remainder);
                    valid_lcc_remainder(call.remainder) && !cutters.is_empty()
                        && cutters.iter().all(|(letters, digits)| letters.len() == 1 && !digits.is_empty())
                        && outline().ranges_by_main.get(&call.letters.chars().next().unwrap())
                            .is_some_and(|ranges| ranges.iter().any(|range| range.start_letters == call.letters))
                }) {
                base = prefix;
            }
        }
    }
    base = lcc_enumeration_base(base).unwrap_or(base);
    if base == value { return None; }
    let valid = canonical_printed_range(base).is_some()
        || parse_lcc_call(base).is_some_and(|call| valid_lcc_remainder(call.remainder) || valid_literature_work_mark(&call));
    valid.then_some(base)
}

// G350 documents PZ title work letters. Historical MARC imports also append
// the PZ item subfield to alternate PR/PS classifications (e.g. LC 35014886).
// Recognize that imported shape without treating it as standard PR/PS shelving.
fn valid_literature_work_mark(call: &LccCall<'_>) -> bool {
    if !matches!(call.letters.as_str(), "PZ" | "PR" | "PS") { return false; }
    let remainder = call.remainder.trim_start();
    let remainder = remainder.strip_prefix('.').unwrap_or(remainder).trim_start();
    let mut parts = remainder.split_whitespace();
    let Some(first) = parts.next() else { return false; };
    let author = first.as_bytes();
    let has_cutter = author.len() >= 2 && author[0].is_ascii_alphabetic()
        && author[1..].iter().all(u8::is_ascii_digit);
    let title = if has_cutter {
        let Some(title) = parts.next() else { return false; };
        title
    } else if matches!(call.letters.as_str(), "PR" | "PS") {
        // Alternate PR/PS numbers may have no Cutter at all (LC 78316959:
        // $a PZ3.D55 $b Su 1978 $a PR4552). The imported item mark adds
        // no author precision to the explicit alternate class.
        first
    } else { return false; };
    // G350 section 8 preserves the historical single-letter PZ practice
    // (PZ7.S268 E). Alternate PR/PS imports retain their existing rule.
    let minimum_title_letters = if call.letters == "PZ" { 1 } else { 2 };
    if title.len() < minimum_title_letters || !title.bytes().all(|b| b.is_ascii_alphabetic()) { return false; }
    if let Some(date) = parts.next() {
        let date = date.as_bytes();
        if !(date.len() == 4 || (date.len() == 5 && date[4].is_ascii_alphabetic()))
            || !date[..4].iter().all(u8::is_ascii_digit) { return false; }
    }
    parts.next().is_none()
}

fn valid_lcc_remainder(mut remainder: &str) -> bool {
    while !remainder.is_empty() {
        remainder = remainder.trim_start();
        if remainder.is_empty() {
            return true;
        }
        let bytes = remainder.as_bytes();
        let mut cursor;
        match bytes[0] {
            b'.' => {
                cursor = 1;
                // Spacing after the Cutter separator does not change its
                // letter/number components. Do not consume a second period.
                while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) { cursor += 1; }
                if !bytes.get(cursor).is_some_and(u8::is_ascii_alphabetic) {
                    return false;
                }
                while bytes.get(cursor).is_some_and(|byte| byte.is_ascii_alphanumeric()) {
                    cursor += 1;
                }
            }
            b'-' => {
                cursor = 1;
                if !bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
                    return false;
                }
                while bytes.get(cursor).is_some_and(|byte| byte.is_ascii_digit() || *byte == b'.') {
                    cursor += 1;
                }
            }
            byte if byte.is_ascii_alphabetic() => {
                cursor = 1;
                while bytes.get(cursor).is_some_and(u8::is_ascii_alphabetic) {
                    cursor += 1;
                }
                if !bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
                    return false;
                }
                while bytes.get(cursor).is_some_and(u8::is_ascii_alphanumeric) {
                    cursor += 1;
                }
            }
            byte if byte.is_ascii_digit() => {
                cursor = 1;
                while bytes.get(cursor).is_some_and(u8::is_ascii_alphanumeric) {
                    cursor += 1;
                }
            }
            _ => return false,
        }
        if bytes.get(cursor).is_some_and(|byte| !byte.is_ascii_whitespace() && *byte != b'.') {
            return false;
        }
        remainder = &remainder[cursor..];
    }
    true
}

fn letters_only_syntax(value: &str) -> bool {
    !value.is_empty() && value.len() <= 4 && value.bytes().all(|byte| byte.is_ascii_alphabetic())
}

fn letters_only_path(value: &str) -> Option<String> {
    let display = value.trim();
    if display.is_empty() || display.len() > 3 || !display.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        return None;
    }
    let letters = display.to_ascii_uppercase();
    let ranges = outline().ranges_by_main.get(&letters.chars().next()?)?;
    let main = ranges.first()?.path.split(" / ").next()?;
    let minimum_depth = ranges.iter().filter(|range| range.start_letters == letters && range.end_letters == letters).map(|range| range.path.matches(" / ").count()).min();
    let paths = minimum_depth
        .map(|depth| ranges.iter().filter(|range| range.start_letters == letters && range.end_letters == letters && range.path.matches(" / ").count() == depth).map(|range| range.path.as_str()).collect::<BTreeSet<_>>())
        .unwrap_or_default();
    let path = if paths.len() == 1 { paths.into_iter().next().unwrap_or(main) } else { main };
    Some(build_subject_path(path, display))
}

fn build_subject_path(outline_path: &str, display: &str) -> String {
    let labels = outline_path.split(" / ").collect::<Vec<_>>();
    let selected = if labels.len() <= 4 { labels } else { vec![labels[0], labels[1], labels[labels.len() - 2], labels[labels.len() - 1]] };
    let mut components = vec!["LCC".to_owned()];
    for label in selected {
        if components.last().is_none_or(|existing| !existing.eq_ignore_ascii_case(label)) {
            components.push(label.to_owned());
        }
    }
    let display = compact_whitespace(display);
    if components.last().is_none_or(|existing| !existing.eq_ignore_ascii_case(&display)) {
        components.push(display);
    }
    components.join(" / ")
}

pub(super) fn parse_lcc_call(value: &str) -> Option<LccCall<'_>> {
    let value = value.trim();
    let bytes = value.as_bytes();
    let mut cursor = 0;
    // Regional law schedules can use a four-letter subclass (for example
    // KJCX), even though the common LCC classes use one to three letters.
    while cursor < bytes.len() && bytes[cursor].is_ascii_alphabetic() && cursor < 4 {
        cursor += 1;
    }
    if cursor == 0 || bytes.get(cursor).is_some_and(u8::is_ascii_alphabetic) {
        return None;
    }
    let letters = value[..cursor].to_ascii_uppercase();
    while bytes.get(cursor).is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b'\'') {
        cursor += 1;
    }
    let number_start = cursor;
    while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
        cursor += 1;
    }
    if cursor == number_start {
        return None;
    }
    if bytes.get(cursor) == Some(&b'.') {
        cursor += 1;
        let decimal_start = cursor;
        while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
            cursor += 1;
        }
        if cursor == decimal_start {
            cursor -= 1;
        }
    }
    let number = value[number_start..cursor].parse::<f64>().ok()?;
    Some(LccCall { letters, number, display: value, remainder: &value[cursor..] })
}

/// Component boundaries, not textual prefixes: C6 and C65 are distinct.
pub(super) fn lcc_match_keys(value: &str) -> Option<Vec<String>> {
    let canonical = canonical_lcc_notation(value)?;
    if let Some(base) = reviewed_call_base(&canonical) {
        let mut keys = lcc_match_keys(base)?; keys.push(canonical); return Some(keys);
    }
    if let Some(base) = documented_literal_date_base(&canonical) {
        let mut keys = lcc_match_keys(&base)?; keys.push(canonical); return Some(keys);
    }
    if let Some((base, heading, suffix)) = documented_date_call(&canonical) {
        let mut keys = lcc_match_keys(base)?;
        if base == "BX870" { keys.push(heading.split_whitespace().take(2).collect::<Vec<_>>().join(" ")); }
        keys.push(heading.clone());
        if !suffix.is_empty() {
            for key in lcc_match_keys(&format!("{base} {suffix}"))? {
                if let Some(tail) = key.strip_prefix(base).filter(|tail| !tail.is_empty()) {
                    keys.push(format!("{heading} {}", tail.trim_start()));
                }
            }
        }
        if keys.last() != Some(&canonical) { keys.push(canonical); }
        return Some(keys);
    }
    if let Some(base) = range_item_base(&canonical) {
        let mut keys = lcc_match_keys(base)?;
        keys.push(canonical);
        return Some(keys);
    }
    if lcc_code_list(&canonical).is_some() { return Some(vec![canonical]); }
    if let Some(base) = marc_subfield_call_base(&canonical) {
        let mut keys = lcc_match_keys(&base)?;keys.push(canonical);
        return Some(keys);
    }
    if let Some((start, end)) = reference_plus_span(&canonical) {
        let mut keys = if start.code == end.code { lcc_match_keys(&start.code)? }
            else { lcc_match_keys(&format!("{}-{}", start.code, end.code))? };
        keys.push(canonical);return Some(keys);
    }
    if let Some(base) = repeated_lcc_base(&canonical) {
        let mut keys = lcc_match_keys(base)?;
        keys.push(canonical);
        return Some(keys);
    }
    if let Some(base) = terminal_slash_base(&canonical) {
        let mut keys = lcc_match_keys(base)?;
        keys.push(canonical);
        return Some(keys);
    }
    if let Some(base) = qualified_table_family(&canonical) {
        let mut keys = lcc_match_keys(&base)?;keys.push(canonical);
        return Some(keys);
    }
    if let Some(base) = qualified_table_span(&canonical) {
        let mut keys = lcc_match_keys(&base)?;keys.push(canonical);
        return Some(keys);
    }
    if let Some(base) = table_cutter_span(&canonical) {
        let mut keys = lcc_match_keys(&base)?;
        keys.push(canonical);
        return Some(keys);
    }
    if let Some(base) = variable_cutter_span(&canonical) {
        let mut keys = lcc_match_keys(&base)?;
        keys.push(canonical);
        return Some(keys);
    }
    if let Some(class) = minimal_subclass_assignment(&canonical) { return Some(vec![class.to_owned(), canonical]); }
    if let Some(base) = documented_outline_heading(&canonical) { return Some(vec![base.to_owned(), canonical]); }
    if let Some(base) = schedule_table_base(&canonical).or_else(|| display_bracket_base(&canonical)) {
        let mut keys = lcc_match_keys(base)?;
        keys.push(canonical);
        return Some(keys);
    }
    if let Some(base) = reference_collection_base(&canonical) {
        let mut keys = lcc_match_keys(base)?;keys.push(canonical);return Some(keys);
    }
    if let Some(base) = local_cutter_x_base(&canonical).or_else(|| bracketed_year_call_base(&canonical)).or_else(|| doubled_cutter_period_base(&canonical)).or_else(|| period_year_call_base(&canonical)).or_else(|| apostrophe_call_base(&canonical)) {
        let mut keys = lcc_match_keys(&base)?;
        keys.push(canonical);
        return Some(keys);
    }
    if let Some(assignment) = minimal_class_assignment(&canonical) {
        return Some(vec![format!("({assignment})"), canonical]);
    }
    if let Some(base) = lcc_item_base(&canonical) {
        let mut keys = lcc_match_keys(base)?;
        keys.push(canonical);
        return Some(keys);
    }
    let Some(call) = parse_lcc_call(&canonical) else { return Some(vec![canonical]) };
    // Printed schedule spans are atomic selectors, not individual call numbers.
    if call.remainder.contains('-') {
        return Some(vec![canonical]);
    }
    let mut key = canonical[..canonical.len() - call.remainder.len()].replace([' ', '\''], "");
    let mut keys = vec![key.clone()];
    let mut rest = call.remainder;
    while !rest.is_empty() {
        rest = rest.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '.');
        if rest.is_empty() { break; }
        let letters = rest.bytes().take_while(u8::is_ascii_alphabetic).count();
        let digits = rest[letters..].bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            // Preserve earlier topic keys for a single-letter year suffix
            // (1999a), without broadening matching of arbitrary Cutter suffixes.
            let year_suffix = rest.len() == 1 && rest.as_bytes()[0].is_ascii_alphabetic()
                && keys.last().and_then(|key| key.rsplit_once(' '))
                    .is_some_and(|(_, year)| year.len() == 4 && year.bytes().all(|byte| byte.is_ascii_digit()));
            if year_suffix || valid_literature_work_mark(&call) {
                if keys.last() != Some(&canonical) { keys.push(canonical); }
                return Some(keys);
            }
            return Some(vec![canonical]);
        }
        let end = letters + digits;
        key.push(if letters == 0 { ' ' } else { '.' });
        key.push_str(&rest[..end]);
        keys.push(key.clone());
        rest = &rest[end..];
    }
    Some(keys)
}

pub(super) fn compare_lcc_key(left_letters: &str, left_number: f64, right_letters: &str, right_number: f64) -> std::cmp::Ordering {
    left_letters.cmp(right_letters).then_with(|| left_number.total_cmp(&right_number))
}

fn range_contains(range: &LccRange, call: &LccCall<'_>) -> bool {
    compare_lcc_key(&range.start_letters, range.start_number, &call.letters, call.number).is_le() && compare_lcc_key(&call.letters, call.number, &range.end_letters, range.end_number).is_le()
}

fn compare_matches(left: &LccRange, right: &LccRange, call: &LccCall<'_>) -> std::cmp::Ordering {
    let left_exact = left.start_letters == call.letters && left.end_letters == call.letters;
    let right_exact = right.start_letters == call.letters && right.end_letters == call.letters;
    left.path.matches(" / ").count().cmp(&right.path.matches(" / ").count()).then_with(|| left_exact.cmp(&right_exact)).then_with(|| {
        let left_span = if left_exact { left.end_number - left.start_number } else { f64::INFINITY };
        let right_span = if right_exact { right.end_number - right.start_number } else { f64::INFINITY };
        right_span.total_cmp(&left_span)
    })
}

#[cfg(test)]
mod tests {
    use super::canonical_lcc_notation;

    #[test]
    fn canonical_lcc_rejects_storage_markers_and_malformed_trailing_text() {
        for invalid in ["CPB", "CPB Box no. 1574", "MLCS", "MLCS 2021/45060", "LAW+", "LCC", "Comic", "Microfiche 82/7", "QA76 BOX NO. 4", "QA76, PN1995"] {
            assert_eq!(canonical_lcc_notation(invalid), None, "accepted invalid LCC value {invalid}");
        }
    }

    #[test]
    fn canonical_lcc_accepts_complete_call_numbers_and_ranges() {
        for valid in ["ML", "QA76.73.R87", "PS3558.O3447 D68 2015", "KJCX 12.4 .A2", "HF4999.2-6182", "PN1-PN6790", "PN1993-PN1999"] {
            assert!(canonical_lcc_notation(valid).is_some(), "rejected valid LCC value {valid}");
        }
    }

    #[test]
    fn pz_title_work_letters_preserve_author_keys() {
        for code in ["PZ7.R79835 Ham 1999", "PZ7.R79835 Halm 2003", "PZ7.T47 Wh", "PZ7.B1387 Lo 1988a"] {
            assert!(canonical_lcc_notation(code).is_some(), "{code}");
            let keys = super::lcc_match_keys(code).unwrap();
            assert_eq!(keys[0], "PZ7");
            assert!(keys[1].starts_with("PZ7."));
            assert_eq!(keys.last(), Some(&code.to_ascii_uppercase()));
        }
        for code in ["PZ7 Wh 1999", "PZ7.R79835 BOX NO. 4", "PZ7.R79835 Wh 12", "PZ7.R79835 Wh 1999 extra", "QA76.R79835 Ham 1999"] {
            assert!(canonical_lcc_notation(code).is_none(), "{code}");
        }
    }

    #[test]
    fn numbered_item_suffixes_preserve_complete_notations_and_base_keys() {
        assert!(canonical_lcc_notation("PZ7.T47 Wh vol. 2").is_some());
        for suffix in ["VOL. 2", "V.2", "VOL. 4, PT. 1", "NO. 1744", "VOL. 12A-13B", "V.119:26", "SECT. 2 SESS. 1", "SUPPL.", "SUPPL", "SUPPL. 2", "VOL. 1 SUPPL."] {
            let code = format!("QA3 .L28 {suffix}");
            assert_eq!(canonical_lcc_notation(&code), Some(code.clone()));
            assert_eq!(super::lcc_match_keys(&code).unwrap(), vec!["QA3".to_owned(), "QA3.L28".to_owned(), code]);
        }
        for code in ["QA3 .L28 VOL. XBOX", "QA3 .L28 VOL. 1,", "QA3 .L28 VOL. 1 BOX 2", "QA3 .L28 VOL. 1-QB2", "QA3 BAD TEXT VOL. 1", "CPB VOL. 2", "QA3 .L28 SUPPLEMENT NOTES", "QA3 .L28 SUPPL. UNKNOWN"] {
            assert!(canonical_lcc_notation(code).is_none(), "{code}");
        }
    }

    #[test]
    fn custodial_notes_preserve_valid_classification_keys() {
        for note in ["<Orien China>", "<Asian Japan>", "<HEBR>", "(Orien China)", "<AMED Arab>", "<Asian China Cage>"] {
            let code = format!("PL2780.F4 1981b {note}");
            let canonical = canonical_lcc_notation(&code).unwrap();
            assert_eq!(canonical, code.to_ascii_uppercase());
            let keys = super::lcc_match_keys(&code).unwrap();
            assert!(keys.contains(&"PL2780.F4".to_owned()));
            assert_eq!(keys.last(), Some(&canonical));
        }
        assert_eq!(super::lcc_item_base("QA3 .L28 VOL. 2 <ASIAN CHINA>"), Some("QA3 .L28"));
        assert_eq!(super::lcc_item_base("QA3 .L28 SUPPL. <ASIAN CHINA>"), Some("QA3 .L28"));
        for code in ["PL2780.F4 <UNKNOWN>", "PL2780.F4 <ASIAN UNKNOWN>", "PL2780.F4 <ASIAN CHINA> EXTRA", "BAD TEXT <ORIEN CHINA>", "CPB <ORIEN CHINA>", "PL2780.F4 <ASIAN CHINA)"] {
            assert!(canonical_lcc_notation(code).is_none(), "{code}");
        }
    }

    #[test]
    fn minimal_class_assignment_parentheses_delimit_the_final_letter() {
        for (code, assignment) in [("MLCL 2006/00304(B)", "(B)"),
            ("MLCM 2006/00248(R)", "(R)"), ("MICROFILM (O) 85/4003(P)", "(P)")] {
            assert_eq!(canonical_lcc_notation(code).as_deref(), Some(code));
            assert_eq!(super::lcc_match_keys(code).unwrap(), vec![assignment.to_owned(), code.to_owned()]);
            let spaced = code.replace(assignment, &format!(" {assignment}"));
            let a = super::lcc_subject_path(code).unwrap();
            let b = super::lcc_subject_path(&spaced).unwrap();
            assert_eq!(a.rsplit_once(" / ").unwrap().0, b.rsplit_once(" / ").unwrap().0);
        }
        for code in ["MLCL 2006/00304(ZZ)", "MLCL 2006/00304(B) NOTE", "MLCL 2006/00304(B)(P)", "MLCL 2006/00304X(B)", "MLCL 2006/00304 (O)(B)"] {
            assert!(canonical_lcc_notation(code).is_none(), "{code}");
        }
    }

    #[test]
    fn cutter_separator_spacing_keeps_the_same_components() {
        for (code, base) in [("TJ217.2. C48 2000", "TJ217.2.C48 2000"),
            ("QA76 . A1 . B2 1999", "QA76.A1.B2 1999"), ("AC106 . A24 2013", "AC106.A24 2013")] {
            assert_eq!(canonical_lcc_notation(code).as_deref(), Some(code));
            assert_eq!(super::lcc_match_keys(code), super::lcc_match_keys(base));
        }
        for code in ["PZ7 . R79835 HAM 1999", "PR6045 . R35 DC"] {
            assert_eq!(canonical_lcc_notation(code).as_deref(), Some(code));
            assert!(super::lcc_match_keys(code).unwrap().len() >= 3);
        }
        for code in ["QA76 . 123", "QA76 . ", "QA76 . A1 UNKNOWN"] {
            assert!(canonical_lcc_notation(code).is_none(), "{code}");
        }
    }

    #[test]
    fn plus_markers_keep_the_widest_documented_scope() {
        let (start, end) = super::reference_plus_span("PK2916+").unwrap();
        assert_eq!(start.code, "PK2916");assert_eq!(end.code, "PK2947");
        let (start, end) = super::reference_plus_span("IN PROCESS [F123+]").unwrap();
        assert_eq!(start.code, "F123");assert_eq!(end.code, "F123");
        assert_eq!(super::lcc_match_keys("PK2916+").unwrap(), vec![canonical_lcc_notation("PK2916-PK2947").unwrap(), "PK2916+".to_owned()]);
        assert!(canonical_lcc_notation("[F123+]").is_some());
        for code in ["PK2916++", "PK2916+ EXTRA", "IN PROCESS F123+", "IN PROCESS [UNKNOWN+]", "BG1068+", "QA99999999+", "LAW+", "MLCS+"] {
            assert!(super::reference_plus_span(code).is_none(), "{code}");
        }
    }

    #[test]
    fn historical_pz_single_title_letters_keep_author_evidence() {
        for (code, class, cutter) in [("PZ7.S268 E", "PZ7", "PZ7.S268"),
            ("PZ7.C54 A", "PZ7", "PZ7.C54"), ("PZ10.831.G586 E 1998", "PZ10.831", "PZ10.831.G586")] {
            assert_eq!(canonical_lcc_notation(code).as_deref(), Some(code));
            assert_eq!(super::lcc_match_keys(code).unwrap(), vec![class.to_owned(), cutter.to_owned(), code.to_owned()]);
        }
        for code in ["PZ7 E", "PZ7.S E", "PZ7.S268 E EXTRA", "PZ7.S268 E 19", "PR4552 S", "PS648.S3 B", "QA76.S268 E"] {
            assert!(canonical_lcc_notation(code).is_none(), "{code}");
        }
    }

    #[test]
    fn series_enumeration_preserves_complete_call_and_evidence() {
        let base = "QA3 .A572";
        for suffix in ["SER. 2, VOL. 130", "SER.2 VOL.130", "SERIES 2:V.130", "SER. II, VOL. 3", "NEW SER. 2, VOL. 4", "NEW SERIES 2 VOL. 4", "NEW SER.:V.1", "NEW SERIES, VOL. 1", "NEW SER."] {
            let code = format!("{base} {suffix}");
            assert_eq!(super::lcc_item_base(&code), Some(base), "{code}");
            let mut keys = super::lcc_match_keys(base).unwrap();keys.push(code.clone());
            assert_eq!(super::lcc_match_keys(&code), Some(keys));
        }
        for suffix in ["SER.", "SER. 2,", "SERIES UNKNOWN", "SER. 2 EXTRA", "NEW SER. UNKNOWN", "NEW SERIES,", "SER. 2 QA76", "SER. IIV", "SERIAL 2"] {
            let code = format!("{base} {suffix}");
            assert!(super::lcc_item_base(&code).is_none(), "{code}");
        }
    }

    #[test]
    fn qualified_table_spans_require_documented_ranges_and_complete_cutters() {
        let expected = canonical_lcc_notation("PR9499.3.S53.Z5-PR9499.3.S53.Z999");
        for note in ["P-PZ40:.XZ5-.XZ999", "P-PZ40: .XZ5-Z999", "P-PZ40:.XZ5-.Z999"] {
            let code = format!("PR9499.3.S53 ({note})+");
            assert_eq!(super::qualified_table_span(&code), expected);
            assert_eq!(canonical_lcc_notation(&code).as_deref(), Some(code.as_str()));
        }
        for code in ["PR9499.3.S (P-PZ40:.XZ5-.XZ999)+", "PR9499.3 (P-PZ40:.XZ5-.XZ999)+", "PR9499.3.S53 (UNKNOWN:.XZ5-.XZ999)+", "PR9499.3.S53 (P-PZ40:.XZ999-.XZ5)+", "PR9499.3.S53 (P-PZ40:.XZ5-.XZ998)+", "PR9499.3.S53 (P-PZ40:.XZ5-.XZ999 EXTRA)+", "PR9499.3.S53 (P-PZ40:.X)+"] {
            assert!(super::qualified_table_span(code).is_none(), "{code}");
        }
    }

    #[test]
    fn used_reference_endpoints_retain_full_documented_scopes() {
        for (code, expected_end) in [("BP65.A1+", "BP65.Z"), ("GV1131+", "GV1132"), ("PN1991.77.A1+", "PN1991.77.Z")] {
            let (_, end) = super::reference_plus_span(code).unwrap();
            assert_eq!(end.code, expected_end);
        }
    }

    #[test]
    fn mineralogy_reference_scopes_retain_qe_subclass() {
        for (code, expected_end) in [("QE372+", "QE387"), ("QE373+", "QE385.6"), ("QE377.7.A1+", "QE377.7.Z"), ("QE385.6.A1+", "QE385.6.Z")] {
            let (_, end) = super::reference_plus_span(code).unwrap();
            assert_eq!(end.code, expected_end);
        }
    }

    #[test]
    fn reviewed_printed_endpoints_retain_class_decimals_and_cutters() {
        for (code, expected_end) in [("B4165.M49+", "B4165.M494"), ("BM520.88.A3+", "BM520.88.A39"), ("B605.P5+", "B605.P54"), ("NX284.4.A+", "NX284.4.Z")] {
            let (_, end) = super::reference_plus_span(code).unwrap();
            assert_eq!(end.code, expected_end);
        }
    }

    #[test]
    fn reviewed_hydrology_scopes_do_not_cross_into_recreation() {
        for (code, expected_end) in [("GB1016+", "GB1016.6"), ("GB1219+", "GB1223"), ("GB2819+", "GB2823")] {
            let (start, end) = super::reference_plus_span(code).unwrap();
            assert_eq!(start.letters, "GB");
            assert_eq!(end.code, expected_end);
        }
    }

    #[test]
    fn table_initials_retain_family_without_inventing_author_digits() {
        for code in ["PR9499.3.S (P-PZ40:.XZ5-.XZ999)+", "PR9499.3.S.XZ5-Z999 (P-PZ40)+"] {
            assert_eq!(super::qualified_table_family(code).as_deref(), Some("PR9499.3.S"));
            assert_eq!(canonical_lcc_notation(code).as_deref(), Some(code));
            assert!(super::qualified_table_span(code).is_none());
        }
        for code in ["PR9499.3.S (UNKNOWN:.XZ5-.XZ999)+", "PR9499.3.S (P-PZ40:.XZ5-.XZ998)+", "PR9499.3.S (P-PZ40:.XZ999-.XZ5)+", "PR9499.3.S.A (P-PZ40:.XZ5-.XZ999)+", "PR9499.3 (P-PZ40:.XZ5-.XZ999)+"] {
            assert!(super::qualified_table_family(code).is_none(), "{code}");
        }
    }

    #[test]
    fn table_ranges_allow_repeated_prefixes_and_terminal_initials() {
        for (code, span) in [
            ("B133.S5A6-S5Z (B-BJ5)+", "B133.S5.A6-B133.S5.Z"),
            ("BL1280.292.R36A6-Z (BL4)+", "BL1280.292.R36.A6-BL1280.292.R36.Z"),
            ("BL1142.6.M3642A-Z 2004 (BL3)+", "BL1142.6.M3642.A-BL1142.6.M3642.Z"),
        ] {
            assert_eq!(super::table_cutter_span(code), canonical_lcc_notation(span), "{code}");
        }
        for code in ["B133.S5A6-S6Z (B-BJ5)+", "B133.S5A6-S5Z (UNKNOWN)+",
            "B133.S5A6-S5Z 200 (B-BJ5)+", "B133.S5A6-S5Z 2004 EXTRA (B-BJ5)+",
            "B133.S5A6-S5A2 (B-BJ5)+", "B133.S.A6-Z (B-BJ5)+",
            "B133.S5A6-D1 (B-BJ5)+", "K1.A1-Z (B-BJ5)+"] {
            assert!(super::table_cutter_span(code).is_none(), "{code}");
        }
    }

    #[test]
    fn preliminary_plus_initials_require_complete_reference_scopes() {
        for code in ["HV6433.A-Z+", "HV6433.A-ZI+", "HV6433.A-.ZI+", "HV6433.A-ZI4+", "HV6433.A-ZI"] {
            assert_eq!(super::documented_preliminary_plus_span(code), canonical_lcc_notation("HV6433.A-HV6433.Z"));
        }
        for code in ["HV6433.A-ZI++", "HV6433.A-ZI+ EXTRA",
            "HV6433.A2-ZI+", "HV6433.J-ZI+", "QA999999.A-ZI+",
            "HV6433.A-ZI.X+", "HV6433.A-ZI1A+", "K1.A-ZI+"] {
            assert!(super::documented_preliminary_plus_span(code).is_none(), "{code}");
        }
    }

    #[test]
    fn unqualified_placeholders_require_published_scope_and_position() {
        for code in ["HV6433.A-Z.X+", "HV6433.A-ZI.X+", "HV6433.A-ZI.X", "HD1289.A-ZI.X+"] {
            let class = if code.starts_with("HV") { "HV6433" } else { "HD1289" };
            assert_eq!(super::documented_placeholder_span(code), canonical_lcc_notation(&format!("{class}.A-{class}.Z")));
        }
        for code in ["HV6433.A-ZI.X99+", "HV6433.A2-ZI.X+", "HV6433.A-ZI.X+ EXTRA",
            "HV6433.A-ZI.XX+", "HV6433.A-ZI.X++", "QA999999.A-ZI.X+",
            "HV6433.A-ZI.X2A+", "HV6433.A-ZI.X (UNKNOWN)+", "K1.A-ZI.X+"] {
            assert!(super::documented_placeholder_span(code).is_none(), "{code}");
        }
    }

    #[test]
    fn numeric_qualified_operations_extend_complete_cutter_digits() {
        for (code, span) in [
            ("PN5499.A4 (PN4:.X2A6-.X2Z)+", "PN5499.A42.A6-PN5499.A42.Z"),
            ("PN5499.A40 (PN4:.X2A6-.X2Z)+", "PN5499.A402.A6-PN5499.A402.Z"),
            ("PN5499.T8.X4A-.X4Z (PN4)+", "PN5499.T84.A-PN5499.T84.Z"),
        ] { assert_eq!(super::qualified_table_span(code), canonical_lcc_notation(span), "{code}"); }
        assert_eq!(super::qualified_table_family("B5134.K (B-BJ5:.X4A6-.X4Z)+").as_deref(), Some("B5134.K"));
        assert!(super::qualified_table_span("B5134.K (B-BJ5:.X4A6-.X4Z)+").is_none());
        for code in ["PN5499 (PN4:.X2A6-.X2Z)+", "PN5499.A4 (PN4:.X2A6-.X3Z)+", "PN5499.A4 (UNKNOWN:.X2A6-.X2Z)+"] {
            assert!(super::qualified_table_span(code).is_none(), "{code}");
        }
    }

    #[test]
    fn numeric_nested_operations_keep_outer_scope() {
        for code in ["HT169.A-ZI.X2A-.X2ZD (H73)+", "HT169.A-ZI.X2A-.X2Z (H73)+",
            "HT169.A-ZI.X2A-2ZD (H73)+"] {
            assert_eq!(super::variable_cutter_span(code), canonical_lcc_notation("HT169.A-HT169.Z"));
        }
        for code in ["HT169.A-ZI.X2A-.X3ZD (H73)+", "HT169.A-ZI.X2A-.X2ZD (H15)+",
            "HT169.A-ZI.X2A-.X2ZD EXTRA (H73)+", "HT169.A-ZI.X2A-.X2ZD.X9 (H73)+",
            "HT169.A-ZI.X2A-.X2MD (H73)+"] {
            assert!(super::variable_cutter_span(code).is_none(), "{code}");
        }
    }

    #[test]
    fn nested_relative_operations_preserve_unresolved_outer_interval() {
        for code in ["BL1175.A-Z.XA6-.XZ (BL4)+", "BL1175.A-ZG .XA6-.XZ (BL4)+",
            "BL1175.A-ZG.XA6-.XZ (BL4)+", "BL1175.A-ZG.XA6-Z (BL4)+",
            "BL1175.A-ZG.XA6-.Z (BL4)+", "BL2017.85.A-ZG.XA27-.XA279 (BL4)+"] {
            let class = if code.starts_with("BL1175") { "BL1175" } else { "BL2017.85" };
            assert_eq!(super::variable_cutter_span(code), canonical_lcc_notation(&format!("{class}.A-{class}.Z")), "{code}");
        }
        for code in ["BL1175.A-ZG.XA6-.X7 (BL4)+", "BL1175.A-ZG.XA6-.XZ EXTRA (BL4)+",
            "BL1175.A-ZG.XA6-.XZ (UNKNOWN)+", "BL1175.A-ZG.XA6-.XZ.X2 (BL4)+",
            "BL1175.A-ZG.XA6-.XZ (DS7)+"] {
            assert!(super::variable_cutter_span(code).is_none(), "{code}");
        }
    }

    #[test]
    fn historical_biography_tables_require_applicable_scope_and_explicit_operation() {
        for table in ["DS7", "DSVII", "DS-DXVII", "VII", "TABLE VII"] {
            let code = format!("DS481.G3 ({table}:.XA6-Z)+");
            assert_eq!(super::qualified_table_span(&code), canonical_lcc_notation("DS481.G3.A6-DS481.G3.Z"));
        }
        assert_eq!(super::qualified_table_span("DS481.G3.XA4 (DS7)+"), canonical_lcc_notation("DS481.G3.A4-DS481.G3.A4"));
        assert_eq!(super::qualified_table_family("DS481.G (DS7:.XA6-.XZ)+").as_deref(), Some("DS481.G"));
        for code in ["DS481.G3 (DS-DX7:.XA6-.XZ)+", "DS481.G3 (UNKNOWN:.XA6-.XZ)+",
            "DS582.G3 (DS7:.XA6-.XZ)+", "DS481.A1 (DS7:.XA6-.XZ)+",
            "DS481.A (DS7:.XA6-.XZ)+", "DS481.G3.A2 (DS7:.XA6-.XZ)+",
            "DS481.G3 (DS7:.XA6-.X7)+", "DS481.G3 (DS7)+"] {
            assert!(super::qualified_table_span(code).is_none(), "{code}");
            assert!(super::qualified_table_family(code).is_none(), "{code}");
        }
    }

    #[test]
    fn preliminary_ranges_require_identical_repeated_prefixes() {
        for (code, span) in [
            ("BL1140.4.B435A-.B435ZC 2008 (BL3)+", "BL1140.4.B435.A-BL1140.4.B435.Z"),
            ("HD8039.T182A-.T182ZI (H50)+", "HD8039.T182.A-HD8039.T182.Z"),
            ("BQ1300.M363A-.M363ZS (BQ3)+", "BQ1300.M363.A-BQ1300.M363.Z"),
        ] { assert_eq!(super::variable_cutter_span(code), canonical_lcc_notation(span), "{code}"); }
        for code in ["HD8039.T182A-.T183ZI (H50)+", "HD8039.T182A-.T18ZI (H50)+",
            "HD8039.T182A-.T182ZI EXTRA (H50)+", "HD8039.T182A-.T182ZI (UNKNOWN)+",
            "HD8039.T182A-.T182ZI 200 (H50)+", "HD8039.T182A-.T182Z (H50)+"] {
            assert!(super::variable_cutter_span(code).is_none(), "{code}");
        }
    }

    #[test]
    fn preliminary_spans_keep_nested_cutters_and_allow_complete_years() {
        for code in ["BL1140.4.B432A-ZE (BL3)+", "BL1140.4.B432.A-ZE 2009 (BL3)+", "BL1140.4.B432A-.ZE 2009 (BL3)+"] {
            assert_eq!(super::variable_cutter_span(code), canonical_lcc_notation("BL1140.4.B432.A-BL1140.4.B432.Z"));
        }
        assert_eq!(super::variable_cutter_span("BL1112.24.A-ZU 2009 (BL1)+"), canonical_lcc_notation("BL1112.24.A-Z"));
        for code in ["BL1140.4.B.A-ZE 2009 (BL3)+", "BL1140.4.B432A-ZE 200 (BL3)+", "BL1140.4.B432A-ZE 2009 EXTRA (BL3)+", "BL1140.4.B432A-ZE 2009 (UNKNOWN)+", "BL1140.4.B432A-ZE 20X9 (BL3)+"] {
            assert!(super::variable_cutter_span(code).is_none(), "{code}");
        }
    }

    #[test]
    fn inline_table_spans_use_the_same_documented_interval() {
        let expected = canonical_lcc_notation("PL4718.9.G678.Z5-PL4718.9.G678.Z999");
        for relative in [".XZ5-Z999", ".XZ5-.XZ999", ".XZ5-.Z999"] {
            let code = format!("PL4718.9.G678{relative} (P-PZ40)+");
            assert_eq!(super::qualified_table_span(&code), expected);
            assert_eq!(canonical_lcc_notation(&code).as_deref(), Some(code.as_str()));
        }
        for code in ["PL4718.9.G.XZ5-Z999 (P-PZ40)+", "PL4718.9.G678.XZ5-Z998 (P-PZ40)+", "PL4718.9.G678.XZ5-Z999 (UNKNOWN)+", "PL4718.9.G678.XZ999-Z5 (P-PZ40)+", "PL4718.9.G678.XZ5-Z999 EXTRA (P-PZ40)+", "PL4718.9.G678.XZ5-Z999 (P-PZ50)+"] {
            assert!(super::qualified_table_span(code).is_none(), "{code}");
        }
    }

    #[test]
    fn explicit_lists_require_complete_known_components() {
        assert_eq!(super::lcc_code_list("[qa76; QC1]").unwrap(), vec!["QA76", "QC1"]);
        assert_eq!(super::lcc_subject_paths("QA76; QC1").len(), 2);
        assert!(super::lcc_code_list("QA76; $AB43").is_some());
        assert!(super::lcc_code_list("QA76; [B43]").is_some());
        assert!(super::lcc_code_list("QA76; E78.2").is_some());
        assert!(super::lcc_subject_path("QA76; QC1").is_none());
        assert!(super::lcc_subject_paths("QA76; prose").is_empty());
        for code in ["QA76;", "QA76; prose", "QA76; QX1", "QA76; QC", "QA76; QC1 truncated", "BF175; B43", "B126; F8", "QA76; K1", "PL4758.85.A-ZS.x (P-PZ25; P-PZ40)+"] {
            assert!(super::lcc_code_list(code).is_none(), "{code}");
        }
    }

    #[test]
    fn marc_markers_keep_class_and_item_and_reject_alternatives() {
        for (code, base) in [("$AQA76.9$B.A25 2008", "QA76.9 .A25 2008"), ("E99.S45$BC76 1996", "E99.S45 C76 1996"), ("$A RC951", "RC951"), ("CS71.C323$B1977", "CS71.C323 1977")] {
            assert_eq!(super::marc_subfield_call_base(code).as_deref(), Some(base));
            assert_eq!(canonical_lcc_notation(code).as_deref(), Some(code));
            assert_eq!(super::lcc_match_keys(code).unwrap().last().unwrap(), code);
        }
        for code in ["QA76$B", "$B.A25", "$AQA76$B.A25$AQC1", "QA76$B.A25$B.A26", "QA76$C.A25", "$AQA76$0ID", "QA76$BUNKNOWN WORDS", "$AK1$0ID", "QA1-10$B.A25"] {
            assert!(super::marc_subfield_call_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn equivalent_numeric_repetitions_require_every_copy_to_be_complete() {
        for (code, first) in [("HD28-70HD28-HD70", "HD28-70"), ("HD28-HD70HD28-70", "HD28-HD70"), ("HD28-70HD28-HD70HD28-70", "HD28-70"), ("QA75.5-76.95QA75.5-QA76.95", "QA75.5-76.95")] {
            assert_eq!(super::repeated_lcc_base(code), Some(first));
            assert_eq!(canonical_lcc_notation(code).as_deref(), Some(code));
        }
        for code in ["HD28-70HD28-HD7", "HD28-70HD28-HD70HD28", "HD28-70HD28-HD71", "HD28-70HF4999-6182", "BG1-10BG1-BG10", "HD70-28HD70-HD28"] {
            assert!(super::repeated_lcc_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn exact_repetitions_require_complete_valid_units() {
        for base in ["HD28-70", "QA76.9.A25", "HM401-1281", "BF1-990"] {
            for count in [2, 3, 4] {
                let code = base.repeat(count);
                assert_eq!(super::repeated_lcc_base(&code), Some(base));
                assert_eq!(canonical_lcc_notation(&code).as_deref(), Some(code.as_str()));
                let mut keys = super::lcc_match_keys(base).unwrap();keys.push(code.clone());
                assert_eq!(super::lcc_match_keys(&code), Some(keys));
            }
        }
        for code in ["HD28-70HD28-7", "HD28-70HF4999-6182", "QB1-991QB4QB1-991QB4", "W5W5", "BG1068BG1068", "QA76.AQA76.A", "HD70-28HD70-28", "HD28-70HD28-70X"] {
            assert!(super::repeated_lcc_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn sleep_outline_uses_bf_and_does_not_register_bg() {
        let subclasses = super::outline_subclasses();
        assert!(subclasses.contains("BF"));
        assert!(!subclasses.contains("BG"));
        for code in ["BF1068", "BF1071", "BF1073.S58", "BF1068-BF1073"] {
            assert!(super::lcc_subject_path(code).unwrap().contains("Parapsychology / Sleep. Somnambulism"), "{code}");
        }
        for code in ["BF1067", "BF1074"] {
            assert!(!super::lcc_subject_path(code).unwrap().contains("Sleep. Somnambulism"), "{code}");
        }
        assert!(super::lcc_subject_path("BG1068").is_none());
        assert!(canonical_lcc_notation("MLCS 2000/1234 (BG)").is_none());
        assert!(canonical_lcc_notation("MLCS 2000/1234 (BF)").is_some());
    }

    #[test]
    fn table_cutter_ranges_retain_all_preceding_components() {
        for (code, span) in [("PL4718.9.B33Z5-Z999 (P-PZ40)+", "PL4718.9.B33.Z5-PL4718.9.B33.Z999"),
            ("PL4718.9.B33.Z5-.Z999 (P-PZ40)+", "PL4718.9.B33.Z5-PL4718.9.B33.Z999"),
            ("HC425.A5-Z6 (H17)+", "HC425.A5-HC425.Z6")] {
            assert_eq!(super::table_cutter_span(code), canonical_lcc_notation(span));
            let mut keys = super::lcc_match_keys(span).unwrap();keys.push(code.to_owned());
            assert_eq!(super::lcc_match_keys(code), Some(keys));
        }
        for code in ["PL4718.9.B33Z999-Z5 (P-PZ40)+", "PL4718.9.B33Z5-Z999 (UNKNOWN)+", "PL4718.9.B33Z5-QA999 (P-PZ40)+", "PL4718.9.B33Z5-Z999 EXTRA (P-PZ40)+", "PL4718.9.B33Z5-Z999"] {
            assert!(super::table_cutter_span(code).is_none(), "{code}");
        }
    }

    #[test]
    fn terminal_slashes_keep_only_valid_class_and_cutter_prefixes() {
        for code in ["PR6023.A93Z/", "DA690.S/", "AM101.B85/", "QA76/", "QA76 . A1/"] {
            assert_eq!(canonical_lcc_notation(code).as_deref(), Some(code));
            assert_eq!(super::terminal_slash_base(code), code.strip_suffix('/'));
            assert_eq!(super::lcc_match_keys(code).unwrap().last().map(String::as_str), Some(code));
        }
        for code in ["QA76//", "QA76/A1/", "QA76.UNKNOWN/", "QA76.A1 1999/", "QA76.A1 VOL. 2/", "QA76./", "QA76.A1 EXTRA/", "QA76-QA80/", "CPB/"] {
            assert!(super::terminal_slash_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn reviewed_unmarked_preliminaries_preserve_both_endpoints_and_evidence() {
        for row in include_str!("../data/lcc-reviewed-preliminary-spans.tsv").lines() {
            let (raw, span) = row.split_once('\t').unwrap();
            assert_eq!(super::reviewed_preliminary_span(raw), canonical_lcc_notation(span));
            assert_eq!(canonical_lcc_notation(raw).as_deref(), Some(raw));
            assert!(super::lcc_match_keys(raw).unwrap().contains(&raw.to_owned()));
        }
        for raw in ["PK1716.5.A-ZF", "PK1716.5.A-ZE EXTRA", "PK1717.5.A-ZE", "PK2598.A3-Z5.Q", "K1716.5.A-ZE"] {
            assert!(super::reviewed_preliminary_span(raw).is_none(), "{raw}");
        }
    }

    #[test]
    fn reviewed_call_bases_preserve_original_evidence_and_exact_boundaries() {
        let raw = "PA85.G 646A3 date";
        assert_eq!(canonical_lcc_notation(raw).as_deref(), Some("PA85.G 646A3 DATE"));
        let mut keys = super::lcc_match_keys("PA85.G646.A3").unwrap(); keys.push(raw.to_ascii_uppercase());
        assert_eq!(super::lcc_match_keys(raw), Some(keys));
        for value in ["PA85.G 647A3 DATE", "PA85.G 646A4 DATE", "PA85.G 646A3 DATE EXTRA", "PA85.G 646A3 2007"] {
            assert!(super::reviewed_call_base(value).is_none(), "{value}");
        }
    }

    #[test]
    fn literal_dates_require_a_documented_complete_base() {
        for (value, base) in [("BS195 .E64 DATE", "BS195.E64"), ("PE1119.A1N5 DATE", "PE1119.A1.N5"),
            ("PA3862.A3 DATE", "PA3862.A3")] {
            assert_eq!(super::documented_literal_date_base(value).as_deref(), Some(base));
            assert_eq!(canonical_lcc_notation(value).as_deref(), Some(value));
            let keys = super::lcc_match_keys(value).unwrap();
            assert!(keys.contains(&base.to_owned())); assert_eq!(keys.last().map(String::as_str), Some(value));
        }
        for value in ["BS195.E642 DATE", "BS195.E6 DATE", "PA85.G 646A3 DATE", "QA76 DATE", "KZ179.5.H2 DATE",
            "BS195.E64 2005 DATE", "BS195.E64 DATE EXTRA", "BS195.E64 DATE DATE"] {
            assert!(super::documented_literal_date_base(value).is_none(), "{value}");
        }
    }

    #[test]
    fn chronological_calls_validate_dates_and_preserve_complete_items() {
        for (input, expected) in [("BX870 1978 October .S65 2012", "BX870 1978 OCT. .S65 2012"),
            ("DC240 jun. 22 B47 2011", "DC240 JUNE 22 B47 2011"),
            ("BX870 2000 Feb. 29", "BX870 2000 FEB. 29")] {
            assert_eq!(canonical_lcc_notation(input).as_deref(), Some(expected));
            assert!(super::lcc_subject_path(input).is_some());
        }
        for input in ["BX870 1900 FEB. 29", "BX870 1978A OCT.", "DC240 APRIL 31", "DC240 JUNE 0",
            "DC240 JUNE", "BX870 1978 OCT..", "BX871 1978 OCT.", "K870 1978 OCT.",
            "DC240 JUNE 22 .B", "DC240 JUNE 22 .B47 UNKNOWN"] {
            assert!(super::documented_date_call(input).is_none(), "{input}");
        }
    }

    #[test]
    fn preliminary_initials_preserve_the_whole_declared_cutter_span() {
        for (code, span) in [("HC437.A-ZM (H15)+", "HC437.A-Z"),
            ("HC440.A-ZE5 (H15)+", "HC440.A-Z"), ("HC437.A-.ZM (H15)+", "HC437.A-Z")] {
            assert_eq!(super::variable_cutter_span(code), canonical_lcc_notation(span));
            let mut keys = super::lcc_match_keys(span).unwrap();
            keys.push(code.to_owned());
            assert_eq!(super::lcc_match_keys(code), Some(keys));
        }
        for code in ["HC437.N-ZM (H15)+", "HC437.A-ZMAN (H15)+", "HC437.A-ZM33X (H15)+", "HC437.A-ZM", "HC437.A-ZM (UNKNOWN)+", "HC437.A-ZM EXTRA (H15)+"] {
            assert!(super::variable_cutter_span(code).is_none(), "{code}");
        }
    }

    #[test]
    fn variable_cutter_templates_preserve_the_full_declared_span() {
        assert_eq!(super::variable_cutter_span("HD2346.A-ZI.X"), canonical_lcc_notation("HD2346.A-HD2346.Z"));
        assert_eq!(super::schedule_table_base("HD2346.A-ZI.X (H73)+"), Some("HD2346.A-ZI.X"));
        for (code, span) in [("HD2346.A-ZI.X (H73)+", "HD2346.A-Z"),
            ("HD9502.A4-ZI.X2 (H20)+", "HD9502.A4-Z")] {
            assert_eq!(canonical_lcc_notation(code).as_deref(), Some(code));
            let mut keys = super::lcc_match_keys(span).unwrap();
            keys.push(code.to_owned());
            assert_eq!(super::lcc_match_keys(code), Some(keys));
        }
        for code in ["HD2346.J-ZI.X (H73)+", "HD2346.A-ZI.X EXTRA (H73)+", "HD2346.A-ZI.X (UNKNOWN)+", "HD2346.A-ZI.X2A (H73)+", "HD2346.A-ZI.X99"] {
            assert!(super::variable_cutter_span(code).is_none(), "{code}");
        }
    }

    #[test]
    fn mlc_delimiter_spacing_retains_digit_and_assignment_boundaries() {
        for code in ["MLCM 2006/05417 ( H)", "MLCM 2006/05417 (H )", "MLCM 2006 / 05417 ( H )", "MLCM 2006/ 05417(H)", "MLCM 2006 /05417 (H)"] {
            assert_eq!(canonical_lcc_notation(code).as_deref(), Some(code));
            assert_eq!(super::lcc_match_keys(code).unwrap(), vec!["(H)", code]);
            assert!(super::lcc_subject_path(code).is_some());
        }
        let code = "MLCF 2013 / 40001 ( PQ )";
        assert_eq!(super::lcc_match_keys(code).unwrap(), vec!["PQ", code]);
        for code in ["MLCM 20 06/05417 (H)", "MLCM 2006/05 417 (H)", "MLCM 2006//05417 (H)", "MLCM 2006/ (H)", "MLCM 2006/05417 EXTRA (H)", "MLCM 2006/05417 (P Q)"] {
            assert!(canonical_lcc_notation(code).is_none(), "{code}");
        }
    }

    #[test]
    fn completed_mlc_subclass_assignments_preserve_explicit_subclass() {
        for (code, class) in [("MLCF 2013/40001 (PQ)", "PQ"), ("MLCM 2021/40683(GV)", "GV"), ("MLCMN 97/144 (PJ)", "PJ")] {
            assert_eq!(canonical_lcc_notation(code).as_deref(), Some(code));
            assert_eq!(super::lcc_match_keys(code).unwrap(), vec![class, code]);
            assert_eq!(super::lcc_subject_path(code), super::lcc_subject_path(class));
        }
        for code in ["MLCF 2013/40001 (ZZ)", "MLCF 2013/40001 (MN)", "MLCF 2013/40001 (SR)", "MLCF 2013/40001 (LAW)", "MLCF 2013/40001 (PQ) EXTRA", "MLCX 2013/40001 (PQ)", "MLCF 2013/ (PQ)", "MLCXXX (PQ)", "MLCF 2013/40001 (PQ123)"] {
            assert!(canonical_lcc_notation(code).is_none(), "{code}");
        }
    }

    #[test]
    fn mlc_placeholders_require_an_explicit_main_class() {
        for code in ["MLCXXX (P)", "MLCXXX(P)", "MLC XXXX (P)", "MLCMA XXXXX (P)", "MLCSA XXX(P)", "MLCSAXXX(P)", "MLCSAXXXX(P)"] {
            assert_eq!(canonical_lcc_notation(code).as_deref(), Some(code));
            assert_eq!(super::lcc_match_keys(code).unwrap(), vec!["(P)", code]);
            assert!(super::lcc_subject_path(code).is_some());
        }
        for code in ["MLCXXX", "MLCMA XXXXX", "MLCXXX (PQ)", "MLCXXX (X)", "MLCXXX (P) EXTRA", "MLCXYX (P)", "MLCB XXXXX (P)", "MLCMA XXXXX OTHER (P)", "MLCMA XXXXXX (P)"] {
            assert!(canonical_lcc_notation(code).is_none(), "{code}");
        }
    }

    #[test]
    fn roman_volume_numbers_preserve_call_components() {
        for suffix in ["VOL. III", "BD. II/4", "VOL. I-IV", "VOL. I:II", "VOL. III, NO. 1", "PT. III 1993", "VOL. XLIX", "VOL. III:NO. II", "VOL. 1:VOL. II"] {
            let code = format!("BR45 .T43 {suffix}");
            assert_eq!(canonical_lcc_notation(&code), Some(code.clone()));
            assert_eq!(super::lcc_match_keys(&code).unwrap(), vec!["BR45".to_owned(), "BR45.T43".to_owned(), code]);
        }
        for suffix in ["VOL. IIII", "VOL. IIV", "VOL. IC", "VOL. VX", "VOL. MMMM", "VOL. IIQ", "VOL. II/QB2", "VOL. II UNKNOWN", "VOL. II/", "VOL. 1:UNKNOWN"] {
            assert!(canonical_lcc_notation(&format!("BR45 .T43 {suffix}")).is_none(), "{suffix}");
        }
    }

    #[test]
    fn combined_issue_numbering_preserves_the_classification() {
        for suffix in ["PT. 19/2", "NO. 69/70", "BD. 40/41", "V.1:NO.1/2", "V.2:NO.5=FASC. 15", "VOL. 1/2 1999"] {
            let code = format!("QK911 .H3 {suffix}");
            assert_eq!(canonical_lcc_notation(&code), Some(code.clone()));
            assert_eq!(super::lcc_match_keys(&code).unwrap(), vec!["QK911".to_owned(), "QK911.H3".to_owned(), code]);
        }
        for suffix in ["PT. 19/", "PT. 19/QB2", "V.1:UNKNOWN2", "V.1=QB2", "V.1//2", "V.1:=NO.2"] {
            assert!(canonical_lcc_notation(&format!("QK911 .H3 {suffix}")).is_none(), "{suffix}");
        }
    }

    #[test]
    fn duplicated_cutter_periods_preserve_component_boundaries() {
        for (code, normal) in [("TK5103.59. .I55 1998", "TK5103.59.I55 1998"),
            ("PK2200..H355", "PK2200.H355"), ("QA76.A1. .B2 1999", "QA76.A1.B2 1999"),
            ("PZ7 . .R79835 HAM", "PZ7 .R79835 HAM")] {
            assert_eq!(canonical_lcc_notation(code).as_deref(), Some(code));
            let mut keys = super::lcc_match_keys(normal).unwrap();
            keys.push(code.to_owned());
            assert_eq!(super::lcc_match_keys(code), Some(keys));
        }
        for code in ["QA76...A1", "QA76. . .A1", "QA76..1", "QA76..A", "QA76..A1 UNKNOWN", "QA76..A1-QA77", "QA76.A..B2"] {
            assert!(super::doubled_cutter_period_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn documented_obsolete_sociology_heading_retains_original_evidence() {
        let code = "HM(1)-1281";
        assert_eq!(canonical_lcc_notation(code).as_deref(), Some(code));
        assert_eq!(super::lcc_match_keys(code).unwrap(), vec!["HM", code]);
        assert_eq!(super::lcc_subject_path(code), super::lcc_subject_path("HM"));
        for code in ["HM(2)-1281", "HM(1)-1282", "HM(1)-1281 Sociology", "HM(1)-1281QA76", "QA(1)-1281"] {
            assert!(canonical_lcc_notation(code).is_none(), "{code}");
        }
    }

    #[test]
    fn final_period_year_preserves_complete_class_and_cutter_keys() {
        for (code, base) in [("LB1140.5.P4 G373.2003", "LB1140.5.P4 G373 2003"),
            ("QD388P65.2004", "QD388P65 2004"), ("QA76 . A1.1999B", "QA76 . A1 1999B")] {
            assert_eq!(canonical_lcc_notation(code).as_deref(), Some(code));
            let mut keys = super::lcc_match_keys(base).unwrap();
            keys.push(code.to_owned());
            assert_eq!(super::lcc_match_keys(code), Some(keys));
        }
        for code in ["QA76.2004", "QA76.A.2004", "QA76.A1..2004", "QA76.A1.200", "QA76.A1.2004 EXTRA", "QA76.A1.2004.2005", "QA76.A1 3.2004", "QA76.A1-B2.2004"] {
            assert!(super::period_year_call_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn display_brackets_preserve_valid_inner_classification_and_evidence() {
        for base in ["B53", "HG4515.15 .S53 2002", "QA76.1-QA76.9", "QA76.A1-QA76.A9", "QA3 .L28 VOL. 2"] {
            let code = format!("[{base}]");
            assert_eq!(canonical_lcc_notation(&code), Some(code.clone()));
            let mut keys = super::lcc_match_keys(base).unwrap();
            keys.push(code.clone());
            assert_eq!(super::lcc_match_keys(&code), Some(keys));
            assert_eq!(super::lcc_subject_path(&code), super::lcc_subject_path(base));
        }
        for code in ["[]", "[[B53]]", "[B53", "B53]", "[B53] EXTRA", "[QA76 .]", "[QA76][QB2]", "[IN PROCESS]", "[QA76 .A1 [1999]]"] {
            assert!(canonical_lcc_notation(code).is_none(), "{code}");
        }
    }

    #[test]
    fn volume_chronology_is_not_a_second_classification() {
        for suffix in ["VOL. 1 1999", "V.1 (1999)", "BD. 2 1980A", "T. 1, 2000", "VOL. 1 PT. 2 1999"] {
            let code = format!("QA3 .L28 {suffix}");
            assert_eq!(canonical_lcc_notation(&code), Some(code.clone()));
            assert_eq!(super::lcc_match_keys(&code).unwrap(), vec!["QA3".to_owned(), "QA3.L28".to_owned(), code]);
        }
        for suffix in ["VOL. 1 99", "VOL. 1 (1999", "VOL. 1 1999)", "VOL. 1 (1999) EXTRA", "VOL. 1 1999 QB2", "VOL. 1 1999 2000"] {
            assert!(canonical_lcc_notation(&format!("QA3 .L28 {suffix}")).is_none(), "{suffix}");
        }
    }

    #[test]
    fn bracketed_book_years_preserve_the_complete_dated_call() {
        for (code, base) in [("AC8 .S6588 [1981]", "AC8 .S6588 1981"),
            ("QA76 .A1 [1999B]", "QA76 .A1 1999B"), ("QA76 [1999]", "QA76 1999")] {
            assert_eq!(canonical_lcc_notation(code).as_deref(), Some(code));
            let mut keys = super::lcc_match_keys(base).unwrap();
            keys.push(code.to_owned());
            assert_eq!(super::lcc_match_keys(code), Some(keys));
        }
        for code in ["QA76 .A1 [99]", "QA76 .A1 [1999?]", "QA76 .A1 [1999 OR 2000]", "QA76 .A1 [1999] EXTRA", "QA76 .A1 [1999", "QA76 .A1 [[1999]]", "QA76 UNKNOWN [1999]"] {
            assert!(canonical_lcc_notation(code).is_none(), "{code}");
        }
    }

    #[test]
    fn registered_table_notes_preserve_complete_base_and_evidence() {
        for (code, base) in [("HC435.3 (H15)+", "HC435.3"), ("PK2166 (P-PZ22)+", "PK2166"),
            ("PL4758.A6-Z (P-PZ25)+", "PL4758.A6-Z"), ("HN683.5 (H8)", "HN683.5")] {
            assert_eq!(canonical_lcc_notation(code).as_deref(), Some(code));
            let mut keys = super::lcc_match_keys(base).unwrap();
            keys.push(code.to_owned());
            assert_eq!(super::lcc_match_keys(code), Some(keys));
            assert_eq!(super::lcc_subject_path(code), super::lcc_subject_path(base));
        }
        for code in ["HC435.3 (UNKNOWN)+", "HC435.3 (H99999)+", "HC435.3 (H15)+ EXTRA", "HC435.3 (H15)++", "HC435.3 (H15", "HC435.3 (H15) (H15)+", "HC4353.. (H15)+"] {
            assert!(super::schedule_table_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn table_lists_validate_every_reference_and_inherit_only_known_families() {
        for note in ["P-PZ25, 26", "P-PZ25; P-PZ40", "P-PZ25,26", "P-PZ25; P-PZ26, 40"] {
            let code = format!("PL4758.85.A-ZS.X ({note})+");
            assert_eq!(super::variable_cutter_span(&code).as_deref(), canonical_lcc_notation("PL4758.85.A-Z").as_deref());
            assert_eq!(canonical_lcc_notation(&code).as_deref(), Some(code.as_str()));
            assert_eq!(super::lcc_subject_path(&code), super::lcc_subject_path(&canonical_lcc_notation("PL4758.85.A-Z").unwrap()));
        }
        for note in ["26", "P-PZ25,", ", P-PZ25", "P-PZ25,,26", "P-PZ25; UNKNOWN", "P-PZ25, 99999", "P-PZ25, Smith", "P-PZ25 26"] {
            let code = format!("PL4758.85 ({note})+");
            assert!(canonical_lcc_notation(&code).is_none(), "{code}");
        }
    }

    #[test]
    fn alternate_literature_classes_without_cutters_keep_class_evidence() {
        for code in ["PR4552 SU 1978", "PS2112 ST", "PR1285 TE", "PS3566 PIC"] {
            assert_eq!(canonical_lcc_notation(code).as_deref(), Some(code));
            let class = code.split_whitespace().next().unwrap();
            assert_eq!(super::lcc_match_keys(code).unwrap(), vec![class, code]);
        }
        for code in ["PR4552 S", "PR4552 SU EXTRA", "PR4552 SU 19", "PR4552 SU 1978 1979", "PZ3 SU 1978", "QA76 SU 1978"] {
            assert!(canonical_lcc_notation(code).is_none(), "{code}");
        }
    }

    #[test]
    fn imported_alternate_literature_classes_retain_class_and_cutter_keys() {
        for (code, class, cutter) in [("PR6045.R35 Dc", "PR6045", "PR6045.R35"),
            ("PS648.S3 Be", "PS648", "PS648.S3"), ("PS3566.I4 Hi 1978", "PS3566", "PS3566.I4")] {
            let canonical = code.to_ascii_uppercase();
            assert_eq!(canonical_lcc_notation(code), Some(canonical.clone()));
            assert_eq!(super::lcc_match_keys(code).unwrap(), vec![class.to_owned(), cutter.to_owned(), canonical]);
        }
        for code in ["PR6045 D", "PS648.S3 B", "PS648.S3 BE EXTRA", "PS648.S3 BE 12", "QA76.R35 DC"] {
            assert!(canonical_lcc_notation(code).is_none(), "{code}");
        }
        assert_eq!(super::lcc_item_base("PS648.S3 BE VOL. 2"), Some("PS648.S3 BE"));
    }

    #[test]
    fn compound_series_enumeration_preserves_complete_call_and_evidence() {
        for (code, base) in [
            ("ML113 .I6 VOL. AI", "ML113 .I6"),
            ("DB879.I6 A3 N.F., BD 24", "DB879.I6 A3"),
            ("H33 .W34 1984 ABT. 2, BD 7", "H33 .W34 1984"),
            ("ML113 .I6 VOL. BIV, 1-2 SUPPL", "ML113 .I6"),
            ("BT21.2 .H35 BD. 2. FASZ. 3B", "BT21.2 .H35"),
            ("BT21.2 .H35 BD.3 FASZ.5A(1)", "BT21.2 .H35"),
            ("NE90 .I45 1978 VOL. 90, COMMENTARY", "NE90 .I45 1978"),
            ("NE90 .I45 1978 VOL. 39, COMMENTARY, PT. 1-2", "NE90 .I45 1978"),
            ("BH221.H84 L76 VOL. 1.", "BH221.H84 L76"),
        ] {
            assert_eq!(super::lcc_enumeration_base(code), Some(base), "{code}");
            let keys = super::lcc_match_keys(code).unwrap();
            assert_eq!(&keys[..keys.len() - 1], super::lcc_match_keys(base).unwrap());
            assert_eq!(keys.last().unwrap(), code);
        }
        for code in ["ML113 .I6 VOL. BIIII", "ML113 .I6 VOL. AI UNKNOWN",
            "BT21.2 .H35 FASZ.5A()", "BT21.2 .H35 FASZ.5A(1", "BT21.2 .H35 FASZ.5A(UNKNOWN)",
            "BT21.2 .H35 BD. 2..", "BT21.2 .H35 BD. 2.3", "BT21.2 .H35 BD. 2. UNKNOWN",
            "NE90 .I45 1978 COMMENTARY", "NE90 .I45 1978 VOL. 90, COMMENTARYUNKNOWN",
            "NE90 .I45 1978 VOL. 90, COMMENTARY, UNKNOWN", "NE90 .I45 1978 VOL. 90, COMMENTARY,"] {
            assert!(super::lcc_enumeration_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn established_vernacular_volume_labels_preserve_call_number_keys() {
        for suffix in ["BD. 12", "BDE. 1-3", "BDCHN. 2", "NR. 45", "T. 6", "FASC. 129",
            "ABT. 12, T. 5", "LFG. 2", "LIVR. 3", "BK. 1", "JAHRG. 1978, NR. 3",
            "JAARG. 4", "KN. 2", "KNJ. 3", "CZ. 2", "D. 5", "SV. 2", "ZESZ. 8", "VYP. 15",
            "HEFT 1", "H. 2", "HFT. 3", "HBD. 1", "HALBBD. 2",
            "BD. 10, FASZ. 2", "BD. 13, FAS. 1 1975", "BD. 293, ABH 1", "BD 24"] {
            let code = format!("QA3 .L28 {suffix}");
            assert_eq!(canonical_lcc_notation(&code), Some(code.clone()));
            assert_eq!(super::lcc_match_keys(&code).unwrap(), vec!["QA3".to_owned(), "QA3.L28".to_owned(), code]);
        }
        for code in ["QA3 .L28 BD.", "QA3 .L28 BD. UNKNOWN", "QA3 .L28 NR. 2 EXTRA",
            "QA3 .L28 FASZ. UNKNOWN", "QA3 .L28 FAS. 1 UNKNOWN",
            "QA3 .L28 ABH EXTRA", "QA3 .L28 BD 24 UNKNOWN", "QA3 .L28 BDX 24",
            "QA3 .L28 T. 1-QB2", "BAD TEXT BD. 1", "QA3 .L28 FASC. 2,", "QA3 .L28 NR. 2 99", "QA3 .L28 HEFT EXTRA"] {
            assert!(canonical_lcc_notation(code).is_none(), "{code}");
        }
    }

    #[test]
    fn apostrophes_separate_complete_components_without_erasing_evidence() {
        for (code, base) in [("TN'291'C35'1977", "TN291.C35 1977"),
            ("HF5415.12'G7'D37'1976", "HF5415.12.G7.D37 1976"),
            ("BR'60'A64", "BR60.A64")] {
            assert_eq!(canonical_lcc_notation(code).as_deref(), Some(code));
            let mut expected = super::lcc_match_keys(base).unwrap();
            expected.push(code.to_owned());
            assert_eq!(super::lcc_match_keys(code).unwrap(), expected);
        }
        for code in ["QA76'73", "QA76'BAD'2000", "TN'291'C35'", "TN''291'C35", "'TN291'C35", "TN'291'C35'1977'NOTE", "516.1'5", "QA76'C35'QB2"] {
            assert!(super::apostrophe_call_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn map_mlc_marker_preserves_the_explicit_classification() {
        let code = "G6513.M6E635 1993 .G4 MLC";
        assert_eq!(canonical_lcc_notation(code).as_deref(), Some(code));
        assert_eq!(super::lcc_item_base(code), Some("G6513.M6E635 1993 .G4"));
        let keys = super::lcc_match_keys(code).unwrap();
        assert!(keys.contains(&"G6513".to_owned()));
        assert_eq!(keys.last().map(String::as_str), Some(code));
        assert_eq!(super::lcc_item_base("G6513.M6E635MLC"), None);
        for invalid in ["G6513 BAD TEXT MLC", "MLC", "ENHANCED MLC", "428.24 .R777P 1999 OISE/UT MLC", "QA76.A1 MLC"] {
            assert!(canonical_lcc_notation(invalid).is_none(), "{invalid}");
        }
    }

    #[test]
    fn mlc_shelf_numbers_require_an_explicit_class_assignment() {
        for (code, key) in [("MLCM 98/02114 (P)", "(P)"), ("MLCMJ 2003/00135 (S)", "(S)"),
            ("MLC R 85/7 (D)", "(D)"), ("MLCS 2005/04905 (Z)", "(Z)"), ("MICROFILM 85/4003 (P)", "(P)"),
            ("MICROFICHE (O) 84/61398 (B)", "(B)"), ("MICROOPAQUE (W) 85/42 (D)", "(D)")] {
            assert_eq!(canonical_lcc_notation(code).as_deref(), Some(code));
            assert_eq!(super::lcc_match_keys(code).unwrap(), vec![key.to_owned(), code.to_owned()]);
        }
        for code in ["MLCS", "MLCMJ 2003/00135", "MLCS 2010/40000 (I)", "MLCS 2010/40000 (ZZ)",
            "MLCX 2010/40000 (P)", "MLCS 201/40000 (P)", "MLCS 2010/40000X (P)", "MLCS 2010/40000 (P) NOTE", "MICROFILM 85/4003", "MICROFICHE (X) 84/61398 (B)", "MICROFILM 85/4003 (PQ)"] {
            assert!(canonical_lcc_notation(code).is_none(), "{code}");
        }
    }

    #[test]
    fn minimal_class_outline_paths_use_the_assignment_not_shelf_prefix() {
        assert!(super::lcc_subject_path("MLCS 2005/00456 (H)").unwrap().contains("Social Sciences"));
        assert!(super::lcc_subject_path("MICROFILM 85/4003 (P)").unwrap().contains("Language and Literature"));
        let broad_b = super::lcc_subject_path("MLCM 98/02114 (B)").unwrap();
        assert!(broad_b.contains("Psychology") && broad_b.contains("Religion"));
        assert!(super::lcc_subject_path("MLCM 98/02114").is_none());
    }

    #[test]
    fn fully_written_ranges_reject_incomplete_or_reversed_endpoints() {
        for invalid in ["PN1-PN6790-PN7000", "PN6790-PN1", "QA76.C5-QA76.A1", "RZ-R"] {
            assert_eq!(canonical_lcc_notation(invalid), None, "{invalid}");
        }
    }
    #[test]
    fn complete_range_endpoints_are_canonicalized() {
        for (input, expected) in [
            (" d 1 – dx 301 ", "D1-DX301"), ("R-RZ", "R-RZ"),
            ("QA273.A1-274.9", "QA273.A1-274.9"),
            ("QA76.A1-.C5", "QA76.A1-76.C5"),
            ("PN1-PN6790.A3", "PN1-6790.A3"),
        ] { assert_eq!(canonical_lcc_notation(input).as_deref(), Some(expected)); }
    }

    #[test]
    fn decimal_only_range_ends_inherit_the_integer_part() {
        for (input, expected) in [
            ("T57.6-.97", "T57.6-57.97"),
            ("TA418.5–.84", "TA418.5-418.84"),
            ("QA76-.95", "QA76-76.95"),
            ("TA418.5-.84.A1", "TA418.5-418.84.A1"),
            ("QB1.A2-.C4", "QB1.A2-1.C4"),
        ] { assert_eq!(canonical_lcc_notation(input).as_deref(), Some(expected)); }
        for invalid in ["T57.6-.5", "T57.6-.", "T-.97", "T57.6-.97.2"] {
            assert!(super::lcc_span(invalid).is_none(), "{invalid}");
        }
    }

    #[test]
    fn bare_letter_cutter_intervals_inherit_the_class_number() {
        for (input, expected) in [
            ("B829.5.A-Z", "B829.5.A-829.5.Z"),
            ("QA76.B-D", "QA76.B-76.D"),
            ("B829.5.A-Z1", "B829.5.A-Z1"),
            ("B829.5.A3-Z", "B829.5.A3-829.5.Z"),
            ("B-Z", "B-Z"),
        ] { assert_eq!(canonical_lcc_notation(input).as_deref(), Some(expected)); }
        assert!(super::lcc_span("B829.5.Z-A").is_none());
    }

    #[test]
    fn encoded_hyphens_require_a_complete_ordered_range() {
        for code in ["G1100#8209;3102", "G1100&#8209;3102", "g1100&#x2011;3102",
            "G1100‑3102", "G1100‐3102", "G1100&#8208;3102", "G1100&#x2010;3102"] {
            assert_eq!(canonical_lcc_notation(code).as_deref(), Some("G1100-3102"), "{code}");
            assert_eq!(super::lcc_subject_path(code), super::lcc_subject_path("G1100-3102"), "{code}");
        }
        for code in ["G3102#8209;1100", "G1100#8209;", "G1100#8209;3102 trailing",
            "#780;ML3470", "G1100#8209;3102#8209;4000", "G1100&#8209;3102;QA76",
            "K1#8209;100", "G1100#8209;K100", "G1100#8210;3102"] {
            assert!(super::encoded_hyphen_range(code).is_none(), "{code}");
            assert!(canonical_lcc_notation(code).is_none(), "{code}");
        }
    }

    #[test]
    fn unfinished_item_captions_preserve_the_valid_call_and_original_evidence() {
        let base = "DS436 .N47 1987";
        for suffix in ["PT.", "PT", "VOL.", "VOL", "V.", "NO.", "NO", "VOL. 1, PT.", "V.1:NO.", "BD. 3, V"] {
            let code = format!("{base} {suffix}");
            assert_eq!(canonical_lcc_notation(&code), Some(code.clone()), "{code}");
            assert_eq!(super::lcc_enumeration_base(&code), Some(base), "{code}");
            let keys = super::lcc_match_keys(&code).unwrap();
            assert_eq!(keys.last(), Some(&code));
            assert!(keys.contains(&"DS436.N47".to_owned()));
        }
        assert_eq!(super::lcc_enumeration_base("QA3 .L28 VOL."), Some("QA3 .L28"));
        for code in ["DS436 BAD TEXT PT.", "DS436 .N47 1987 PT. UNKNOWN", "DS436 .N47 1987 PT. 1/",
            "DS436 .N47 1987 VOL. 1,", "DS436-500 VOL.", "D810.M8 V", "PZ7 BOX NO", "CPB VOL.", "K1 .A1 VOL."] {
            assert!(super::lcc_enumeration_base(code).is_none(), "{code}");
            assert!(canonical_lcc_notation(code).is_none(), "{code}");
        }
    }

    #[test]
    fn explicit_closed_plus_ranges_keep_their_documented_upper_boundary() {
        for code in ["DS785.A5-Z+", "DS785.A5-.Z+", "DS785.A5-DS785.Z+"] {
            assert_eq!(super::documented_closed_plus_span(code), canonical_lcc_notation("DS785.A5-DS785.Z"));
            assert!(canonical_lcc_notation(code).is_some());
        }
        let (_, open_end) = super::reference_plus_span("DS785.A5+").unwrap();
        assert_eq!(open_end.code, "DS786");
        assert!(super::documented_preliminary_plus_span("DS785.A5-ZI+").is_none());
        for code in ["DS785.A4-Z+", "DS785.A5-Y+", "DS785.A5-ZI+", "DS785.A5-Z++", "DS785.A5-Z+ NOTE", "K1.A-Z+"] {
            assert!(super::documented_closed_plus_span(code).is_none(), "{code}");
        }
    }

    #[test]
    fn numbered_table_scopes_preserve_points_and_complete_intervals() {
        let (a, z) = super::reference_plus_span("PK8375+").unwrap();
        assert_eq!((a.code.as_str(), z.code.as_str()), ("PK8375", "PK8375"));
        let (a, z) = super::reference_plus_span("PK6729+").unwrap();
        assert_eq!((a.code.as_str(), z.code.as_str()), ("PK6729", "PK6732.9"));
        assert!(super::reference_plus_span("PK2166+").is_none());
        let generated = include_str!("../data/lcc-numbered-table-scopes.tsv");
        assert!(!generated.lines().any(|line| line.starts_with('K') || line.starts_with("PR4741\t")));
    }

}

#[cfg(test)]
mod joined_classification_tests {
    #[test]
    fn complete_joined_fields_are_split_without_dropping_components() {
        for (input, expected) in [
            ("QH506R-RZ", vec!["QH506", "R-RZ"]),
            ("QA76.76.M52QA76.758", vec!["QA76.76.M52", "QA76.758"]),
            ("QH541.5.W3QH541.5.F7", vec!["QH541.5.W3", "QH541.5.F7"]),
            ("QA76.9.U83QA76.9.H85", vec!["QA76.9.U83", "QA76.9.H85"]),
            ("QH301-705R-RZ", vec!["QH301-705", "R-RZ"]),
            ("SB123-123.5S494.5.B563", vec!["SB123-123.5", "S494.5.B563"]),
            ("B67D1-DX301", vec!["B67", "D1-DX301"]),
            ("R-RZQH506", vec!["R-RZ", "QH506"]),
            ("B67D1-DX301QH331", vec!["B67", "D1-DX301", "QH331"]),
            ("HD28-70HF4999.2-6182", vec!["HD28-70", "HF4999.2-6182"]),
            ("HD28-HD70HF4999.2-HF6182", vec!["HD28-HD70", "HF4999.2-HF6182"]),
        ] {
            assert_eq!(super::joined_lcc_codes(input).unwrap(), expected, "{input}");
            assert_eq!(super::joined_lcc_codes(&input.to_ascii_lowercase()).unwrap(), expected);
        }
        for input in ["QA76.A1B2", "QH506R", "B67D1-DX301QH", "QH506R-RZUNKNOWN", "QH506R-RZ.", "B67D1-DX", "D1-DX301", "R-RZ", "QH506R-RZ 2000", "D1-DX301B53", "B67B53D1-DX301", "HD28-70HF4999.2-618", "HD28-70HF4999.2-6182HF", "HD28-70HF4999.2-6182.5", "HD28-70HF4999.2-6182UNKNOWN", "QA76.A1QA2", "QA76.76.M52QA76.", "QA76.758QA76.758", "QA76.9.D3QA76.9.D3Q"] {
            assert!(super::joined_lcc_codes(input).is_none(), "{input}");
        }
    }

    #[test]
    fn joined_fields_match_each_component_in_both_taxonomies() {
        let taxonomy = crate::UnifiedTaxonomy::from_concepts(crate::read_unified_taxonomy_sqlite(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/data/unified-taxonomy-v2.sqlite3"))).unwrap()).unwrap();
        for (input, parts) in [("QH506R-RZ", vec!["QH506", "R-RZ"]), ("B67D1-DX301", vec!["B67", "D1-DX301"]), ("HD28-70HF4999.2-6182", vec!["HD28-70", "HF4999.2-6182"])] {
            // Fixed selectors prove that joined input preserves every component.
            // Curated umbrella ranges can legitimately be removed during curation.
            let fixture = crate::UnifiedTaxonomy::from_concepts(parts.iter().enumerate().map(|(index, part)| {
                crate::UnifiedConceptDefinition::new(
                    index as i64 + 1, format!("Component {index}"), vec![],
                    std::collections::BTreeMap::from([("lcc".into(), vec![(*part).into()])]), vec![],
                )
            }).collect()).unwrap();
            let fixture_ids: std::collections::BTreeSet<_> = (1..=parts.len() as i64).collect();
            for (index, part) in parts.iter().enumerate() {
                assert_eq!(fixture.matching_concept_ids("lcc", part), vec![index as i64 + 1], "{part}");
            }
            assert_eq!(fixture.matching_concept_ids("lcc", input).into_iter().collect::<std::collections::BTreeSet<_>>(), fixture_ids, "{input}");
            let expected: std::collections::BTreeSet<_> = parts.iter().flat_map(|part| taxonomy.matching_concept_ids("lcc", part)).collect();
            assert_eq!(taxonomy.matching_concept_ids("lcc", input).into_iter().collect::<std::collections::BTreeSet<_>>(), expected, "{input}");
            let paths: std::collections::BTreeSet<_> = parts.iter().filter_map(|part| super::lcc_subject_path(part)).collect();
            assert_eq!(super::lcc_subject_paths(input).into_iter().collect::<std::collections::BTreeSet<_>>(), paths);
        }
    }
}

#[cfg(test)]
mod range_item_tests {
    #[test]
    fn complete_ranges_keep_book_metadata_as_evidence() {
        for (code, base) in [
            ("P121-143.3 .S66 2021", "P121-143.3"),
            ("NA9000-9284 S337 2018", "NA9000-9284"),
            ("ML3757-3757.9 .T87 2017", "ML3757-3757.9"),
            ("P121-143.3 2021", "P121-143.3"),
            ("P121.A1-P143.3.Z9 .S66 A2 2021B", "P121.A1-P143.3.Z9"),
            ("QA75-QB76 .S66 2021", "QA75-QB76"),
        ] {
            assert_eq!(super::range_item_base(code), Some(base));
            assert_eq!(super::canonical_lcc_notation(code).as_deref(), Some(code));
            let keys = super::lcc_match_keys(code).unwrap();
            assert_eq!(keys.first(), super::canonical_lcc_notation(base).as_ref());
            assert_eq!(keys.last().unwrap(), code);
            assert_eq!(super::lcc_subject_path(code), super::lcc_subject_path(base));
        }
        assert_eq!(super::canonical_lcc_notation("p121–143.3 .s66 2021").as_deref(), Some("P121-143.3 .S66 2021"));
        for code in ["P143.3-121 .S66 2021", "P121-143.3 .S 2021", "P121-143.3 .S66 21", "P121-143.3 .S66 2021 NOTES", "P121-143.3 .S66 2021 2022", "P121-143.3 .S66", "P121-143.3.Z9", "P121-??? .S66 2021"] {
            assert!(super::range_item_base(code).is_none(), "{code}");
        }
    }
}

#[cfg(test)]
mod physical_set_tests {
    #[test]
    fn set_qualifiers_preserve_complete_call_and_evidence() {
        for (code, base) in [("N7340 .K816 (set)", "N7340 .K816"), ("GR335 .L44 2007(set)", "GR335 .L44 2007")] {
            let canonical = super::canonical_lcc_notation(code).unwrap();
            assert_eq!(canonical, code.to_ascii_uppercase());
            assert_eq!(super::lcc_item_base(&canonical), Some(base));
            let path = super::lcc_subject_path(code).unwrap();
            let base_path = super::lcc_subject_path(base).unwrap();
            assert_eq!(path.rsplit_once(" / ").unwrap().0, base_path.rsplit_once(" / ").unwrap().0);
            assert_eq!(super::lcc_match_keys(&canonical).unwrap().last(), Some(&canonical));
        }
        for code in ["N7340 ??? (SET)", "N7340 .K816 (SETS)", "WB18.2 (SET)", "LAW (SET)", "N7340 .K816 (SET) EXTRA"] {
            assert!(super::lcc_item_base(code).is_none(), "{code}");
        }
    }
}

#[cfg(test)]
mod composed_item_metadata_tests {
    #[test]
    fn ordinal_and_plural_item_captions_preserve_classification() {
        for (code, base) in [("AC9 .A6 2D SER., PT. 3", "AC9 .A6"),
            ("AM101 .M9743 38.JAHRG., HEFT 3", "AM101 .M9743"),
            ("AE5 .E5 15TH ED. 1986", "AE5 .E5"),
            ("B1602 .A2 1963 VOLS. 18-19", "B1602 .A2 1963"),
            ("DF10 .A825 AR.197", "DF10 .A825"), ("CD6151.S57 S37 ZV. 31", "CD6151.S57 S37")] {
            assert_eq!(super::lcc_enumeration_base(code), Some(base));
            assert!(super::lcc_match_keys(code).is_some());
        }
        for code in ["AC9 .A6 13RD SER.", "AC9 .A6 3RD UNKNOWN", "AC9 .A6 3RD SER.,", "AC9 .A6 3..SER.", "AC9 .A6 3RD SER. UNKNOWN"] {
            assert!(super::lcc_enumeration_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn marc_display_delimiters_preserve_call_structure_and_evidence() {
        for (code, base) in [("QA76.9 ǂB .A25 2008", "QA76.9 .A25 2008"),
            ("‡AQA76.9 ‡B.A25 2008", "QA76.9 .A25 2008"),
            ("K1188.A8 $B A3 2007", "K1188.A8 A3 2007"),
            ("KD358.M3 ǂB R8612 2008", "KD358.M3 R8612 2008")] {
            assert_eq!(super::marc_subfield_call_base(code).as_deref(), Some(base));
            let keys = super::lcc_match_keys(code).unwrap();
            assert_eq!(&keys[..keys.len() - 1], super::lcc_match_keys(base).unwrap());
            assert_eq!(keys.last().unwrap(), code);
        }
        for code in ["QA76 ǂB", "QA76 ‡C.A25", "QA76 ǂB.A25 ‡AQC1", "K1188.A8 ǂBUNKNOWN WORDS"] {
            assert!(super::marc_subfield_call_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn compound_serial_captions_keep_complete_chronology() {
        for (code, base) in [("AM101 .M9743 N.F, BD. 12", "AM101 .M9743"),
            ("AM101 .V5344 N. F., NR. 296", "AM101 .V5344"),
            ("AS182 .H4 1971 ABH. 3", "AS182 .H4 1971"),
            ("B2900 .H432 BEIHEFT 44 2001", "B2900 .H432"),
            ("AS182 .G811 FOLGE 3, NR. 31", "AS182 .G811"),
            ("AM101 .M9743 JAHRG. 44, 1976, HEFT 1", "AM101 .M9743")] {
            assert_eq!(super::lcc_enumeration_base(code), Some(base));
            assert!(super::lcc_match_keys(code).is_some());
        }
        for code in ["AM101 .M9743 N.F.UNKNOWN", "AM101 .M9743 JAHRG. 44, 1976, UNKNOWN",
            "AM101 .M9743 JAHRG. 44, 1976,", "AM101 .M9743 JAHRG. 44, 76, HEFT 1"] {
            assert!(super::lcc_enumeration_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn accompanying_print_descriptors_follow_complete_dated_calls() {
        for (code, base) in [("QD33 .Z843 1998 GUIDE", "QD33 .Z843 1998"),
            ("BC71 .C49 1990 MANUAL", "BC71 .C49 1990"),
            ("BF121 .B47 1991, GUIDE", "BF121 .B47 1991"),
            ("BF121 .C522 2010 STUDY GUIDE", "BF121 .C522 2010"),
            ("BF637 C45 G35 2005 INSTRUCTOR'S RESOURCE MANUAL", "BF637 C45 G35 2005"),
            ("QA303 .A1 2010 SOLUTIONS MANUAL", "QA303 .A1 2010")] {
            assert_eq!(super::lcc_item_base(code), Some(base));
            let keys = super::lcc_match_keys(code).unwrap();
            assert_eq!(keys.last().unwrap(), code);
            assert_eq!(&keys[..keys.len() - 1], super::lcc_match_keys(base).unwrap());
        }
        for code in ["GUIDE RECORD", "QD33 UNKNOWN 1998 GUIDE", "QD33 .Z843 1998 GUIDE EXTRA",
            "QD33 .Z843 98 GUIDE", "QD33 .Z843 1998 UNKNOWN", "WB18.2 .A1 1998 GUIDE"] {
            assert!(super::lcc_item_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn documented_vernacular_captions_keep_the_complete_call() {
        for (code, base) in [("AM101 .M9743 N.F., BD. 12", "AM101 .M9743"),
            ("AS244 .A512 DEEL 12, NO. 1, 1949", "AS244 .A512"),
            ("AM101 .P295845 SZ. 18", "AM101 .P295845"), ("AM49 F67 HEFT. 16", "AM49 F67"),
            ("B56 .H53 NIDE 360", "B56 .H53"), ("AS262.T84 A3 OSA 139", "AS262.T84 A3")] {
            assert_eq!(super::lcc_enumeration_base(code), Some(base));
            assert!(super::lcc_match_keys(code).is_some());
        }
        for code in ["AM101 .M9743 N.F.UNKNOWN", "AS244 .A512 DEEL ?", "AM101 .P295845 SZ.UNKNOWN"] {
            assert!(super::lcc_enumeration_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn alphanumeric_item_numbers_follow_recognized_captions() {
        for (code, base) in [("HD4456 .A49 NO. M5", "HD4456 .A49"),
            ("TD491 .A49 NO. M5", "TD491 .A49"), ("QA1 .A1 VOL. A12", "QA1 .A1"),
            ("TD193 .Y46 1999 VOL. B", "TD193 .Y46 1999"), ("TA1245 .C56 1984 VOL. 2, PT. E", "TA1245 .C56 1984"),
            ("PJ2463 .M66 VOL. 7, ISSUE 1", "PJ2463 .M66"), ("QA1 .A1 VOLUME 2, PART A", "QA1 .A1"),
            ("QA1 .A1 NO 12", "QA1 .A1"), ("QA1 .A1 VOL 2", "QA1 .A1")] {
            assert_eq!(super::lcc_enumeration_base(code), Some(base));
            let keys = super::lcc_match_keys(code).unwrap();
            assert_eq!(&keys[..keys.len() - 1], super::lcc_match_keys(base).unwrap());
        }
        for code in ["HD4456 .A49 NO. M5UNKNOWN", "HD4456 .A49 NO. M?", "HD4456 .A49 NO. M5-", "HD4456 .A49 M5"] {
            assert!(super::lcc_enumeration_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn preliminary_initials_preserve_registered_numbered_table_bounds() {
        for (code, span) in [
            ("PK2598.B487A61-Z458S (P-PZ40)+", "PK2598.B487.A61-PK2598.B487.Z458"),
            ("PL4758.9.K27A61-Z458R (P-PZ40)+", "PL4758.9.K27.A61-PL4758.9.K27.Z458"),
        ] {
            let expected = super::canonical_printed_range(span).unwrap();
            assert_eq!(super::variable_cutter_span(code), Some(expected.clone()));
            let keys = super::lcc_match_keys(code).unwrap();
            assert_eq!(keys.last().unwrap(), code);
            assert!(keys.contains(&expected));
        }
        for code in ["PK2598.B487A61-Z459S (P-PZ40)+", "PK2598.B487A62-Z458S (P-PZ40)+",
            "PK2598.B487A61-Z458S", "PK2598.B487A61-Z458S (UNKNOWN)+",
            "PK2598.B487A61-Z458S (P-PZ40)+ EXTRA"] {
            assert!(super::variable_cutter_span(code).is_none(), "{code}");
        }
    }

    #[test]
    fn index_designation_keeps_original_call_number() {
        for (code, base) in [("ML1.M2 INDEX", "ML1.M2"), ("KF101 .A313 INDEX", "KF101 .A313"),
            ("M2127 .U55 1989 INDEX", "M2127 .U55 1989"),
            ("Z1215 .S5 INDEX 2000", "Z1215 .S5"),
            ("AC149.S73 H792 1996 INDEX 1999", "AC149.S73 H792 1996")] {
            assert_eq!(super::lcc_enumeration_base(code), Some(base));
            let keys = super::lcc_match_keys(code).unwrap();
            assert_eq!(&keys[..keys.len() - 1], super::lcc_match_keys(base).unwrap());
            assert_eq!(keys.last().unwrap(), code);
        }
        for code in ["INDEX", "ML1.M2 INDEX UNKNOWN", "ML1.M2 INDEX 200",
            "ML1.M2 INDEX 2000 UNKNOWN", "ML1.M2 INDEX 2000-", "ML1.M2 INDEX 2000/01"] {
            assert!(super::lcc_enumeration_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn complete_joined_single_letter_decimal_classes_are_supported() {
        assert_eq!(super::joined_lcc_codes("RC466.8R726.7").unwrap(), vec!["RC466.8", "R726.7"]);
        assert_eq!(super::joined_lcc_codes("rc466.8r726.7").unwrap(), vec!["RC466.8", "R726.7"]);
        for code in ["RC466.8R726", "RC466.8R726.", "RC466.8.R726", "RC466.8R726.7UNKNOWN"] {
            assert!(super::joined_lcc_codes(code).is_none(), "{code}");
        }
    }

    #[test]
    fn source_confirmed_clipped_joined_field_is_not_split() {
        for code in ["T385TA1637-1638TK788", "QB1-991QB460-466QB98", "QA315-316QA402.3QA40",
            "TJ1075-1081TA467TA41", "QC173.45-173.458TA40", "SB123-123.5S494.5.B5", "Q334-342TA329-348TA6",
            "QC138-168.86QA930QC1", "QA639.5-640.7QA640.7", "QC173.45-173.458TA41", "QA76.9.C66QA76.76.A6"] {
            assert!(super::joined_lcc_codes(code).is_none(), "{code}");
        }
        assert_eq!(super::joined_lcc_codes("T385TA1637-1638TK7882.P3").unwrap(),
            vec!["T385", "TA1637-1638", "TK7882.P3"]);
        assert_eq!(super::joined_lcc_codes("HD28-70HD28-HD70HF54").unwrap(),
            vec!["HD28-70", "HD28-HD70", "HF54"]);
    }

    #[test]
    fn numbered_volume_continuations_do_not_change_subjects() {
        for (code, base) in [("CS434 .B7 VOL. 82, ETC.", "CS434 .B7"),
            ("KF8742.A45 H55 VOL. 6, ETC. 2010", "KF8742.A45 H55"),
            ("E78.I3 A58 1983, VOL. 10", "E78.I3 A58 1983")] {
            assert_eq!(super::lcc_enumeration_base(code), Some(base));
            assert!(super::lcc_match_keys(code).is_some());
        }
        for code in ["CS434 .B7 ETC.", "CS434 .B7 VOL. ETC.", "CS434 .B7 VOL. 82, ETC.UNKNOWN", "CS434 .B7 VOL. 82, ETC. 20", "CS434 .B7 VOL. 82- ETC."] {
            assert!(super::lcc_enumeration_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn dutch_and_german_new_series_preserve_original_evidence() {
        for (code, base) in [
            ("AS244 .A51 N.R. DEEL 53, NO.4", "AS244 .A51"),
            ("AS244 .A52 N.R. DEEL140", "AS244 .A52"),
            ("AS244 .A52 N. R., DEEL 61, NO. 1", "AS244 .A52"),
            ("AS244 .A52 N.R, DEEL 77, NO. 2", "AS244 .A52"),
            ("AS244 .A512 N.R. DL.70 NO.2", "AS244 .A512"),
            ("PT6407 .B57 2005 DL. 10", "PT6407 .B57 2005"),
            ("H31 .W5 NEUE REIHE, BD. 1", "H31 .W5"),
        ] {
            assert_eq!(super::lcc_enumeration_base(code), Some(base), "{code}");
            let keys = super::lcc_match_keys(code).unwrap();
            assert_eq!(&keys[..keys.len() - 1], super::lcc_match_keys(base).unwrap());
            assert_eq!(keys.last().unwrap(), code);
        }
        for code in ["AS244 .A51 N.R.UNKNOWN", "AS244 .A51 N.R.,",
            "AS244 .A51 N.R. DL.UNKNOWN", "AS244 .A51 N.R. DEEL-100",
            "AS244 .A51 N.R. DEELUNKNOWN", "AS244 .A51 N.R. DL.70 UNKNOWN",
            "H31 .W5 NEUE REIHE UNKNOWN", "H31 .W5 NEUE REIHE, BD."] {
            assert!(super::lcc_enumeration_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn supplied_item_numbers_require_complete_brackets() {
        for (code, base) in [
            ("CT3990.Y83 A2 2009 [V.2]", "CT3990.Y83 A2 2009"),
            ("BQ4630 .B645125 2012 [V. 4]", "BQ4630 .B645125 2012"),
            ("BS741 .A3 1982, [V. 11D]", "BS741 .A3 1982"),
            ("BQ164 .H39 1992X V.[16]", "BQ164 .H39 1992X"),
            ("BS550.2 .A7 V.[120]", "BS550.2 .A7"),
            ("PN5370 .D53 2018 VOL.[10]", "PN5370 .D53 2018"),
            ("QA3 .L28 V.[1]-[3]", "QA3 .L28"),
        ] {
            assert_eq!(super::lcc_enumeration_base(code), Some(base), "{code}");
            let keys = super::lcc_match_keys(code).unwrap();
            assert_eq!(&keys[..keys.len() - 1], super::lcc_match_keys(base).unwrap());
            assert_eq!(keys.last().unwrap(), code);
        }
        for code in ["QA3 .L28 V.[]", "QA3 .L28 V.[ ]", "QA3 .L28 V.[1",
            "QA3 .L28 V.[UNKNOWN]", "QA3 .L28 V.[[1]]", "QA3 .L28 V.[1] UNKNOWN",
            "QA3 .L28 V.[1]-[]", "QA3 .L28 V.[1?]",
            "QA3 .L28 [V.]", "QA3 .L28 [V.1-]", "QA3 .L28 [V.1] UNKNOWN",
            "QA3 .L28 [[V.1]]", "QA3 .L28 [UNKNOWN]", "QA3 .L28 [V.1 UNKNOWN]"] {
            assert!(super::lcc_enumeration_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn coverage_year_spans_keep_the_common_call_and_evidence() {
        for (code, base) in [
            ("AM101.S2494 A33 1979-80", "AM101.S2494 A33"),
            ("BX7715 .B3 1833-1838", "BX7715 .B3"),
            ("QA1 .S3575 2000-2001", "QA1 .S3575"),
            ("QA76 .A1 1998-99", "QA76 .A1"),
            ("QA76 .A1 1999-00", "QA76 .A1"),
            ("AS613.S8 A13 1990/1", "AS613.S8 A13"),
            ("PF861 A13 1990/1", "PF861 A13"),
            ("AS284.A1 S37 1978/1979:5", "AS284.A1 S37"),
            ("AS182 .H4 1979/80 ABH. 5", "AS182 .H4"),
            ("QD255 .H4 1979/80 ABH. 5", "QD255 .H4"),
            ("QA3 .L28 1999-2000 V.2", "QA3 .L28"),
            ("QA3 .L28 V.1 1990/12", "QA3 .L28"),
            ("QC1 .M23 1985-02", "QC1 .M23"),
            ("QB981 .M23 1985-02", "QB981 .M23"),
            ("QA3 .L28 V.1 1999-2000", "QA3 .L28"),
        ] {
            assert_eq!(super::lcc_item_base(code), Some(base), "{code}");
            let keys = super::lcc_match_keys(code).unwrap();
            assert_eq!(&keys[..keys.len() - 1], super::lcc_match_keys(base).unwrap());
            assert_eq!(keys.last().unwrap(), code);
        }
        for code in ["QA76 .A1 2001-2000", "QA76 .A1 2001-", "QA76 .A1 2001-2",
            "QA76 .A1 2001-2002 UNKNOWN", "QA76 .A1 5000-5001", "QA76 1999-2000",
            "QA76 .A 1999-2000", "QA75-76 1999-2000", "KF1 .A1 1999-2000",
            "WB18.2 .A1 1999-2000", "QA3 .L28 V.1 1999-2000-2001",
            "QA76 .A1 1985-02 UNKNOWN", "AS613.S8 A13 1990/",
            "AS613.S8 A13 1990/1 UNKNOWN", "AS613.S8 A13 1990/1/2",
            "AS613.S8 A13 90/1", "AS613.S8 A13 1990/A", "AS613.S8 A13 1990/123456",
            "KF1 .A1 1990/1", "WB18.2 .A1 1990/1",
            "AS284.A1 S37 1978/1979:", "AS284.A1 S37 1978/1977:5",
            "AS284.A1 S37 1978/1979:5 UNKNOWN", "AS284.A1 S37 1978/1979:5:6",
            "AS182 .H4 1979/80 ABH.", "AS182 .H4 1979/80 ABH. 5 UNKNOWN",
            "AS182 .H4 1979/80 UNKNOWN 5", "KF1 .H4 1979/80 ABH. 5"] {
            assert!(super::lcc_item_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn reference_collection_labels_preserve_calls_and_evidence() {
        for (code, base) in [
            ("REF BF31 .C65 2006", "BF31 .C65 2006"),
            ("REF BL31 .F33", "BL31 .F33"),
            ("AY 414 C2 1995 REF", "AY 414 C2 1995"),
            ("DA190 D5 VOL.2 1976 REF", "DA190 D5"),
        ] {
            assert_eq!(super::reference_collection_base(code), Some(base), "{code}");
            let keys = super::lcc_match_keys(code).unwrap();
            assert_eq!(&keys[..keys.len() - 1], super::lcc_match_keys(base).unwrap());
            assert_eq!(keys.last().unwrap(), code);
        }
        for code in ["REF", "REF UNKNOWN", "REF WB18.2 .A1", "QA76 .A REF",
            "REF REF QA76 .A1", "REF QA75-76", "QA76 .A1 201 REF",
            "QA76 .A1 2001 CHINA REF", "REF QA76 .A1 UNKNOWN", "REFERENCE QA76 .A1"] {
            assert!(super::reference_collection_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn oversize_letters_preserve_dated_and_enumerated_calls() {
        for (code, base) in [
            ("N6853.P5 D83213 2008 F", "N6853.P5 D83213 2008"),
            ("NA123 .A72 1999 FF", "NA123 .A72 1999"),
            ("NA123 .A72 1999 FFF", "NA123 .A72 1999"),
            ("N6490 P56 V.6 F", "N6490 P56"),
            ("B74 P4414 2004 F", "B74 P4414 2004"),
            ("PZ7.A1 AD 2001 F", "PZ7.A1 AD 2001"),
        ] {
            assert_eq!(super::lcc_item_base(code), Some(base), "{code}");
            let keys = super::lcc_match_keys(code).unwrap();
            assert_eq!(&keys[..keys.len() - 1], super::lcc_match_keys(base).unwrap());
            assert_eq!(keys.last().unwrap(), code);
        }
        for code in ["F", "FF", "FFF", "QA76 .A1 F", "PZ7.A1 F", "QA76 .A1 201 F",
            "QA76 .A1 2001 FFFF", "QA76 .A1 2001 F UNKNOWN", "QA76 .A 2001 F",
            "QA76 UNKNOWN 2001 F", "WB18.2 .A1 2001 F", "QA75-76 2001 F",
            "QA76 .A1 V.2- F"] {
            assert!(super::lcc_item_base(code).is_none(), "{code}");
        }
        // Existing lettered enumeration interprets this as volume F.
        assert_eq!(super::lcc_enumeration_base("QA76 .A1 V. F"), Some("QA76 .A1"));
    }

    #[test]
    fn local_cutter_x_preserves_complete_call_and_evidence() {
        for (code, base) in [
            ("PG7137 .K35 X", "PG7137 .K35"),
            ("B105.A35 J6315 X, 1981", "B105.A35 J6315 1981"),
            ("QA76 .A1X 2001", "QA76 .A1 2001"),
            ("QA76 .A1X,2001A", "QA76 .A1 2001A"),
            ("QA76.A1B2X", "QA76.A1B2"),
            ("PR9619.2.L3 A17 X, 1975B", "PR9619.2.L3 A17 1975B"),
            ("GV722 1976 .M32 X", "GV722 1976 .M32"),
            ("HD5397.7.T42 1844 .B5412 X, 1985", "HD5397.7.T42 1844 .B5412 1985"),
        ] {
            assert_eq!(super::local_cutter_x_base(code).as_deref(), Some(base), "{code}");
            let keys = super::lcc_match_keys(code).unwrap();
            assert_eq!(&keys[..keys.len() - 1], super::lcc_match_keys(base).unwrap());
            assert_eq!(keys.last().unwrap(), code);
        }
        for code in ["QA76 X", "QA76 .AX", "QA76 .A1 X,", "QA76 .A1X2001",
            "QA76 .A1X 201", "QA76 .A1X UNKNOWN", "QA76 .A1XX", "QA76 .A1X,2001 UNKNOWN",
            "QA76 .A1 2001 X", "QA76 .X123", "QA75-76.A1X", "WB18.2 .A1X",
            "WEB .A1X", "PZ7.A1 X", "PZ7.A1 X 2001", "PR123.A1 X 2001",
            "GV722 197 .M32 X", "GV722 19765 .M32 X"] {
            assert!(super::local_cutter_x_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn terminal_period_preserves_complete_dated_call_and_evidence() {
        for (code, base) in [
            ("LB3013.32 .A23 2005.", "LB3013.32 .A23 2005"),
            ("AC75 .S379 1993.", "AC75 .S379 1993"),
            ("PT9875.D5 M3 1976.", "PT9875.D5 M3 1976"),
            ("QA76 .A1 2001A.", "QA76 .A1 2001A"),
            ("E457.92 1967.", "E457.92 1967"),
            ("PZ7.T888 AD 1982.", "PZ7.T888 AD 1982"),
        ] {
            assert_eq!(super::lcc_item_base(code), Some(base), "{code}");
            let keys = super::lcc_match_keys(code).unwrap();
            assert_eq!(&keys[..keys.len() - 1], super::lcc_match_keys(base).unwrap());
            assert_eq!(keys.last().unwrap(), code);
        }
        for code in ["QA76.", "QA76 .A1.", "QA76 .A1 200.", "QA76 .A1 2001..",
            "QA76 .A1 2001.UNKNOWN.", "QA76 .A1 UNKNOWN 2001.", "QA76 .A 2001.",
            "QA76 .A1 2 001.", "QA75-76 2001.", "WB18.2 .A1 2001.", "WEB 2001."] {
            assert!(super::lcc_item_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn ebook_suffix_preserves_underlying_call_and_evidence() {
        for (code, base) in [
            ("PZ7.T888 AD 1982EB", "PZ7.T888 AD 1982"),
            ("B130 .I63 2001 VOL. 2EB", "B130 .I63 2001"),
            ("GT1720EB", "GT1720"),
            ("QA76 .A1 2001 EBOOK", "QA76 .A1 2001"),
            ("QA76 .A1 EB", "QA76 .A1"),
            ("BF357 .P47EB V. 2EB", "BF357 .P47"),
            ("QA76.76.D47 H46 2002EB EB", "QA76.76.D47 H46 2002"),
            ("QC178 .Q37 2002EBEB", "QC178 .Q37 2002"),
            ("QA76.5 1977.M5217EB", "QA76.5 1977.M5217"),
            ("PZ7.A1 DEBEB", "PZ7.A1 DEB"),
            ("TX553.A3 W67A NO.60EB", "TX553.A3 W67A"),
            ("QP905 .H236S V.175EB", "QP905 .H236S"),
            ("PZ3.M8346 LO5EB", "PZ3.M8346 LO5"),
        ] {
            assert_eq!(super::lcc_item_base(code), Some(base), "{code}");
            let keys = super::lcc_match_keys(code).unwrap();
            assert_eq!(&keys[..keys.len() - 1], super::lcc_match_keys(base).unwrap());
            assert_eq!(keys.last().unwrap(), code);
        }
        for code in ["EB", "WEB", "WB18.2 .A1 2001EB", "QA76 UNKNOWN EBOOK",
            "QA76 .A1 2001EB UNKNOWN", "QA76 .A1 VOL.2-EB", "QA76 . EBOOK",
            "QA76 .UNKNOWN EBOOK", "QA76 .A EBOOK", "QA76 .A1 2001UNKNOWN EBOOK",
            "QA76 .A1 2001 .UNKNOWN EBOOK", "QA76.5 197.M5217EB", "QA76.5 1977.UNKNOWN EBOOK",
            "TX553.A3 W67B NO.60EB", "TX553.A3 W67A UNKNOWN EBOOK",
            "QP905 .H236T V.175EB", "QP905 .H236S UNKNOWN EBOOK",
            "PZ3.M8346 LO6EB", "PZ3.M8346 LO5 UNKNOWN EBOOK",
            "DT159.944 S69 201 EB", "QA76.76.C672 044 2000EB"] {
            assert!(super::lcc_item_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn textbook_grade_suffix_preserves_subject_and_evidence() {
        for (code, base) in [
            ("BF723.A4 S27 2005 GR.3-6", "BF723.A4 S27 2005"),
            ("E175.8 .H37 2007 GR.5", "E175.8 .H37 2007"),
            ("QA107 .A1 2005 GRADE 3", "QA107 .A1 2005"),
            ("QA107 .A1 2005 GR.K-2", "QA107 .A1 2005"),
        ] {
            assert_eq!(super::lcc_item_base(code), Some(base));
            let keys = super::lcc_match_keys(code).unwrap();
            assert_eq!(&keys[..keys.len() - 1], super::lcc_match_keys(base).unwrap());
            assert_eq!(keys.last().unwrap(), code);
        }
        for code in ["QA107 .A1 2005 GR.3-", "QA107 .A1 2005 GR.6-3",
            "QA107 .A1 2005 GR.97", "QA107 .A1 2005 GR.3 UNKNOWN",
            "QA107 .A1 GR.3", "WB18.2 .A1 2005 GR.3", "GR.3"] {
            assert!(super::lcc_item_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn spaced_caption_period_preserves_evidence() {
        for (code, base) in [
            ("AM101 .I374 NO .56", "AM101 .I374"),
            ("DA670.S97 S97 VOL .76", "DA670.S97 S97"),
            ("PG3199 .G526 V .25", "PG3199 .G526"),
        ] {
            assert_eq!(super::lcc_enumeration_base(code), Some(base));
            let keys = super::lcc_match_keys(code).unwrap();
            assert_eq!(&keys[..keys.len() - 1], super::lcc_match_keys(base).unwrap());
            assert_eq!(keys.last().unwrap(), code);
        }
        for code in ["AM101 .I374 NO .", "DA670.S97 S97 VOL .76-",
            "PG3199 .G526 V .25 UNKNOWN", "PG3199 .G526 V ..25"] {
            assert!(super::lcc_enumeration_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn serial_year_issue_preserves_dated_call_and_evidence() {
        for (code, base) in [
            ("AM64 .R15 1974:43", "AM64 .R15 1974"),
            ("AS281 .A34 1978:51", "AS281 .A34 1978"),
            ("BF38 .H45 1975:4", "BF38 .H45 1975"),
        ] {
            assert_eq!(super::lcc_item_base(code), Some(base));
            let keys = super::lcc_match_keys(code).unwrap();
            assert_eq!(&keys[..keys.len() - 1], super::lcc_match_keys(base).unwrap());
            assert_eq!(keys.last().unwrap(), code);
        }
        for code in ["AM64 .R15 74:43", "AM64 .R15 1974:", "AM64 .R15 1974:43-",
            "AM64 .R15 1974:43 UNKNOWN", "AM64 .R15 1974:4:3", "AM64 1974:43",
            "WB18.2 .C1 1974:43", "KF1 .A1 1974:43"] {
            assert!(super::lcc_item_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn additional_conser_captions_preserve_evidence() {
        for (code, base) in [
            ("AS161 .B39 SéR. 2, VOL. 1, ETC.", "AS161 .B39"),
            ("AS613.S8 A14 JAARG. 1, AFL. 1", "AS613.S8 A14"),
            ("DC611.R437 .P48 1985 PTIE 3", "DC611.R437 .P48 1985"),
            ("PE1119.A2 B4 1989 LVL.4", "PE1119.A2 B4 1989"),
            ("LB1576 .W71 1996, LVL.4, V.6", "LB1576 .W71 1996"),
            ("AS142 .C445 ROč.101, SEš.2", "AS142 .C445"),
            ("AS142 .C445 ROC\u{30c}.101, SES\u{30c}.2", "AS142 .C445"),
        ] {
            assert_eq!(super::lcc_enumeration_base(code), Some(base), "{code}");
            let keys = super::lcc_match_keys(code).unwrap();
            assert_eq!(&keys[..keys.len() - 1], super::lcc_match_keys(base).unwrap());
            assert_eq!(keys.last().unwrap(), code);
        }
        for code in ["AS161 .B39 SéR. UNKNOWN", "AS613.S8 A14 AFL.1-",
            "DC611.R437 .P48 1985 PTIE", "AS142 .C445 ROč.101, SEš.2 UNKNOWN",
            "AS142 .C445 ROč.101, 76, SEš.2", "PE1119.A2 B4 1989 LVL.",
            "PE1119.A2 B4 1989 LVL.4-", "PE1119.A2 B4 1989 LVL.4 UNKNOWN"] {
            assert!(super::lcc_enumeration_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn finnish_number_caption_preserves_evidence() {
        for code in ["B2391.F5 A36 N:O 84", "B2391.F5 A36 N:O84", "B2391.F5 A36 N:O 84-85"] {
            let base = "B2391.F5 A36";
            assert_eq!(super::lcc_enumeration_base(code), Some(base));
            let keys = super::lcc_match_keys(code).unwrap();
            assert_eq!(&keys[..keys.len() - 1], super::lcc_match_keys(base).unwrap());
            assert_eq!(keys.last().unwrap(), code);
        }
        for code in ["B2391.F5 A36 N:O", "B2391.F5 A36 N:O 84-", "B2391.F5 A36 N:O UNKNOWN", "B2391.F5 A36 N:O 84 UNKNOWN"] {
            assert!(super::lcc_enumeration_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn workbook_series_and_volume_year_preserve_evidence() {
        for (code, base) in [
            ("BX1751.3 .I58 WORKBOOK", "BX1751.3 .I58"),
            ("PN4121 .L72 2004 STUDENT WORKBOOK", "PN4121 .L72 2004"),
            ("LC149.7 .A65 1999 V.9-15 WORKBOOK", "LC149.7 .A65 1999"),
            ("PE1128.A2 R43 1994 LVL 1, WORKBOOK", "PE1128.A2 R43 1994"),
            ("E178.1 .T67 1982 WORKBOOK TEACHER'S EDITION", "E178.1 .T67 1982"),
            ("GN31 .B7 WORKBOOK 1976A", "GN31 .B7"),
            ("PC2129.E5 S546 2007 WORKBOOK PART 2", "PC2129.E5 S546 2007"),
            ("QD251.3 .B78 2004 WORKBOOK", "QD251.3 .B78 2004"),
            ("BF637.H4 C37 1980, WORKBOOK", "BF637.H4 C37 1980"),
            ("BC25 VOL. 16 OF 2004", "BC25"),
            ("QE295.G4 V. 73 OF 2008", "QE295.G4"),
            ("N6841 .A19 NOUV. PéRIODE, T. 34", "N6841 .A19"),
            ("N6841 .A9 NOUV. PE\u{301}RIODE, T. 31", "N6841 .A9"),
        ] {
            assert_eq!(super::lcc_item_base(code), Some(base), "{code}");
            let keys = super::lcc_match_keys(code).unwrap();
            assert_eq!(&keys[..keys.len() - 1], super::lcc_match_keys(base).unwrap());
            assert_eq!(keys.last().unwrap(), code);
        }
        for code in ["BC25 VOL. 16 OF 04", "BC25 VOL. 16 OF 2004 UNKNOWN",
            "BC25 VOL. OF 2004", "BC25 VOL. 16- OF 2004",
            "N6841 .A9 NOUV. PéRIODEUNKNOWN", "N6841 .A9 NOUV. PéRIODE, T. 31-",
            "QD251.3 .B78 2004 WORKBOOK UNKNOWN", "WB18.2 .C1 2004 WORKBOOK",
            "BX1751.3 WORKBOOK", "BX1751.3 .I WORKBOOK", "LC149.7 .A65 V.9- WORKBOOK",
            "GN31 .B7 WORKBOOK 76", "GN31 .B7 WORKBOOK 1976 UNKNOWN",
            "PC2129.E5 S546 2007 WORKBOOK PART 2-", "E178.1 .T67 1982 WORKBOOK TEACHER'S EDITION UNKNOWN"] {
            assert!(super::lcc_item_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn hungarian_volume_labels_preserve_evidence() {
        for (code, base) in [
            ("LB1105 .P7 KöT. 25", "LB1105 .P7"),
            ("LB1105 .P7 KO\u{308}T 28", "LB1105 .P7"),
            ("LA681.7 B833 KöT., 8", "LA681.7 B833"),
            ("PH3194 .M3 3 KöT", "PH3194 .M3"),
            ("AS142 .M323 NEW SER., 80 KO\u{308}T.", "AS142 .M323"),
            ("AS142 .M323 NEW SER., KöT. 108", "AS142 .M323"),
            ("B809.8 .S985 1984 RéSZ 1, KöT.1", "B809.8 .S985 1984"),
            ("QE1 .H94 KöT. 63, FüZET 4", "QE1 .H94"),
            ("LA832 .P35 FU\u{308}ZET 22", "LA832 .P35"),
        ] {
            assert_eq!(super::lcc_enumeration_base(code), Some(base), "{code}");
            let keys = super::lcc_match_keys(code).unwrap();
            assert_eq!(&keys[..keys.len() - 1], super::lcc_match_keys(base).unwrap());
            assert_eq!(keys.last().unwrap(), code);
        }
        for code in ["LB1105 .P7 KöT.", "LB1105 .P7 KöT.,",
            "LB1105 .P7 KöT. 25-", "LB1105 .P7 KöT.UNKNOWN",
            "PH3194 .M3 3 KöT UNKNOWN", "AS142 .M323 NEW SER., 80 KöT.UNKNOWN",
            "QE1 .H94 KöT. 63, FüZET", "QE1 .H94 KöT. 63, FüZET 4-",
            "B809.8 .S985 1984 RéSZ UNKNOWN", "LA832 .P35 FüZET 22 UNKNOWN"] {
            assert!(super::lcc_enumeration_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn ordinal_session_enumeration_preserves_evidence() {
        for (code, base) in [
            ("HD4813 .R37 67TH SESS., NO. 4", "HD4813 .R37"),
            ("HD1521 .G46 7TH SESS., NO. 2", "HD1521 .G46"),
            ("HD5718.M615 R46 10TH SESS., NO. 2", "HD5718.M615 R46"),
        ] {
            assert_eq!(super::lcc_enumeration_base(code), Some(base), "{code}");
            let keys = super::lcc_match_keys(code).unwrap();
            assert_eq!(&keys[..keys.len() - 1], super::lcc_match_keys(base).unwrap());
            assert_eq!(keys.last().unwrap(), code);
        }
        for code in ["HD4813 .R37 71TH SESS., NO. 6", "HD4813 .R37 0TH SESS.",
            "HD4813 .R37 67TH SESS.UNKNOWN", "HD4813 .R37 67TH SESS., NO. 4-",
            "HD4813 .R37 67TH SESS., UNKNOWN"] {
            assert!(super::lcc_enumeration_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn explicit_folio_size_after_a_dated_call_preserves_evidence() {
        for (code, base) in [
            ("B74 P4414 2004 FOL.", "B74 P4414 2004"),
            ("BS575 .S52713 1994B FOL.", "BS575 .S52713 1994B"),
            ("CN1178.S5 F8 1997 FOLIO", "CN1178.S5 F8 1997"),
            ("F452 .A72 1982 <FOL>", "F452 .A72 1982"),
            ("NA6245.L92 L827 1994 <FOL.>", "NA6245.L92 L827 1994"),
            ("N6549.S5 A4 1981 FOL", "N6549.S5 A4 1981"),
            ("TA175 .P53 1989<FOL.>", "TA175 .P53 1989"),
            ("DT57 .D47 NO. 39, ETC. <FOL.>", "DT57 .D47"),
            ("PZ7.V266 WI 1992 <FOL.>", "PZ7.V266 WI 1992"),
            ("N7615.4 .N36 FOL", "N7615.4 .N36"),
            ("B74 P4414 FOLIO", "B74 P4414"),
        ] {
            assert_eq!(super::lcc_item_base(code), Some(base), "{code}");
            let keys = super::lcc_match_keys(code).unwrap();
            assert_eq!(&keys[..keys.len() - 1], super::lcc_match_keys(base).unwrap());
            assert_eq!(keys.last().unwrap(), code);
        }
        for code in ["B74 P4414 2004 FFFF", "PZ7.V266 FOLIO", "B74 P4414 2004 FOL.UNKNOWN",
            "B74 P4414 2004 FOLIO UNKNOWN", "FOLIO", "WB18.2 .C1 2004 FOLIO",
            "F452 <FOL>", "F452 .A72 1982 <FOL", "F452 .A72 1982 <FOL> UNKNOWN",
            "F452 .A72 1982 <UNKNOWN>", "WB18.2 .C1 2004 <FOL>"] {
            assert!(super::lcc_item_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn oversize_plus_after_a_complete_dated_call_preserves_evidence() {
        for (code, base) in [
            ("AP95.P3 T393 2016+", "AP95.P3 T393 2016"),
            ("B753.F34 S245 2017+", "B753.F34 S245 2017"),
            ("M452 .B415 1989++", "M452 .B415 1989"),
        ] {
            assert_eq!(super::lcc_item_base(code), Some(base), "{code}");
            let keys = super::lcc_match_keys(code).unwrap();
            assert_eq!(&keys[..keys.len() - 1], super::lcc_match_keys(base).unwrap());
            assert_eq!(keys.last().unwrap(), code);
        }
        for code in ["QA76+", "QA76.C1 20+", "QA76.C1 2017+++", "QA76 2017+",
            "QA76.C1 2017+ UNKNOWN", "LAW+", "WB18.2 .C1 2017+", "QA76.C1-QA77.C1 2017+"] {
            assert!(super::lcc_item_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn shelving_lc_suffix_requires_a_complete_known_call() {
        for (code, base) in [
            ("AC149 .H758 1989 (LC)", "AC149 .H758 1989"),
            ("B2799.A4 K63 2001 (LC)", "B2799.A4 K63 2001"),
            ("QA3 .L28 VOL. 1-3 (LC)", "QA3 .L28"),
        ] {
            assert_eq!(super::lcc_item_base(code), Some(base), "{code}");
            let keys = super::lcc_match_keys(code).unwrap();
            assert_eq!(&keys[..keys.len() - 1], super::lcc_match_keys(base).unwrap());
            assert_eq!(keys.last().unwrap(), code);
        }
        for code in ["(LC)", "CPB (LC)", "LAW (LC)", "WB18.2 (LC)",
            "QA3 UNKNOWN (LC)", "QA3 .L28 VOL. 1- (LC)", "QA3 (LC) EXTRA"] {
            assert!(super::lcc_item_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn french_and_scandinavian_series_preserve_accented_evidence() {
        for (code, base) in [
            ("GA862 .A33 NOUV. SER., VOL. 18", "GA862 .A33"),
            ("GA862 .A33 NOUV. SéR., VOL. 14", "GA862 .A33"),
            ("GA862 .A33 NOUV. SE\u{301}R., VOL. 14", "GA862 .A33"),
            ("AS162 .P44 NOUV. SéR., 2", "AS162 .P44"),
            ("AS283 .O54 NY SER., NO. 18", "AS283 .O54"),
            ("QA3 .L28 N.S., 2", "QA3 .L28"),
        ] {
            assert_eq!(super::lcc_enumeration_base(code), Some(base), "{code}");
            let keys = super::lcc_match_keys(code).unwrap();
            assert_eq!(&keys[..keys.len() - 1], super::lcc_match_keys(base).unwrap());
            assert_eq!(keys.last().unwrap(), code);
        }
        for code in ["GA862 .A33 NOUV. SER.,", "GA862 .A33 NOUV. SéR.UNKNOWN",
            "GA862 .A33 NOUV. SER., 2 UNKNOWN", "AS283 .O54 NY SER., NO.?",
            "QA3 .L28 N.S.,, 2"] {
            assert!(super::lcc_enumeration_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn ordinal_volume_and_series_captions_keep_complete_endpoints() {
        for (code, base) in [
            ("AS142 .V32 294. BD.", "AS142 .V32"),
            ("BL25 .R4 II. REIHE, 15. HFT.", "BL25 .R4"),
            ("BT25 R4 II. REIHE, 21./22. HFT.", "BT25 R4"),
            ("BR45 .T4 3. REIHE, 7. BD., HFT. 2", "BR45 .T4"),
        ] {
            assert_eq!(super::lcc_enumeration_base(code), Some(base), "{code}");
            let keys = super::lcc_match_keys(code).unwrap();
            assert_eq!(&keys[..keys.len() - 1], super::lcc_match_keys(base).unwrap());
            assert_eq!(keys.last().unwrap(), code);
        }
        for code in ["BT25 R4 IVX. REIHE", "BT25 R4 II. UNKNOWN", "BT25 R4 21./ HFT.",
            "BT25 R4 21./22 HFT.", "BT25 R4 0. BD.", "BT25 R4 21.. HFT."] {
            assert!(super::lcc_enumeration_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn additional_part_and_chapter_captions_preserve_original_evidence() {
        for (code, base) in [
            ("BS410 .Z7 BAND 236", "BS410 .Z7"),
            ("BS1154.2 .B5 BD. 14, TEIL 4", "BS1154.2 .B5"),
            ("BS1154 .A6 TEILBD. 22/2 2001", "BS1154 .A6"),
            ("B3240 .F524 2006X, ABT. 2, BD. 5, TEILBD. 2", "B3240 .F524 2006X"),
            ("BS1235.52 .H473 CH.1-11 2018", "BS1235.52 .H473"),
            ("GB1217 .F58 1993 CH. K", "GB1217 .F58 1993"),
            ("BJ1661 .S25 1984, SEC.3, NO.4", "BJ1661 .S25 1984"),
            ("DS251 .S79 CAHIER 44", "DS251 .S79"),
        ] {
            assert_eq!(super::lcc_enumeration_base(code), Some(base), "{code}");
            let keys = super::lcc_match_keys(code).unwrap();
            assert_eq!(&keys[..keys.len() - 1], super::lcc_match_keys(base).unwrap());
            assert_eq!(keys.last().unwrap(), code);
        }
        for code in ["BS1154 .A6 TEILBD., 5", "BS1154 .A6 TEILBD. 22/",
            "BS1154 .A6 TEIL UNKNOWN", "BS1154 .A6 BAND", "BS1154 .A6 CH.UNKNOWN",
            "BS1154 .A6 SEC. 1ERCYCLE", "BS1154 .A6 CAHIER 44 UNKNOWN"] {
            assert!(super::lcc_enumeration_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn explicit_copy_labels_preserve_classification_and_evidence() {
        for (code, base) in [
            ("BS2585.3 .M37 COPY 2", "BS2585.3 .M37"),
            ("DD286.4 .M57 2007 COPY 1", "DD286.4 .M57 2007"),
            ("BS2545.P68 M377 COP. 3", "BS2545.P68 M377"),
            ("BV4211.2 .E382 1982 COP. 2, COP. 3", "BV4211.2 .E382 1982"),
        ] {
            assert_eq!(super::lcc_enumeration_base(code), Some(base), "{code}");
            let keys = super::lcc_match_keys(code).unwrap();
            assert_eq!(&keys[..keys.len() - 1], super::lcc_match_keys(base).unwrap());
            assert_eq!(keys.last().unwrap(), code);
        }
        for code in ["BS2585.3 .M37 COPY", "BS2585.3 .M37 COPY UNKNOWN",
            "BS2585.3 .M37 COPY 2 UNKNOWN", "BS2585.3 .M37 COP. 2,",
            "BS2585.3 .M37 COP. 2-", "BS2585.3 .M37 COPIED 2"] {
            assert!(super::lcc_enumeration_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn comma_separated_item_numbers_keep_the_complete_classification() {
        for (code, base) in [
            ("BF1301 .N65 VOL.5,6,7", "BF1301 .N65"),
            ("BR300 .V5 NO.88-89,94", "BR300 .V5"),
            ("BR60 .C49 V. 138, 138A, SUPP. A", "BR60 .C49"),
            ("BR60 .C5 V.389-390, 398-399", "BR60 .C5"),
            ("BR743.2 .E3 NO. 7,8 1966A", "BR743.2 .E3"),
            ("DA685.S79 L62X NO. 1, 2ND ED.", "DA685.S79 L62X"),
            ("PN4 .T45 HEFT 44, 3RD ED.", "PN4 .T45"),
            ("PT2603.E46 .T45 HEFT 44, 3RD ED.", "PT2603.E46 .T45"),
            ("TA1245 .C56 1984 VOL. 2, PT H, 2ND ED.", "TA1245 .C56 1984"),
        ] {
            assert_eq!(super::lcc_enumeration_base(code), Some(base), "{code}");
            let keys = super::lcc_match_keys(code).unwrap();
            assert_eq!(&keys[..keys.len() - 1], super::lcc_match_keys(base).unwrap());
            assert_eq!(keys.last().unwrap(), code);
        }
        for code in ["BF1301 .N65 VOL.5,", "BF1301 .N65 VOL.5,,6",
            "BF1301 .N65 VOL.5,6-", "BF1301 .N65 VOL.5,6UNKNOWN",
            "BF1301 .N65 VOL.5,QA76", "BF1301 .N65 5,6,7"] {
            assert!(super::lcc_enumeration_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn supplementary_volumes_require_complete_numbered_captions() {
        for (code, base) in [
            ("ML5.A63 SUPPL.BD.14", "ML5.A63"),
            ("ML1700 .A63 SUPPL. BD. 20", "ML1700 .A63"),
            ("B11 .A7 SUPPL., V.62, 1988", "B11 .A7"),
            ("B11 .A7 SUPP. V.61", "B11 .A7"),
        ] {
            assert_eq!(super::lcc_enumeration_base(code), Some(base), "{code}");
            let keys = super::lcc_match_keys(code).unwrap();
            assert_eq!(&keys[..keys.len() - 1], super::lcc_match_keys(base).unwrap());
            assert_eq!(keys.last().unwrap(), code);
        }
        for code in ["ML5.A63 SUPPL.BD.", "ML5.A63 SUPPL.BD.UNKNOWN",
            "ML5.A63 SUPPL.BD.14UNKNOWN", "ML5.A63 SUPPL., UNKNOWN 2",
            "ML5.A63 SUPPL.,", "ML5.A63 SUPPL..BD.14", "ML5.A63 SUPPL. V."] {
            assert!(super::lcc_enumeration_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn documented_supplement_and_numbered_item_labels_preserve_evidence() {
        // Western Libraries' spine-label table documents supp., supl., Reihe,
        // book and unit. Only a complete captioned suffix is detached.
        for (code, base) in [
            ("AS911.A2 F65 2014 SUPP.", "AS911.A2 F65 2014"),
            ("AC145 .I93 1982 SUPP.2 V.14", "AC145 .I93 1982"),
            ("BR45 .C6 V.115 SUPP.A", "BR45 .C6"),
            ("HF5351 .K383 1975 SUPL.", "HF5351 .K383 1975"),
            ("B2967 .B6 1975 REIHE 2A BD. 12", "B2967 .B6 1975"),
            ("CD1375.L854 V44 REIHE B, BD. 29", "CD1375.L854 V44"),
            ("QA3 .L28 BOOK 2", "QA3 .L28"),
            ("D387 .R425 PT. 1, UNIT 1", "D387 .R425"),
            ("KF3369.A1 L26 UNIT 4 1971", "KF3369.A1 L26"),
        ] {
            assert_eq!(super::lcc_enumeration_base(code), Some(base), "{code}");
            let keys = super::lcc_match_keys(code).unwrap();
            assert_eq!(&keys[..keys.len() - 1], super::lcc_match_keys(base).unwrap());
            assert_eq!(keys.last().unwrap(), code);
        }
        for code in ["QA3 .L28 SUPP.UNKNOWN", "QA3 .L28 SUPL.?", "QA3 .L28 REIHE UNKNOWN",
            "QA3 .L28 BOOK NOT YET IN LC", "QA3 .L28 UNIT", "QA3 .L28 UNIT 1 UNKNOWN",
            "BOOK NOT YET IN LC", "IN PROCESS UNIT 1"] {
            assert!(super::lcc_enumeration_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn legacy_new_series_caption_keeps_the_classification() {
        for code in ["QL255 .S95 N.S., NO.", "QL255 .S95 N.S. VOL. 2", "QL255 .S95 N.S.2"] {
            assert_eq!(super::lcc_enumeration_base(code), Some("QL255 .S95"));
            let keys = super::lcc_match_keys(code).unwrap();
            assert_eq!(&keys[..keys.len() - 1], super::lcc_match_keys("QL255 .S95").unwrap());
            assert_eq!(keys.last().unwrap(), code);
        }
        for code in ["QL255 .S95 N.S.UNKNOWN", "QL255 .S95 N.S.,", "QL255 .S95 N.S. VOL.?"] {
            assert!(super::lcc_enumeration_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn semicolon_years_keep_the_complete_call_and_original_evidence() {
        for (code, base) in [("GR203.135 .K33;1998", "GR203.135 .K33"),
            ("Z1073 .P63;1999", "Z1073 .P63"), ("BF633 .D87; 1994", "BF633 .D87")] {
            assert_eq!(super::lcc_item_base(code), Some(base));
            let keys = super::lcc_match_keys(code).unwrap();
            assert_eq!(keys.last().unwrap(), code);
            assert_eq!(&keys[..keys.len() - 1], super::lcc_match_keys(base).unwrap());
        }
        for code in ["WB18.2;1998", "LAW;1998", "BF633 .D;1994", "BF633 .D87;", "BF633 .D87;19", "BF633 .D87;1994 EXTRA", "BF633 .D87;QA76", "QA75-;1998"] {
            assert!(super::lcc_item_base(code).is_none(), "{code}");
        }
    }

    #[test]
    fn apostrophe_calls_can_retain_recognized_volume_metadata() {
        for code in ["BR'60'A64'V.", "BR'60'A64'VOL. 2"] {
            let canonical = super::canonical_lcc_notation(code).unwrap();
            let keys = super::lcc_match_keys(&canonical).unwrap();
            assert!(keys.iter().any(|k| k == "BR60.A64"));
            assert_eq!(keys.last().unwrap(), code);
        }
        assert!(super::canonical_lcc_notation("BR'60'A64'V.UNKNOWN").is_none());
    }
}
