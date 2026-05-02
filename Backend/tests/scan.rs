use epub_svelte_lib::library::Library;
use std::time::Duration;

#[test]

pub fn test_scan() {
    let library = Library::new("/home/johan/Hem/Programmering/RustroverProjects/Html-Prototype/Backend/tests/test_folder", "library");
    std::thread::sleep(Duration::from_secs(10));
}
