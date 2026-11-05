use app_preferences::{Client, ReaderPreferencePatch, ReaderPreferences};

#[tokio::test]
async fn concurrent_reader_preference_updates_preserve_independent_patches() {
    let root = tempfile::tempdir().unwrap();
    let client = Client::new(root.path().join("preferences"));

    let first = client.update_reader(vec![ReaderPreferencePatch::LineHeight(1.8)]);
    let second = client.update_reader(vec![ReaderPreferencePatch::LineHeight(2.0), ReaderPreferencePatch::Zoom(1.7)]);
    let (first, second) = futures_util::future::join(first, second).await;
    first.unwrap();
    second.unwrap();

    let preferences = serde_json::from_slice::<ReaderPreferences>(&std::fs::read(root.path().join("preferences/reader.json")).unwrap()).unwrap();
    assert!([1.8, 2.0].contains(&preferences.line_height()));
    assert_eq!(preferences.zoom(), 1.7);
}
