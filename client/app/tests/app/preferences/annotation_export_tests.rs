use app_preferences::Client;
#[tokio::test]
async fn annotation_export_folder_is_device_local_and_survives_reopening() {
    let root = tempfile::tempdir().unwrap();
    let client = Client::new(root.path().join("preferences"));
    let folder = root.path().join("chosen-export-folder").to_string_lossy().into_owned();
    assert!(client.set_default_annotation_export_folder("relative/folder".into()).await.is_err());
    client.set_default_annotation_export_folder(folder.clone()).await.unwrap();
    assert_eq!(client.default_annotation_export_folder().await.unwrap(), Some(folder.clone()));
    assert!(!std::path::Path::new(&folder).exists(), "choosing a preference should not create an output directory");
    drop(client);
    let client = Client::new(root.path().join("preferences"));
    assert_eq!(client.default_annotation_export_folder().await.unwrap(), Some(folder));
}
