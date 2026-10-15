//! Exact source headings can supply classification even without an indexed ISBN.
use book_model::{BookRecord, BookSubject};
use metadata_contract::matching::bibliographic_match_key;
use subject_projection::{has_usable_lcc_classification, project_subject_codes, reviewed_source_heading_subject, unified_subject_paths, BISAC_SYSTEM_ID};

const HEADINGS: &[(&str, &str)] = &[
    ("science fiction", "FIC028000"),
    ("science fiction american", "FIC028000"),
    ("adventure stories", "FIC002000"),
    ("war stories", "FIC032000"),
    ("sea stories", "FIC047000"),
    ("detective and mystery stories", "FIC022000"),
    ("detective and mystery stories english", "FIC022000"),
    ("mystery and detective stories", "FIC022000"),
    ("historical fiction", "FIC014000"),
    ("horror tales", "FIC015000"),
    ("ghost stories", "FIC015000"),
    ("love stories", "FIC027000"),
    ("humorous stories", "FIC016000"),
    ("western stories", "FIC033000"),
    ("fairy tales", "FIC010000"),
    ("children s literature", "JUV000000"),
    ("adventure and adventurers juvenile fiction", "JUV001000"),
    ("fairies juvenile fiction", "JUV037000"),
    ("pirates juvenile fiction", "JUV001020"),
    ("fantasy fiction", "FIC009000"),
    ("fantasy", "FIC009000"),
    ("epic poetry english old", "POE005020"),
    ("poetry", "POE000000"),
    ("australian poetry", "POE010000"),
    ("english poetry", "POE005020"),
    ("world history", "HIS037000"),
    ("history ancient", "HIS002000"),
    ("military art and science early works to 1800", "TEC025000"),
    ("fiction", "FIC000000"),
    ("short stories", "FIC029000"),
    ("short stories american", "FIC029000"),
    ("short stories english", "FIC029000"),
    ("horror tales american", "FIC015000"),
    ("ghost stories american", "FIC015000"),
    ("humorous stories english", "FIC016000"),
    ("humorous stories american", "FIC016000"),
    ("detective and mystery stories american", "FIC022000"),
    ("detective and mystery stories english translations into dutch", "FIC022000"),
    ("mystery fiction", "FIC022000"),
    ("religious fiction", "FIC026000"),
    ("christmas stories", "FIC058000"),
    ("fables", "FIC010000"),
    ("children s poetry", "JUV070000"),
    ("nursery rhymes", "JUV070000"),
    ("dogs juvenile fiction", "JUV002070"),
    ("cats juvenile fiction", "JUV002050"),
    ("schools fiction", "FIC000000"),
    ("american wit and humor", "HUM000000"),
    ("english wit and humor", "HUM000000"),
    ("biography", "BIO000000"),
    ("software", "COM000000"),
    ("software engineering", "COM051230"),
    ("programming", "COM051000"),
    ("library", "LAN025000"),
    ("libraries", "LAN025000"),
    ("library science", "LAN025000"),
    ("comparative linguistics", "LAN009010"),
    ("philosophy chinese", "PHI003000"),
    ("ethics", "PHI005000"),
    ("religions", "REL000000"),
    ("bible", "BIB000000"),
    ("buddhism", "REL007000"),
    ("taoism", "REL065000"),
    ("folklore", "SOC011000"),
    ("folklore italy", "SOC011000"),
    ("mythology", "SOC011000"),
    ("literature history and criticism", "LIT000000"),
    ("chinese literature history and criticism", "LIT008010"),
    ("french literature history and criticism", "LIT004150"),
    ("science fiction history and criticism", "LIT000000"),
    ("essays", "LCO010000"),
    ("geology", "SCI031000"),
    ("utopias", "POL051000"),
    ("crusades", "HIS027350"),
    ("historiography", "HIS016000"),
    ("europe historical geography", "HIS052000"),
    ("europe history", "HIS010000"),
    ("europe history 1517 1648", "HIS010000"),
    ("europe history 1492 1648", "HIS010000"),
    ("europe history 1789 1900", "HIS010000"),
    ("china history", "HIS008000"),
    ("switzerland description and travel", "TRV009140"),
    ("italy description and travel", "TRV009110"),
    ("germany description and travel", "TRV009060"),
    ("france description and travel", "TRV009050"),
    ("england description and travel", "TRV009070"),
    ("mexico description and travel", "TRV014000"),
];

fn reviewed_code(heading: &str, original: &str) -> Option<&'static str> {
    if let Some((_, code)) = HEADINGS.iter().find(|(name, _)| *name == heading) {
        return Some(code);
    }
    // An explicit final LCSH form subdivision supports a broad form category.
    // Keep other qualifiers: criticism, nonfiction and juvenile nonfiction
    // cannot become fiction by substring matching.
    let (_, subdivision) = original.rsplit_once("--")?;
    match bibliographic_match_key(subdivision).as_str() {
        "juvenile fiction" => Some("JUV000000"),
        "fiction" => Some("FIC000000"),
        "biography" => Some("BIO000000"),
        "poetry" => Some("POE000000"),
        "humor" => Some("HUM000000"),
        _ => None,
    }
}

