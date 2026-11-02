//! Resource construction for one concrete library session.
//!
//! The application registry decides *which* location and account policy to
//! use.  Everything opened from that location belongs to the library session.

use crate::BackendError;
#[cfg(not(target_arch = "wasm32"))]
use crate::asset_store::AssetStore;
use crate::library::discovery::LibraryScanner;
use crate::notification_interest::NotificationInterest;
use client_platform_runtime::startup_profile::StartupProfile;
use cpu_host::CpuHost;
use library_database::Database;
#[cfg(not(target_arch = "wasm32"))]
use library_model::{BookDownloadStatus, DownloadState, LibraryUpdate};
use library_sync::{LibrarySync, LibrarySyncConfig};
use std::sync::Arc;
use std::{cell::Cell, rc::Rc};

use super::LibrarySession;

pub struct LibraryRuntimeSpec {
    pub config: super::open::LibraryOpenConfig,
    pub account: crate::integration::LibraryAccount,
    pub notification_interest: NotificationInterest,
    #[cfg(not(target_arch = "wasm32"))]
    pub root_failures: Option<async_channel::Sender<sync_common::LibraryId>>,
}

impl LibrarySession {
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn recover(spec: LibraryRuntimeSpec) -> Result<Self, BackendError> {
        let session = Self::open(spec)?;
        for hash in session.db.reset_local_storage()? {
            session.events.publish_update(LibraryUpdate::Download(BookDownloadStatus::new(hash, DownloadState::NotDownloaded)));
        }
        Ok(session)
    }

    pub(crate) fn open(spec: LibraryRuntimeSpec) -> Result<Self, BackendError> {
        let mut profile = StartupProfile::for_library("library_resources", spec.config.id);
        // One connection per library file, owned by the session. Sync and
        // asset workers borrow it through the owner; their futures stay
        // local to their tasks, so no sharing primitive is needed.
        let database = Arc::new(Database::open(&spec.config.database_locator)?);
        profile.mark("database_open");
        #[cfg(not(target_arch = "wasm32"))]
        {
            use content_address::hash_file;
            use library_files::import_move::recover_import_moves;
            use std::fs::File;
            use std::io;
            use std::path::Path;
            let root = Path::new(&spec.config.root).canonicalize()?;
            let unresolved = recover_import_moves(&root, |path| {
                let checksum = hash_file(&mut File::open(path)?)?;
                let Some(book) = database.book_for_local_content(checksum).map_err(io::Error::other)? else { return Ok(false) };
                Ok(database.book_paths(&book).map_err(io::Error::other)?.into_iter().any(|relative| root.join(relative.as_str().trim_start_matches('/')) == path))
            })?;
            for error in unresolved {
                log::warn!("interrupted import move needs recovery: {error}");
            }
        }
        let cpu = cpu_host::HOST
            .get_or_init(|| -> Arc<dyn CpuHost> {
                #[cfg(target_arch = "wasm32")]
                {
                    Arc::new(client_platform_web::cpu_host::WebHost::default())
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    Arc::new(client_platform_native::cpu_host::NativeHost::default())
                }
            })
            .clone();
        profile.mark("cpu_host");
        #[cfg(not(target_arch = "wasm32"))]
        let assets = AssetStore::open(&spec.config.root).map_err(BackendError::operation)?;
        #[cfg(target_arch = "wasm32")]
        let assets = crate::asset_store::open(&spec.config.root, cpu.clone()).map_err(BackendError::operation)?;
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(failures) = spec.root_failures.clone() {
            let id = spec.config.id;
            assets.on_root_unavailable(move || {
                let _ = failures.try_send(id);
            });
        }
        profile.mark("asset_store");
        // `Database::open` already configured the connection (pragmas, sync
        // functions) internally.
        let replica_id = database.initialize_library_for_deferred_taxonomy(spec.config.id)?;
        profile.mark("database_initialize");
        database.set_remote_library_name(&spec.config.display_name)?;
        profile.mark("remote_name");
        let (sync_wake, sync_requests) = library_runtime::scheduler::SyncScheduler::new();
        let (asset_wake, asset_requests) = library_runtime::scheduler::SyncScheduler::new();
        let (file_wake, file_requests) = library_runtime::scheduler::SyncScheduler::new();
        let (enrichment_wake, enrichment_requests) = library_runtime::scheduler::SyncScheduler::new();
        let (events, coordination) = library_runtime::events::LibraryEventRouter::new(sync_wake, asset_wake, file_wake, enrichment_wake);
        let scanner = LibraryScanner { root: spec.config.root };
        profile.mark("events_and_scanner");
        database.set_storage_policy(&spec.config.storage, spec.config.reset_cloud_presence)?;
        profile.mark("storage_policy");
        let sync = Arc::new(LibrarySync::new(LibrarySyncConfig {
            database: database.clone(),
            account: Some(spec.account.clone()),
            assets: assets.clone(),
            replica_id,
            // Opening runs while the storage owner waits. The configured server
            // is known already; account authorization may read that owner's
            // registry and must wait until the library is serving commands.
            server_url: spec.config.default_sync_server_url,
            library_id: spec.config.id,
            events: Some(events.clone()),
            credentials: spec.account.credentials(),
            notification_interest: spec.notification_interest,
            cpu: cpu.clone(),
        }));
        sync.set_asset_storage_enabled(spec.config.storage.uses_bokheim_assets());
        profile.mark("sync_construct");
        let client = library_enrichment::MetadataClient::new(&spec.config.metadata_server_url).map_err(BackendError::operation)?;
        profile.mark("metadata_client");
        let covers = library_enrichment::CoverClient::default();
        profile.mark("cover_client");
        let enrichment = library_enrichment::LibraryEnrichment { database: database.clone(), assets: assets.clone(), client, covers, cpu: cpu.clone(), events: events.clone() };
        profile.mark("enrichment_construct");
        let placement = Arc::new(library_placement::LibraryPlacement { database: database.clone(), assets: assets.clone(), events: events.clone() });
        let import = library_import::LibraryImport {
            library_id: spec.config.id,
            database: database.clone(),
            assets: assets.clone(),
            cpu: cpu.clone(),
            events: events.clone(),
            placement: placement.clone(),
            #[cfg(not(target_arch = "wasm32"))]
            root: scanner.root.clone(),
        };
        #[cfg(not(target_arch = "wasm32"))]
        if spec.config.opds.is_some() {
            database.configure_opds()?;
            if let Err(error) = assets.trim_thumbnail_cache(super::opds::COVER_CACHE_MAX_BYTES) {
                log::warn!("OPDS cover cache trim failed: {error}");
            }
        }
        let library = Self {
            #[cfg(not(target_arch = "wasm32"))]
            opds: spec.config.opds,
            id: spec.config.id,
            db: database,
            assets,
            enrichment,
            placement,
            import,
            sync,
            events,
            initial_coordination: Some(coordination),
            directory_imports: Cell::new(0),
            requests: Some(super::workers::WorkerRequests { sync: sync_requests, assets: asset_requests, files: file_requests, enrichment: enrichment_requests }),
            runtime: Rc::new(crate::library::session::LibraryRuntime::default()),
            account: spec.account,
            #[cfg(not(target_arch = "wasm32"))]
            root_failures: spec.root_failures,
            _scanner: Arc::new(scanner),
            cpu,
            scanning: Cell::new(false),
        };
        profile.mark("remaining_resources");
        Ok(library)
    }
}
