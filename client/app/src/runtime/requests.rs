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
    Libraries,
    DefaultLibrarySaveLocation,
    SetDefaultLibrarySaveLocation { path: String },
    UpdateBrowsingPreference { patch: app_preferences::BrowsingPreferencePatch },
    UpdateReaderPreferences { patches: Vec<app_preferences::ReaderPreferencePatch> },
    EstimateLibraryFolders { locators: Vec<String> },
    CreateLibraryWithAssetStorage { name: String, asset_storage_enabled: bool },
    CreateLibrariesWithAssetStorage { locators: Vec<String>, asset_storage_enabled: bool },
    SetLibraryAssetStorage { library_id: LibraryId, enabled: bool },
    EnsureImportLibrary,
    RenameLibrary { library_id: LibraryId, name: String },
    DeleteLibrary { library_id: LibraryId },
    RequestPublicRegistration { email: String, password: String },
    ResendVerification { email: String },
    RequestPasswordReset { email: String },
    ResetPassword { token: String, password: String },
    AccountSnapshot,
    LocalStorageUsage,
    Login { email: String, password: String },
    VerifyEmail { email: String, pin: String },
    Logout,
    SubscribeNotifications { browsing: bool },
    SubscribeLibraryList,
    OpdsFeeds,
    AddOpdsFeed { url: String, name: Option<String>, credentials: Option<library_model::OpdsCredentials> },
    RemoveOpdsFeed { id: library_model::OpdsSourceId },
    SetOpdsCredentials { id: library_model::OpdsSourceId, credentials: library_model::OpdsCredentials },
    BrowseOpds { query: library_model::LibraryBrowseQuery },
    OpdsCover { source: library_model::OpdsSourceId, entry: library_model::CardKey, width: u32 },
    DownloadOpdsBook { source: library_model::OpdsSourceId, entry: library_model::CardKey },
    OpdsDescription { entry: library_model::CardKey },
}

impl AppCommand {
    /// Settings writes run in receive order so two patches issued together
    /// cannot land in the opposite order. Everything else runs concurrently.
    pub(crate) fn is_settings_update(&self) -> bool {
        matches!(self, Self::DefaultLibrarySaveLocation | Self::SetDefaultLibrarySaveLocation { .. } | Self::UpdateBrowsingPreference { .. } | Self::UpdateReaderPreferences { .. })
    }
}

