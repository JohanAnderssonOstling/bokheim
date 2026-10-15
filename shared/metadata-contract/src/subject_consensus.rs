//! Collect classifications from verified candidates without choosing an edition ISBN.

use crate::matching::NormalizedTitle;
use crate::{
    Classification, ClassificationEvidence, ClassificationScheme, ClassificationSource, EditionIdentityQuery, EditionIdentityResult, EditionIdentityStatus,
    matching::{author_credits, author_names_compatible_with_title, matching_author_count, matching_edition_author_count, title_qualifiers, title_substring_keys},
};
use std::collections::BTreeSet;
pub const METHOD: &str = "verified_matching_edition_classifications";

fn title_key(title: &str) -> String {
    let mut key = NormalizedTitle::new(title).full;
    if let Some(rest) = key.strip_prefix("the ") {
        key = rest.to_owned();
    }
    // This known series label is not a volume, revision, or adaptation qualifier.
    for suffix in [" very short introductions", " a very short introduction"] {
        if let Some(base) = key.strip_suffix(suffix) {
            key = base.to_owned();
        }
    }
    key
}

/// Existing classification title matching, shared by sibling consensus.
/// Lookup can find an edition by a whole-word fragment of the local title.
/// Keep qualifier evidence: a subtitle must not conceal a different volume or adaptation.
pub fn titles_compatible(local: &str, remote: &str) -> bool {
    let local_title = NormalizedTitle::new(local);
    let remote_title = NormalizedTitle::new(remote);
    if local_title.qualifiers != remote_title.qualifiers {
        return false;
    }
    if local_title.subtitle.is_none() && local_title.full == remote_title.main {
        return true;
    }
    let local = title_key(local);
    let remote = title_key(remote);
    if local == remote {
        return true;
    }
    if remote.len() < 5 || title_qualifiers(&local) != title_qualifiers(&remote) {
        return false;
    }
    let edition_markers = |text: &str| text.split_whitespace().filter(|word| matches!(*word, "edition" | "ed" | "revised" | "revision" | "translation" | "translated" | "unabridged")).map(str::to_owned).collect::<BTreeSet<_>>();
    // A long subtitle can push even the complete main title beyond the
    // bounded fragment list. Check it explicitly, keeping the same qualifier
    // and edition-marker guards used for other shortened titles.
    edition_markers(&local) == edition_markers(&remote)
        && (title_key(&local_title.main) == remote || title_substring_keys(&local, 64).iter().any(|(key, _)| key == &remote))
}

pub fn classifications(query: &EditionIdentityQuery, result: &EditionIdentityResult) -> Vec<Classification> {
    let mut ranked = result.clone();
    let best = ranked.candidates.iter().map(|c| matching_edition_author_count(query, c)).max().unwrap_or(0);
    if best > 0 {
        ranked.candidates.retain(|c| matching_edition_author_count(query, c) == best);
    }
    let result = &ranked;
    if result.status != EditionIdentityStatus::Ambiguous || result.query_id != query.query_id || result.candidates_truncated || result.candidates.len() < 2 {
        return Vec::new();
    }
    let title = title_key(&query.title);
    if title.len() < 5 || query.authors.is_empty() {
        return Vec::new();
    }
    let local_authors = author_credits(&query.authors);
    let mut credited_names: Vec<Vec<String>> = vec![Vec::new(); local_authors.len()];
    for candidate in &result.candidates {
        if !titles_compatible(&query.title, &candidate.full_title()) || candidate.authors.is_empty() {
            return Vec::new();
        }
        if matching_edition_author_count(query, candidate) == 0 {
            return Vec::new();
        }
        // An initial must not merge explicitly different full names across editions.
        for (i, local) in local_authors.iter().enumerate() {
            for remote in author_credits(&candidate.authors).into_iter().filter(|remote| author_names_compatible_with_title(local, remote, &query.title, &candidate.full_title())) {
                if credited_names[i].iter().any(|previous| !author_names_compatible_with_title(previous, &remote, &query.title, &candidate.full_title())) {
                    return Vec::new();
                }
                credited_names[i].push(remote);
            }
        }
    }
    let year_codes = crate::year_consensus::classifications(query, result);
    if !year_codes.is_empty() {
        return year_codes;
    }
    candidate_classifications(result.candidates.iter(), METHOD)
}

/// Union valid codes while attributing each code only to records that supplied it.
pub(crate) fn candidate_classifications<'a>(candidates: impl IntoIterator<Item = &'a crate::EditionIdentityMatch>, method: &str) -> Vec<Classification> {
    let mut codes = std::collections::BTreeMap::<String, Vec<ClassificationEvidence>>::new();
    for candidate in candidates {
        for code in &candidate.classifications {
            if code.scheme != ClassificationScheme::LibraryOfCongress {
                continue;
            }
            let Some(notation) = crate::classification_consensus_notation(code.scheme, &code.notation) else {
                continue;
            };
            let evidence = ClassificationEvidence {
                method: method.into(),
                isbn13: candidate.canonical_isbn13.clone(),
                open_library_work_id: candidate.open_library_work_id.clone().unwrap_or_default(),
                open_library_edition_id: candidate.open_library_edition_id.clone(),
            };
            let sources = codes.entry(notation).or_default();
            if !sources.contains(&evidence) {
                sources.push(evidence);
            }
        }
    }
    codes.into_iter().map(|(notation, evidence)| Classification { notation, scheme: ClassificationScheme::LibraryOfCongress, source: ClassificationSource::Work, evidence }).collect()
}

