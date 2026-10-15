//! Classifications from large work searches need corroboration across editions.
use crate::{ClassificationScheme, ClassificationSource, EditionIdentityMatch, EditionIdentityQuery};
use metadata_contract::classification_consensus_notation;
use std::collections::{BTreeMap, BTreeSet};
use subject_projection::{unified_subject_paths, LCC_SYSTEM_ID};

fn paths(code: &str) -> BTreeSet<String> {
    unified_subject_paths(LCC_SYSTEM_ID, code).into_iter().filter(|path| path.contains(" / ")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Classification, EditionTitleMatch};

    fn query() -> EditionIdentityQuery {
        EditionIdentityQuery { query_id: "work".into(), title: "Example work".into(), authors: vec!["Known Author".into()], author_surname: None, publishers: vec![], book_year: None }
    }

    fn candidate(id: usize, notation: &str) -> (usize, usize, usize, EditionIdentityMatch) {
        (
            12,
            1,
            0,
            EditionIdentityMatch {
                canonical_isbn13: "9780306406157".into(),
                open_library_edition_id: format!("OL{id}M"),
                open_library_work_id: Some(format!("OL{id}W")),
                title: "Example work".into(),
                subtitle: None,
                work_title: Some("Example work".into()),
                title_match: EditionTitleMatch::Both,
                authors: vec!["Known Author".into()],
                publishers: vec![],
                book_year: None,
                matched_publisher: false,
                matched_book_year: false,
                classifications: vec![Classification { notation: notation.into(), scheme: ClassificationScheme::LibraryOfCongress, source: ClassificationSource::ExactEdition, evidence: vec![] }],
            },
        )
    }

    #[test]
    fn one_miscatalogued_edition_does_not_add_neurophysiology_to_childrens_fiction() {
        let mut matches = (0..9).map(|id| candidate(id, "PZ7")).collect::<Vec<_>>();
        matches.extend([candidate(9, "PR4074"), candidate(10, "PR4074"), candidate(11, "QP395")]);
        corroborate(&query(), &mut matches);
        assert_eq!(matches.len(), 11);
        assert!(matches.iter().all(|(_, _, _, m)| m.classifications.iter().all(|c| c.notation != "QP395")));
        assert!(matches.iter().any(|(_, _, _, m)| m.classifications.iter().any(|c| c.notation == "PR4074")));
    }

    #[test]
    fn dominant_military_evidence_does_not_inherit_a_sales_adaptation() {
        let mut matches = (0..10).map(|id| candidate(id, "U101")).collect::<Vec<_>>();
        matches.push(candidate(10, "HF5438.4"));
        corroborate(&query(), &mut matches);
        assert_eq!(matches.len(), 10);
    }

    #[test]
    fn sparse_or_explicit_edition_evidence_is_preserved() {
        let mut matches = vec![candidate(0, "U101"), candidate(1, "HF5438.4")];
        corroborate(&query(), &mut matches);
        assert_eq!(matches.len(), 2);
        matches.extend((2..12).map(|id| candidate(id, "U101")));
        let mut exact = query();
        exact.book_year = Some(2005);
        corroborate(&exact, &mut matches);
        assert_eq!(matches.len(), 12);
    }
}

