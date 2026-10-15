#![cfg(not(target_arch = "wasm32"))]

mod support;

use subject_projection::{installed, unified_taxonomy_revision_id, UnifiedTaxonomy};

#[test]
fn selected_file_installs_the_expected_matcher() {
    let (image, definitions) = support::selected_snapshot();
    let expected = UnifiedTaxonomy::from_concepts(definitions.clone()).unwrap();
    let file = tempfile::NamedTempFile::new().unwrap();
    installed::store_image(&image, file.path()).unwrap();
    installed::prepare(file.path()).unwrap();
    installed::activate(file.path()).unwrap();

    assert_eq!(unified_taxonomy_revision_id(), expected.revision_id());
    support::assert_selected_matches(&expected, &definitions);
}
