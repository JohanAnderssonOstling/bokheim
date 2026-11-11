//! Prepare the browser's taxonomy database from its immutable app bundle.
//! Published taxonomy updates are installed in the background for the next start.
use client_protocol::{BackendError, Failure, OperationError};
use super::download_bundle;
use crate::executor::spawn_detached;
use client_platform_web::web_storage;
use std::path::Path;
use subject_projection::installed::{self, StoredMetadata};
use update_client::DiscoveryConfig;

const STORED: &str = "taxonomy.sqlite3";
/// Formats this build reads, as native `DeviceVersion::taxonomy_formats`.
const FORMATS: [u32; 1] = [1];

/// Activates the stored taxonomy and looks for a newer release in the
/// background, taking effect on the next start like native updates.
pub(crate) async fn install() -> Result<(), BackendError> {
    let bundled = StoredMetadata::bundled();
    let bundled_path = format!("taxonomy-{}.sqlite3", bundled.source.replace(':', "-"));
    let selected = match web_storage::reference_database(STORED)? {
        Some(path) if web_storage::database_exists(&path)? => Some(path),
        _ => None,
    };
    let stored = if let Some(path) = selected {
        if path == bundled_path {
            Some((path, StoredMetadata::bundled()))
        } else if path.starts_with("taxonomy-bundled-") {
            // A different build's packaged reference is replaced without
            // opening it; its filename already identifies the old artifact.
            None
        } else {
            match installed::load_metadata(Path::new(&path)) {
                Ok(stored) => Some((path, stored)),
                Err(error) => {
                    log::warn!("Replacing unreadable taxonomy cache: {error}");
                    None
                }
            }
        }
    } else {
        None
    };
    let (path, stored) = match stored.filter(|(_, stored)| FORMATS.contains(&stored.format) && stored.release >= installed::BUNDLED_RELEASE_ID && (!stored.source.starts_with("bundled:") || stored.source == bundled.source)) {
        Some(stored) => stored,
        None => {
            let path = bundled_path;
            if !web_storage::database_exists(&path)? {
                let image = download_bundle(STORED).await?;
                client_platform_web::reserve_database_capacity(1).await?;
                web_storage::import_reference_database(&path, &image)?;
            }
            (path, bundled)
        }
    };
    web_storage::publish_reference_database(STORED, &path)?;
    web_storage::prune_reference_databases("taxonomy-", &path)?;
    if !stored.source.starts_with("bundled:") {
        client_platform_web::cpu_host::set_taxonomy(serde_json::from_str(&stored.source).map_err(BackendError::operation)?);
    }
    installed::activate(Path::new(&path))?;
    spawn_detached(async move {
        if let Err(error) = download_newer(stored.release).await {
            log::warn!("Taxonomy update will retry on next start: {error}");
        }
    });
    Ok(())
}

/// CPU workers use the same packaged reference on their first inspection.
pub async fn install_bundled_matching() -> Result<(), BackendError> {
    installed::install_snapshot(&download_bundle(STORED).await?).map_err(BackendError::operation)
}

/// Downloads, prepares and stores the published taxonomy when it is newer
/// than `installed` and this build can read it.
async fn download_newer(installed: u64) -> Result<(), BackendError> {
    let config = DiscoveryConfig::bundled()?;
    let now = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_err(BackendError::operation)?.as_secs();
    let manifest = update_client::fetch::discover(&config, now, 0).await?;
    // The deployed bundle is always the current application, so only the
    // format and schema decide compatibility.
    let schema = library_database::LIBRARY_SCHEMA_VERSION as u32;
    let Some(release) = manifest.manifest().taxonomy.as_ref().filter(|release| release.release_id > installed && FORMATS.contains(&release.format_version) && release.minimum_schema_version <= schema) else {
        return Ok(());
    };
    let path = format!("taxonomy-{}-{}.sqlite3", release.release_id, release.artifact.sha256);
    if !web_storage::database_exists(&path)? {
        let snapshot = update_client::fetch::download(&release.artifact).await?;
        let source = serde_json::to_string(&release.artifact).map_err(BackendError::operation)?;
        let image = installed::prepare_image(&snapshot, (release.release_id, release.format_version), &source)?;
        client_platform_web::reserve_database_capacity(1).await?;
        web_storage::import_reference_database(&path, &image)?;
    }
    web_storage::publish_reference_database(STORED, &path).map_err(BackendError::operation)
}
