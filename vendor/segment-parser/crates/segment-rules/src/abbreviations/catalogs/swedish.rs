//! Existing Swedish abbreviation catalog.
use super::super::{AbbreviationCase, AbbreviationEvidence, AbbreviationPolicy, AbbreviationRule};
use super::common::{ENGLISH_GERMAN_SWEDISH_ABBREVIATIONS, ENGLISH_SWEDISH_ABBREVIATIONS, GERMAN_SWEDISH_ABBREVIATIONS};

pub(crate) static SWEDISH_ABBREVIATIONS: &[AbbreviationRule] = &[
    AbbreviationRule::new(&[
        "dvs.", "t.ex.", "m.fl.", "osv.", "s:t", "s.k.", "obs.", "f.d.", "bl.a.", "fr.o.m.", "t.o.m.", "resp.", "ev.", "jfr.", "ang.", "fig.", "avd.", "avsn.", "spec.", "d.v.s.", "e.d.", "kl.", "m.m.", "o.d.", "p.a.", "p.g.a.", "s.a.", "u.s.c.", "v.g.",
    ], AbbreviationCase::CaseInsensitive, AbbreviationEvidence::Listed),
    AbbreviationRule::new(&["forts.", "bil.", "bl.", "lat.", "long.", "sp."], AbbreviationCase::CaseInsensitive, AbbreviationEvidence::Continuation),
    AbbreviationRule::new(&["Res.", "St."], AbbreviationCase::ExactCase, AbbreviationEvidence::Listed),
];
static SWEDISH_ABBREVIATION_CATALOGS: &[&[AbbreviationRule]] = &[ENGLISH_GERMAN_SWEDISH_ABBREVIATIONS, ENGLISH_SWEDISH_ABBREVIATIONS, GERMAN_SWEDISH_ABBREVIATIONS, SWEDISH_ABBREVIATIONS];

pub const SWEDISH_POLICY: AbbreviationPolicy = AbbreviationPolicy::new(SWEDISH_ABBREVIATION_CATALOGS, false);
