//! Resolve shared subjects without choosing an ambiguous edition's ISBN.
use crate::{
    matching::{author_credits, author_match_keys, author_names_compatible, bibliographic_title_match_key, title_substring_keys, title_qualifiers},
    Classification, ClassificationEvidence, ClassificationScheme, ClassificationSource, EditionIdentityQuery, EditionIdentityResult, EditionIdentityStatus,
};
use std::collections::BTreeSet;
pub const METHOD: &str = "verified_shared_edition_classification";

fn title_key(title: &str) -> String {
    let prepared = crate::identity_evidence::subject_identity_title(title);
    let mut key = bibliographic_title_match_key(&prepared);
    // This known series label is not a volume, revision, or adaptation qualifier.
    for suffix in [" very short introductions", " a very short introduction"] {
        if let Some(base) = key.strip_suffix(suffix) {
            key = base.to_owned();
        }
    }
    key
}

// Lookup can find an edition by a whole-word fragment of the local title.
// Keep qualifier evidence: a subtitle must not conceal a different volume or adaptation.
fn titles_compatible(local: &str, remote: &str) -> bool {
    let local = title_key(local);
    let remote = title_key(remote);
    if local == remote { return true; }
    if remote.len() < 5 || title_qualifiers(&local) != title_qualifiers(&remote) { return false; }
    let edition_markers = |text: &str| text.split_whitespace()
        .filter(|word| matches!(*word, "edition" | "ed" | "revised" | "revision" | "translation" | "translated" | "unabridged"))
        .map(str::to_owned).collect::<BTreeSet<_>>();
    edition_markers(&local) == edition_markers(&remote)
        && title_substring_keys(&local, 64).iter().any(|(key, _)| key == &remote)
}

pub fn classifications(query: &EditionIdentityQuery, result: &EditionIdentityResult) -> Vec<Classification> {
    let year_codes = crate::year_consensus::classifications(query, result);
    if !year_codes.is_empty() {
        return year_codes;
    }
    if result.status != EditionIdentityStatus::Ambiguous || result.query_id != query.query_id || result.candidates_truncated || result.candidates.len() < 2 {
        return Vec::new();
    }
    let title = title_key(&query.title);
    if title.len() < 5 || query.authors.is_empty() {
        return Vec::new();
    }
    let local_authors = author_credits(&query.authors);
    let mut common: Option<BTreeSet<String>> = None;
    let mut evidence = Vec::new();
    let mut credited_names: Vec<Vec<String>> = vec![Vec::new(); local_authors.len()];
    for candidate in &result.candidates {
        if !titles_compatible(&query.title, &candidate.full_title()) || candidate.authors.is_empty() {
            return Vec::new();
        }
        let mut matched = BTreeSet::new();
        let remote_authors = author_credits(&candidate.authors);
        for author in &remote_authors {
            if author_match_keys(author).iter().all(|key| key.split_whitespace().filter(|t| t.chars().count() > 1).count() < 2) {
                continue;
            }
            let indices = local_authors.iter().enumerate().filter(|(_, local)| author_names_compatible(local, author)).map(|(i, _)| i).collect::<Vec<_>>();
            if indices.is_empty() {
                continue;
            }
            if indices.len() != 1 || !matched.insert(indices[0]) {
                return Vec::new();
            }
            let names = &mut credited_names[indices[0]];
            if names.iter().any(|previous| !author_names_compatible(previous, author)) {
                return Vec::new();
            }
            names.push(author.clone());
        }
        if matched.is_empty() {
            return Vec::new();
        }
        // Both edition and work classifications participate; only codes shared
        // by every identity-compatible candidate are retained.
        let codes = candidate
            .classifications
            .iter()
            .filter(|c| c.scheme == ClassificationScheme::LibraryOfCongress)
            .filter_map(|c| crate::classification_consensus_notation(c.scheme, &c.notation))
            .collect::<BTreeSet<_>>();
        if codes.is_empty() {
            return Vec::new();
        }
        common = Some(match common {
            None => codes,
            Some(previous) => previous.intersection(&codes).cloned().collect(),
        });
        if common.as_ref().is_some_and(BTreeSet::is_empty) {
            return Vec::new();
        }
        evidence.push(ClassificationEvidence {
            method: METHOD.into(),
            isbn13: candidate.canonical_isbn13.clone(),
            open_library_work_id: candidate.open_library_work_id.clone().unwrap_or_default(),
            open_library_edition_id: candidate.open_library_edition_id.clone(),
        });
    }
    common.unwrap_or_default().into_iter().map(|notation| Classification { notation, scheme: ClassificationScheme::LibraryOfCongress, source: ClassificationSource::Work, evidence: evidence.clone() }).collect()
}

