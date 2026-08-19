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
}

fn outline() -> &'static LccOutline {
    static OUTLINE: OnceLock<LccOutline> = OnceLock::new();
    OUTLINE.get_or_init(|| {
        let mut outline = LccOutline::default();
        let mut reader = csv::Reader::from_reader(include_str!("../../data/lcc-2024-outline.csv").as_bytes());
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

pub fn lcc_subject_path(notation: &str) -> Option<String> {
    if let Some(path) = letters_only_path(notation) {
        return Some(path);
    }
    let call = parse_lcc_call(notation)?;
    let ranges = outline().ranges_by_main.get(&call.letters.chars().next()?)?;
    let best = ranges.iter().filter(|range| range_contains(range, &call)).max_by(|left, right| compare_matches(left, right, &call))?;
    Some(build_subject_path(&best.path, call.display))
}

/// Normalizes a syntactically valid LCC class or call number without
/// requiring that the currently installed outline already contains it.
/// This keeps collection of external identifiers independent from local
/// taxonomy coverage so finer server-side schedules can be requested later.
pub fn canonical_lcc_notation(notation: &str) -> Option<String> {
    let notation = compact_whitespace(notation).to_ascii_uppercase();
    if letters_only_syntax(&notation) || parse_lcc_call(&notation).is_some() { Some(notation) } else { None }
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
    Some(LccCall { letters, number, display: value })
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
