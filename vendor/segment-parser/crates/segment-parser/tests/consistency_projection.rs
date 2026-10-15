use segment_parser::{ParseLanguage, consistency_projection, parse_tokenized};
use structure::{Run, SegmentTree, TagData, TagKind, TokenizedSegment};

fn tagged(text: &str, id: &str, kind: TagKind) -> SegmentTree {
    let mut run = Run::new(text);
    run.push_tag_at(0, TagData::new(id.to_string(), "g".to_string(), kind, "<g>".to_string(), None, None));
    segment_parser::parse_runs([run])
}

#[test]
fn projection_normalizes_unicode_and_whitespace_without_losing_case_by_default() {
    let composed = segment_parser::parse_text("  CAFÉ\tfile  ");
    let decomposed = segment_parser::parse_text("CAFE\u{301}\u{2003}file");
    let lowercase = segment_parser::parse_text("café file");

    assert_eq!(consistency_projection(&composed, false), consistency_projection(&decomposed, false));
    assert_ne!(consistency_projection(&composed, false), consistency_projection(&lowercase, false));
    assert_eq!(consistency_projection(&composed, true), consistency_projection(&lowercase, true));
}

#[test]
fn case_insensitive_projection_uses_full_unicode_case_folding() {
    assert_eq!(consistency_projection(&segment_parser::parse_text("Straße"), true), consistency_projection(&segment_parser::parse_text("STRASSE"), true));
    assert_eq!(consistency_projection(&segment_parser::parse_text("οδυσσεύς"), true), consistency_projection(&segment_parser::parse_text("ΟΔΥΣΣΕΎΣ"), true));
}

#[test]
fn projection_preserves_punctuation_numbers_and_unit_order() {
    let first = segment_parser::parse_text("Save 10.");
    let punctuation = segment_parser::parse_text("Save 10!");
    let number = segment_parser::parse_text("Save 11.");
    let order = segment_parser::parse_text("10 Save.");

    assert_ne!(consistency_projection(&first, false), consistency_projection(&punctuation, false));
    assert_ne!(consistency_projection(&first, false), consistency_projection(&number, false));
    assert_ne!(consistency_projection(&first, false), consistency_projection(&order, false));
}

#[test]
fn projection_preserves_tag_identity_and_role() {
    let first = tagged("Save", "1", TagKind::Open);
    let other_id = tagged("Save", "2", TagKind::Open);
    let other_role = tagged("Save", "1", TagKind::Standalone);

    assert_ne!(consistency_projection(&first, false), consistency_projection(&other_id, false));
    assert_ne!(consistency_projection(&first, false), consistency_projection(&other_role, false));
}

#[test]
fn projection_ignores_derived_unit_classification() {
    let lexical = TokenizedSegment::from_text("Save 12 files.");
    let parsed = parse_tokenized(lexical.clone(), ParseLanguage::English);

    assert_ne!(lexical.tree().iter_units().map(|unit| unit.kind()).collect::<Vec<_>>(), parsed.iter_units().map(|unit| unit.kind()).collect::<Vec<_>>());
    assert_eq!(consistency_projection(lexical.tree(), false), consistency_projection(&parsed, false));
}
