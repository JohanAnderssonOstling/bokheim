use super::*;
use sync_common::LibraryId;

#[test]
fn first_launch_prepares_app_data_for_startup_locks_before_opening_the_backend() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("new-profile/bokheim");
    let location = AppDataLocation::native_path(&data);
    let launch = BackendLaunch::initialize(location.clone()).unwrap();
    assert_eq!(launch.app_data_dir(), data);
    assert!(data.is_dir());
    assert!(!data.join("libraries").exists());

    for name in ["windows-update-launch.lock", "desktop-instance.lock"] {
        let lock = data.join(name);
        let handle = fs::OpenOptions::new().create(true).truncate(false).read(true).write(true).open(&lock).unwrap();
        handle.try_lock().unwrap();
        BackendLaunch::initialize(location.clone()).unwrap();
        drop(handle);
        fs::write(&lock, b"existing startup state").unwrap();
        BackendLaunch::initialize(location.clone()).unwrap();
        assert_eq!(fs::read(lock).unwrap(), b"existing startup state");
    }
}

#[test]
fn launch_reports_app_data_directory_creation_failures() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("not-a-directory");
    fs::write(&file, b"keep").unwrap();
    let error = BackendLaunch::initialize(AppDataLocation::native_path(file.join("bokheim"))).unwrap_err();
    assert!(error.contains("failed to create app-data directory"), "{error}");
    assert_eq!(fs::read(file).unwrap(), b"keep");
}

#[test]
fn private_storage_replaces_a_shared_default_without_moving_registered_libraries() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("private");
    let location = AppDataLocation::native_path(&data);
    let original = BackendContext::initialize(location.clone()).unwrap();
    let shared = root.path().join("Documents");
    app_preferences::library_folder::set_library_save_location(original.preferences.storage_root(), shared.to_str().unwrap()).unwrap();
    let existing = original.registry.create("Existing", None).unwrap();
    let existing_path = PathBuf::from(existing.storage_locator());
    fs::write(existing_path.join("keep.txt"), "original library").unwrap();
    drop(original);

    let context = BackendLaunch::initialize(location).unwrap().with_private_library_storage().into_context().unwrap();
    assert_eq!(context.registry.entries().unwrap()[0].storage_locator(), existing.storage_locator());
    assert_eq!(fs::read_to_string(existing_path.join("keep.txt")).unwrap(), "original library");
    let created = context.registry.create("Imported", None).unwrap();
    let remote = context.registry.attach_remote(&LibraryId::new_v4(), "Remote", context.registry.default_library_root()).unwrap();
    let private = data.canonicalize().unwrap().join("libraries");
    assert_eq!(Path::new(created.storage_locator()), private.join("Imported"));
    assert_eq!(Path::new(remote.storage_locator()), private.join("Remote"));
}

#[tokio::test]
async fn desktop_startup_preserves_documents_default_and_saved_override() {
    let root = tempfile::tempdir().unwrap();
    let mut launch = BackendLaunch::initialize(AppDataLocation::SharedDesktop).unwrap();
    // Exercise desktop startup without opening the user's registry or
    // creating libraries in their Documents directory.
    launch.data_dir = root.path().join("app");
    let expected = client_platform_desktop::default_library_root().unwrap();
    let context = launch.clone().into_context().unwrap();
    assert_eq!(context.registry.default_library_root(), expected.to_string_lossy());
    let chosen = fs::canonicalize(root.path()).unwrap().join("Chosen");
    app_preferences::library_folder::set_library_save_location(context.preferences.storage_root(), chosen.to_str().unwrap()).unwrap();
    let restarted = launch.into_context().unwrap();
    let registry = restarted.registry.clone();
    let backend = crate::app::AppBackend::new(restarted).await.unwrap();
    let created = backend.library_service.register_with_asset_storage("Created", None, true).await.unwrap();
    let remote = registry.attach_remote(&LibraryId::new_v4(), "Remote", chosen.to_str().unwrap()).unwrap();
    assert_eq!(Path::new(created.storage_locator()), chosen.join("Created"));
    assert_eq!(Path::new(remote.storage_locator()), chosen.join("Remote"));
}

