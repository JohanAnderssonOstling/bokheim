#![cfg(not(target_arch = "wasm32"))]

mod support;

use subject_projection::{installed, loaded_unified_taxonomy_revision_id, unified_taxonomy_revision_id};

#[test]
fn selected_file_installs_the_expected_matcher() {
    let (image, expected, revision) = support::selected_snapshot();
    let file = tempfile::NamedTempFile::new().unwrap();
    // Activation validates the prepared image before pinning its path.
    assert!(installed::activate(file.path()).is_err());
    assert!(loaded_unified_taxonomy_revision_id().is_none());
    std::fs::write(file.path(), &image).unwrap();
    installed::validate(file.path(), installed::metadata(file.path()).unwrap()).unwrap();
    installed::activate(file.path()).unwrap();
    assert!(loaded_unified_taxonomy_revision_id().is_none(), "selecting a file must not initialize the matcher");

    assert_eq!(unified_taxonomy_revision_id(), revision);
    support::assert_selected_matches(&expected);
}