/// Prefer a unique exact title unless known authors conflict. Multiple exact
/// titles can still be resolved by a unique complete author match.
/// This resolves subjects only; it does not assert an exact local edition ISBN.
pub fn resolved_classifications(query: &EditionIdentityQuery, result: &EditionIdentityResult) -> Vec<Classification> {
    let agreed = classifications(query, result);
    if !agreed.is_empty() { return agreed; }
    if result.status != EditionIdentityStatus::Ambiguous || result.candidates.is_empty() || result.candidates_truncated || result.query_id != query.query_id { return vec![]; }
    let known_authors = |authors: &[String]| author_credits(authors).into_iter()
        .filter(|name| !matches!(name.trim().to_ascii_lowercase().as_str(), "" | "unknown" | "untitled"))
        .collect::<Vec<_>>();
    let local = known_authors(&query.authors);
    let title = title_key(&query.title);
    if title.is_empty() { return vec![]; }
    let full_key = bibliographic_title_match_key(&crate::identity_evidence::subject_identity_title(&query.title));
    let mut exact = result.candidates.iter().filter(|candidate|
        full_key == bibliographic_title_match_key(&crate::identity_evidence::subject_identity_title(&candidate.full_title()))
    ).collect::<Vec<_>>();
    if exact.is_empty() {
        exact = result.candidates.iter().filter(|candidate| title == title_key(&candidate.full_title())).collect();
    }
    let filename_surname = exact.is_empty() && local.is_empty();
    if filename_surname {
        exact = result.candidates.iter().filter(|candidate|
            crate::identity_evidence::filename_surname_title_matches(&query.title, &candidate.title, &candidate.authors)
        ).collect();
    }
    let unique_title = exact.len() == 1;
    let matches = exact.into_iter().filter(|candidate| {
        if unique_title {
            let remote = known_authors(&candidate.authors);
            return local.is_empty() || remote.is_empty()
                || local.iter().any(|name| remote.iter().any(|other| author_names_compatible(name, other)));
        }
        if local.is_empty() { return false; }
        let remote = author_credits(&candidate.authors);
        local.len() == remote.len() && local.iter().all(|name| {
            author_match_keys(name).iter().any(|key| key.split_whitespace().filter(|t| t.chars().count() > 1).count() >= 2)
                && remote.iter().filter(|other| author_names_compatible(name, other)).count() == 1
        }) && remote.iter().all(|name| local.iter().filter(|other| author_names_compatible(name, other)).count() == 1)
    }).collect::<Vec<_>>();
    let [candidate] = matches.as_slice() else { return vec![]; };
    candidate.classifications.iter().filter(|code| code.scheme == ClassificationScheme::LibraryOfCongress)
        .filter_map(|code| crate::classification_consensus_notation(code.scheme, &code.notation))
        .collect::<BTreeSet<_>>().into_iter().map(|notation| Classification {
            notation, scheme: ClassificationScheme::LibraryOfCongress, source: ClassificationSource::Work,
            evidence: vec![ClassificationEvidence { method: if filename_surname { "verified_filename_surname_title_classification" } else if unique_title { "verified_unique_title_classification" } else { "verified_unique_title_author_classification" }.into(), isbn13: candidate.canonical_isbn13.clone(), open_library_work_id: candidate.open_library_work_id.clone().unwrap_or_default(), open_library_edition_id: candidate.open_library_edition_id.clone() }],
        }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EditionIdentityMatch, EditionTitleMatch};
    fn code(value: &str, source: ClassificationSource) -> Classification {
        Classification { notation: value.into(), scheme: ClassificationScheme::LibraryOfCongress, source, evidence: vec![] }
    }
    fn fixture() -> (EditionIdentityQuery, EditionIdentityResult) {
        let query = EditionIdentityQuery {
            query_id: "art".into(),
            title: "Art History: A Very Short Introduction (Very Short Introductions)".into(),
            authors: vec!["Arnold, Dana".into()],
            publishers: vec!["Oxford University Press".into()],
            publication_year: Some(2004),
        };
        let candidates = [2004, 2020]
            .into_iter()
            .map(|year| EditionIdentityMatch { subtitle: None,
                canonical_isbn13: if year == 2004 { "9780192801814" } else { "9780198831808" }.into(),
                open_library_edition_id: format!("OL{year}M"),
                open_library_work_id: Some(format!("OL{year}W")),
                title: "Art History".into(),
                work_title: Some("Art History".into()),
                title_match: EditionTitleMatch::Both,
                authors: vec!["Dana Arnold".into()],
                publishers: vec![if year == 2004 { "Oxford University Press, USA" } else { "Oxford University Press" }.into()],
                publication_year: Some(year),
                matched_publisher: year == 2020,
                matched_publication_year: year == 2004,
                classifications: vec![
                    code("N5300", ClassificationSource::ExactEdition),
                    code(if year == 2004 { "N7425 .A646 2004" } else { "N7480 .A773 2010" }, if year == 2004 { ClassificationSource::ExactEdition } else { ClassificationSource::Work }),
                ],
            })
            .collect();
        (query, EditionIdentityResult { catalog_subjects: None, query_id: "art".into(), status: EditionIdentityStatus::Ambiguous, matched: None, candidates, candidates_truncated: false })
    }
    #[test]
    fn underscore_space_is_a_subtitle_separator() {
        let evidence = crate::identity_evidence::edition_identity_evidence("Logic_ a very short introduction");
        assert_eq!(evidence.title, "Logic: a very short introduction");
        assert_eq!(evidence.author, None);
        assert_eq!(title_key(&evidence.title), title_key("Logic"));
        assert_eq!(crate::identity_evidence::edition_identity_evidence("knuth_mathematical_writing").title, "knuth mathematical writing");
    }
    #[test]
    fn filename_surname_disambiguates_knuth_without_inventing_an_author() {
        let (mut q, mut r) = fixture();
        q.title = "knuth_mathematical_writing".into();
        q.authors.clear();
        for c in &mut r.candidates { c.title = "Mathematical Writing".into(); }
        r.candidates[0].authors = vec!["Donald Knuth".into()];
        r.candidates[0].classifications = vec![code("QA42 .K59 1989", ClassificationSource::ExactEdition)];
        r.candidates[1].authors = vec!["Franco Vivaldi".into()];
        r.candidates[1].classifications = vec![code("QA1-939", ClassificationSource::ExactEdition)];
        let codes = resolved_classifications(&q, &r);
        assert_eq!(codes.len(), 1);
        assert_eq!(codes[0].notation, "QA42");
        assert_eq!(codes[0].evidence[0].method, "verified_filename_surname_title_classification");
        r.candidates_truncated = true;
        assert!(resolved_classifications(&q, &r).is_empty());
        r.candidates_truncated = false;
        q.authors = vec!["Franco Vivaldi".into()];
        assert!(resolved_classifications(&q, &r).is_empty());
        q.authors.clear();
        r.candidates[1].authors = vec!["Another Knuth".into()];
        assert!(resolved_classifications(&q, &r).is_empty());
        r.candidates.pop();
        for title in ["donald_mathematical_writing", "knuth_different_title", "knuth mathematical writing"] {
            q.title = title.into();
            assert!(resolved_classifications(&q, &r).is_empty(), "{title}");
        }
        q.title = "knuth_mathematical_writing".into();
        r.candidates[0].authors = vec!["Knuth, Donald E.".into()];
        assert!(!resolved_classifications(&q, &r).is_empty());
    }
    #[test]
    fn unique_exact_title_resolves_single_or_mixed_results() {
        let (mut q, mut r) = fixture();
        q.title = "Refactoring UI".into();
        q.authors.clear();
        r.candidates[0].title = q.title.clone();
        r.candidates[1].title = "Refactoring Databases".into();
        assert!(!resolved_classifications(&q, &r).is_empty());
        r.candidates.truncate(1);
        assert!(!resolved_classifications(&q, &r).is_empty());
        r.candidates_truncated = true;
        assert!(resolved_classifications(&q, &r).is_empty());
        r.candidates_truncated = false;
        q.authors = vec!["Steve Schoger".into()];
        assert!(resolved_classifications(&q, &r).is_empty());
        r.candidates[0].authors = vec!["Steve Schoger".into(), "Adam Wathan".into()];
        assert!(!resolved_classifications(&q, &r).is_empty());
        r.candidates[0].classifications.clear();
        assert!(resolved_classifications(&q, &r).is_empty());
    }
    #[test]
    fn duplicate_exact_titles_with_different_subjects_still_need_evidence() {
        let (mut q, mut r) = fixture();
        q.title = "Refactoring UI".into();
        q.authors.clear();
        for c in &mut r.candidates { c.title = q.title.clone(); }
        assert!(resolved_classifications(&q, &r).is_empty());
    }
    #[test]
    fn sutton_middle_initial_expansion_preserves_shared_subject() {
        let (mut q, mut r) = fixture();
        q.title = "Wall Street and the Rise of Hitler".into();
        q.authors = vec!["Antony C. Sutton".into()];
        for (i, c) in r.candidates.iter_mut().enumerate() {
            c.title = q.title.clone();
            c.authors = vec![if i == 0 { "Antony Cyril Sutton" } else { "Antony C. Sutton" }.into()];
            c.classifications = vec![code("HF1456.5.G3 S87", ClassificationSource::ExactEdition)];
        }
        assert!(!resolved_classifications(&q, &r).is_empty());
        r.candidates[1].authors = vec!["Antony Charles Sutton".into()];
        assert!(resolved_classifications(&q, &r).is_empty());
    }
    #[test]
    fn whiteout_exact_author_outweighs_unrelated_title_hit() {
        let (mut q, mut r) = fixture();
        q.title = "Whiteout".into(); q.authors = vec!["Alexander Cockburn".into()];
        for c in &mut r.candidates { c.title = q.title.clone(); }
        r.candidates[0].authors = q.authors.clone();
        r.candidates[0].classifications = vec![code("HV5825 .C59 1998", ClassificationSource::ExactEdition)];
        r.candidates[1].authors = vec!["Alexander Lowe".into(), "Sebastian Kadlecik".into()];
        r.candidates[1].classifications = vec![code("QC926.37", ClassificationSource::Work)];
        assert_eq!(resolved_classifications(&q, &r)[0].notation, "HV5825");
        r.candidates_truncated = true;
        assert!(resolved_classifications(&q, &r).is_empty());
        r.candidates_truncated = false;
        r.candidates[1].authors = q.authors.clone();
        assert!(resolved_classifications(&q, &r).is_empty());
    }

    #[test]
    fn lotte_language_edition_note_does_not_block_shared_subject() {
        let (mut q, mut r) = fixture();
        q.title = "Lotte in Weimar: Roman (Fischer Klassik PLUS) (German Edition)".into();
        q.authors = vec!["Mann, Thomas".into(), "Frizen, Werner".into()];
        for (i, c) in r.candidates.iter_mut().enumerate() {
            c.title = "Lotte in Weimar".into();
            c.authors = vec!["Thomas Mann".into()];
            c.classifications = vec![code(if i == 0 { "PT2625.A44 L62 1990" } else { "PT2625.A44" }, ClassificationSource::ExactEdition)];
        }
        assert_eq!(resolved_classifications(&q, &r)[0].notation, "PT2625");
    }
    #[test]
    fn athwart_history_duplicate_records_share_subject() {
        let (mut q, mut r) = fixture();
        q.title = "Athwart History".into();
        q.authors = vec!["Buckley, William F., Jr., Roger Kimball, Linda Bridges".into()];
        for c in &mut r.candidates {
            c.title = "Athwart history".into();
            c.authors = vec!["William F. Buckley".into()];
            c.classifications = vec![code("JC573.2.U6 B83 2010", ClassificationSource::ExactEdition)];
            c.canonical_isbn13 = "9781594033797".into();
        }
        assert!(!classifications(&q, &r).is_empty());
    }

    #[test]
    fn metternich_subtitle_omission_resolves_shared_subject_only() {
        let (mut q, mut r) = fixture();
        q.title = "Metternich Strategist and Visionary".into();
        q.authors = vec!["Wolfram Siemann".into()];
        for c in &mut r.candidates {
            c.title = "Metternich".into();
            c.authors = q.authors.clone();
            c.classifications = vec![code("DB80.8.M5", ClassificationSource::ExactEdition)];
        }
        assert_eq!(classifications(&q, &r).len(), 1);
        assert!(r.matched.is_none());
        assert_ne!(r.candidates[0].canonical_isbn13, r.candidates[1].canonical_isbn13);
        for title in ["Metternich Volume 2", "Metternich: An Adaptation", "Metternich Revised Edition", "Metternichs"] {
            r.candidates[1].title = title.into();
            assert!(classifications(&q, &r).is_empty(), "{title}");
        }
    }

    #[test]
    fn master_of_disguise_combined_credits_resolve_shared_subject() {
        let (mut q, mut r) = fixture();
        q.title = "The Master of Disguise".into();
        for c in &mut r.candidates {
            c.title = q.title.clone();
            c.authors = vec!["Antonio Mendez".into(), "Malcolm McConnell".into()];
            c.classifications = vec![code("UB271.U52", ClassificationSource::ExactEdition)];
        }
        for separator in [" with ", " AND ", " & ", "; "] {
            q.authors = vec![format!("Antonio J. Mendez{separator}Malcolm McConnell")];
            let codes = classifications(&q, &r);
            assert_eq!(codes.len(), 1, "{separator}");
            assert_eq!(codes[0].evidence.len(), 2);
        }
        r.candidates[1].authors = vec!["Antonio Smith".into(), "Malcolm Jones".into()];
        assert!(classifications(&q, &r).is_empty());
        r.candidates[1].authors = vec!["Antonio Q. Mendez".into()];
        assert!(classifications(&q, &r).is_empty());
    }

    #[test]
    fn about_face_accepts_additional_coauthors_but_requires_a_shared_author() {
        let (mut q, mut r) = fixture();
        q.title = "About Face".into();
        q.authors = vec!["Alan Cooper".into()];
        for (i, candidate) in r.candidates.iter_mut().enumerate() {
            candidate.title = "About Face".into();
            candidate.authors = vec!["Alan Cooper".into()];
            candidate.classifications = vec![code(if i == 0 {"QA76.9.U83 C6 1995"} else {"QA76.9.U83 C6596 2014"}, if i == 0 {ClassificationSource::ExactEdition} else {ClassificationSource::Work})];
        }
        r.candidates[1].authors.extend(["Robert Reimann", "David Cronin", "Christopher Noessel"].map(str::to_owned));
        let codes = classifications(&q, &r);
        assert_eq!(codes.len(), 1);
        assert_eq!(codes[0].notation, "QA76.9");
        assert_eq!(codes[0].evidence.len(), 2);
        r.candidates[1].authors.remove(0);
        assert!(classifications(&q, &r).is_empty());
    }

    #[test]
    fn work_classifications_participate_in_agreement() {
        let (q, mut r) = fixture();
        let expected = classifications(&q, &r);
        assert!(!expected.is_empty());
        r.candidates[1].classifications[0].source = ClassificationSource::Work;
        assert_eq!(classifications(&q, &r), expected);
        r.candidates[0].classifications[0].source = ClassificationSource::Work;
        assert_eq!(classifications(&q, &r), expected);
    }

    #[test]
    fn art_history_recovers_only_shared_edition_code() {
        let (q, r) = fixture();
        let codes = classifications(&q, &r);
        assert_eq!(codes.len(), 1);
        assert_eq!(codes[0].notation, "N5300");
        assert_eq!(codes[0].evidence.len(), 2);
        assert!(r.matched.is_none());
    }
    #[test]
    fn incomplete_coauthors_can_support_the_same_subject() {
        let (mut q, mut r) = fixture();
        q.authors = vec!["Clive H. Church".into(), "Randolph C. Head".into()];
        r.candidates[0].authors = vec![q.authors[0].clone()];
        r.candidates[1].authors = vec![q.authors[1].clone()];
        assert_eq!(classifications(&q, &r).len(), 1);
    }
    #[test]
    fn unrelated_titles_authors_or_invalid_codes_cannot_vote() {
        for change in 0..7 {
            let (mut q, mut r) = fixture();
            match change {
                0 => r.candidates[1].authors = vec!["Dana Smith".into()],
                1 => r.candidates[1].title = "Art History: Volume 2".into(),
                2 => r.candidates[1].classifications[0].notation = "invalid".into(),
                3 => r.candidates[1].classifications[0].notation = "N5301".into(),
                4 => r.candidates_truncated = true,
                5 => q.authors.clear(),
                _ => r.candidates[1].authors.clear(),
            }
            assert!(classifications(&q, &r).is_empty(), "change {change}");
        }
    }
    #[test]
    fn shared_first_name_is_not_author_identity() {
        let (mut q, mut r) = fixture();
        q.authors = vec!["Fred C. Piper".into(), "Sean Murphy".into()];
        r.candidates[0].authors = vec!["Fred C. Piper".into()];
        r.candidates[1].authors = vec!["Sean-Philip Oriyano".into()];
        assert!(classifications(&q, &r).is_empty());
    }

    #[test]
    fn real_switzerland_title_and_split_coauthors_share_dq54() {
        let (mut q, mut r) = fixture();
        q.title = "A Concise History of Switzerland (Cambridge Concise Histories)".into();
        q.authors = vec!["Clive H. Church".into(), "Randolph C. Head".into()];
        for (i, c) in r.candidates.iter_mut().enumerate() {
            c.title = "A Concise History Of Switzerland".into();
            c.authors = vec![q.authors[i].clone()];
            c.classifications = vec![code("DQ54 .C47 2013", ClassificationSource::ExactEdition)];
        }
        r.candidates[0].classifications.push(code("DQ54", ClassificationSource::ExactEdition));
        let codes = classifications(&q, &r);
        assert_eq!(codes.len(), 1);
        assert_eq!(codes[0].notation, "DQ54");
    }
    #[test]
    fn real_filename_debris_and_cutter_difference_share_hq31() {
        let (mut q, mut r) = fixture();
        q.title = "(by-Anne-Hooper)-Great-Sex-Guide-692141-(z-lib.org)".into();
        q.authors = vec!["Anne Hooper".into()];
        for (i, c) in r.candidates.iter_mut().enumerate() {
            c.title = "Great sex guide".into();
            c.authors = q.authors.clone();
            c.classifications = vec![code(if i == 0 { "HQ31 .H742 1999" } else { "HQ31" }, ClassificationSource::ExactEdition)];
        }
        let codes = classifications(&q, &r);
        assert_eq!(codes.len(), 1);
        assert_eq!(codes[0].notation, "HQ31");
    }
    #[test]
    fn cleanup_keeps_volumes_but_demotes_parenthesized_editions() {
        let (mut q, mut r) = fixture();
        q.title = "Art History (Volume 2)".into();
        for c in &mut r.candidates {
            c.title = "Art History (Volume 3)".into();
        }
        assert!(classifications(&q, &r).is_empty());
        q.title = "Art History (Revised Edition)".into();
        for c in &mut r.candidates {
            c.title = "Art History".into();
        }
        assert!(!classifications(&q, &r).is_empty());
    }
}
