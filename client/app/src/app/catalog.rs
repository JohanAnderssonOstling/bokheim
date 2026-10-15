//! The public-domain catalog: the snapshot the signed update manifest names,
//! stored in the app's data directory and queried through its SQLite browse projection.
//! Unlike application and taxonomy updates it needs no approval or restart: a
//! newer snapshot replaces source and browse rows together, then refreshes library views.
use crate::executor::BackendExecutor;
use library_backend::library::LibraryDirectory;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use update_client::DiscoveryConfig;

/// Snapshot formats this build reads.
const FORMATS: [u32; 1] = [library_database::catalog::FORMAT_VERSION];

/// Activates the catalog before libraries open, then looks for a newer
/// release in the background. `BOKHEIM_CATALOG_FILE` names a local snapshot to
/// use instead, for development before the catalog is hosted (native only).
pub(super) fn start(app_data_dir: &Path, executor: &BackendExecutor, libraries: Arc<LibraryDirectory>) {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(local) = std::env::var_os("BOKHEIM_CATALOG_FILE") {
        match activate_stored(Path::new(&local)) {
            Ok(0) => log::warn!("BOKHEIM_CATALOG_FILE is not a catalog snapshot this build reads"),
            Ok(_) => {}
            Err(error) => log::warn!("BOKHEIM_CATALOG_FILE could not be activated: {error}"),
        }
        return;
    }
    let path = app_data_dir.join("catalog.sqlite3");
    let installed = activate_stored(&path).unwrap_or_else(|error| {
        log::warn!("Stored catalog is unreadable; fetching it again: {error}");
        0
    });
    executor.spawn_detached(async move {
        match refresh(path, installed).await {
            Ok(true) => match libraries.all() {
                Ok(libraries) => {
                    for library in libraries {
                        library.refresh_catalog();
                    }
                }
                Err(error) => log::warn!("catalog views could not be refreshed: {error}"),
            },
            Ok(false) => {}
            Err(error) => log::warn!("Catalog update will retry on next start: {error}"),
        }
    });
}

/// Opens the stored snapshot for catalog queries and returns its release,
/// or 0 when none is stored.
fn activate_stored(path: &Path) -> Result<u64, String> {
    Ok(library_database::catalog::activate_path(path).map_err(|e| e.to_string())?.unwrap_or(0))
}

/// Stores a published snapshot newer than `installed`. A first one is also
/// activated at once, for libraries opened from then on.
async fn refresh(path: PathBuf, installed: u64) -> Result<bool, String> {
    let config = DiscoveryConfig::bundled()?;
    let now = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_err(|e| e.to_string())?.as_secs();
    let manifest = update_client::fetch::discover(&config, now, 0).await?;
    let Some(release) = manifest.manifest().catalog.as_ref().filter(|release| release.release_id > installed && FORMATS.contains(&release.format_version)) else {
        return Ok(false);
    };
    let image = update_client::fetch::download(&release.artifact).await?;
    crate::platform::reserve_database_capacity(1).await?;
    let expected = (release.release_id, release.format_version);
    crate::executor::run_blocking(move || {
        library_database::catalog::store(&image, &path, expected).map_err(|e| e.to_string())?;
        if installed == 0 {
            library_database::catalog::activate_path(&path).map_err(|e| e.to_string())?;
        }
        Ok(true)
    })
    .await
    .map_err(|error| error.to_string())?
}
