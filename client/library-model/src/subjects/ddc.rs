use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Default)]
struct DdcSummaries {
    main: HashMap<String, String>,
    division: HashMap<String, String>,
    section: HashMap<String, String>,
}

fn summaries() -> &'static DdcSummaries {
    static SUMMARIES: OnceLock<DdcSummaries> = OnceLock::new();
    SUMMARIES.get_or_init(|| {
        let mut summaries = DdcSummaries::default();
        let mut reader = csv::Reader::from_reader(include_str!("../../data/ddc23-summaries.csv").as_bytes());
        for record in reader.records() {
            let record = record.expect("the embedded DDC summary CSV must be valid");
            let target = match record.get(0).unwrap_or_default() {
                "main" => &mut summaries.main,
                "division" => &mut summaries.division,
                "section" => &mut summaries.section,
                _ => panic!("the embedded DDC summary CSV contains an unknown level"),
            };
            target.insert(record.get(1).unwrap_or_default().to_owned(), record.get(2).unwrap_or_default().to_owned());
        }
        summaries
    })
}

pub fn ddc_subject_path(notation: &str) -> Option<String> {
    let notation = canonical_ddc_notation(notation)?;
    let digits = notation.as_bytes();
    let main_code = format!("{}00", char::from(digits[0]));
    let division_code = format!("{}{}0", char::from(digits[0]), char::from(digits[1]));
    let summaries = summaries();
    let labels = [summaries.main.get(&main_code)?, summaries.division.get(&division_code)?, summaries.section.get(&notation[..3])?];
    if labels.iter().any(|label| label.starts_with('[')) {
        return None;
    }
    let mut components = vec!["DDC".to_owned()];
    for label in labels {
        if components.last().is_none_or(|existing| !existing.eq_ignore_ascii_case(label)) {
            components.push(label.clone());
        }
    }
    if notation.len() > 3 {
        components.push(notation);
    }
    Some(components.join(" / "))
}

pub(super) fn canonical_ddc_notation(value: &str) -> Option<String> {
    let compact = value.chars().filter(|character| !character.is_whitespace() && *character != '/').collect::<String>();
    if compact.len() == 3 && compact.bytes().all(|byte| byte.is_ascii_digit()) {
        return Some(compact);
    }
    let (whole, decimal) = compact.split_once('.')?;
    if whole.len() == 3 && whole.bytes().all(|byte| byte.is_ascii_digit()) && !decimal.is_empty() && decimal.bytes().all(|byte| byte.is_ascii_digit()) {
        Some(format!("{whole}.{decimal}"))
    } else {
        None
    }
}
