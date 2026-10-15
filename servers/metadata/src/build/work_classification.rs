//! Work-level LCC selection uses edition records, never counts of call-number variants.
use metadata_contract::{classification_consensus_notation, ClassificationScheme};
use std::collections::{BTreeMap, BTreeSet};
use subject_projection::{unified_subject_paths, LCC_SYSTEM_ID};

#[derive(Default)]
pub(super) struct SiblingLccEvidence {
    editions: BTreeSet<i64>,
    support: BTreeMap<String, BTreeSet<i64>>,
}

impl SiblingLccEvidence {
    pub(super) fn add(&mut self, edition: i64, notation: &str) {
        let Some(notation) = classification_consensus_notation(ClassificationScheme::LibraryOfCongress, notation) else { return };
        // Unmapped numeric codes still contribute to the denominator. Editions
        // with no numeric LCC, including bare subclasses, are not votes.
        self.editions.insert(edition);
        self.support.entry(notation).or_default().insert(edition);
    }

    pub(super) fn select(&self, imported: BTreeSet<String>) -> BTreeSet<String> {
        let roots = self.support.keys().map(|code| (code, classification_roots(code))).collect::<BTreeMap<_, _>>();
        let mut root_support = BTreeMap::<String, BTreeSet<i64>>::new();
        for (code, editions) in &self.support {
            for root in &roots[code] {
                root_support.entry(root.clone()).or_default().extend(editions);
            }
        }
        // One observation cannot establish corroborated dominance. A failed
        // dominance check preserves sparse or substantial secondary evidence.
        let primary = (self.editions.len() >= 2)
            .then(|| {
                root_support.iter().find_map(|(root, editions)| (editions.len() * 10 >= self.editions.len() * 9 && root_support.iter().all(|(other, records)| other == root || records.len() * 10 <= self.editions.len())).then_some(root))
            })
            .flatten();
        let consensus = self
            .support
            .iter()
            .filter(|(code, editions)| self.editions.len() == 1 || editions.len() >= 2 || primary.is_some_and(|root| roots[*code].contains(root)))
            .filter(|(code, _)| primary.is_none_or(|root| roots[*code].is_empty() || roots[*code].contains(root)))
            .map(|(code, _)| code.clone())
            .collect::<BTreeSet<_>>();

        let sufficient_work = primary.is_some_and(|primary| {
            !imported.is_empty() && imported.iter().all(|code| classification_consensus_notation(ClassificationScheme::LibraryOfCongress, code).is_some() && classification_roots(code) == BTreeSet::from([primary.clone()]))
        });
        if sufficient_work {
            // A usable, corroborated work classification covers the primary
            // subtree. Keep unknown consensus codes conservatively.
            return imported.into_iter().chain(consensus.into_iter().filter(|code| roots[code].is_empty())).collect();
        }
        imported
            .into_iter()
            .filter(|code| {
                let roots = classification_roots(code);
                primary.is_none_or(|root| roots.is_empty() || roots.contains(root))
            })
            .chain(consensus)
            .collect()
    }
}

fn classification_roots(code: &str) -> BTreeSet<String> {
    let paths = unified_subject_paths(LCC_SYSTEM_ID, code);
    // A top-level fallback alone does not make a work classification sufficient.
    if paths.is_empty() || paths.iter().any(|path| !path.contains(" / ")) {
        return BTreeSet::new();
    }
    paths.into_iter().filter_map(|path| path.split(" / ").next().map(str::to_owned)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(values: &[&str]) -> BTreeSet<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn emma_work_category_stops_edition_variants_after_record_corroboration() {
        let mut evidence = SiblingLccEvidence::default();
        for edition in 0..105 {
            evidence.add(edition, "PR4034 .E5 1994");
        }
        evidence.add(105, "PE1126");
        evidence.add(106, "PE1128");
        assert_eq!(evidence.select(codes(&["PR4030"])), codes(&["PR4030"]));
        assert_eq!(evidence.select(BTreeSet::new()), codes(&["PR4034"]));
    }

    #[test]
    fn edition_variants_cannot_outvote_literature_or_validate_a_reader_work_code() {
        let mut evidence = SiblingLccEvidence::default();
        for edition in 0..55 {
            evidence.add(edition, "PR4750");
        }
        for edition in 55..57 {
            for variant in 0..100 {
                evidence.add(edition, &format!("PE1128 .A{variant}"));
            }
        }
        evidence.add(57, "RR4750");
        assert_eq!(evidence.select(codes(&["PE1128"])), codes(&["PR4750"]));
    }

    #[test]
    fn wealth_of_nations_retains_economics_and_substantial_secondary_evidence() {
        let mut evidence = SiblingLccEvidence::default();
        for edition in 0..10 {
            evidence.add(edition, "HB161");
        }
        evidence.add(0, "AC7");
        evidence.add(1, "AC7");
        assert_eq!(evidence.select(codes(&["AC7"])), codes(&["AC7", "HB161"]));
    }

    #[test]
    fn tractatus_keeps_logic_even_when_the_work_lists_only_the_author_class() {
        let mut evidence = SiblingLccEvidence::default();
        for edition in 0..26 {
            evidence.add(edition, "B3376");
        }
        for edition in 0..9 {
            evidence.add(edition, "BC135");
        }
        assert_eq!(evidence.select(codes(&["B3376"])), codes(&["B3376", "BC135"]));
    }

    #[test]
    fn sparse_and_unmapped_evidence_is_preserved_without_a_dominance_decision() {
        let mut evidence = SiblingLccEvidence::default();
        evidence.add(1, "PR4034");
        assert_eq!(evidence.select(codes(&["PE1128"])), codes(&["PE1128", "PR4034"]));
        evidence.add(2, "PR4034");
        evidence.add(3, "ZZ123");
        assert_eq!(evidence.select(codes(&["PE1128"])), codes(&["PE1128", "PR4034"]));
        assert_eq!(SiblingLccEvidence::default().select(codes(&["ZZ123"])), codes(&["ZZ123"]));
    }

    #[test]
    fn code_count_does_not_limit_a_corroborated_work_subtree() {
        let mut evidence = SiblingLccEvidence::default();
        evidence.add(1, "PR4034");
        evidence.add(2, "PR4034");
        let imported = (0..20).map(|year| format!("PR4030 .A1 {}", 1900 + year)).collect::<BTreeSet<_>>();
        assert_eq!(evidence.select(imported.clone()), imported);
    }

    #[test]
    fn root_support_corroborates_rare_codes_without_requiring_each_code_to_repeat() {
        let mut evidence = SiblingLccEvidence::default();
        evidence.add(1, "PR4030");
        evidence.add(2, "PR4034");
        assert_eq!(evidence.select(BTreeSet::new()), codes(&["PR4030", "PR4034"]));
    }

    #[test]
    fn substantial_secondary_support_cannot_be_hidden_by_primary_code_variants() {
        let mut evidence = SiblingLccEvidence::default();
        for edition in 0..20 {
            for class in 3370..3380 {
                evidence.add(edition, &format!("B{class}"));
            }
        }
        evidence.add(0, "BC135");
        evidence.add(1, "BC135");
        evidence.add(2, "BC135");
        assert!(evidence.select(codes(&["B3376"])).contains("BC135"));
    }
}
