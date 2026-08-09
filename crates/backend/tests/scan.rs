use epub_svelte_lib::{AppBackend, AppDataLocation, BackendContext};

#[test]

pub fn test_scan() {
    let app_data = tempfile::tempdir().unwrap();
    let context = BackendContext::initialize(AppDataLocation::PlatformLocal(app_data.path().to_path_buf())).unwrap();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/test_folder");
    let app = AppBackend::new(context).unwrap();
    let entry = app.create_library("library".to_owned(), Some(fixture)).unwrap();
    let library = app.library(entry.library_id().as_str()).unwrap();
    assert_eq!(library.id(), entry.library_id());
}