/// Prefer a unique exact title unless known authors conflict. Multiple exact
/// titles are ranked by the number of distinct matching author credits.
/// This resolves subjects only; it does not assert an exact local edition ISBN.
pub fn resolved_classifications(query: &EditionIdentityQuery, result: &EditionIdentityResult) -> Vec<Classification> {
    let agreed = classifications(query, result);
    if !agreed.is_empty() {
        return agreed;
    }
    if result.status != EditionIdentityStatus::Ambiguous || result.candidates.is_empty() || result.candidates_truncated || result.query_id != query.query_id {
        return vec![];
    }
    let known_authors = |authors: &[String]| author_credits(authors).into_iter().filter(|name| !matches!(name.trim().to_ascii_lowercase().as_str(), "" | "unknown" | "untitled")).collect::<Vec<_>>();
    let local = known_authors(&query.authors);
    let title = title_key(&query.title);
    if title.is_empty() {
        return vec![];
    }
    let full_key = NormalizedTitle::new(&query.title).full;
    let mut exact = result.candidates.iter().filter(|candidate| full_key == NormalizedTitle::new(&candidate.full_title()).full).collect::<Vec<_>>();
    if exact.is_empty() {
        exact = result.candidates.iter().filter(|candidate| title == title_key(&candidate.full_title())).collect();
    }
    let surname = query.author_surname.as_deref().filter(|_| local.is_empty());
    if let Some(surname) = surname {
        exact.retain(|candidate| surname_matches(surname, &candidate.authors));
    }
    let unique_title = exact.len() == 1;
    let best = exact.iter().map(|candidate| matching_edition_author_count(query, candidate)).max().unwrap_or(0);
    let matches = exact
        .into_iter()
        .filter(|candidate| {
            if local.is_empty() {
                return unique_title;
            }
            best > 0 && matching_edition_author_count(query, candidate) == best
        })
        .collect::<Vec<_>>();
    if matches.is_empty() || (local.is_empty() && matches.len() != 1) {
        return vec![];
    }
    for author in &local {
        let names = matches.iter().flat_map(|candidate| author_credits(&candidate.authors)).filter(|remote| author_names_compatible_with_title(author, remote, &query.title, &query.title)).collect::<Vec<_>>();
        if names.iter().enumerate().any(|(i, name)| names[..i].iter().any(|other| !author_names_compatible_with_title(name, other, &query.title, &query.title))) {
            return vec![];
        }
    }
    if matches.len() == 1 && matching_author_count(&query.authors, &matches[0].authors) == 0 {
        // A single shortened-name donor needs its own classification. A
        // work-only inherited code may belong to an adaptation of that work.
        let mut direct = (*matches[0]).clone();
        direct.classifications.retain(|code| code.source == ClassificationSource::ExactEdition);
        return candidate_classifications(std::iter::once(&direct), "verified_short_credit_edition_classification");
    }
    candidate_classifications(
        matches,
        if surname.is_some() {
            "verified_filename_surname_title_classification"
        } else if unique_title {
            "verified_unique_title_classification"
        } else {
            METHOD
        },
    )
}

/// Whether a candidate credits a full-named author with this surname.
fn surname_matches(surname: &str, authors: &[String]) -> bool {
    let surname = crate::matching::bibliographic_match_key(surname);
    author_credits(authors).iter().any(|author| {
        crate::matching::author_match_keys(author).iter().any(|key| {
            let words = key.split_whitespace().filter(|word| crate::matching::author_suffix(word).is_none()).collect::<Vec<_>>();
            words.len() >= 2 && words.last().copied() == Some(surname.as_str())
        })
    })
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
            author_surname: None,
            publishers: vec!["Oxford University Press".into()],
            book_year: Some(2004),
        };
        let candidates = [2004, 2020]
            .into_iter()
            .map(|year| EditionIdentityMatch {
                subtitle: None,
                canonical_isbn13: if year == 2004 { "9780192801814" } else { "9780198831808" }.into(),
                open_library_edition_id: format!("OL{year}M"),
                open_library_work_id: Some(format!("OL{year}W")),
                title: "Art History".into(),
                work_title: Some("Art History".into()),
                title_match: EditionTitleMatch::Both,
                authors: vec!["Dana Arnold".into()],
                publishers: vec![if year == 2004 { "Oxford University Press, USA" } else { "Oxford University Press" }.into()],
                book_year: Some(year),
                matched_publisher: year == 2020,
                matched_book_year: year == 2004,
                classifications: vec![
                    code("N5300", ClassificationSource::ExactEdition),
                    code(if year == 2004 { "N7425 .A646 2004" } else { "N7480 .A773 2010" }, if year == 2004 { ClassificationSource::ExactEdition } else { ClassificationSource::Work }),
                ],
            })
            .collect();
        (query, EditionIdentityResult { authority_subjects: None, sibling_subjects: None, query_id: "art".into(), status: EditionIdentityStatus::Ambiguous, matched: None, candidates, candidates_truncated: false })
    }
    #[test]
    fn duplicate_exact_titles_with_different_subjects_still_need_evidence() {
        let (mut q, mut r) = fixture();
        q.title = "Refactoring UI".into();
        q.authors.clear();
        for c in &mut r.candidates {
            c.title = q.title.clone();
        }
        assert!(resolved_classifications(&q, &r).is_empty());
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
            if matches!(change, 2 | 3) {
                let codes = classifications(&q, &r);
                assert!(!codes.is_empty());
                assert!(codes.iter().all(|c| c.notation != "invalid"));
            } else {
                assert!(classifications(&q, &r).is_empty(), "change {change}");
            }
        }
    }

}
