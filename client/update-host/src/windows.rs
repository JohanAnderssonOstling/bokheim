//! Windows transaction host. Replacement/recovery runs in a copied helper executable
//! after the application process releases ownership, never in the installed EXE.
use super::*;

pub enum WindowsStartup {
    Run,
    Helper,
}

pub type WindowsHost = NativeHost<Windows>;

pub struct Windows {
    pub installation: PathBuf,
}

impl Platform for Windows {
    fn target(&self) -> String {
        "windows-x86_64-zip".into()
    }
}

impl WindowsHost {
    pub fn open(data: &Path, installation: &Path, version: &str, config: DiscoveryConfig) -> Result<Self, String> {
        Self::open_platform(data, version, config, Windows { installation: installation.canonicalize().map_err(|e| e.to_string())? })
    }

    pub fn startup(&mut self, trial: Option<&str>) -> Result<WindowsStartup, String> {
        let _lock = self.reload_locked()?;
        if let Some(journal) = Journal::load(&self.root)? {
            if journal.state == State::Committed {
                if self.installed()?.application < journal.next_version.parse().map_err(|e| format!("{e}"))? {
                    return Ok(WindowsStartup::Helper);
                }
                self.acknowledge(&journal)?;
                Journal::clear(&self.root)?;
            } else if !self.accept_trial(&journal, trial)? {
                return Ok(WindowsStartup::Helper);
            }
        } else if self.coordinator.activation_due() {
            return Ok(WindowsStartup::Helper);
        }
        if !self.trial {
            if let Err(error) = self.cleanup_committed() {
                // Antivirus or another reader can temporarily hold old files.
                // Cleanup is retried next launch and cannot block a healthy app.
                log::warn!("Windows update cleanup will retry: {error}");
            }
        }
        self.select_taxonomy()?;
        Ok(WindowsStartup::Run)
    }

    /// A helper that never started has not changed live files. Preserve approval
    /// and resume preparation while allowing this application session to run.
    pub fn helper_unavailable(&mut self, reason: WaitReason, diagnostic: String) -> Result<(), String> {
        let _lock = UpdateLock::acquire(&self.root)?;
        if Journal::load(&self.root)?.is_some() {
            return Err(diagnostic);
        }
        self.coordinator.reload()?;
        self.coordinator.activation_recovered_for_retry(now(), reason, diagnostic)
    }

    /// The caller must hold exclusive desktop ownership and execute from a copy
    /// outside installation. Returns the plan ID for the trial process, or None
    /// when the restored/committed application should run normally.
    pub fn prepare_or_recover(&mut self, prepare: impl FnOnce(&Path, &Path) -> Result<(), String>) -> Result<Option<String>, String> {
        let lock = self.reload_locked()?;
        if let Some(journal) = Journal::load(&self.root)? {
            if journal.state != State::Committed {
                self.recover(&journal, "Windows update startup was interrupted".into())?;
            }
            return Ok(None);
        }
        if !self.coordinator.activation_due() {
            return Ok(None);
        }
        match self.promote(&lock, prepare) {
            Ok(id) => Ok(Some(id)),
            Err(error) => {
                self.failed_activation(error)?;
                Ok(None)
            }
        }
    }

    /// Run from the external helper while it holds desktop ownership. The
    /// candidate runs against private copies and must exit before promotion.
    pub fn prepare_or_recover_in_process(&mut self) -> Result<Option<String>, String> {
        self.prepare_or_recover(run_candidate)
    }

    pub fn recover_stopped_trial(&mut self, reason: String) -> Result<(), String> {
        let _lock = self.reload_locked()?;
        if let Some(journal) = Journal::load(&self.root)? {
            self.recover(&journal, reason)?;
        }
        Ok(())
    }

