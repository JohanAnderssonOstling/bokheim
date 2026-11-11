//! The public-domain catalog: the snapshot the signed update manifest names,
//! stored in the app's data directory and queried through its SQLite browse projection.
//! Unlike application and taxonomy updates it needs no approval or restart: a
//! newer snapshot replaces source and browse rows together, then notifies its host.
use super::{configure_cover_cache, COVER_CACHE_MAX_BYTES};
#[cfg(not(target_arch = "wasm32"))]
use client_platform_native::reserve_database_capacity;
use client_platform_runtime::executor::{run_blocking, BackendExecutor, BackendSend};
#[cfg(target_arch = "wasm32")]
use client_platform_web::reserve_database_capacity;
use std::path::{Path, PathBuf};
use update_client::DiscoveryConfig;
use client_protocol::BackendError;

/// Snapshot formats this build reads.
const FORMATS: [u32; 1] = [library_database::catalog::FORMAT_VERSION];

/// Activate cached data and fetch updates in the background. Rebuilding a
/// stored catalog for a new taxonomy must not delay opening the application.
/// Start once per application; `changed` runs whenever a snapshot is ready for
/// library views to refresh. `BOKHEIM_CATALOG_FILE` selects a local snapshot
/// for native development.
pub fn start(app_data_dir: &Path, executor: &BackendExecutor, changed: impl Fn() + BackendSend + 'static) {
    #[cfg(not(target_arch = "wasm32"))]
    let local = std::env::var_os("BOKHEIM_CATALOG_FILE").map(PathBuf::from);
    #[cfg(target_arch = "wasm32")]
    let local: Option<PathBuf> = None;
    let path = local.clone().unwrap_or_else(|| app_data_dir.join("catalog.sqlite3"));
    let cover_cache = app_data_dir.join("catalog-covers-v2");
    executor.spawn_detached(async move {
        if let Err(error) = configure_cover_cache(&cover_cache, COVER_CACHE_MAX_BYTES).await {
            log::warn!("Catalog cover disk cache is unavailable: {error}");
        }
        let stored_path = path.clone();
        let installed = run_blocking(move || activate_stored(&stored_path)).await.map_err(BackendError::operation).and_then(|result| result).unwrap_or_else(|error| {
            log::warn!("Stored catalog is unreadable; fetching it again: {error}");
            0
        });
        if installed != 0 {
            changed();
        }
        if local.is_some() {
            return;
        }
        match refresh(path, installed).await {
            Ok(true) => changed(),
            Ok(false) => {}
            Err(error) => log::warn!("Catalog update will retry on next start: {error}"),
        }
    });
}

/// Opens the stored snapshot for catalog queries and returns its release,
/// or 0 when none is stored.
fn activate_stored(path: &Path) -> Result<u64, BackendError> {
    Ok(library_database::catalog::activate_path(path).map_err(BackendError::operation)?.unwrap_or(0))
}

/// Stores a published snapshot newer than `installed`. A first one is also
/// activated at once, for libraries opened from then on.
async fn refresh(path: PathBuf, installed: u64) -> Result<bool, BackendError> {
    let config = DiscoveryConfig::bundled()?;
    let now = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_err(BackendError::operation)?.as_secs();
    let manifest = update_client::fetch::discover(&config, now, 0).await?;
    let Some(release) = manifest.manifest().catalog.as_ref().filter(|release| release.release_id > installed && FORMATS.contains(&release.format_version)) else {
        return Ok(false);
    };
    let image = update_client::fetch::download(&release.artifact).await?;
    reserve_database_capacity(1).await?;
    let expected = (release.release_id, release.format_version);
    run_blocking(move || {
        library_database::catalog::store(&image, &path, expected).map_err(BackendError::operation)?;
        if installed == 0 {
            library_database::catalog::activate_path(&path).map_err(BackendError::operation)?;
        }
        Ok(true)
    })
    .await
    .map_err(BackendError::operation)?
}
