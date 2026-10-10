//! The browser's taxonomy. Native hosts bundle one and install newer releases
//! through update-host; the browser bundles none. It fetches the release the
//! signed update manifest names and keeps it in OPFS.
use crate::executor::spawn_detached;
use client_platform_web::web_storage;
use std::path::Path;
use subject_projection::installed;
use update_client::{Artifact, DiscoveryConfig};

const STORED: &str = "taxonomy.sqlite3";
/// Formats this build reads, as native `DeviceVersion::taxonomy_formats`.
const FORMATS: [u32; 1] = [1];

/// Activates the stored taxonomy and looks for a newer release in the
/// background, taking effect on the next start like native updates. Only a
/// first start, with nothing stored, waits for the download.
pub(crate) async fn install() -> Result<(), String> {
    let stored = if web_storage::database_exists(STORED)? { Some(installed::load_image(Path::new(STORED))?) } else { None };
    let (artifact, image) = match stored.filter(|stored| FORMATS.contains(&stored.format)) {
        Some(stored) => {
            let release = stored.release;
            spawn_detached(async move {
                if let Err(error) = download_newer(release).await {
                    log::warn!("Taxonomy update will retry on next start: {error}");
                }
            });
            (serde_json::from_str(&stored.source).map_err(|e| e.to_string())?, stored.image)
        }
        None => download_newer(0).await?.ok_or("the update manifest publishes no taxonomy")?,
    };
    installed::activate_image(image)?;
    client_platform_web::cpu_host::set_taxonomy(artifact);
    Ok(())
}

/// Downloads, prepares and stores the published taxonomy when it is newer
/// than `installed` and this build can read it.
async fn download_newer(installed: u64) -> Result<Option<(Artifact, Vec<u8>)>, String> {
    let config = DiscoveryConfig::bundled()?;
    let now = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_err(|e| e.to_string())?.as_secs();
    let manifest = update_client::fetch::discover(&config, now, 0).await?;
    // The deployed bundle is always the current application, so only the
    // format and schema decide compatibility.
    let schema = library_database::LIBRARY_SCHEMA_VERSION as u32;
    let Some(release) = manifest.manifest().taxonomy.as_ref().filter(|release| release.release_id > installed && FORMATS.contains(&release.format_version) && release.minimum_schema_version <= schema) else {
        return Ok(None);
    };
    let snapshot = update_client::fetch::download(&release.artifact).await?;
    let source = serde_json::to_string(&release.artifact).map_err(|e| e.to_string())?;
    let image = installed::prepare_image(&snapshot, (release.release_id, release.format_version), &source)?;
    if !web_storage::database_exists(STORED)? {
        crate::platform::reserve_database_capacity(1).await?;
    }
    installed::store_image(&image, Path::new(STORED))?;
    Ok(Some((release.artifact.clone(), image)))
}
