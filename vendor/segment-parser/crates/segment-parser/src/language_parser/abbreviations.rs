//! Segment-tree language adapters for shared abbreviation policies.
use segment_rules::abbreviations::AbbreviationEvidence;
#[cfg(test)]
use segment_rules::abbreviations::{AbbreviationCase, AbbreviationRule};
use super::{AbbreviationPolicy, LanguageProfile};
use super::english_parser::ENGLISH_PROFILE;
use super::german_parser::GERMAN_PROFILE;
use super::swedish_parser::SWEDISH_PROFILE;

pub(crate) const LANGUAGE_PROFILES: &[LanguageProfile] = &[ENGLISH_PROFILE, SWEDISH_PROFILE, GERMAN_PROFILE];

pub(crate) fn abbreviation_policy_contains(policy: AbbreviationPolicy, exact: &str, normalized: &str) -> bool { policy.contains_normalized(exact, normalized) }

pub(crate) fn abbreviation_policy_has_prefix(policy: AbbreviationPolicy, exact: &str, normalized: &str) -> bool { policy.has_prefix_normalized(exact, normalized) }

pub(crate) fn abbreviation_policy_evidence(policy: AbbreviationPolicy, exact: &str, normalized: &str) -> Option<AbbreviationEvidence> { policy.evidence_normalized(exact, normalized) }

pub(crate) fn profile_from_alias(primary_language: &str) -> Option<&'static LanguageProfile> {
    LANGUAGE_PROFILES
        .iter()
        .find(|profile| profile.aliases.iter().any(|alias| alias.eq_ignore_ascii_case(primary_language)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn distinct_catalogs() -> Vec<&'static [AbbreviationRule]> {
        LANGUAGE_PROFILES
            .iter()
            .flat_map(|profile| profile.abbreviation.catalogs().iter().copied())
            .fold(Vec::new(), |mut catalogs, catalog| {
                if !catalogs.iter().any(|known| *known == catalog) {
                    catalogs.push(catalog);
                }
                catalogs
            })
    }

    fn definition_spelling(spelling: &str, case: AbbreviationCase) -> String {
        match case {
            AbbreviationCase::ExactCase => spelling.to_owned(),
            AbbreviationCase::CaseInsensitive | AbbreviationCase::CaseInsensitiveExceptAllCaps => spelling.to_lowercase(),
        }
    }

    #[test]
    fn abbreviation_rules_have_one_catalog_owner() {
        let mut definitions = Vec::new();
        distinct_catalogs().iter().enumerate().for_each(|(catalog_index, catalog)| {
            catalog.iter().for_each(|rule| {
                rule.spellings().iter().for_each(|spelling| {
                    let normalized = definition_spelling(spelling, rule.case());
                    let previous = definitions.iter().find(|(defined, case, evidence, _)| defined == &normalized && *case == rule.case() && *evidence == rule.evidence());
                    assert!(previous.is_none(), "abbreviation rule {spelling:?}/{:?}/{:?} is defined in catalogs {} and {catalog_index}", rule.case(), rule.evidence(), previous.map_or(catalog_index, |(_, _, _, owner)| *owner));
                    definitions.push((normalized, rule.case(), rule.evidence(), catalog_index));
                });
            });
        });
    }

    #[test]
    fn abbreviation_catalogs_are_maximal_by_language_membership() {
        let mut memberships = Vec::new();
        distinct_catalogs().iter().enumerate().for_each(|(catalog_index, catalog)| {
            let membership = LANGUAGE_PROFILES
                .iter()
                .filter(|profile| profile.abbreviation.catalogs().iter().any(|selected| *selected == *catalog))
                .map(|profile| profile.primary_language)
                .collect::<Vec<_>>();
            assert!(!memberships.contains(&membership), "catalog {catalog_index} duplicates language membership {membership:?}");
            memberships.push(membership);
        });
    }

    #[test]
    fn defensible_german_abbreviations_are_shared_with_matching_languages() {
        let contains = |profile: &LanguageProfile, spelling: &str| abbreviation_policy_contains(profile.abbreviation, spelling, &spelling.to_lowercase());
        ["ff."].iter().for_each(|spelling| LANGUAGE_PROFILES.iter().for_each(|profile| assert!(contains(profile, spelling), "{spelling} missing from {}", profile.primary_language)));
        ["biol.", "chron.", "geol.", "geom.", "hr.", "pharm.", "phys.", "prov.", "psychol.", "subj.", "sym.", "syst.", "theor.", "ult.", "Univ.", "zool."].iter().for_each(|spelling| {
            assert!(contains(&ENGLISH_PROFILE, spelling), "{spelling} missing from English");
            assert!(contains(&GERMAN_PROFILE, spelling), "{spelling} missing from German");
            assert!(!contains(&SWEDISH_PROFILE, spelling), "{spelling} unexpectedly present in Swedish");
        });
        ["fr.", "sek.", "sep.", "stud.", "u.a."].iter().for_each(|spelling| {
            assert!(!contains(&ENGLISH_PROFILE, spelling), "{spelling} unexpectedly present in English");
            assert!(contains(&GERMAN_PROFILE, spelling), "{spelling} missing from German");
            assert!(contains(&SWEDISH_PROFILE, spelling), "{spelling} missing from Swedish");
        });
    }
}
