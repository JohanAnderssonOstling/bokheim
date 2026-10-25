//! Android owns APK replacement. This host owns the approved data transaction.
//! The migration callback must run prepare_candidate in a fresh private process;
//! it must return only after that process has exited and all SQLite handles close.
use super::*;

#[derive(Clone, Serialize)]
pub struct InstallRequest {
    pub path: PathBuf,
    pub version: String,
    pub bytes: u64,
    pub sha256: String,
}

pub type AndroidHost = NativeHost<Android>;

pub struct Android {
    target: String,
}

impl Android {
    fn new(target: &str) -> Result<Self, String> {
        if !target.starts_with("android-") || !target.ends_with("-apk") { return Err("invalid Android package target".into()); }
        Ok(Self { target: target.into() })
    }
}

impl Platform for Android {
    fn target(&self) -> String {
        self.target.clone()
    }
    fn workspace(&self, host: &AndroidHost) -> Result<u64, String> {
        host.data_workspace()
    }
}

impl AndroidHost {
    pub fn open(data: &Path, version: &str, config: DiscoveryConfig, target: &str) -> Result<Self, String> {
        Self::open_platform(data, version, config, Android::new(target)?)
    }

    /// Keep one lock across construction and recovery so the initial state and
    /// taxonomy can be reused before any library connections open.
    pub fn open_and_startup(data: &Path, version: &str, config: DiscoveryConfig, target: &str, prepare: impl FnOnce(&Path) -> Result<(), String>) -> Result<(Self, bool), String> {
        let (mut host, lock, taxonomy) = Self::open_for_startup(data, version, config, Android::new(target)?)?;
        let trial = host.startup_locked(&lock, taxonomy, prepare)?;
        Ok((host, trial))
    }

    /// Package identity/version validation failed before the installer opened.
    /// Live code and data are untouched; await a corrected signed publication.
    pub fn reject_package(&mut self) -> Result<(), String> {
        let _lock = self.reload_locked()?;
        if !self.coordinator.record().approved.as_ref().is_some_and(|p| p.phase == Phase::Ready) {
            return Err("no ready package to reject".into());
        }
        self.coordinator.quarantine_after_recovery("APK identity or version does not match approved release".into())
    }

    /// An explicit Install click obtains a reverified pinned APK. Returning from
    /// Android's installer never calls activated; only a later startup can do so.
    pub fn install_request(&mut self) -> Result<InstallRequest, String> {
        let lock = self.reload_locked()?;
        let plan = self.coordinator.record().approved.as_ref().ok_or("no approved update")?.clone();
        if plan.phase != Phase::Ready {
            return Err("APK is not ready to install".into());
        }
        let paths = self.coordinator.staged_paths(&self.root, &lock)?;
        let path = paths.into_iter().find(|(k, _)| *k == ArtifactKind::Application).ok_or("no APK in approved update")?.1;
        let artifact = &plan.artifacts.iter().find(|a| a.kind == ArtifactKind::Application).ok_or("no APK metadata")?.artifact;
        let manifest = self.coordinator.approved_manifest()?;
        let version = manifest.application.ok_or("missing APK version")?.version.to_string();
        Ok(InstallRequest { path, version, bytes: artifact.bytes, sha256: artifact.sha256.clone() })
    }

    /// Called before the registry/backend opens. Returns whether startup is a
    /// trial: its background tasks must remain gated until healthy is called.
    pub fn startup(&mut self, prepare: impl FnOnce(&Path) -> Result<(), String>) -> Result<bool, String> {
        let lock = self.reload_locked()?;
        let taxonomy = active(&self.root)?;
        self.startup_locked(&lock, taxonomy, prepare)
    }

    fn startup_locked(&mut self, lock: &UpdateLock, mut taxonomy: Option<ActiveTaxonomy>, prepare: impl FnOnce(&Path) -> Result<(), String>) -> Result<bool, String> {
        if let Some(journal) = Journal::load(&self.root)? {
            if journal.state == State::Committed {
                self.finish_committed(&journal)?;
            } else {
                self.recover(&journal, "Android data startup was interrupted; restored previous data".into())?;
            }
            taxonomy = active(&self.root)?;
        }
        self.coordinator.set_installed(self.installed_with_taxonomy(taxonomy.as_ref())?);
        if self.coordinator.activation_due() {
            let manifest = self.coordinator.approved_manifest()?;
            let needs_apk = self.coordinator.record().approved.as_ref().ok_or("no approved update")?.artifacts.iter().any(|a| a.kind == ArtifactKind::Application);
            let installed = self.installed_with_taxonomy(taxonomy.as_ref())?;
            let apk_present = !needs_apk || manifest.application.as_ref().is_some_and(|app| installed.application >= app.version);
            if apk_present {
                if let Err(error) = self.promote(lock, prepare) {
                    self.failed_activation(error)?;
                    taxonomy = active(&self.root)?;
                    self.coordinator.set_installed(self.installed_with_taxonomy(taxonomy.as_ref())?);
                } else {
                    self.trial = true;
                    taxonomy = active(&self.root)?;
                }
            }
        }
        activate_taxonomy(taxonomy.as_ref())?;
        Ok(self.trial)
    }

