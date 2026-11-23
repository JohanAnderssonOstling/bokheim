//! Browser storage adapter for the shared immutable taxonomy lifecycle.
use super::download_bundle;
use crate::executor::spawn_detached;
use client_platform_web::web_storage;
use client_protocol::BackendError;
use std::path::Path;
use subject_projection::{installed, runtime_format};
use update_client::{
    DiscoveryConfig, UpdateError,
    coordinator::{ApplicationActivation, ArtifactKind, Coordinator, DeviceVersion, Phase, Record, Store, WaitReason},
    taxonomy::{Reference, Selection},
};

const STORED: &str = "taxonomy.sqlite3";
const RECORD: &str = "taxonomy-selection-v2";
const UPDATE_STATE: &str = "taxonomy-update-state-v2";

struct BrowserStore;
impl Store for BrowserStore {
    fn load(&self) -> Result<Record, UpdateError> {
        web_storage::reference_database(UPDATE_STATE)?.map(|json| serde_json::from_str(&json).map_err(UpdateError::operation)).transpose().map(|record| record.unwrap_or_default())
    }
    fn save(&mut self, expected_revision: u64, next: &Record) -> Result<(), UpdateError> {
        // This synchronous adapter runs on the sole storage owner; there is no await between CAS and commit.
        if self.load()?.revision != expected_revision {
            return Err("taxonomy update state changed".into());
        }
        web_storage::publish_reference_database(UPDATE_STATE, &serde_json::to_string(next).map_err(UpdateError::operation)?).map_err(Into::into)
    }
}

fn publish(selected: &Selection) -> Result<(), BackendError> {
    let record = serde_json::to_string(selected).map_err(BackendError::operation)?;
    web_storage::publish_reference_database(RECORD, &record).map_err(BackendError::operation)
}

pub(crate) async fn install() -> Result<(), BackendError> {
    let bundled_release = installed::BUNDLED_RELEASE_ID;
    let bundled_path = format!("taxonomy-{}.sqlite3", installed::bundled_source().replace(':', "-"));
    let stored = web_storage::reference_database(RECORD)?.and_then(|record| serde_json::from_str::<Selection>(&record).ok());
    let stored = stored.and_then(|selected| selected.recover().ok()).filter(|selected| selected.current.release >= bundled_release && (selected.current.artifact.is_some() || selected.current.path == bundled_path));
    let selected = match stored {
        Some(selected) => selected,
        None => {
            let reference = Reference { path: bundled_path, release: bundled_release, format: runtime_format::FORMAT_VERSION, artifact: None };
            if reference.validate().is_err() {
                let image = download_bundle(STORED).await?;
                installed::validate_image(&image, (reference.release, reference.format))?;
                client_platform_web::reserve_database_capacity(1).await?;
                web_storage::import_reference_database(&reference.path, &image)?;
            }
            Selection::new(reference)
        }
    };
    publish(&selected)?;
    let mut retained = vec![selected.current.path.clone()];
    if let Some(previous) = &selected.previous {
        retained.push(previous.path.clone());
    }
    web_storage::prune_reference_databases_except("taxonomy-", &retained)?;
    if let Some(artifact) = &selected.current.artifact {
        client_platform_web::cpu_host::set_taxonomy(artifact.clone());
    }
    installed::activate(Path::new(&selected.current.path))?;
    spawn_detached(async move {
        if let Err(error) = download_newer(selected).await {
            log::warn!("Taxonomy update will retry on next start: {error}");
        }
    });
    Ok(())
}

pub async fn install_bundled_matching() -> Result<(), BackendError> {
    installed::install_snapshot(&download_bundle(STORED).await?).map_err(BackendError::operation)
}

async fn download_newer(selected: Selection) -> Result<(), BackendError> {
    let config = DiscoveryConfig::bundled()?;
    let now = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_err(BackendError::operation)?.as_secs();
    let mut device = DeviceVersion {
        application: update_client::APPLICATION_VERSION.parse().map_err(BackendError::operation)?,
        schema: library_database::LIBRARY_SCHEMA_VERSION as u32,
        taxonomy_formats: subject_projection::runtime_format::DOWNLOAD_FORMATS.to_vec(),
        taxonomy_release: selected.current.release,
        target: "taxonomy-only".into(),
    };
    let mut updates = Coordinator::open(BrowserStore, config, device.clone(), js_sys::Date::now().to_string(), ApplicationActivation::Restart)?;
    match update_client::fetch::manifest_bytes(updates.config()).await {
        Ok(envelope) => {
            updates.discovered(envelope, now)?;
        }
        Err(error) => {
            updates.discovery_failed(error.clone())?;
            if updates.record().approved.is_none() {
                return Err(error.into());
            }
        }
    }
    updates.approve_taxonomy(now)?;
    let Some(_) = updates.record().approved.as_ref().filter(|plan| plan.phase != Phase::Quarantined) else {
        return Ok(());
    };
    let manifest = updates.approved_manifest()?;
    let release = manifest.taxonomy.as_ref().ok_or("missing approved taxonomy")?;
    if !update_client::taxonomy::compatible(release, &device.application, device.schema, &device.taxonomy_formats) {
        updates.quarantine_after_recovery("approved taxonomy needs a supported runtime publication".into())?;
        return Ok(());
    }
    let reference = Reference { path: format!("taxonomy-{}-{}.sqlite3", release.release_id, release.artifact.sha256), release: release.release_id, format: release.format_version, artifact: Some(release.artifact.clone()) };
    if reference.validate().is_err() {
        if !updates.needs_preparation() {
            updates.activation_recovered_for_retry(WaitReason::Retry, "prepared taxonomy image is missing".into())?;
        }
        let snapshot = match update_client::fetch::download(&release.artifact).await {
            Ok(snapshot) => snapshot,
            Err(error) => {
                updates.preparation_failed(WaitReason::from_error(&error), error.clone())?;
                return Err(error.into());
            }
        };
        update_client::taxonomy::validate_download(&snapshot, release)?;
        client_platform_web::reserve_database_capacity(1).await?;
        web_storage::import_reference_database(&reference.path, &snapshot)?;
    }
    if updates.needs_preparation() {
        updates.artifact_prepared(ArtifactKind::Taxonomy)?;
    }
    let next = if selected.current.path == reference.path { selected } else { selected.stage(reference)? };
    updates.begin_activation()?;
    publish(&next)?;
    device.taxonomy_release = next.current.release;
    updates.activated(device)?;
    Ok(())
}
