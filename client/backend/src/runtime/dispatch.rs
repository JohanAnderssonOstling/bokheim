use super::requests::dispatch_request;
use super::*;

pub(crate) async fn storage_command<T: crate::executor::BackendOutput>(operation: impl FnOnce() -> Result<T, String> + crate::executor::BackendSend + 'static) -> Result<T, String> {
    crate::storage::storage_queue::run(crate::storage::storage_queue::Priority::Interactive, operation).await?
}

pub async fn dispatch_worker_request(backend: AppBackend, request: AppWorkerRequest) -> Result<WorkerDispatchResult, String> {
    match request.command {
        WorkerCommand::App(command) => dispatch_request(backend, command).await,
        WorkerCommand::Library { library_id, command } => {
            let handle = backend.library_directory().get(&library_id).map_err(|error| error.to_string())?;
            Ok(WorkerDispatchResult::Response(handle.dispatch(command).await?))
        }
        WorkerCommand::LibrarySubscription { library_id, subscription } => {
            let handle = backend.library_directory().get(&library_id).map_err(|error| error.to_string())?;
            match subscription {
                LibrarySubscription::Updates => {
                    let events = handle.subscribe_updates();
                    let scanning = handle.scanning().await.map_err(|error| error.to_string())?;
                    Ok(WorkerDispatchResult::Subscription { initial: encode_reply(scanning)?, events: WorkerSubscription::LibraryUpdates(events) })
                }
                LibrarySubscription::BookDownload { content_hash } => {
                    let events = handle.subscribe_download(content_hash);
                    let initial = handle.download_state(content_hash).await.map_err(|error| error.to_string())?;
                    Ok(WorkerDispatchResult::Subscription { initial: encode_reply(initial)?, events: WorkerSubscription::DownloadStates(events) })
                }
            }
        }
    }
}

pub fn app_startup_state(backend: &AppBackend) -> Result<AppStartupState, String> {
    Ok(AppStartupState {
        libraries: backend.libraries().map_err(|error| error.to_string())?,
        browsing_preferences: backend.browsing_preferences().map_err(|error| error.to_string())?,
        account_status: backend.account_status().map_err(|error| error.to_string())?,
        reader_preferences: backend.reader_preferences().map_err(|error| error.to_string())?,
    })
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod ownership_tests {
    use super::*;

    #[test]
    fn remote_epub_wire_response_contains_only_the_descriptor_and_prefix() {
        let remote = serde_json::from_value(serde_json::json!({
            "hash": test_support::fixture_content_hash(991), "checksum": test_support::fixture_content_hash(992), "length":10485760
        }))
        .unwrap();
        let resolved = ResolvedBookData { pdf_metadata: None, format: library_model::BookFormat::Epub, source: PublicationSource::Remote { source: remote, prefix: vec![42; 65536] } };
        let WorkerPayload(bytes) = encode_reply(resolved).unwrap();
        let decoded: ResolvedBookData = decode_worker_message(&bytes).unwrap();
        let PublicationSource::Remote { source, prefix } = decoded.source else {
            panic!("expected remote source");
        };
        assert_eq!(prefix, vec![42; 65536]);
        assert!(bytes.len() < 65536 + 1024);
        assert!(encode_worker_message(&source).unwrap().len() < 1024);
    }

    #[tokio::test]
    async fn settings_patches_are_ordered_by_the_worker() {
        let root = tempfile::tempdir().unwrap();
        let context = crate::app::BackendContext::initialize(crate::app::AppDataLocation::native_path(root.path())).unwrap();
        let persisted = context.clone();
        let (client, _) = AppClient::start_native(crate::BackendLaunch::initialize(crate::AppDataLocation::native_path(root.path())).unwrap()).unwrap();

        let first = client.update_reader_preferences(vec![app_preferences::ReaderPreferencePatch::LineHeight(1.8)]);
        let second = client.update_reader_preferences(vec![app_preferences::ReaderPreferencePatch::Zoom(1.7)]);
        let (first, second) = futures_util::future::join(first, second).await;
        first.unwrap();
        second.unwrap();

        let preferences = persisted.load_reader_preferences().unwrap();
        assert_eq!(preferences.line_height(), 1.8);
        assert_eq!(preferences.zoom(), 1.7);
    }

    #[test]
    fn only_settings_commands_use_ordered_execution() {
        assert!(AppCommand::UpdateBrowsingPreference { patch: app_preferences::BrowsingPreferencePatch::Theme(app_preferences::ApplicationTheme::Cool) }.is_settings_update());
        assert!(!AppCommand::ReconcileLibraries.is_settings_update());
    }
}