pub(super) fn corroborate(query: &EditionIdentityQuery, matches: &mut Vec<(usize, usize, usize, EditionIdentityMatch)>) {
    // A publisher/year query identifies an edition. This guard is for broad
    // searches of a work, where one miscatalogued edition must not add a topic.
    if query.book_year.is_some() || !query.publishers.is_empty() {
        return;
    }
    let mut editions = BTreeSet::new();
    let mut support = BTreeMap::<String, BTreeSet<String>>::new();
    let mut path_support = BTreeMap::<String, BTreeSet<String>>::new();
    for (_, author_count, _, matched) in matches.iter() {
        if *author_count == 0 {
            continue;
        }
        for code in &matched.classifications {
            if code.source != ClassificationSource::ExactEdition || code.scheme != ClassificationScheme::LibraryOfCongress || classification_consensus_notation(code.scheme, &code.notation).is_none() {
                continue;
            }
            editions.insert(matched.open_library_edition_id.clone());
            for path in paths(&code.notation) {
                let root = path.split_once(" / ").expect("numeric subject path").0.to_owned();
                support.entry(root).or_default().insert(matched.open_library_edition_id.clone());
                path_support.entry(path).or_default().insert(matched.open_library_edition_id.clone());
            }
        }
    }
    if support.is_empty() {
        // With no mapped edition evidence, a work union spanning unrelated
        // routes cannot establish which subjects describe this edition.
        let inherited_paths = matches
            .iter()
            .flat_map(|(_, _, _, matched)| &matched.classifications)
            .filter(|code| code.scheme == ClassificationScheme::LibraryOfCongress && code.source != ClassificationSource::ExactEdition)
            .flat_map(|code| paths(&code.notation))
            .collect::<BTreeSet<_>>();
        let leaves = inherited_paths.iter().filter(|path| !inherited_paths.iter().any(|other| other.starts_with(&format!("{path} / ")))).count();
        if leaves > 1 {
            for (_, _, _, matched) in matches.iter_mut() {
                matched.classifications.retain(|code| code.scheme != ClassificationScheme::LibraryOfCongress || code.source == ClassificationSource::ExactEdition || paths(&code.notation).is_empty());
            }
        }
        return;
    }
    // Sparse records cannot establish a majority. Keep exact classifications;
    // inherited subjects must agree with the available edition evidence.
    if editions.len() < 10 {
        let supported_paths = path_support.keys().cloned().collect::<BTreeSet<_>>();
        for (_, _, _, matched) in matches.iter_mut() {
            matched.classifications.retain(|code| {
                if code.scheme != ClassificationScheme::LibraryOfCongress || code.source == ClassificationSource::ExactEdition {
                    return true;
                }
                let code_paths = paths(&code.notation);
                code_paths.is_empty() || !code_paths.is_disjoint(&supported_paths)
            });
        }
        return;
    }
    let dominant = support.iter().find_map(|(root, records)| (records.len() * 10 >= editions.len() * 9 && support.iter().all(|(other, votes)| other == root || votes.len() * 10 <= editions.len())).then_some(root.clone()));
    let accepted_roots = if let Some(root) = dominant { BTreeSet::from([root]) } else { support.iter().filter_map(|(root, records)| (records.len() >= 2).then_some(root.clone())).collect() };
    let mut accepted_paths = BTreeSet::new();
    for root in accepted_roots {
        let records = &support[&root];
        let prefix = format!("{root} / ");
        let branches = path_support.iter().filter(|(path, _)| path.starts_with(&prefix)).collect::<Vec<_>>();
        let dominant = branches.iter().find_map(|(path, votes)| (votes.len() * 10 >= records.len() * 9 && branches.iter().all(|(other, votes)| other == path || votes.len() * 10 <= records.len())).then_some((*path).clone()));
        if let Some(path) = dominant {
            accepted_paths.insert(path);
        } else {
            accepted_paths.extend(branches.into_iter().filter_map(|(path, votes)| (votes.len() >= 2).then_some(path.clone())));
        }
    }
    for (_, _, _, matched) in matches.iter_mut() {
        matched.classifications.retain(|code| {
            if code.scheme != ClassificationScheme::LibraryOfCongress {
                return true;
            }
            let code_paths = paths(&code.notation);
            code_paths.is_empty() || !code_paths.is_disjoint(&accepted_paths)
        });
    }
    matches.retain(|(_, _, _, matched)| matched.classifications.iter().any(|code| code.scheme == ClassificationScheme::LibraryOfCongress && classification_consensus_notation(code.scheme, &code.notation).is_some()));
}
