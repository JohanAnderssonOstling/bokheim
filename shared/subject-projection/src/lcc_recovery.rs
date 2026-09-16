//! Evidence-bearing recovery of complete classifications from damaged fields.
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LccMatchMethod { Exact, Normalized, RecoveredComponent, BroadFallback }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LccMatchEvidence {
    pub method: LccMatchMethod,
    pub code: String,
    /// Candidate concepts whose shared ancestry supports a broad fallback.
    pub supporting_concept_ids: Vec<i64>,
    /// Complete alternate readings considered for an ambiguous source string.
    pub interpretations: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LccMatchAssignment {
    pub concept_id: i64,
    pub evidence: Vec<LccMatchEvidence>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LccMatchReport {
    pub original: String,
    pub assignments: Vec<LccMatchAssignment>,
    pub unresolved_fragments: Vec<String>,
}

pub(super) fn report(
    original: &str,
    strict: impl Fn(&str) -> Vec<i64>,
    known_subclass: impl Fn(&str) -> bool,
) -> LccMatchReport {
    let mut result = LccMatchReport { original: original.to_owned(), assignments: Vec::new(), unresolved_fragments: Vec::new() };
    let exact = strict(original);
    if !exact.is_empty() {
        let canonical = crate::canonical_lcc_notation(original);
        let method = if canonical.as_deref() == Some(original.trim())
            && canonical.as_deref().is_some_and(|s| crate::lcc::lcc_item_base(s).is_none() && crate::lcc::lcc_code_list(s).is_none()) {
            LccMatchMethod::Exact
        } else { LccMatchMethod::Normalized };
        result.assignments = exact.into_iter().map(|concept_id| LccMatchAssignment {
            concept_id, evidence: vec![LccMatchEvidence { method, code: original.to_owned(), supporting_concept_ids: Vec::new(), interpretations: Vec::new() }],
        }).collect();
        return result;
    }
    // Keep recovery bounded. Individual list members may use the strict
    // parser's Unicode/MARC normalization; joined boundary scanning is ASCII.
    if original.len() > 512 {
        result.unresolved_fragments.push(original.to_owned());
        return result;
    }
    let is_start = |value: &str| {
        let letters = value.bytes().take_while(u8::is_ascii_alphabetic).count();
        letters > 0 && letters <= 4 && known_subclass(&value[..letters])
            && value[letters..].trim_start().as_bytes().first().is_some_and(u8::is_ascii_digit)
    };
    let mut assignments: BTreeMap<i64, Vec<LccMatchEvidence>> = BTreeMap::new();
    for (part_index, raw) in original.split([';', '|', '\n']).enumerate() {
        let segment = raw.trim();
        if segment.is_empty() { continue; }
        let upper = segment.to_ascii_uppercase();
        let initial_letters = upper.bytes().take_while(u8::is_ascii_alphabetic).count();
        if (part_index > 0 && initial_letters == 1 && !upper.contains(['.', '-']))
            || upper.split_whitespace().any(|word| matches!(word, "DATE" | "YEAR")) {
            result.unresolved_fragments.push(segment.to_owned());
            continue;
        }
        if confirmed_clipped_prefix(&upper).is_none() && (!segment.is_ascii() || !is_start(&upper)) {
            let ids = strict(segment);
            if !ids.is_empty() {
                for id in ids {
                    let evidence = LccMatchEvidence { method: LccMatchMethod::RecoveredComponent, code: segment.to_owned(), supporting_concept_ids: Vec::new(), interpretations: Vec::new() };
                    let entries = assignments.entry(id).or_default();
                    if !entries.contains(&evidence) { entries.push(evidence); }
                }
                continue;
            }
        }
        if !segment.is_ascii() {
            result.unresolved_fragments.push(segment.to_owned());
            continue;
        }
        // A processing marker or another system cannot be turned into LCC by
        // searching arbitrary prose for an embedded class-looking token.
        if !is_start(&upper) {
            result.unresolved_fragments.push(segment.to_owned());
            continue;
        }
        // The original editions prove these final components were clipped.
        // Syntactic validity of a surviving prefix (e.g. QB98) is not proof
        // that it is the original complete classification.
        let verified_prefix = confirmed_clipped_prefix(&upper);
        let limit = verified_prefix.map_or(segment.len(), str::len);
        if limit < segment.len() { result.unresolved_fragments.push(segment[limit..].to_owned()); }
        let segment = &segment[..limit];
        let upper = &upper[..limit];
        let mut boundaries = vec![0];
        for at in 1..upper.len() {
            if !upper.as_bytes()[at].is_ascii_uppercase()
                || !(upper.as_bytes()[at - 1].is_ascii_digit() || upper.as_bytes()[at - 1] == b'.') { continue; }
            let letters = upper[at..].bytes().take_while(u8::is_ascii_alphabetic).count();
            // Undotted single-letter suffixes can be item Cutters. Preserve
            // that ambiguity until a separate boundary can establish a call.
            let range_boundary = letters == 1 && upper[..at].contains('-') && upper[at..].contains('-');
            let terminal_class = letters >= 2 && at + letters == upper.len() && known_subclass(&upper[at..]);
            if terminal_class || ((letters >= 2 || range_boundary) && is_start(&upper[at..])) { boundaries.push(at); }
        }
        boundaries.push(segment.len());
        let mut cursor = 0;
        while cursor + 1 < boundaries.len() {
            let start = boundaries[cursor];
            // A trailing bare subclass in a damaged joined field may be the
            // beginning of a clipped call. Keep it as a fragment, not a book's
            // independent broad classification.
            if cursor > 0 && upper[start..].bytes().all(|b| b.is_ascii_alphabetic()) && known_subclass(&upper[start..]) {
                result.unresolved_fragments.push(segment[start..].to_owned());
                cursor += 1;
                continue;
            }
            let mut recovered = None;
            // Once whole-field matching has failed, preserve independently
            // complete components. A longer accepted prefix can merely be a
            // broad interpretation that hides another valid classification.
            // Try a later boundary only if the first cut is incomplete (for
            // example a dot preceding a multi-letter Cutter).
            for next in cursor + 1..boundaries.len() {
                let candidate = segment[start..boundaries[next]].trim();
                let ids = strict(candidate);
                if !ids.is_empty() { recovered = Some((next, candidate, ids)); break; }
            }
            if let Some((next, candidate, ids)) = recovered {
                for id in ids {
                    let evidence = LccMatchEvidence { method: LccMatchMethod::RecoveredComponent, code: candidate.to_owned(), supporting_concept_ids: Vec::new(), interpretations: Vec::new() };
                    let entries = assignments.entry(id).or_default();
                    if !entries.contains(&evidence) { entries.push(evidence); }
                }
                cursor = next;
                continue;
            }
            let fragment = segment[start..boundaries[cursor + 1]].trim();
            // Unknown trailing text may follow a complete leading call. Only
            // whitespace establishes this boundary; never trim missing digits.
            let mut leading = None;
            for (at, _) in fragment.match_indices(char::is_whitespace).rev() {
                let candidate = fragment[..at].trim_end();
                if candidate.to_ascii_uppercase().ends_with(" BOX") { continue; }
                let ids = strict(candidate);
                if !ids.is_empty() { leading = Some((at, candidate, ids)); break; }
            }
            if let Some((at, candidate, ids)) = leading {
                for id in ids {
                    let evidence = LccMatchEvidence { method: LccMatchMethod::RecoveredComponent, code: candidate.to_owned(), supporting_concept_ids: Vec::new(), interpretations: Vec::new() };
                    let entries = assignments.entry(id).or_default();
                    if !entries.contains(&evidence) { entries.push(evidence); }
                }
                result.unresolved_fragments.push(fragment[at..].trim().to_owned());
            } else { result.unresolved_fragments.push(fragment.to_owned()); }
            cursor += 1;
        }
    }
    result.assignments = assignments.into_iter().map(|(concept_id, evidence)| LccMatchAssignment { concept_id, evidence }).collect();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn resolve(code: &str) -> Vec<i64> {
        match code { "QA76" | "QA76 .A1" => vec![1], "QC1" => vec![2], "HD28-70" => vec![3], _ => vec![] }
    }
    fn known(code: &str) -> bool { matches!(code, "QA" | "QC" | "HD") }
    #[test]
    fn recovery_keeps_complete_components_and_unresolved_evidence() {
        let r = report("HD28-70HD28-70QA76.", resolve, known);
        assert_eq!(r.assignments.len(), 1);
        assert_eq!(r.assignments[0].concept_id, 3);
        assert_eq!(r.assignments[0].evidence[0].code, "HD28-70");
        assert_eq!(r.unresolved_fragments, ["QA76."]);
        assert_eq!(r.original, "HD28-70HD28-70QA76.");
    }
    #[test]
    fn confirmed_clipped_tail_cannot_masquerade_as_complete_code() {
        let r = report("QB1-991QB460-466QB98", |s| match s {
            "QB1-991QB460-466" => vec![1, 2], "QB98" => vec![3], _ => vec![],
        }, |s| s == "QB");
        assert_eq!(r.assignments.iter().map(|a| a.concept_id).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(r.unresolved_fragments, ["QB98"]);
    }

    #[test]
    fn explicit_lists_and_trailing_text_preserve_good_calls() {
        let r = report("QA76 .A1 UNKNOWN; QC1; LAW", resolve, known);
        assert_eq!(r.assignments.iter().map(|a| a.concept_id).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(r.unresolved_fragments, ["UNKNOWN", "LAW"]);
        assert!(r.assignments.iter().all(|a| a.evidence[0].method == LccMatchMethod::RecoveredComponent));
    }
    #[test]
    fn unknown_systems_and_embedded_prose_are_not_lcc() {
        for code in ["WB18.2", "IN PROCESS QA76", "CPB", "LAW", "WEB QA76"] {
            let r = report(code, resolve, known);
            assert!(r.assignments.is_empty());
            assert_eq!(r.unresolved_fragments, [code]);
        }
        assert_eq!(report("QA76", resolve, known).assignments[0].evidence[0].method, LccMatchMethod::Exact);
    }
}

/// Numeric space represented by a visibly unfinished code, not a repaired code.
#[derive(Clone, Debug)]
pub(super) struct FallbackScope {
    pub letters: String,
    pub lower: f64,
    pub upper: Option<f64>,
    pub point_family: bool,
    pub numeric_prefix: Option<String>,
}
impl FallbackScope {
    pub fn intersects_numeric_prefix(&self, lower: f64, upper: f64) -> bool {
        let Some(prefix) = &self.numeric_prefix else { return false; };
        if let Some((_, fraction)) = prefix.split_once('.') {
            let end = self.lower + 10_f64.powi(-(fraction.len() as i32));
            return lower < end && upper >= self.lower;
        }
        // Integer prefixes can continue before the decimal point too.
        // Include every numeric interval sharing those leading digits.
        (0..=19).any(|power| {
            let scale = 10_f64.powi(power);
            lower < (self.lower + 1.0) * scale && upper >= self.lower * scale
        })
    }

    pub fn contains(&self, number: f64) -> bool {
        if let Some(prefix) = &self.numeric_prefix { number.to_string().starts_with(prefix) }
        else if self.point_family { number == self.lower }
        else { number >= self.lower && self.upper.is_none_or(|upper| number < upper) }
    }
}

pub(super) fn fallback_scope(fragment: &str) -> Option<FallbackScope> {
    let fragment = fragment.trim().to_ascii_uppercase();
    if fragment.len() > 128 || fragment.contains(char::is_whitespace) { return None; }
    let (stem, open_range) = if let Some(stem) = fragment.strip_suffix('-').or_else(|| fragment.strip_suffix('+')) {
        (stem, true)
    } else { (fragment.strip_suffix('.').or_else(|| fragment.strip_suffix('*'))?, false) };
    if stem.contains('-') { return None; }
    let call = crate::lcc::parse_lcc_call(stem)?;
    if !call.remainder.is_empty() || !call.number.is_finite() { return None; }
    let point_family = !open_range && stem.contains('.');
    Some(FallbackScope {
        letters: call.letters, lower: call.number,
        upper: if open_range || point_family { None } else { Some(call.number + 1.0) },
        point_family,
        // A star may indicate truncation at any following digit. Include
        // both QA7.x and QA70x rather than assuming an item-Cutter suffix.
        numeric_prefix: fragment.ends_with('*').then(|| call.number.to_string()),
    })
}

/// Enumerate bounded 0/O readings of a malformed call, preserving every other
/// character. Valid original calls are never rewritten just to obtain a match.
pub(super) fn ambiguous_interpretations(fragment: &str, known: impl Fn(&str) -> bool) -> Vec<String> {
    let text = fragment.trim().to_ascii_uppercase();
    if text.len() > 128 || !text.is_ascii() || text.split([';', '|', '\n']).any(|part| confirmed_clipped_prefix(part.trim()).is_some()) || crate::canonical_lcc_notation(&text).is_some() { return Vec::new(); }
    let letters = text.bytes().take_while(u8::is_ascii_alphabetic).count();
    if letters == 0 || letters > 4 || !known(&text[..letters]) { return Vec::new(); }
    let mut positions = Vec::new();
    let mut offset = 0;
    for token in text.split_inclusive(char::is_whitespace) {
        let word = token.trim_end();
        // An already complete publication year supplies no character ambiguity
        // to repair. Unknown literal dates retain their existing safeguards.
        if matches!(word, "DATE" | "YEAR") { return Vec::new(); }
        let year = offset > 0 && word.len() == 4 && word.bytes().all(|b| b.is_ascii_digit());
        if !year {
            for (i, byte) in word.bytes().enumerate() {
                let at = offset + i;
                if at < letters || !matches!(byte, b'0' | b'O') { continue; }
                let previous = at.checked_sub(1).and_then(|n| text.as_bytes().get(n)).copied();
                let next_digit = text.as_bytes().get(at + 1).is_some_and(u8::is_ascii_digit);
                let plausible = if byte == b'O' {
                    // An undotted O next to a number can be a damaged digit.
                    // A dotted O is already a valid Cutter initial.
                    previous.is_some_and(|b| b.is_ascii_digit())
                        || (previous.is_some_and(|b| b.is_ascii_whitespace()) && next_digit)
                } else if previous == Some(b'.') && next_digit {
                    // A numeric zero is only a possible Cutter initial after
                    // the class number. Preserve numeric parts of every head
                    // in joined/ranged fields, even when a later suffix fails.
                    let mut numeric_end = None;
                    for start in 0..at {
                        if !text.as_bytes()[start].is_ascii_alphabetic()
                            || (start > 0 && text.as_bytes()[start - 1].is_ascii_alphabetic()) { continue; }
                        let count = text[start..].bytes().take_while(u8::is_ascii_alphabetic).count();
                        if count <= 4 && known(&text[start..start + count]) {
                            if let Some(call) = crate::lcc::parse_lcc_call(&text[start..]) {
                                numeric_end = Some(text.len() - call.remainder.len());
                            }
                        }
                    }
                    numeric_end.is_some_and(|end| at >= end && !text[end..at].contains('-'))
                } else { false };
                if plausible { positions.push(at); }
            }
        }
        offset += token.len();
    }
    if positions.is_empty() || positions.len() > 5 { return Vec::new(); }
    let mut interpretations = std::collections::BTreeSet::new();
    for mask in 1..(1usize << positions.len()) {
        let mut bytes = text.as_bytes().to_vec();
        for (bit, at) in positions.iter().enumerate() {
            if mask & (1 << bit) != 0 { bytes[*at] = if bytes[*at] == b'0' { b'O' } else { b'0' }; }
        }
        let variant = String::from_utf8(bytes).unwrap();
        if crate::canonical_lcc_notation(&variant).is_some() { interpretations.insert(variant); }
    }
    interpretations.into_iter().collect()
}

fn confirmed_clipped_prefix(value: &str) -> Option<&'static str> {
    match value {
            "T385TA1637-1638TK788" => Some("T385TA1637-1638"),
            "QB1-991QB460-466QB98" => Some("QB1-991QB460-466"),
            "QA315-316QA402.3QA40" => Some("QA315-316QA402.3"),
            "TJ1075-1081TA467TA41" => Some("TJ1075-1081TA467"),
            "QC173.45-173.458TA40" | "QC173.45-173.458TA41" => Some("QC173.45-173.458"),
            "SB123-123.5S494.5.B5" => Some("SB123-123.5"),
            "Q334-342TA329-348TA6" => Some("Q334-342TA329-348"),
            "QC138-168.86QA930QC1" => Some("QC138-168.86QA930"),
            "QA639.5-640.7QA640.7" => Some("QA639.5-640.7"),
            "QA76.9.C66QA76.76.A6" => Some("QA76.9.C66"),
            _ => None,
    }
}

#[cfg(test)]
mod ambiguity_tests {
    use super::*;
    #[test]
    fn numeric_zeros_are_not_changed_to_hide_unrelated_damage() {
        for code in ["B11902001b", "B28051962a", "B2905x 1981", "PN1010-PN15", "QA76.063UNKNOWN", "B65H61.15HV6001-HV72"] {
            assert!(ambiguous_interpretations(code, |_| true).is_empty(), "{code}");
        }
        assert_eq!(ambiguous_interpretations("QA76.76.063", |s| s == "QA"), ["QA76.76.O63"]);
        assert_eq!(ambiguous_interpretations("AP5O", |s| s == "AP"), ["AP50"]);
    }

    #[test]
    fn character_recovery_preserves_known_clipping_and_processing_boundaries() {
        for code in ["QB1-991QB460-466QB98", "QA76.76.063; QB1-991QB460-466QB98", "IN PROCESS QA76.76.063", "QA76.76.063 YEAR", "QA76.76.063 DATE"] {
            assert!(ambiguous_interpretations(code, |s| matches!(s, "QA" | "QB")).is_empty(), "{code}");
        }
    }
}
