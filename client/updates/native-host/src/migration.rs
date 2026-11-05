//! Isolated database preparation and journal assembly shared by every host.
use super::*;
use std::process::Command;
use std::time::Duration;
use update_client::Manifest;

pub(crate) enum CandidatePolicy {
    /// A desktop candidate must exactly match the approved release.
    Release,
    /// Android migrates with the already installed APK, which may be newer.
    Installed,
}

pub(crate) struct Migration {
    root: PathBuf,
    id: String,
    pub work: PathBuf,
    replacements: Vec<Replacement>,
    job: PrepareJob,
    manifest: Manifest,
    application: bool,
}

impl Migration {
    pub fn new(root: &Path, data: &Path, id: &str, paths: &[(ArtifactKind, PathBuf)], manifest: Manifest) -> Result<Self, String> {
        let work = root.join("generations").join(id);
        fs::create_dir_all(&work).map_err(|e| e.to_string())?;
        let mut replacements = Vec::new();
        let mut databases = Vec::new();
        for (index, destination) in databases::paths(data)?.into_iter().enumerate() {
            let previous = work.join(format!("{index}.prior.sqlite3"));
            let prepared = work.join(format!("{index}.next.sqlite3"));
            activation::copy_database(&destination, &previous)?;
            activation::copy_database(&previous, &prepared)?;
            if destination.file_name().is_some_and(|name| name == "library.db") {
                databases.push(prepared.clone());
            }
            replacements.push(Replacement { destination, previous: Some(previous), prepared, sqlite: true });
        }
        let taxonomy = paths
            .iter()
            .find(|(kind, _)| *kind == ArtifactKind::Taxonomy)
            .map(|(_, source)| {
                let path = work.join("taxonomy.sqlite3");
                activation::copy_file(source, &path)?;
                Ok::<_, String>(path)
            })
            .transpose()?;
        let expected_taxonomy = manifest.taxonomy.as_ref().filter(|_| taxonomy.is_some()).map(|t| (t.release_id, t.format_version));
        let result = work.join("prepared.json");
        if result.try_exists().map_err(|e| e.to_string())? {
            fs::remove_file(&result).map_err(|e| e.to_string())?;
        }
        let job = PrepareJob { databases, taxonomy, previous_taxonomy: active(root)?, expected_taxonomy, result };
        Ok(Self { root: root.into(), id: id.into(), work, replacements, job, manifest, application: paths.iter().any(|(kind, _)| *kind == ArtifactKind::Application) })
    }

    pub fn run(&self, policy: CandidatePolicy, version: &str, prepare: impl FnOnce(&Path) -> Result<(), String>) -> Result<Prepared, String> {
        let job_path = self.work.join("prepare-job.json");
        activation::write_json(&job_path, &self.job)?;
        prepare(&job_path)?;
        let prepared: Prepared = activation::read_json(&self.job.result)?;
        let application = self.manifest.application.as_ref().filter(|_| self.application);
        let (expected_version, expected_schema, minimum_schema) = match policy {
            CandidatePolicy::Release => application.map(|app| (app.version.to_string(), Some(app.schema_version), None)).unwrap_or_else(|| (version.to_owned(), None, None)),
            CandidatePolicy::Installed => (version.to_owned(), Some(library_database::LIBRARY_SCHEMA_VERSION as u32), application.map(|app| app.schema_version)),
        };
        if prepared.device.application.to_string() != expected_version || expected_schema.is_some_and(|schema| prepared.device.schema != schema) || minimum_schema.is_some_and(|schema| prepared.device.schema < schema) {
            return Err("migration helper version/schema differs from expected application".into());
        }
        if let Some((release, _)) = self.job.expected_taxonomy {
            if prepared.device.taxonomy_release != release || prepared.taxonomy.as_ref().map(|t| t.release) != Some(release) {
                return Err("migration helper did not prepare approved taxonomy".into());
            }
        }
        Ok(prepared)
    }

    pub fn finish(&mut self, prepared: &Prepared) -> Result<(), String> {
        let next = self.work.join("active-taxonomy.json");
        activation::write_json(&next, &prepared.taxonomy)?;
        self.add_file(self.root.join("active-taxonomy.json"), next, self.work.join("prior-taxonomy.json"))
    }

    pub fn add_file(&mut self, destination: PathBuf, prepared: PathBuf, backup: PathBuf) -> Result<(), String> {
        fs::create_dir_all(parent_directory(&destination)?).map_err(|e| e.to_string())?;
        let previous = if destination.try_exists().map_err(|e| e.to_string())? {
            activation::copy_file(&destination, &backup)?;
            Some(backup)
        } else {
            None
        };
        self.replacements.push(Replacement { destination, previous, prepared, sqlite: false });
        Ok(())
    }

    pub fn promote(self, previous_version: String, next_version: String, appimage: Option<PathBuf>) -> Result<String, String> {
        activation::sync_dir(&self.work)?;
        activation::sync_dir(parent_directory(&self.work)?)?;
        activation::sync_dir(&self.root)?;
        Journal { plan_id: self.id.clone(), previous_version, next_version, appimage, replacements: self.replacements, state: State::Applying }.promote(&self.root)?;
        Ok(self.id)
    }
}

/// The candidate must exit before any prepared SQLite files are promoted.
pub(crate) fn run_candidate(executable: &Path, job: &Path) -> Result<(), String> {
    let mut command = Command::new(executable);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.arg("--bokheim-prepare-update").arg(job).env_remove("APPIMAGE").env_remove("APPDIR").spawn().map_err(|e| e.to_string())?;
    let status = update_client::supervisor::wait_for_preparation(&mut child, Duration::from_secs(30 * 60))?;
    if status.success() { Ok(()) } else { Err(format!("candidate migration failed: {status}")) }
}
