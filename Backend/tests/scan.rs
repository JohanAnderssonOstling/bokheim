use epub_svelte_lib::library::Library;
use epub_svelte_lib::{AppDataLocation, BackendContext};
use std::time::Duration;

#[test]

pub fn test_scan() {
    let app_data = tempfile::tempdir().unwrap();
    let context = BackendContext::initialize(AppDataLocation::PlatformLocal(app_data.path().to_path_buf())).unwrap();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/test_folder");
    let fixture = fixture.to_str().expect("fixture path must be valid UTF-8");
    let _library = Library::try_new(&context, fixture, "library").unwrap();
    std::thread::sleep(Duration::from_secs(10));
}