fn source_heading(subject: &BookSubject) -> Option<String> {
    if subject.authority().is_some_and(|authority| !matches!(bibliographic_match_key(authority).as_str(), "lcsh" | "library of congress subject headings")) || (subject.source() != "dc:subject" && !subject.source().starts_with("catalog:")) {
        return None;
    }
    Some(bibliographic_match_key(subject.name()))
}

pub(super) fn recover(record: &mut BookRecord) -> bool {
    if has_usable_lcc_classification(&record.book.subjects) {
        return false;
    }
    let christian_context = record.book.subjects.iter().filter_map(source_heading).any(|heading| matches!(heading.as_str(), "christianity" | "christianity essence genius nature"));
    let library_context = record.book.subjects.iter().filter_map(source_heading).any(|heading| matches!(heading.as_str(), "library" | "libraries" | "library science"));
    let mut additions = Vec::new();
    for subject in &record.book.subjects {
        if let Some(projected) = reviewed_source_heading_subject(subject) {
            if !record.book.subjects.contains(&projected) && !additions.contains(&projected) {
                additions.push(projected);
            }
            continue;
        }
        let Some(heading) = source_heading(subject) else { continue };
        let code = if heading == "apologetics" && christian_context {
            Some("REL067030")
        } else if heading == "digital preservation" && library_context {
            Some("LAN025060")
        } else if heading == "archives" && library_context {
            Some("LAN025020")
        } else {
            reviewed_code(&heading, subject.name())
        };
        let Some(code) = code else { continue };
        let paths = unified_subject_paths(BISAC_SYSTEM_ID, code);
        if paths.is_empty() || paths.iter().all(|path| path == "Literature / Genres") {
            continue;
        }
        let projected = BookSubject::new(None, subject.name(), format!("metadata:source-heading:{}", subject.source()), Some(BISAC_SYSTEM_ID.into()), Some(code.into())).expect("reviewed source heading must be valid");
        if !record.book.subjects.contains(&projected) && !additions.contains(&projected) {
            additions.push(projected);
        }
    }
    let all_subjects = record.book.subjects.iter().chain(&additions).cloned().collect::<Vec<_>>();
    // The shared matcher removes broad ancestors when a more specific route
    // is supported. Materialize only additions it actually uses as evidence.
    let projected = project_subject_codes(&all_subjects);
    let offset = record.book.subjects.len();
    additions = additions.into_iter().enumerate().filter(|(position, _)| projected.iter().any(|code| code.evidence_positions().contains(&(offset + position)))).map(|(_, subject)| subject).collect();
    let changed = !additions.is_empty();
    record.book.subjects.extend(additions);
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use book_model::BookDetails;

    fn record(name: &str) -> BookRecord {
        let subject = BookSubject::new(None, name, "dc:subject", None, None).unwrap();
        let mut record = BookRecord { metadata: BookDetails { title: "Source title".into(), ..Default::default() }, description: "Source summary".into() };
        record.book.subjects.push(subject);
        record
    }

    #[test]
    fn exact_source_genre_recovers_supported_navigation_without_changing_the_book() {
        let mut book = record("Science fiction");
        let original = book.clone();
        assert!(recover(&mut book));
        assert_eq!(book.title, original.title);
        assert_eq!(book.description, original.description);
        assert_eq!(book.book.subjects[0], original.book.subjects[0]);
        assert_eq!(book.book.subjects[1].code(), Some("Literature / Genres / Speculative / Science Fiction"));
        assert!(!recover(&mut book));
    }

    #[test]
    fn genre_history_and_criticism_are_not_classified_as_fiction() {
        for name in ["Nonfiction", "Unknown", "A history of ghost stories"] {
            assert!(!recover(&mut record(name)), "{name}");
        }
        let mut criticism = record("Science fiction -- History and criticism");
        assert!(recover(&mut criticism));
        assert_eq!(criticism.book.subjects[1].code(), Some("LIT000000"));
    }

    #[test]
    fn juvenile_and_adult_headings_keep_their_declared_scope() {
        for (name, code) in [("Detective and mystery stories", "Literature / Genres / Mystery & Thrillers / Mystery & Detective"), ("Adventure and adventurers -- Juvenile fiction", "JUV001000")] {
            let mut book = record(name);
            assert!(recover(&mut book));
            assert_eq!(book.book.subjects[1].code(), Some(code));
        }
        for (_, code) in HEADINGS {
            assert!(!unified_subject_paths(BISAC_SYSTEM_ID, code).is_empty(), "{code}");
        }
    }
}
