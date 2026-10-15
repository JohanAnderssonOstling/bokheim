//! Abbreviation rules shared by exact sets of language profiles.
use super::super::{AbbreviationCase, AbbreviationEvidence, AbbreviationRule};

pub(crate) static ENGLISH_GERMAN_SWEDISH_ABBREVIATIONS: &[AbbreviationRule] = &[
    AbbreviationRule::new(&["etc.", "e.g.", "i.e.", "vs.", "cf.", "ca.", "p.s.", "et al.", "diag.", "dr.", "ff.", "orig.", "prof.", "tel."], AbbreviationCase::CaseInsensitive, AbbreviationEvidence::Listed),
    AbbreviationRule::new(&["art.", "fax.", "max."], AbbreviationCase::CaseInsensitive, AbbreviationEvidence::Continuation),
];

pub(crate) static ENGLISH_GERMAN_ABBREVIATIONS: &[AbbreviationRule] = &[
    AbbreviationRule::new(&["biol.", "chron.", "engl.", "hr.", "mio.", "n.b.", "st.", "zool."], AbbreviationCase::CaseInsensitive, AbbreviationEvidence::Listed),
    AbbreviationRule::new(&["geol.", "geom.", "ger.", "mult.", "pharm.", "phys.", "prov.", "psychol.", "subj.", "sym.", "syst.", "theor.", "ult."], AbbreviationCase::CaseInsensitiveExceptAllCaps, AbbreviationEvidence::Listed),
    AbbreviationRule::new(&["Co.", "Feb.", "Ill.", "Jul.", "Lt.", "Nov.", "Prod.", "Pt.", "Rd.", "Sen.", "Univ."], AbbreviationCase::ExactCase, AbbreviationEvidence::Listed),
];

pub(crate) static ENGLISH_SWEDISH_ABBREVIATIONS: &[AbbreviationRule] = &[
    AbbreviationRule::new(&["spp.", "ssp.", "suppl.", "ver.", "vol."], AbbreviationCase::CaseInsensitive, AbbreviationEvidence::Listed),
    AbbreviationRule::new(&["rev."], AbbreviationCase::CaseInsensitive, AbbreviationEvidence::Continuation),
    AbbreviationRule::new(&["Reg."], AbbreviationCase::ExactCase, AbbreviationEvidence::Listed),
];

pub(crate) static GERMAN_SWEDISH_ABBREVIATIONS: &[AbbreviationRule] = &[
    AbbreviationRule::new(&["adr.", "exkl.", "fr.", "inkl.", "nr.", "ref.", "stud.", "u.a."], AbbreviationCase::CaseInsensitive, AbbreviationEvidence::Listed),
    AbbreviationRule::new(&["sek.", "sep."], AbbreviationCase::CaseInsensitiveExceptAllCaps, AbbreviationEvidence::Listed),
    AbbreviationRule::new(&["kap."], AbbreviationCase::CaseInsensitive, AbbreviationEvidence::Continuation),
];
