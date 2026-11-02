//! Device-global commands and their execution against the application backend.
//!
//! One enum carries the command on the wire and one match runs it. Settings
//! writes are the only commands that must observe receive order; they are
//! listed by [`AppCommand::is_settings_update`] and executed by
//! [`execute_ordered`], which both transports reach through their ordered path.

use std::path::PathBuf;

use super::{WorkerDispatchResult, WorkerSubscription};
use crate::BackendError;
use crate::app::AppBackend;
use crate::executor::{BackendOutput, BackendSend};
use client_platform_runtime::storage_queue::{self, Priority};
use client_runtime::reply::Reply;
use library_backend::library::discovery::estimate_folders;
use sync_common::LibraryId;

fn reply<T: BackendOutput + serde::Serialize>(value: T) -> Result<Reply, BackendError> {
    Ok(Reply::new(value))
}

async fn storage_command<T: BackendOutput>(operation: impl FnOnce() -> Result<T, BackendError> + BackendSend + 'static) -> Result<T, BackendError> {
    storage_queue::run(Priority::Interactive, operation).await?
}

fn message(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> BackendError {
    BackendError::operation(error)
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum AppCommand {
    DictionaryDownloads {
        languages: Vec<String>,
    },
    DownloadDictionary {
        id: String,
    },
    SubscribeDictionaryChanges,
    Dictionaries,
    ImportDictionary {
        files: Vec<dictionary::DictionaryFile>,
    },
    ConfigureDictionary {
        id: String,
        enabled: bool,
        direction: i8,
    },
    RemoveDictionary {
        id: String,
    },
    LookupDictionary {
        query: String,
        languages: Vec<String>,
    },
    LookupWikipedia {
        query: String,
        languages: Vec<String>,
    },
    TranslateSelection {
        text: String,
        source: String,
        target: String,
    },
    Libraries,
    DefaultLibrarySaveLocation,
    SetDefaultLibrarySaveLocation {
        path: String,
    },
    UpdateBrowsingPreference {
        patch: app_preferences::BrowsingPreferencePatch,
    },
    UpdateReaderPreferences {
        patches: Vec<app_preferences::ReaderPreferencePatch>,
    },
    EstimateLibraryFolders {
        locators: Vec<String>,
    },
    CreateLibraryWithAssetStorage {
        name: String,
        asset_storage_enabled: bool,
    },
    CreateLibrariesWithAssetStorage {
        locators: Vec<String>,
        asset_storage_enabled: bool,
    },
    SetLibraryAssetStorage {
        library_id: LibraryId,
        enabled: bool,
    },
    EnsureImportLibrary,
    RenameLibrary {
        library_id: LibraryId,
        name: String,
    },
    DeleteLibrary {
        library_id: LibraryId,
    },
    RequestPublicRegistration {
        email: String,
        password: String,
    },
    ResendVerification {
        email: String,
    },
    RequestPasswordReset {
        email: String,
    },
    ResetPassword {
        token: String,
        password: String,
    },
    AccountSnapshot,
    LocalStorageUsage,
    TextSearchIndexBytes,
    Login {
        email: String,
        password: String,
    },
    VerifyEmail {
        email: String,
        pin: String,
        password: String,
    },
    Logout,
    SubscribeNotifications {
        browsing: bool,
    },
    SubscribeLibraryList,
    OpdsFeeds,
    AddOpdsFeed {
        url: String,
        name: Option<String>,
        credentials: Option<library_model::OpdsCredentials>,
    },
    RemoveOpdsFeed {
        id: library_model::OpdsSourceId,
    },
    SetOpdsCredentials {
        id: library_model::OpdsSourceId,
        credentials: library_model::OpdsCredentials,
    },
    BrowseOpds {
        query: library_model::LibraryBrowseQuery,
    },
    OpdsCover {
        source: library_model::OpdsSourceId,
        entry: library_model::CardKey,
        width: u32,
        #[serde(default)]
        card: bool,
    },
    EvictCoverCaches,
    DownloadOpdsBook {
        source: library_model::OpdsSourceId,
        entry: library_model::CardKey,
    },
    OpdsDescription {
        entry: library_model::CardKey,
    },
    BuyOpdsBook {
        entry: library_model::CardKey,
        library_id: LibraryId,
    },
    StoresAvailable,
    Stores,
    AddStore {
        url: String,
        name: Option<String>,
    },
    RemoveStore {
        id: library_model::StoreId,
    },
    OpenStore {
        id: library_model::StoreId,
        library_id: LibraryId,
    },
    OpenBrowser {
        library_id: LibraryId,
    },
    DeleteAccount {
        email: String,
        password: String,
    },
    CopyLibraryStorage {
        library_id: LibraryId,
        bokheim: bool,
    },
    DefaultAnnotationExportFolder,
    SetDefaultAnnotationExportFolder {
        path: String,
    },
}

impl AppCommand {
    /// Settings writes run in receive order so two patches issued together
    /// cannot land in the opposite order. Everything else runs concurrently.
    pub(crate) fn is_settings_update(&self) -> bool {
        matches!(
            self,
            Self::DefaultAnnotationExportFolder
                | Self::SetDefaultAnnotationExportFolder { .. }
                | Self::DefaultLibrarySaveLocation
                | Self::SetDefaultLibrarySaveLocation { .. }
                | Self::UpdateBrowsingPreference { .. }
                | Self::UpdateReaderPreferences { .. }
        )
    }
}

/// The body of every ordered command. Native dispatch runs it directly in
/// receive order; the wire dispatcher queues the same call for storage.
pub(crate) fn execute_ordered(backend: &AppBackend, command: AppCommand) -> Result<Reply, BackendError> {
    match command {
        AppCommand::DefaultAnnotationExportFolder => reply(backend.default_annotation_export_folder()?),
        AppCommand::SetDefaultAnnotationExportFolder { path } => reply(backend.set_default_annotation_export_folder(path)?),
        AppCommand::DefaultLibrarySaveLocation => reply(backend.default_library_save_location().map_err(message)?),
        AppCommand::SetDefaultLibrarySaveLocation { path } => reply(backend.set_default_library_save_location(path).map_err(message)?),
        AppCommand::UpdateBrowsingPreference { patch } => reply(backend.update_browsing_preference(patch).map_err(message)?),
        AppCommand::UpdateReaderPreferences { patches } => {
            if !patches.is_empty() {
                backend.update_reader_preferences(patches).map_err(message)?;
            }
            reply(())
        }
        command => Err(BackendError::message(format!("{command:?} does not support ordered execution"))),
    }
}

type RequestResult = Result<WorkerDispatchResult<Reply>, BackendError>;
type RequestFuture = futures_util::future::LocalBoxFuture<'static, RequestResult>;

/// Select a leaf before creating its future, keeping unrelated command state
/// out of each handler's polling and cancellation machinery.
pub(super) fn dispatch_request(backend: AppBackend, command: AppCommand) -> RequestFuture {
    if command.is_settings_update() {
        return Box::pin(async move { storage_command(move || execute_ordered(&backend, command)).await.map(WorkerDispatchResult::Response) });
    }
    match command {
        AppCommand::DictionaryDownloads { languages } => Box::pin(dictionary_downloads(backend, languages)),
        AppCommand::DownloadDictionary { id } => Box::pin(download_dictionary(backend, id)),
        AppCommand::SubscribeDictionaryChanges => Box::pin(subscribe_dictionary_changes(backend)),
        AppCommand::Dictionaries => Box::pin(dictionaries(backend)),
        AppCommand::ImportDictionary { files } => Box::pin(import_dictionary(backend, files)),
        AppCommand::ConfigureDictionary { id, enabled, direction } => Box::pin(configure_dictionary(backend, id, enabled, direction)),
        AppCommand::RemoveDictionary { id } => Box::pin(remove_dictionary(backend, id)),
        AppCommand::LookupDictionary { query, languages } => Box::pin(lookup_dictionary(backend, query, languages)),
        AppCommand::LookupWikipedia { query, languages } => Box::pin(lookup_wikipedia(backend, query, languages)),
        AppCommand::TranslateSelection { text, source, target } => Box::pin(async move {
            Ok(WorkerDispatchResult::Response(reply(backend.translate_selection(&text, &source, &target).await)?))
        }),
        AppCommand::Libraries => Box::pin(libraries(backend)),
        AppCommand::CopyLibraryStorage { library_id, bokheim } => Box::pin(copy_library_storage(backend, library_id, bokheim)),
        AppCommand::EstimateLibraryFolders { locators } => Box::pin(estimate_library_folders(locators)),
        AppCommand::CreateLibraryWithAssetStorage { name, asset_storage_enabled } => Box::pin(create_library_with_asset_storage(backend, name, asset_storage_enabled)),
        AppCommand::CreateLibrariesWithAssetStorage { locators, asset_storage_enabled } => Box::pin(create_libraries_with_asset_storage(backend, locators, asset_storage_enabled)),
        AppCommand::SetLibraryAssetStorage { library_id, enabled } => Box::pin(set_library_asset_storage(backend, library_id, enabled)),
        AppCommand::EnsureImportLibrary => Box::pin(ensure_import_library(backend)),
        AppCommand::RenameLibrary { library_id, name } => Box::pin(rename_library(backend, library_id, name)),
        AppCommand::DeleteLibrary { library_id } => Box::pin(delete_library(backend, library_id)),
        AppCommand::RequestPublicRegistration { email, password } => Box::pin(request_public_registration(email, password)),
        AppCommand::ResendVerification { email } => Box::pin(resend_verification(email)),
        AppCommand::RequestPasswordReset { email } => Box::pin(request_password_reset(email)),
        AppCommand::ResetPassword { token, password } => Box::pin(reset_password(token, password)),
        AppCommand::AccountSnapshot => Box::pin(account_snapshot(backend)),
        AppCommand::LocalStorageUsage => Box::pin(local_storage_usage(backend)),
        AppCommand::TextSearchIndexBytes => Box::pin(async move { Ok(WorkerDispatchResult::Response(reply(backend.text_search_index_bytes().await?)?)) }),
        AppCommand::Login { email, password } => Box::pin(login(backend, email, password)),
        AppCommand::VerifyEmail { email, pin, password } => Box::pin(verify_email(backend, email, pin, password)),
        AppCommand::Logout => Box::pin(logout(backend)),
        AppCommand::DeleteAccount { email, password } => Box::pin(delete_account(backend, email, password)),
        AppCommand::SubscribeNotifications { browsing } => Box::pin(subscribe_notifications(backend, browsing)),
        AppCommand::SubscribeLibraryList => Box::pin(subscribe_library_list(backend)),
        #[cfg(not(target_arch = "wasm32"))]
        AppCommand::OpdsFeeds => Box::pin(opds_feeds(backend)),
        #[cfg(not(target_arch = "wasm32"))]
        AppCommand::AddOpdsFeed { url, name, credentials } => Box::pin(add_opds_feed(backend, url, name, credentials)),
        #[cfg(not(target_arch = "wasm32"))]
        AppCommand::RemoveOpdsFeed { id } => Box::pin(remove_opds_feed(backend, id)),
        #[cfg(not(target_arch = "wasm32"))]
        AppCommand::SetOpdsCredentials { id, credentials } => Box::pin(set_opds_credentials(backend, id, credentials)),
        #[cfg(not(target_arch = "wasm32"))]
        AppCommand::BrowseOpds { query } => Box::pin(browse_opds(backend, query)),
        #[cfg(not(target_arch = "wasm32"))]
        AppCommand::OpdsCover { source, entry, width, card } => Box::pin(opds_cover(backend, source, entry, width, card)),
        AppCommand::EvictCoverCaches => Box::pin(evict_cover_caches()),
        #[cfg(not(target_arch = "wasm32"))]
        AppCommand::DownloadOpdsBook { source, entry } => Box::pin(download_opds_book(backend, source, entry)),
        #[cfg(not(target_arch = "wasm32"))]
        AppCommand::OpdsDescription { entry } => Box::pin(opds_description(entry)),
        #[cfg(not(target_arch = "wasm32"))]
        AppCommand::BuyOpdsBook { entry, library_id } => Box::pin(buy_opds_book(backend, entry, library_id)),
        // Asks the helper once, so it waits on a process start.
        #[cfg(not(target_arch = "wasm32"))]
        AppCommand::StoresAvailable => Box::pin(stores_available()),
        #[cfg(not(target_arch = "wasm32"))]
        AppCommand::Stores => Box::pin(stores(backend)),
        #[cfg(not(target_arch = "wasm32"))]
        AppCommand::AddStore { url, name } => Box::pin(add_store(backend, url, name)),
        #[cfg(not(target_arch = "wasm32"))]
        AppCommand::RemoveStore { id } => Box::pin(remove_store(backend, id)),
        #[cfg(not(target_arch = "wasm32"))]
        AppCommand::OpenStore { id, library_id } => Box::pin(open_store(backend, id, library_id)),
        #[cfg(not(target_arch = "wasm32"))]
        AppCommand::OpenBrowser { library_id } => Box::pin(open_browser(backend, library_id)),
        // The browser lists no OPDS feeds: most servers refuse a browser's
        // requests. Nor stores, which open in the desktop's webview.
        #[cfg(target_arch = "wasm32")]
        AppCommand::OpdsFeeds => Box::pin(opds_feeds()),
        #[cfg(target_arch = "wasm32")]
        AppCommand::StoresAvailable => Box::pin(stores_available()),
        #[cfg(target_arch = "wasm32")]
        AppCommand::Stores => Box::pin(stores()),
        #[cfg(target_arch = "wasm32")]
        AppCommand::AddOpdsFeed { .. }
        | AppCommand::RemoveOpdsFeed { .. }
        | AppCommand::SetOpdsCredentials { .. }
        | AppCommand::BrowseOpds { .. }
        | AppCommand::OpdsCover { .. }
        | AppCommand::DownloadOpdsBook { .. }
        | AppCommand::OpdsDescription { .. } => Box::pin(add_opds_feed()),
        #[cfg(target_arch = "wasm32")]
        AppCommand::BuyOpdsBook { .. } | AppCommand::AddStore { .. } | AppCommand::RemoveStore { .. } | AppCommand::OpenStore { .. } | AppCommand::OpenBrowser { .. } => Box::pin(buy_opds_book()),
        // Routed above; listed so a new settings command cannot be missed.
        AppCommand::DefaultAnnotationExportFolder
        | AppCommand::SetDefaultAnnotationExportFolder { .. }
        | AppCommand::DefaultLibrarySaveLocation
        | AppCommand::SetDefaultLibrarySaveLocation { .. }
        | AppCommand::UpdateBrowsingPreference { .. }
        | AppCommand::UpdateReaderPreferences { .. } => {
            unreachable!("settings commands run through execute_ordered")
        }
    }
}

async fn dictionary_downloads(backend: AppBackend, languages: Vec<String>) -> RequestResult {
    let response = { reply(backend.dictionary_downloads(languages).await?)? };
    Ok(WorkerDispatchResult::Response(response))
}

async fn download_dictionary(backend: AppBackend, id: String) -> RequestResult {
    let response = { reply(backend.download_dictionary(id).await?)? };
    Ok(WorkerDispatchResult::Response(response))
}

async fn subscribe_dictionary_changes(backend: AppBackend) -> RequestResult {
    return Ok(WorkerDispatchResult::Subscription { initial: reply(())?, events: WorkerSubscription::Dictionary(backend.dictionary_changes()) });
}

async fn dictionaries(backend: AppBackend) -> RequestResult {
    let response = { reply(backend.dictionaries().await?)? };
    Ok(WorkerDispatchResult::Response(response))
}

async fn import_dictionary(backend: AppBackend, files: Vec<dictionary::DictionaryFile>) -> RequestResult {
    let response = { reply(backend.import_dictionary(files).await?)? };
    Ok(WorkerDispatchResult::Response(response))
}

async fn configure_dictionary(backend: AppBackend, id: String, enabled: bool, direction: i8) -> RequestResult {
    let response = { reply(backend.configure_dictionary(id, enabled, direction).await?)? };
    Ok(WorkerDispatchResult::Response(response))
}

async fn remove_dictionary(backend: AppBackend, id: String) -> RequestResult {
    let response = { reply(backend.remove_dictionary(id).await?)? };
    Ok(WorkerDispatchResult::Response(response))
}

async fn lookup_dictionary(backend: AppBackend, query: String, languages: Vec<String>) -> RequestResult {
    let response = { reply(backend.lookup_dictionary(query, languages).await?)? };
    Ok(WorkerDispatchResult::Response(response))
}

async fn lookup_wikipedia(backend: AppBackend, query: String, languages: Vec<String>) -> RequestResult {
    let response = { reply(backend.lookup_wikipedia(&query, &languages).await)? };
    Ok(WorkerDispatchResult::Response(response))
}

async fn libraries(backend: AppBackend) -> RequestResult {
    let response = { storage_command(move || reply(backend.libraries().map_err(message)?)).await? };
    Ok(WorkerDispatchResult::Response(response))
}

async fn copy_library_storage(backend: AppBackend, library_id: LibraryId, bokheim: bool) -> RequestResult {
    let response = { reply(backend.copy_library_storage(library_id, bokheim).await?)? };
    Ok(WorkerDispatchResult::Response(response))
}

async fn estimate_library_folders(locators: Vec<String>) -> RequestResult {
    let response = {
        crate::executor::run_blocking(move || {
            let cancelled = std::sync::atomic::AtomicBool::new(false);
            reply(estimate_folders(locators, &cancelled).map_err(message)?)
        })
        .await
        .map_err(message)??
    };
    Ok(WorkerDispatchResult::Response(response))
}

async fn create_library_with_asset_storage(backend: AppBackend, name: String, asset_storage_enabled: bool) -> RequestResult {
    let response = { storage_command(move || reply(backend.create_library_with_asset_storage(name, asset_storage_enabled).map_err(message)?)).await? };
    Ok(WorkerDispatchResult::Response(response))
}

async fn create_libraries_with_asset_storage(backend: AppBackend, locators: Vec<String>, asset_storage_enabled: bool) -> RequestResult {
    let response = { storage_command(move || reply(backend.create_libraries_from_paths_with_asset_storage(locators.into_iter().map(PathBuf::from).collect(), asset_storage_enabled).map_err(message)?)).await? };
    Ok(WorkerDispatchResult::Response(response))
}

async fn set_library_asset_storage(backend: AppBackend, library_id: LibraryId, enabled: bool) -> RequestResult {
    let response = { reply(backend.set_library_asset_storage(library_id, enabled).await.map_err(message)?)? };
    Ok(WorkerDispatchResult::Response(response))
}

async fn ensure_import_library(backend: AppBackend) -> RequestResult {
    let response = { storage_command(move || reply(backend.ensure_import_library().map_err(message)?)).await? };
    Ok(WorkerDispatchResult::Response(response))
}

async fn rename_library(backend: AppBackend, library_id: LibraryId, name: String) -> RequestResult {
    let response = { storage_command(move || reply(backend.rename_library(&library_id, &name).map_err(message)?)).await? };
    Ok(WorkerDispatchResult::Response(response))
}

async fn delete_library(backend: AppBackend, library_id: LibraryId) -> RequestResult {
    let response = { reply(backend.delete_library(&library_id).await.map_err(message)?)? };
    Ok(WorkerDispatchResult::Response(response))
}

async fn request_public_registration(email: String, password: String) -> RequestResult {
    let response = { reply(account_client::request_public_registration(&crate::api_server_url(), &email, &password).await.map_err(message)?)? };
    Ok(WorkerDispatchResult::Response(response))
}

async fn resend_verification(email: String) -> RequestResult {
    let response = { reply(account_client::resend_verification(&crate::api_server_url(), &email).await.map_err(message)?)? };
    Ok(WorkerDispatchResult::Response(response))
}

async fn request_password_reset(email: String) -> RequestResult {
    let response = { reply(account_client::request_password_reset(&crate::api_server_url(), &email).await.map_err(message)?)? };
    Ok(WorkerDispatchResult::Response(response))
}

async fn reset_password(token: String, password: String) -> RequestResult {
    let response = { reply(account_client::reset_password(&crate::api_server_url(), &token, &password).await.map_err(message)?)? };
    Ok(WorkerDispatchResult::Response(response))
}

async fn account_snapshot(backend: AppBackend) -> RequestResult {
    let response = { reply(backend.account_snapshot().await.map_err(message)?)? };
    Ok(WorkerDispatchResult::Response(response))
}

async fn local_storage_usage(backend: AppBackend) -> RequestResult {
    let response = { reply(backend.local_storage_usage().await.map_err(message)?)? };
    Ok(WorkerDispatchResult::Response(response))
}

async fn login(backend: AppBackend, email: String, password: String) -> RequestResult {
    let response = {
        backend.login(&email, &password).await.map_err(message)?;
        reply(backend.account_snapshot().await.map_err(message)?)?
    };
    Ok(WorkerDispatchResult::Response(response))
}

async fn verify_email(backend: AppBackend, email: String, pin: String, password: String) -> RequestResult {
    let response = {
        backend.verify_email(&email, &pin, &password).await.map_err(message)?;
        reply(backend.account_snapshot().await.map_err(message)?)?
    };
    Ok(WorkerDispatchResult::Response(response))
}

async fn logout(backend: AppBackend) -> RequestResult {
    let response = {
        backend.logout().map_err(message)?;
        reply(backend.account_snapshot().await.map_err(message)?)?
    };
    Ok(WorkerDispatchResult::Response(response))
}

async fn delete_account(backend: AppBackend, email: String, password: String) -> RequestResult {
    let response = {
        backend.delete_account(&email, &password).await.map_err(message)?;
        reply(backend.account_snapshot().await.map_err(message)?)?
    };
    Ok(WorkerDispatchResult::Response(response))
}

async fn subscribe_notifications(backend: AppBackend, browsing: bool) -> RequestResult {
    return Ok(WorkerDispatchResult::Subscription { initial: reply(())?, events: WorkerSubscription::Unit(backend.notification_interest(browsing)) });
}

async fn subscribe_library_list(backend: AppBackend) -> RequestResult {
    return Ok(WorkerDispatchResult::Subscription { initial: reply(())?, events: WorkerSubscription::LibraryList(backend.library_list_changes()) });
}

#[cfg(not(target_arch = "wasm32"))]
async fn opds_feeds(backend: AppBackend) -> RequestResult {
    let response = { reply(backend.opds_feeds().await.map_err(message)?)? };
    Ok(WorkerDispatchResult::Response(response))
}

#[cfg(not(target_arch = "wasm32"))]
async fn add_opds_feed(backend: AppBackend, url: String, name: Option<String>, credentials: Option<library_model::OpdsCredentials>) -> RequestResult {
    let response = { reply(backend.add_opds_feed(url, name, credentials).await.map_err(message)?)? };
    Ok(WorkerDispatchResult::Response(response))
}

#[cfg(not(target_arch = "wasm32"))]
async fn remove_opds_feed(backend: AppBackend, id: library_model::OpdsSourceId) -> RequestResult {
    let response = { reply(backend.remove_opds_feed(id).await.map_err(message)?)? };
    Ok(WorkerDispatchResult::Response(response))
}

#[cfg(not(target_arch = "wasm32"))]
async fn set_opds_credentials(backend: AppBackend, id: library_model::OpdsSourceId, credentials: library_model::OpdsCredentials) -> RequestResult {
    let response = { reply(backend.set_opds_credentials(id, credentials).map_err(message)?)? };
    Ok(WorkerDispatchResult::Response(response))
}

#[cfg(not(target_arch = "wasm32"))]
async fn browse_opds(backend: AppBackend, query: library_model::LibraryBrowseQuery) -> RequestResult {
    let response = { reply(backend.browse_opds(query).await.map_err(message)?)? };
    Ok(WorkerDispatchResult::Response(response))
}

#[cfg(not(target_arch = "wasm32"))]
async fn opds_cover(backend: AppBackend, source: library_model::OpdsSourceId, entry: library_model::CardKey, width: u32, card: bool) -> RequestResult {
    let response = { reply(backend.opds_cover(source, entry, width, card).await.map_err(message)?.map(serde_bytes::ByteBuf::from))? };
    Ok(WorkerDispatchResult::Response(response))
}

async fn evict_cover_caches() -> RequestResult {
    let response = {
        library_backend::evict_catalog_cover_caches();
        #[cfg(not(target_arch = "wasm32"))]
        library_opds::evict_cover_cache();
        reply(())?
    };
    Ok(WorkerDispatchResult::Response(response))
}

#[cfg(not(target_arch = "wasm32"))]
async fn download_opds_book(backend: AppBackend, source: library_model::OpdsSourceId, entry: library_model::CardKey) -> RequestResult {
    let response = {
        let (file_name, bytes) = backend.download_opds_book(source, entry).await.map_err(message)?;
        reply((file_name, serde_bytes::ByteBuf::from(bytes)))?
    };
    Ok(WorkerDispatchResult::Response(response))
}

#[cfg(not(target_arch = "wasm32"))]
async fn opds_description(entry: library_model::CardKey) -> RequestResult {
    let response = { reply(library_opds::description(entry)?)? };
    Ok(WorkerDispatchResult::Response(response))
}

#[cfg(not(target_arch = "wasm32"))]
async fn buy_opds_book(backend: AppBackend, entry: library_model::CardKey, library_id: LibraryId) -> RequestResult {
    let response = { reply(backend.buy_opds_book(entry, library_id).await.map_err(message)?)? };
    Ok(WorkerDispatchResult::Response(response))
}

#[cfg(not(target_arch = "wasm32"))]
async fn stores_available() -> RequestResult {
    let response = { reply(client_platform_runtime::executor::run_blocking(library_opds::browser::stores_available).await.map_err(message)?)? };
    Ok(WorkerDispatchResult::Response(response))
}

#[cfg(not(target_arch = "wasm32"))]
async fn stores(backend: AppBackend) -> RequestResult {
    let response = { reply(backend.stores().map_err(message)?)? };
    Ok(WorkerDispatchResult::Response(response))
}

#[cfg(not(target_arch = "wasm32"))]
async fn add_store(backend: AppBackend, url: String, name: Option<String>) -> RequestResult {
    let response = { reply(backend.add_store(url, name).map_err(message)?)? };
    Ok(WorkerDispatchResult::Response(response))
}

#[cfg(not(target_arch = "wasm32"))]
async fn remove_store(backend: AppBackend, id: library_model::StoreId) -> RequestResult {
    let response = { reply(backend.remove_store(id).map_err(message)?)? };
    Ok(WorkerDispatchResult::Response(response))
}

#[cfg(not(target_arch = "wasm32"))]
async fn open_store(backend: AppBackend, id: library_model::StoreId, library_id: LibraryId) -> RequestResult {
    let response = { reply(backend.open_store(id, library_id).await.map_err(message)?)? };
    Ok(WorkerDispatchResult::Response(response))
}

#[cfg(not(target_arch = "wasm32"))]
async fn open_browser(backend: AppBackend, library_id: LibraryId) -> RequestResult {
    let response = { reply(backend.open_browser(library_id).await.map_err(message)?)? };
    Ok(WorkerDispatchResult::Response(response))
}

#[cfg(target_arch = "wasm32")]
async fn opds_feeds() -> RequestResult {
    let response = { reply(Vec::<library_model::OpdsSource>::new())? };
    Ok(WorkerDispatchResult::Response(response))
}

#[cfg(target_arch = "wasm32")]
async fn stores_available() -> RequestResult {
    let response = { reply(false)? };
    Ok(WorkerDispatchResult::Response(response))
}

#[cfg(target_arch = "wasm32")]
async fn stores() -> RequestResult {
    let response = { reply(Vec::<library_model::Store>::new())? };
    Ok(WorkerDispatchResult::Response(response))
}

#[cfg(target_arch = "wasm32")]
async fn add_opds_feed() -> RequestResult {
    return Err(BackendError::message("OPDS feeds are not available in the browser"));
}

#[cfg(target_arch = "wasm32")]
async fn buy_opds_book() -> RequestResult {
    return Err(BackendError::message("stores are not available in the browser"));
}