/// The body of every ordered command. Native dispatch runs it directly in
/// receive order; the wire dispatcher queues the same call for storage.
pub(crate) fn execute_ordered(backend: &AppBackend, command: AppCommand) -> Result<Reply, BackendError> {
    match command {
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

pub(super) async fn dispatch_request(backend: AppBackend, command: AppCommand) -> Result<WorkerDispatchResult<Reply>, BackendError> {
    if command.is_settings_update() {
        return storage_command(move || execute_ordered(&backend, command)).await.map(WorkerDispatchResult::Response);
    }
    let reply = match command {
        AppCommand::Libraries => storage_command(move || reply(backend.libraries().map_err(message)?)).await?,

        AppCommand::EstimateLibraryFolders { locators } => crate::executor::run_blocking(move || {
            let cancelled = std::sync::atomic::AtomicBool::new(false);
            reply(estimate_folders(locators, &cancelled).map_err(message)?)
        })
        .await
        .map_err(message)??,

        AppCommand::CreateLibraryWithAssetStorage { name, asset_storage_enabled } => storage_command(move || reply(backend.create_library_with_asset_storage(name, asset_storage_enabled).map_err(message)?)).await?,
        AppCommand::CreateLibrariesWithAssetStorage { locators, asset_storage_enabled } => {
            storage_command(move || reply(backend.create_libraries_from_paths_with_asset_storage(locators.into_iter().map(PathBuf::from).collect(), asset_storage_enabled).map_err(message)?)).await?
        }
        AppCommand::SetLibraryAssetStorage { library_id, enabled } => storage_command(move || reply(backend.set_library_asset_storage(library_id, enabled).map_err(message)?)).await?,
        AppCommand::EnsureImportLibrary => storage_command(move || reply(backend.ensure_import_library().map_err(message)?)).await?,
        AppCommand::RenameLibrary { library_id, name } => storage_command(move || reply(backend.rename_library(&library_id, &name).map_err(message)?)).await?,
        AppCommand::DeleteLibrary { library_id } => reply(backend.delete_library(&library_id).await.map_err(message)?)?,

        AppCommand::RequestPublicRegistration { email, password } => reply(account_client::request_public_registration(&crate::api_server_url(), &email, &password).await.map_err(message)?)?,
        AppCommand::ResendVerification { email } => reply(account_client::resend_verification(&crate::api_server_url(), &email).await.map_err(message)?)?,
        AppCommand::RequestPasswordReset { email } => reply(account_client::request_password_reset(&crate::api_server_url(), &email).await.map_err(message)?)?,
        AppCommand::ResetPassword { token, password } => reply(account_client::reset_password(&crate::api_server_url(), &token, &password).await.map_err(message)?)?,

        AppCommand::AccountSnapshot => reply(backend.account_snapshot().await.map_err(message)?)?,
        AppCommand::LocalStorageUsage => reply(backend.local_storage_usage().await.map_err(message)?)?,
        AppCommand::Login { email, password } => {
            backend.login(&email, &password).await.map_err(message)?;
            reply(backend.account_snapshot().await.map_err(message)?)?
        }
        AppCommand::VerifyEmail { email, pin } => {
            backend.verify_email(&email, &pin).await.map_err(message)?;
            reply(backend.account_snapshot().await.map_err(message)?)?
        }
        AppCommand::Logout => {
            backend.logout().map_err(message)?;
            reply(backend.account_snapshot().await.map_err(message)?)?
        }

        AppCommand::SubscribeNotifications { browsing } => {
            return Ok(WorkerDispatchResult::Subscription { initial: reply(())?, events: WorkerSubscription::Unit(backend.notification_interest(browsing)) });
        }
        AppCommand::SubscribeLibraryList => {
            return Ok(WorkerDispatchResult::Subscription { initial: reply(())?, events: WorkerSubscription::LibraryList(backend.library_list_changes()) });
        }

        #[cfg(not(target_arch = "wasm32"))]
        AppCommand::OpdsFeeds => reply(backend.opds_feeds().await.map_err(message)?)?,
        #[cfg(not(target_arch = "wasm32"))]
        AppCommand::AddOpdsFeed { url, name, credentials } => reply(backend.add_opds_feed(url, name, credentials).await.map_err(message)?)?,
        #[cfg(not(target_arch = "wasm32"))]
        AppCommand::RemoveOpdsFeed { id } => reply(backend.remove_opds_feed(id).await.map_err(message)?)?,
        #[cfg(not(target_arch = "wasm32"))]
        AppCommand::SetOpdsCredentials { id, credentials } => reply(backend.set_opds_credentials(id, credentials).map_err(message)?)?,
        #[cfg(not(target_arch = "wasm32"))]
        AppCommand::BrowseOpds { query } => reply(backend.browse_opds(query).await.map_err(message)?)?,
        #[cfg(not(target_arch = "wasm32"))]
        AppCommand::OpdsCover { source, entry, width } => reply(backend.opds_cover(source, entry, width).await.map_err(message)?.map(serde_bytes::ByteBuf::from))?,
        #[cfg(not(target_arch = "wasm32"))]
        AppCommand::DownloadOpdsBook { source, entry } => {
            let (file_name, bytes) = backend.download_opds_book(source, entry).await.map_err(message)?;
            reply((file_name, serde_bytes::ByteBuf::from(bytes)))?
        }
        #[cfg(not(target_arch = "wasm32"))]
        AppCommand::OpdsDescription { entry } => reply(library_opds::description(entry).map_err(BackendError::message)?)?,
        // The browser lists no OPDS feeds: most servers refuse a browser's requests.
        #[cfg(target_arch = "wasm32")]
        AppCommand::OpdsFeeds => reply(Vec::<library_model::OpdsSource>::new())?,
        #[cfg(target_arch = "wasm32")]
        AppCommand::AddOpdsFeed { .. } | AppCommand::RemoveOpdsFeed { .. } | AppCommand::SetOpdsCredentials { .. } | AppCommand::BrowseOpds { .. } | AppCommand::OpdsCover { .. } | AppCommand::DownloadOpdsBook { .. } | AppCommand::OpdsDescription { .. } => {
            return Err(BackendError::message("OPDS feeds are not available in the browser"));
        }

        // Routed above; listed so a new settings command cannot be missed.
        AppCommand::DefaultLibrarySaveLocation | AppCommand::SetDefaultLibrarySaveLocation { .. } | AppCommand::UpdateBrowsingPreference { .. } | AppCommand::UpdateReaderPreferences { .. } => {
            unreachable!("settings commands run through execute_ordered")
        }
    };
    Ok(WorkerDispatchResult::Response(reply))
}
