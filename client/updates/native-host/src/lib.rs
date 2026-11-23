#![cfg(any(target_os = "linux", target_os = "android", target_os = "windows", target_os = "macos"))]
//! Native update lifecycle and platform installation policies. Application
//! activation precedes workers; reference-only preparation runs in the background.

use serde::{Deserialize, Serialize};
use std::env::consts::ARCH;
use std::{
    fs::{self},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
pub use update_client::UpdateError;
use update_client::native_files::parent_directory;
use update_client::taxonomy::{Reference, Selection};
use update_client::{
    DiscoveryConfig,
    activation::{self, Journal, Replacement, State},
    coordinator::{ApplicationActivation, ArtifactKind, Coordinator, DeviceVersion, Phase, UpdateView, WaitReason},
    native_state::{SqliteStore, UpdateLock},
};
use update_files::{probe_write, same_volume};

#[cfg(any(target_os = "linux", target_os = "android"))]
pub mod android;
mod databases;
mod host;
mod migration;
pub mod windows;
use host::activate_taxonomy;
pub use host::{NativeHost, Platform};
use migration::{CandidatePolicy, Migration, run_candidate};
#[cfg(windows)]
pub mod windows_process;

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActiveTaxonomy {
    pub path: PathBuf,
    pub release: u64,
}
#[derive(Serialize, Deserialize)]
struct PrepareJob {
    databases: Vec<PathBuf>,
    taxonomy: Option<PathBuf>,
    previous_taxonomy: Option<ActiveTaxonomy>,
    expected_taxonomy: Option<(u64, u32)>,
    result: PathBuf,
}
#[derive(Serialize, Deserialize)]
struct Prepared {
    device: DeviceVersion,
    taxonomy: Option<ActiveTaxonomy>,
}

pub fn device(version: &str, taxonomy: Option<&ActiveTaxonomy>) -> Result<DeviceVersion, UpdateError> {
    Ok(DeviceVersion {
        application: version.parse().map_err(UpdateError::operation)?,
        schema: library_database::LIBRARY_SCHEMA_VERSION as u32,
        taxonomy_formats: subject_projection::runtime_format::DOWNLOAD_FORMATS.to_vec(),
        taxonomy_release: taxonomy.map(|t| t.release).unwrap_or(subject_projection::bundled::RELEASE_ID),
        target: if cfg!(target_os = "android") {
            format!("android-{}-apk", ARCH)
        } else if cfg!(target_os = "windows") {
            format!("windows-{}-zip", ARCH)
        } else {
            format!("linux-{}-appimage", ARCH)
        },
    })
}

/// Candidate subprocess entry point. It only opens the isolated files in its job.
pub fn prepare_candidate(job_path: &Path, version: &str) -> Result<(), UpdateError> {
    let job: PrepareJob = activation::read_json(job_path)?;
    let taxonomy = if let Some(path) = job.taxonomy {
        let actual = subject_projection::installed::metadata(&path)?;
        if Some(actual) != job.expected_taxonomy {
            return Err("taxonomy version differs from signed manifest".into());
        }
        subject_projection::installed::validate(&path, actual)?;
        Some(ActiveTaxonomy { path, release: actual.0 })
    } else {
        job.previous_taxonomy
    };
    activate_taxonomy(taxonomy.as_ref())?;
    for path in job.databases {
        (|| -> Result<(), library_database::DatabaseError> {
            let db = library_database::Database::open(&path)?;
            db.initialize_library()?;
            db.check_migrated()
        })()
        .map_err(UpdateError::operation)?;
    }
    activation::write_json(&job.result, &Prepared { device: device(version, taxonomy.as_ref())?, taxonomy })
}

/// A data-only host shares signed discovery, approval, staging and selection.
/// Its target never selects an application package from the feed.
pub struct ReferenceOnly {
    running_release: u64,
}
pub type TaxonomyHost = NativeHost<ReferenceOnly>;
impl Platform for ReferenceOnly {
    fn target(&self) -> String {
        "taxonomy-only".into()
    }
}
impl TaxonomyHost {
    pub fn open(data: &Path, version: &str, config: DiscoveryConfig) -> Result<Self, UpdateError> {
        let mut host = Self::open_platform(data, version, config, ReferenceOnly { running_release: 0 })?;
        host.select_taxonomy()?;
        host.platform.running_release = host.installed()?.taxonomy_release;
        Ok(host)
    }
    pub fn taxonomy_view(&self) -> UpdateView {
        if self.installed().is_ok_and(|device| device.taxonomy_release > self.platform.running_release) { UpdateView::Ready { action: update_client::coordinator::UpdateAction::Restart } } else { self.view() }
    }
}

pub enum Startup {
    Run,
    Relaunch { executable: PathBuf, trial: Option<String> },
}
pub type Host = NativeHost<Linux>;

pub struct Linux {
    pub executable: PathBuf,
    appimage: Option<PathBuf>,
}

#[cfg(any(target_os = "linux", target_os = "android"))]
impl Platform for Linux {
    fn target(&self) -> String {
        if self.appimage.is_some() { format!("linux-{ARCH}-appimage") } else { "linux-taxonomy-only".into() }
    }
    fn workspace(&self, host: &Host) -> Result<u64, UpdateError> {
        host.linux_workspace()
    }
    fn preparation_wait(&self, host: &Host, workspace: u64) -> Result<Option<WaitReason>, UpdateError> {
        host.preflight(workspace)
    }
}
#[cfg(any(target_os = "linux", target_os = "android"))]
impl Host {
    pub fn open(data: &Path, version: &str, config: DiscoveryConfig, appimage: Option<PathBuf>, executable: PathBuf) -> Result<Self, UpdateError> {
        Self::open_platform(data, version, config, Linux { executable, appimage })
    }
    pub fn startup(&mut self, trial: Option<&str>) -> Result<Startup, UpdateError> {
        let lock = self.reload_locked()?;
        if let Some(journal) = Journal::load(&self.root)? {
            if journal.state == State::Committed {
                if self.installed()?.application < journal.next_version.parse().map_err(UpdateError::operation)? {
                    return Ok(Startup::Relaunch { executable: journal.appimage.ok_or("committed application path missing")?, trial: None });
                }
                if let Err(error) = self.finish_committed(&journal) {
                    log::warn!("committed update bookkeeping will retry: {error}");
                    self.select_taxonomy()?;
                    return Ok(Startup::Run);
                }
            } else if self.accept_trial(&journal, trial)? {
                self.select_taxonomy()?;
                return Ok(Startup::Run);
            } else {
                self.recover(&journal, "interrupted or failed startup trial; prior version restored".into())?;
                if journal.previous_version != self.version {
                    return Ok(Startup::Relaunch { executable: journal.appimage.ok_or("restored application path missing")?, trial: None });
                }
            }
        }
        if self.coordinator.activation_due() && !self.coordinator.is_taxonomy_only() {
            match self.prepare_and_promote(&lock) {
                Ok(id) => return Ok(Startup::Relaunch { executable: self.appimage.clone().unwrap_or_else(|| self.executable.clone()), trial: Some(id) }),
                Err(error) => self.failed_activation(error)?,
            }
        }
        self.coordinator.set_installed(self.installed()?);
        self.select_taxonomy()?;
        Ok(Startup::Run)
    }

    fn preflight(&self, mut required: u64) -> Result<Option<WaitReason>, UpdateError> {
        if let Some(plan) = &self.coordinator.record().approved {
            for entry in &plan.artifacts {
                if !plan.completed.contains(&entry.kind) {
                    required = required.checked_add(entry.artifact.bytes).ok_or("workspace overflow")?;
                }
            }
            if let Some(artifact) = plan.artifacts.iter().find(|a| a.kind == ArtifactKind::Application) {
                let path = self.appimage.as_ref().ok_or("application is not an AppImage")?;
                let parent = path.parent().ok_or("AppImage has no parent")?;
                match probe_write(parent) {
                    Ok(()) => {}
                    Err(error) if matches!(error.kind(), std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem) => return Ok(Some(WaitReason::Permission)),
                    Err(error) => return Err(UpdateError::operation(error)),
                }
                if same_volume(parent, &self.root).map_err(UpdateError::operation)? {
                    required = required.checked_add(artifact.artifact.bytes).ok_or("workspace overflow")?;
                } else {
                    let deficit = artifact.artifact.bytes.saturating_sub(fs2::available_space(parent).map_err(UpdateError::operation)?);
                    if deficit > 0 {
                        return Ok(Some(WaitReason::Space { additional_bytes: deficit }));
                    }
                }
            }
        }
        let deficit = required.saturating_sub(fs2::available_space(&self.root).map_err(UpdateError::operation)?);
        Ok((deficit > 0).then_some(WaitReason::Space { additional_bytes: deficit }))
    }
    fn linux_workspace(&self) -> Result<u64, UpdateError> {
        let mut bytes = self.data_workspace()?;
        if self.coordinator.record().approved.as_ref().is_some_and(|p| p.artifacts.iter().any(|a| a.kind == ArtifactKind::Application)) {
            if let Some(path) = &self.appimage {
                bytes = bytes.checked_add(fs::metadata(path).map_err(UpdateError::operation)?.len()).ok_or("workspace overflow")?;
            }
        }
        Ok(bytes)
    }
    fn prepare_and_promote(&mut self, lock: &UpdateLock) -> Result<String, UpdateError> {
        let paths = self.coordinator.staged_paths(&self.root, lock)?;
        if let Some(reason) = self.preflight(self.workspace_bytes()?)? {
            self.coordinator.activation_recovered_for_retry(reason, "update needs storage or installation permission".into())?;
            return Err("update waits for storage or permission".into());
        }
        let manifest = self.coordinator.begin_activation()?;
        let id = self.coordinator.record().approved.as_ref().ok_or("no approved update")?.id.clone();
        let mut migration = Migration::new(&self.root, &self.data, &id, &paths, manifest)?;
        let application = paths.iter().find(|(kind, _)| *kind == ArtifactKind::Application).map(|(_, path)| path);
        let candidate = if let Some(source) = application {
            let path = migration.work.join("candidate.AppImage");
            activation::copy_file(source, &path)?;
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).map_err(UpdateError::operation)?;
            path
        } else {
            self.executable.clone()
        };
        let prepared = migration.run(CandidatePolicy::Release, &self.version, |job| run_candidate(&candidate, job))?;
        migration.finish(&prepared)?;
        if application.is_some() {
            let destination = self.appimage.clone().ok_or("application update requires an AppImage installation")?;
            let required = fs::metadata(&candidate).map_err(UpdateError::operation)?.len();
            let free = fs2::available_space(parent_directory(&destination)?).map_err(UpdateError::operation)?;
            if free < required {
                return Err(format!("AppImage volume needs {} additional bytes", required - free).into());
            }
            let backup = migration.work.join("previous.AppImage");
            migration.add_file(destination, candidate, backup)?;
        }
        migration.promote(self.version.clone(), prepared.device.application.to_string(), self.appimage.clone())
    }
}
fn selection(root: &Path) -> Result<Option<Selection>, UpdateError> {
    let path = root.join("active-taxonomy.json");
    if !path.try_exists().map_err(UpdateError::operation)? {
        return Ok(None);
    }
    let selected: Option<Selection> = activation::read_json(&path)?;
    let Some(selected) = selected else {
        return Ok(None);
    };
    Ok(Some(selected))
}