    fn promote(&mut self, lock: &UpdateLock, prepare: impl FnOnce(&Path, &Path) -> Result<(), String>) -> Result<String, String> {
        let paths = self.coordinator.staged_paths(&self.root, lock)?;
        let app = paths.iter().find(|(kind, _)| *kind == ArtifactKind::Application).map(|(_, path)| path);
        let package = app.map(|path| update_client::windows_package::inspect(path)).transpose()?;
        let mut required = self.workspace_bytes()?;
        if let Some(package) = &package {
            // Candidate plus temporary replacement copies. Backups below count
            // existing bytes independently; no fixed free-space threshold.
            required = required.checked_add(package.expanded_bytes).ok_or("workspace overflow")?;
            for relative in &package.files {
                let destination = self.installation.join(relative);
                let mut parent = destination.parent().ok_or("missing package directory")?;
                while !parent.exists() {
                    parent = parent.parent().ok_or("invalid installation directory")?;
                }
                for ancestor in parent.ancestors().take_while(|p| p.starts_with(&self.installation)) {
                    let metadata = fs::symlink_metadata(ancestor).map_err(|e| e.to_string())?;
                    if metadata.file_type().is_symlink() {
                        return Err("installation contains a linked directory".into());
                    }
                    #[cfg(windows)]
                    {
                        use std::os::windows::fs::MetadataExt;
                        if metadata.file_attributes() & 0x400 != 0 {
                            return Err("installation contains a reparse point".into());
                        }
                    }
                }
                match probe_write(parent) {
                    Ok(()) => {}
                    Err(error) if matches!(error.kind(), std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem) => {
                        self.coordinator.activation_recovered_for_retry(now(), WaitReason::Permission, "installation directory is not writable".into())?;
                        return Err("waiting for installation permission".into());
                    }
                    Err(error) => return Err(error.to_string()),
                }
                if destination.try_exists().map_err(|e| e.to_string())? {
                    required = required.checked_add(fs::metadata(destination).map_err(|e| e.to_string())?.len()).ok_or("backup workspace overflow")?;
                }
            }
            if same_volume(&self.root, &self.installation).map_err(|e| e.to_string())? {
                required = required.checked_add(package.expanded_bytes).ok_or("replacement workspace overflow")?;
            } else {
                let deficit = package.expanded_bytes.saturating_sub(fs2::available_space(&self.installation).map_err(|e| e.to_string())?);
                if deficit > 0 {
                    self.coordinator.activation_recovered_for_retry(now(), WaitReason::Space { additional_bytes: deficit }, "insufficient installation volume space".into())?;
                    return Err("waiting for installation storage".into());
                }
            }
        }
        if let Some((_, taxonomy)) = paths.iter().find(|(kind, _)| *kind == ArtifactKind::Taxonomy) {
            required = required.checked_add(fs::metadata(taxonomy).map_err(|e| e.to_string())?.len()).ok_or("taxonomy workspace overflow")?;
        }
        let deficit = required.saturating_sub(fs2::available_space(&self.root).map_err(|e| e.to_string())?);
        if deficit > 0 {
            self.coordinator.activation_recovered_for_retry(now(), WaitReason::Space { additional_bytes: deficit }, "insufficient Windows update workspace".into())?;
            return Err("waiting for storage".into());
        }
        let manifest = self.coordinator.begin_activation()?;
        let id = self.coordinator.record().approved.as_ref().ok_or("no approved update")?.id.clone();
        let mut migration = Migration::new(&self.root, &self.data, &id, &paths, manifest)?;
        let candidate_dir = migration.work.join("application");
        let candidate = if let Some(source) = app {
            if candidate_dir.exists() {
                fs::remove_dir_all(&candidate_dir).map_err(|e| e.to_string())?;
            }
            update_client::windows_package::unpack(source, &candidate_dir, fs2::available_space(&migration.work).map_err(|e| e.to_string())?)?;
            candidate_dir.join("Bokheim.exe")
        } else {
            self.installation.join("Bokheim.exe")
        };
        let prepared = migration.run(CandidatePolicy::Release, &self.version, |job| prepare(&candidate, job))?;
        migration.finish(&prepared)?;
        if let Some(package) = package {
            for (index, relative) in package.files.iter().enumerate() {
                let backup = migration.work.join(format!("app-{index}.prior"));
                migration.add_file(self.installation.join(relative), candidate_dir.join(relative), backup)?;
            }
        }
        migration.promote(self.version.clone(), prepared.device.application.to_string(), None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::{digest::{digest, SHA256}, signature::{Ed25519KeyPair, KeyPair}};
    use std::io::{Cursor, Write};
    use update_client::{ApplicationRelease, Artifact, Manifest, SignedManifest, SIGNATURE_CONTEXT};
    fn hex(bytes: &[u8]) -> String { bytes.iter().map(|b| format!("{b:02x}")).collect() }

    struct Fixture { temp: tempfile::TempDir, config: DiscoveryConfig, key: Ed25519KeyPair }
    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            fs::create_dir_all(temp.path().join("app")).unwrap();
            fs::create_dir(temp.path().join("data")).unwrap();
            fs::write(temp.path().join("app/Bokheim.exe"), b"old app").unwrap();
            rusqlite::Connection::open(temp.path().join("data/app-state-v1.sqlite3")).unwrap().execute_batch("CREATE TABLE setting(v); INSERT INTO setting VALUES('old')").unwrap();
            let key = Ed25519KeyPair::from_seed_unchecked(&[23;32]).unwrap();
            let config = DiscoveryConfig { endpoint: "https://example.org/v1/stable".into(), channel: "stable".into(), trusted_keys: [("test".into(), key.public_key().as_ref().try_into().unwrap())].into() };
            Self { temp, config, key }
        }
        fn host(&self, version: &str) -> WindowsHost {
            WindowsHost::open(&self.temp.path().join("data"), &self.temp.path().join("app"), version, self.config.clone()).unwrap()
        }
        fn approved(&self) -> WindowsHost {
            let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
            for (name, contents) in [("Bokheim.exe", b"new app".as_slice())] {
                zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap(); zip.write_all(contents).unwrap();
            }
            let bytes = zip.finish().unwrap().into_inner();
            let artifact = Artifact { url: "https://example.org/releases/2/app.zip".into(), bytes: bytes.len() as u64, sha256: hex(digest(&SHA256, &bytes).as_ref()) };
            let manifest = Manifest { protocol_version: 1, channel: "stable".into(), sequence: 1, issued_at_unix: now(), expires_at_unix: now()+3600, taxonomy: None, catalog: None,
                application: Some(ApplicationRelease { version: "2.0.0".parse().unwrap(), name: "Release".into(), schema_version: library_database::LIBRARY_SCHEMA_VERSION as u32, taxonomy_format_versions: vec![1], artifacts: [("windows-x86_64-zip".into(), artifact)].into() }) };
            let payload = serde_json::to_string(&manifest).unwrap(); let mut message = SIGNATURE_CONTEXT.to_vec(); message.extend_from_slice(payload.as_bytes());
            let envelope = serde_json::to_vec(&SignedManifest { key_id: "test".into(), payload, signature: hex(self.key.sign(&message).as_ref()) }).unwrap();
            let mut host = self.host("1.0.0"); host.coordinator.discovered(envelope, now()).unwrap(); host.approve().unwrap();
            let stage = host.root.join("staging").join(&host.coordinator.record().approved.as_ref().unwrap().id);
            fs::create_dir_all(&stage).unwrap(); fs::write(stage.join("application.bin"), bytes).unwrap();
            host.coordinator.artifact_prepared(ArtifactKind::Application).unwrap(); host
        }
        fn executable(&self) -> Vec<u8> { fs::read(self.temp.path().join("app/Bokheim.exe")).unwrap() }
    }

