//! Existing English abbreviation catalog.
use super::super::{AbbreviationCase, AbbreviationEvidence, AbbreviationPolicy, AbbreviationRule};
use super::common::{ENGLISH_GERMAN_ABBREVIATIONS, ENGLISH_GERMAN_SWEDISH_ABBREVIATIONS, ENGLISH_SWEDISH_ABBREVIATIONS};

pub(crate) static ENGLISH_ABBREVIATIONS: &[AbbreviationRule] = &[
    AbbreviationRule::new(&[
        "mr.", "mrs.", "ms.", "sr.", "jr.", "mt.", "a.m.", "p.m.", "ph.d.", "approx.", "incl.", "excl.", "attn.", "ch.", "cont.", "dept.", "addr.", "encl.", "qty.", "misc.", "mfg.", "mfr.", "acct.", "amt.", "blvd.", "corp.", "abbr.", "appx.", "bldg.", "capt.", "chapt.", "contd.", "estd.", "gov.", "hon.", "ibid.", "inc.", "incog.", "instr.", "intl.", "ltd.", "regd.", "senr.", "subsp.", "suppr.", "vols.", "advt.", "adm.", "aftn.", "asym.", "auth.", "bds.", "bls.", "bsh.", "cath.", "cert.", "circ.", "cks.", "dft.", "disct.", "dpt.", "glos.", "gns.", "guar.", "hhd.", "outl.", "oxon.", "pcl.", "prol.", "prot.", "prox.", "pte.", "retd.", "stg.", "stk.", "suff.", "tgm.",
    ], AbbreviationCase::CaseInsensitive, AbbreviationEvidence::Listed),
    AbbreviationRule::new(&["no.", "fig.", "app.", "sec.", "ref.", "spec.", "ans.", "arr.", "def.", "dep.", "diff.", "dist.", "doc.", "ed.", "ext.", "par.", "pp.", "prev.", "quot.", "rel.", "seq.", "sub."], AbbreviationCase::CaseInsensitive, AbbreviationEvidence::Continuation),
    AbbreviationRule::new(&["resp."], AbbreviationCase::CaseInsensitive, AbbreviationEvidence::TerminalCompatible),
    AbbreviationRule::new(&["Ave."], AbbreviationCase::ExactCase, AbbreviationEvidence::PrecedingProperName),
    AbbreviationRule::new(&["Gen.", "Soc."], AbbreviationCase::ExactCase, AbbreviationEvidence::FollowingProperName),
    AbbreviationRule::new(&[
        "Acc.", "Ariz.", "Ark.", "Asst.", "Bk.", "Br.", "Bros.", "Bus.", "Calif.", "Col.", "Conn.", "Cpt.", "Div.", "Eng.", "Esq.", "Est.", "Ind.", "Maj.", "Mass.", "Messrs.", "Mich.", "Min.", "Minn.", "Miss.", "Mont.", "Mss.", "Mx.", "Neb.", "Nebr.", "Nev.", "Nr.", "Nos.", "Okla.", "Ore.", "Oreg.", "Penn.", "Penns.", "Pg.", "Pk.", "Plc.", "Pos.", "Pref.", "Pres.", "Pri.", "Pty.", "Pvt.", "Qr.", "Qu.", "Ret.", "Revd.", "Sergt.", "Sgt.", "Sq.", "Tenn.", "Tex.", "Va.", "Var.", "Viz.", "Wash.", "Wis.", "Wt.", "Wy.", "Wyo.", "Xt.", "Yr.",
    ], AbbreviationCase::ExactCase, AbbreviationEvidence::Listed),
    // Low-collision and capitalization-dependent entries absent from the
    // native catalog but present in Trados' English segmentation resources.
    // Lexical collisions deliberately excluded here are documented in
    // `research/TRADOS_ABBREVIATION_PARITY.md`.
    AbbreviationRule::new(&[
        "apr.", "dia.", "dur.", "fahr.", "fla.", "galv.", "hrs.", "pcs.", "typ.", "vis.", "yrs.",
    ], AbbreviationCase::CaseInsensitiveExceptAllCaps, AbbreviationEvidence::Listed),
    AbbreviationRule::new(&[
        "Dec.", "Fr.", "Lb.", "Oct.", "Qt.", "Sep.", "Yd.",
    ], AbbreviationCase::ExactCase, AbbreviationEvidence::Listed),
];
static ENGLISH_ABBREVIATION_CATALOGS: &[&[AbbreviationRule]] = &[ENGLISH_GERMAN_SWEDISH_ABBREVIATIONS, ENGLISH_GERMAN_ABBREVIATIONS, ENGLISH_SWEDISH_ABBREVIATIONS, ENGLISH_ABBREVIATIONS];

pub const ENGLISH_POLICY: AbbreviationPolicy = AbbreviationPolicy::new(ENGLISH_ABBREVIATION_CATALOGS, true);