fn bundled_reference(root: &Path) -> Result<Reference, UpdateError> {
    let source = subject_projection::bundled::path()?;
    let identity = subject_projection::installed::bundled_source();
    let digest = identity.rsplit(':').next().ok_or("missing bundled identity")?;
    let directory = root.join("generations").join(format!("bundled-{digest}"));
    let path = directory.join("taxonomy.sqlite3");
    if !path.exists() {
        fs::create_dir_all(&directory).map_err(UpdateError::operation)?;
        activation::copy_file(&source, &path)?;
        activation::sync_dir(&directory)?;
    }
    let (release, format) = subject_projection::installed::metadata(&source)?;
    let reference = Reference { path: path.to_str().ok_or("taxonomy path is not UTF-8")?.into(), release, format, artifact: None };
    reference.validate()?;
    Ok(reference)
}

fn recover_selection(root: &Path) -> Result<(), UpdateError> {
    let selected = match selection(root) {
        Ok(selected) => selected,
        Err(error) => {
            log::warn!("taxonomy selection is unreadable; selecting the bundled runtime: {error}");
            activation::write_json(&root.join("active-taxonomy.json"), &Selection::new(bundled_reference(root)?))?;
            return Ok(());
        }
    };
    if let Some(selected) = selected {
        let recovered = match selected.recover() {
            Ok(recovered) => recovered,
            Err(error) => {
                log::warn!("taxonomy references are unreadable; selecting the bundled runtime: {error}");
                Selection::new(bundled_reference(root)?)
            }
        };
        let recovered = if recovered.current.release < subject_projection::installed::BUNDLED_RELEASE_ID { recovered.stage(bundled_reference(root)?)? } else { recovered };
        if recovered.current.path != selected.current.path {
            activation::write_json(&root.join("active-taxonomy.json"), &recovered)?;
        }
    }
    Ok(())
}

fn active(root: &Path) -> Result<Option<ActiveTaxonomy>, UpdateError> {
    Ok(selection(root)?.map(|selected| ActiveTaxonomy { path: selected.current.path.into(), release: selected.current.release }))
}

fn next_selection(root: &Path, taxonomy: &ActiveTaxonomy, artifact: Option<update_client::Artifact>) -> Result<Selection, UpdateError> {
    let next = Reference { path: taxonomy.path.to_str().ok_or("taxonomy path is not UTF-8")?.into(), release: taxonomy.release, format: subject_projection::installed::metadata(&taxonomy.path)?.1, artifact };
    let previous = match selection(root)? {
        Some(selected) => selected,
        None => Selection::new(bundled_reference(root)?),
    };
    if previous.current.path == next.path { Ok(previous) } else { previous.stage(next) }
}

#[cfg(all(test, target_os = "linux"))]
mod tests;