#[tokio::test]
async fn discovery_root_places_new_and_remote_libraries_visibly_without_moving_existing_libraries() {
    let root = tempfile::tempdir().unwrap();
    let location = AppDataLocation::native_path(root.path().join("private"));
    let original = BackendContext::initialize(location.clone()).unwrap();
    let existing = original.registry.create("Existing", None).unwrap();
    drop(original);
    let visible = fs::canonicalize(root.path()).unwrap().join("Bokheim/Libraries");
    let launch = BackendLaunch::initialize(location).unwrap().with_library_discovery(visible.clone(), "Kobo".into(), visible.join("default"));
    let context = launch.clone().into_context().unwrap();
    let created = context.registry.create("Created", None).unwrap();
    let remote = context.registry.attach_remote(&LibraryId::new_v4(), "Remote", context.registry.default_library_root()).unwrap();
    assert_eq!(Path::new(created.storage_locator()), visible.join("Created"));
    assert_eq!(Path::new(remote.storage_locator()), visible.join("Remote"));
    let restarted = launch.clone().into_context().unwrap();
    let entries = restarted.registry.entries().unwrap();
    assert_eq!(entries.iter().find(|entry| entry.library_id() == existing.library_id()).unwrap().storage_locator(), existing.storage_locator());
    assert_eq!(restarted.registry.attach_remote(remote.library_id(), "Remote", restarted.registry.default_library_root()).unwrap().storage_locator(), remote.storage_locator());

    let chosen = fs::canonicalize(root.path()).unwrap().join("User chosen");
    app_preferences::library_folder::set_library_save_location(context.preferences.storage_root(), chosen.to_str().unwrap()).unwrap();
    let restarted = launch.into_context().unwrap();
    let backend = crate::app::AppBackend::new(restarted).await.unwrap();
    let next = backend.library_service.register_with_asset_storage("Next", None, true).await.unwrap();
    assert_eq!(Path::new(next.storage_locator()), chosen.join("Next"));
}

#[test]
fn discovery_registers_a_fallback_once_and_preserves_it_on_restart() {
    let root = tempfile::tempdir().unwrap();
    let location = AppDataLocation::native_path(root.path().join("app"));
    let libraries = root.path().join("libraries");
    fs::create_dir_all(&libraries).unwrap();
    let fallback = libraries.join("default");
    let launch = BackendLaunch::initialize(location).unwrap().with_library_discovery(libraries, "Device library".into(), fallback.clone());
    assert!(!fallback.exists(), "configuration must not mutate the library registry");
    let first = launch.clone().into_context().unwrap();
    let entries = first.registry.entries().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].library_name(), "Device library");
    let restarted = launch.into_context().unwrap().registry.entries().unwrap();
    assert_eq!(restarted.len(), 1);
    assert_eq!(restarted[0].library_id(), entries[0].library_id());
}

#[test]
fn discovery_does_not_recreate_a_library_awaiting_local_purge() {
    let root = tempfile::tempdir().unwrap();
    let location = AppDataLocation::native_path(root.path().join("app"));
    let libraries = root.path().join("libraries");
    let fallback = libraries.join("default");
    let launch = BackendLaunch::initialize(location).unwrap().with_library_discovery(libraries, "Device library".into(), fallback);
    let context = launch.clone().into_context().unwrap();
    let id = *context.registry.entries().unwrap()[0].library_id();
    context.registry.begin_local_removal(&id).unwrap();
    drop(context);

    let restarted = launch.into_context().unwrap();
    assert!(restarted.registry.entries().unwrap().is_empty());
    assert_eq!(restarted.registry.pending_local_purges().unwrap(), [id]);
}