    #[test]
    fn executable_and_registry_recover_after_interrupted_trial() {
        let f = Fixture::new(); let mut host = f.approved();
        assert!(host.prepare_or_recover(|_, _| panic!("same-session activation")).unwrap().is_none()); drop(host);
        let mut helper = f.host("1.0.0");
        assert!(matches!(helper.startup(None).unwrap(), WindowsStartup::Helper));
        helper.prepare_or_recover(|_, path| prepare_candidate(path, "2.0.0")).unwrap().unwrap();
        assert_eq!(f.executable(), b"new app");
        rusqlite::Connection::open(f.temp.path().join("data/app-state-v1.sqlite3")).unwrap().execute_batch("UPDATE setting SET v='trial'").unwrap(); drop(helper);
        let mut recovery = f.host("2.0.0"); recovery.prepare_or_recover(|_, _| panic!("must recover first")).unwrap();
        assert_eq!(f.executable(), b"old app");
        let value: String = rusqlite::Connection::open(f.temp.path().join("data/app-state-v1.sqlite3")).unwrap().query_row("SELECT v FROM setting", [], |r| r.get(0)).unwrap();
        assert_eq!(value, "old");
        assert_eq!(recovery.coordinator.record().approved.as_ref().unwrap().phase, Phase::Quarantined);
    }

    #[test]
    fn healthy_trial_commits_then_cleans_old_workspace() {
        let f = Fixture::new(); drop(f.approved()); let mut helper = f.host("1.0.0");
        let id = helper.prepare_or_recover(|_, path| prepare_candidate(path, "2.0.0")).unwrap().unwrap(); drop(helper);
        let mut app = f.host("2.0.0");
        assert!(matches!(app.startup(Some(&id)).unwrap(), WindowsStartup::Run)); assert!(app.is_trial());
        app.healthy().unwrap(); assert!(app.coordinator.record().approved.is_none()); drop(app);
        let mut app = f.host("2.0.0"); assert!(matches!(app.startup(None).unwrap(), WindowsStartup::Run));
        assert_eq!(f.executable(), b"new app"); assert!(!app.root.join("generations").join(id).exists());
    }

    #[cfg(windows)]
    #[test]
    fn locked_obsolete_file_does_not_block_startup_and_cleanup_retries() {
        use std::os::windows::fs::OpenOptionsExt;
        let f = Fixture::new();
        let old = f.temp.path().join("data/updates/staging").join("ab".repeat(32));
        fs::create_dir_all(&old).unwrap();
        let file = old.join("application.bin"); fs::write(&file, b"obsolete").unwrap();
        let reader = fs::OpenOptions::new().read(true).share_mode(0).open(&file).unwrap();
        let mut app = f.host("1.0.0");
        assert!(matches!(app.startup(None).unwrap(), WindowsStartup::Run));
        assert!(file.exists()); drop(app); drop(reader);
        let mut app = f.host("1.0.0");
        assert!(matches!(app.startup(None).unwrap(), WindowsStartup::Run));
        assert!(!old.exists());
    }
}
