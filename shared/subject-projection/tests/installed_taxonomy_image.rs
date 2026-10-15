#![cfg(not(target_arch = "wasm32"))]

mod support;

use subject_projection::{installed, unified_taxonomy_revision_id, UnifiedTaxonomy};

#[test]
fn selected_image_installs_the_expected_matcher() {
    let (image, definitions) = support::selected_snapshot();
    let expected = UnifiedTaxonomy::from_concepts(definitions.clone()).unwrap();
    installed::activate_image(image).unwrap();

    assert_eq!(unified_taxonomy_revision_id(), expected.revision_id());
    support::assert_selected_matches(&expected, &definitions);
}