    fn promote(&mut self, lock: &UpdateLock, prepare: impl FnOnce(&Path) -> Result<(), String>) -> Result<(), String> {
        let paths = self.coordinator.staged_paths(&self.root, lock)?;
        let deficit = self.workspace_bytes()?.saturating_sub(fs2::available_space(&self.root).map_err(|e| e.to_string())?);
        if deficit > 0 {
            self.coordinator.activation_recovered_for_retry(now(), WaitReason::Space { additional_bytes: deficit }, "insufficient migration workspace".into())?;
            return Err("waiting for migration workspace".into());
        }
        let manifest = self.coordinator.begin_activation()?;
        let plan = self.coordinator.record().approved.as_ref().ok_or("no approved update")?.clone();
        let mut migration = Migration::new(&self.root, &self.data, &plan.id, &paths, manifest)?;
        let prepared = migration.run(CandidatePolicy::Installed, &self.version, prepare)?;
        migration.finish(&prepared)?;
        migration.promote(plan.installed_at_approval.application.to_string(), self.version.clone(), None).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::{digest::{digest, SHA256}, signature::{Ed25519KeyPair, KeyPair}};
    use update_client::{ApplicationRelease, Artifact, Manifest, SignedManifest, SIGNATURE_CONTEXT};

    fn hex(bytes: &[u8]) -> String { bytes.iter().map(|b| format!("{b:02x}")).collect() }

    struct Fixture {
        dir: tempfile::TempDir,
        config: DiscoveryConfig,
        key: Ed25519KeyPair,
    }
    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            rusqlite::Connection::open(dir.path().join("app-state-v1.sqlite3")).unwrap()
                .execute_batch("CREATE TABLE setting(v); INSERT INTO setting VALUES('prior')").unwrap();
            let key = Ed25519KeyPair::from_seed_unchecked(&[17;32]).unwrap();
            let config = DiscoveryConfig { endpoint: "https://example.org/v1/stable".into(), channel: "stable".into(),
                trusted_keys: [("test".into(), key.public_key().as_ref().try_into().unwrap())].into() };
            Self { dir, config, key }
        }
        fn host(&self, version: &str) -> AndroidHost {
            AndroidHost::open(self.dir.path(), version, self.config.clone(), "android-aarch64-apk").unwrap()
        }
        fn startup(&self, version: &str) -> (AndroidHost, bool) {
            AndroidHost::open_and_startup(self.dir.path(), version, self.config.clone(), "android-aarch64-apk", |path| prepare_candidate(path, version)).unwrap()
        }
        fn approved(&self) -> AndroidHost {
            let mut host = self.host("1.0.0");
            let manifest = Manifest { protocol_version: 1, channel: "stable".into(), sequence: 1,
                issued_at_unix: now(), expires_at_unix: now()+3600, taxonomy: None, catalog: None,
                application: Some(ApplicationRelease { version: "2.0.0".parse().unwrap(), name: "Test".into(),
                    schema_version: library_database::LIBRARY_SCHEMA_VERSION as u32, taxonomy_format_versions: vec![1],
                    artifacts: [("android-aarch64-apk".into(), Artifact { url: "https://example.org/releases/2/app.apk".into(), bytes: 3, sha256: hex(digest(&SHA256, b"apk").as_ref()) })].into() }) };
            let payload = serde_json::to_string(&manifest).unwrap();
            let mut message = SIGNATURE_CONTEXT.to_vec(); message.extend_from_slice(payload.as_bytes());
            let envelope = serde_json::to_vec(&SignedManifest { key_id: "test".into(), payload, signature: hex(self.key.sign(&message).as_ref()) }).unwrap();
            host.coordinator.discovered(envelope, now()).unwrap();
            assert_eq!(host.view(), UpdateView::Available);
            assert!(host.install_request().is_err());
            host.approve().unwrap();
            let stage = host.root.join("staging").join(&host.coordinator.record().approved.as_ref().unwrap().id);
            fs::create_dir_all(&stage).unwrap(); fs::write(stage.join("application.bin"), b"apk").unwrap();
            host.coordinator.artifact_prepared(ArtifactKind::Application).unwrap();
            host
        }
        fn value(&self) -> String {
            rusqlite::Connection::open(self.dir.path().join("app-state-v1.sqlite3")).unwrap()
                .query_row("SELECT v FROM setting", [], |row| row.get(0)).unwrap()
        }
    }

    #[test]
    fn ordinary_startup_reuses_state_without_committing_or_retaining_the_lock() {
        let f = Fixture::new();
        let (host, trial) = f.startup("1.0.0");
        assert!(!trial);
        let revision = host.coordinator.record().revision;
        drop(host);

        let root = f.dir.path().join("updates");
        let observer = rusqlite::Connection::open(root.join("state.sqlite")).unwrap();
        let before: u32 = observer.pragma_query_value(None, "data_version", |r| r.get(0)).unwrap();
        let (host, trial) = AndroidHost::open_and_startup(f.dir.path(), "1.0.0", f.config.clone(), "android-aarch64-apk", |_| panic!("ordinary startup must not migrate")).unwrap();
        assert!(!trial);
        assert_eq!(host.coordinator.record().revision, revision);
        let after: u32 = observer.pragma_query_value(None, "data_version", |r| r.get(0)).unwrap();
        assert_eq!(before, after, "ordinary startup must not commit update-state writes");
        assert!(UpdateLock::acquire(&root).is_ok(), "startup must release the lock before returning the host");
    }

    #[test]
    fn separate_startup_reloads_changes_since_host_creation() {
        let f = Fixture::new();
        let mut host = f.host("1.0.0");
        let mut other = f.host("1.0.0");
        other.coordinator.discovery_failed("changed in another session".into()).unwrap();
        assert!(!host.startup(|_| panic!("ordinary startup must not migrate")).unwrap());
        assert_eq!(host.coordinator.record().last_discovery_error.as_deref(), Some("changed in another session"));
    }

    #[test]
    fn consent_and_installed_apk_gate_data_activation() {
        let f = Fixture::new();
        let mut host = f.approved();
        assert_eq!(host.install_request().unwrap().version, "2.0.0");
        assert!(!host.startup(|_| panic!("same-session activation")).unwrap());
        drop(host);
        let mut host = f.host("1.0.0");
        assert!(!host.startup(|_| panic!("APK not installed")).unwrap());
        assert_eq!(host.view(), UpdateView::Ready { action: update_client::coordinator::UpdateAction::Install });
        drop(host);
        let (mut host, trial) = f.startup("2.0.0");
        assert!(trial);
        host.healthy().unwrap();
        assert!(host.coordinator.record().approved.is_none());
        assert_eq!(f.value(), "prior");
        drop(host);
        let (_, trial) = f.startup("2.0.0");
        assert!(!trial);
    }

    #[test]
    fn interrupted_trial_restores_database_and_taxonomy_before_next_startup() {
        let f = Fixture::new(); drop(f.approved());
        let (host, trial) = f.startup("2.0.0");
        assert!(trial);
        rusqlite::Connection::open(f.dir.path().join("app-state-v1.sqlite3")).unwrap()
            .execute_batch("UPDATE setting SET v='trial change'").unwrap();
        let root = f.dir.path().join("updates");
        let trial_taxonomy = ActiveTaxonomy { path: root.join("uncommitted-taxonomy.sqlite3"), release: 99 };
        activation::write_json(&root.join("active-taxonomy.json"), &Some(trial_taxonomy)).unwrap();
        drop(host);
        let (host, trial) = f.startup("2.0.0");
        assert!(!trial);
        assert_eq!(f.value(), "prior");
        assert!(active(&root).unwrap().is_none());
        assert_eq!(host.coordinator.record().approved.as_ref().unwrap().phase, Phase::Quarantined);
    }

    #[test]
    fn corrupt_staged_apk_retries_without_touching_live_data() {
        let f = Fixture::new(); let mut host = f.approved();
        let path = host.install_request().unwrap().path;
        fs::write(path, b"bad").unwrap();
        assert!(host.install_request().is_err()); drop(host);
        let mut host = f.host("2.0.0");
        assert!(!host.startup(|_| panic!("unverified APK")).unwrap());
        assert_eq!(host.coordinator.record().approved.as_ref().unwrap().phase, Phase::Preparing);
        assert_eq!(f.value(), "prior");
    }

    #[test]
    fn invalid_apk_is_quarantined_without_repeated_install_prompts() {
        let f = Fixture::new(); let mut host = f.approved();
        host.reject_package().unwrap();
        assert!(host.install_request().is_err());
        assert_eq!(host.view(), UpdateView::Hidden);
        assert_eq!(f.value(), "prior");
    }
}
