//! Device-global commands and their execution against the application backend.
//!
//! One enum carries the command on the wire and one match runs it. Settings
//! writes remaining here must observe receive order; they are
//! listed by [`AppCommand::is_settings_update`] and executed by
//! [`execute_ordered`], which both transports reach through their ordered path.

use std::path::PathBuf;

use super::WorkerDispatchResult;
use crate::app::AppBackend;
use crate::executor::{BackendOutput, BackendSend};
use crate::BackendError;
use client_platform_runtime::storage_queue::{self, Priority};
use client_runtime::reply::Reply;
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
    UpdateBrowsingPreference {
        patch: app_preferences::BrowsingPreferencePatch,
    },
    CreateLibrariesWithAssetStorage {
        locators: Vec<String>,
        asset_storage_enabled: bool,
    },
    SetLibraryAssetStorage {
        library_id: LibraryId,
        enabled: bool,
    },
    RenameLibrary {
        library_id: LibraryId,
        name: String,
    },
    #[cfg(not(target_arch = "wasm32"))]
    MoveLibrary {
        library_id: LibraryId,
        destination: String,
    },
    DeleteLibrary {
        library_id: LibraryId,
    },
    CopyLibraryStorage {
        library_id: LibraryId,
        bokheim: bool,
    },
}

impl AppCommand {
    /// Settings writes run in receive order so two patches issued together
    /// cannot land in the opposite order. Everything else runs concurrently.
    pub(crate) fn is_settings_update(&self) -> bool {
        matches!(self, Self::UpdateBrowsingPreference { .. })
    }
}

/// The body of every ordered command. Native dispatch runs it directly in
/// receive order; the preference service serializes writes and their effects.
pub(crate) async fn execute_ordered(backend: &AppBackend, command: AppCommand) -> Result<Reply, BackendError> {
    match command {
        AppCommand::UpdateBrowsingPreference { patch } => reply(backend.state.preferences.update_browsing(patch).await.map_err(message)?),
        command => Err(BackendError::message(format!("{command:?} does not support ordered execution"))),
    }
}

type RequestResult = Result<WorkerDispatchResult<Reply>, BackendError>;
type RequestFuture = futures_util::future::LocalBoxFuture<'static, RequestResult>;

/// Select a leaf before creating its future, keeping unrelated command state
/// out of each handler's polling and cancellation machinery.
pub(super) fn dispatch_request(backend: AppBackend, command: AppCommand) -> RequestFuture {
    if command.is_settings_update() {
        return Box::pin(async move { execute_ordered(&backend, command).await.map(WorkerDispatchResult::Response) });
    }
    match command {
        AppCommand::CopyLibraryStorage { library_id, bokheim } => Box::pin(copy_library_storage(backend, library_id, bokheim)),
        AppCommand::CreateLibrariesWithAssetStorage { locators, asset_storage_enabled } => Box::pin(create_libraries_with_asset_storage(backend, locators, asset_storage_enabled)),
        AppCommand::SetLibraryAssetStorage { library_id, enabled } => Box::pin(set_library_asset_storage(backend, library_id, enabled)),
        AppCommand::RenameLibrary { library_id, name } => Box::pin(rename_library(backend, library_id, name)),
        #[cfg(not(target_arch = "wasm32"))]
        AppCommand::MoveLibrary { library_id, destination } => Box::pin(async move { Ok(WorkerDispatchResult::Response(reply(backend.state.library_service.move_library(library_id, destination).await?)?)) }),
        AppCommand::DeleteLibrary { library_id } => Box::pin(delete_library(backend, library_id)),
        // Routed above; listed so a new settings command cannot be missed.
        AppCommand::UpdateBrowsingPreference { .. } => {
            unreachable!("settings commands run through execute_ordered")
        }
    }
}

async fn copy_library_storage(backend: AppBackend, library_id: LibraryId, bokheim: bool) -> RequestResult {
    let response = { reply(backend.copy_library_storage(library_id, bokheim).await?)? };
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

async fn rename_library(backend: AppBackend, library_id: LibraryId, name: String) -> RequestResult {
    let response = { storage_command(move || reply(backend.rename_library(&library_id, &name).map_err(message)?)).await? };
    Ok(WorkerDispatchResult::Response(response))
}

async fn delete_library(backend: AppBackend, library_id: LibraryId) -> RequestResult {
    let response = { reply(backend.delete_library(&library_id).await.map_err(message)?)? };
    Ok(WorkerDispatchResult::Response(response))
}
