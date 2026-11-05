//! Optional outline reference for curation reports. Application matching uses
//! the selected taxonomy's own selectors and classification indexes.
use std::{error::Error, path::Path};
use subject_taxonomy::{
    compact_whitespace,
    lcc::{canonical_lcc_notation, compare_lcc_key, lcc_match_keys, parse_lcc_call},
};

pub struct LccOutline {
    ranges: Vec<Range>,
}

struct Range {
    start_letters: String,
    start_number: f64,
    end_letters: String,
    end_number: f64,
    path: String,
}

impl LccOutline {
    pub fn read(path: &Path) -> Result<Self, Box<dyn Error>> {
        let mut input = csv::Reader::from_path(path)?;
        let mut ranges = Vec::new();
        for row in input.records() {
            let row = row?;
            ranges.push(Range {
                start_letters: row.get(0).unwrap_or_default().to_owned(),
                start_number: row.get(1).unwrap_or_default().parse()?,
                end_letters: row.get(2).unwrap_or_default().to_owned(),
                end_number: row.get(3).unwrap_or_default().parse()?,
                path: row.get(4).unwrap_or_default().to_owned(),
            });
        }
        Ok(Self { ranges })
    }

    pub fn subject_path(&self, notation: &str) -> Option<String> {
        if notation.contains(';') {
            return None;
        }
        let normalized = canonical_lcc_notation(notation)?;
        let keys = lcc_match_keys(&normalized)?;
        if let Some(assignment) = keys.first().filter(|key| key.starts_with('(')) {
            let letter = assignment.chars().nth(1)?;
            let main = self.ranges.iter().find(|range| range.start_letters.starts_with(letter))?.path.split(" / ").next()?;
            return Some(subject_path(main, &normalized));
        }
        if normalized.contains('-') {
            return None;
        }
        let call = parse_lcc_call(&normalized)?;
        let best = self
            .ranges
            .iter()
            .filter(|range| compare_lcc_key(&range.start_letters, range.start_number, &call.letters, call.number).is_le() && compare_lcc_key(&call.letters, call.number, &range.end_letters, range.end_number).is_le())
            .max_by(|left, right| {
                let left_exact = left.start_letters == call.letters && left.end_letters == call.letters;
                let right_exact = right.start_letters == call.letters && right.end_letters == call.letters;
                left.path.matches(" / ").count().cmp(&right.path.matches(" / ").count()).then_with(|| left_exact.cmp(&right_exact)).then_with(|| {
                    let left_span = if left_exact { left.end_number - left.start_number } else { f64::INFINITY };
                    let right_span = if right_exact { right.end_number - right.start_number } else { f64::INFINITY };
                    right_span.total_cmp(&left_span)
                })
            })?;
        Some(subject_path(&best.path, notation))
    }
}

fn subject_path(outline_path: &str, display: &str) -> String {
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
