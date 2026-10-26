//! One durable lifecycle for native hosts; policies supply package differences.
use super::*;
use std::ops::Deref;
use update_client::blocking;

pub trait Platform: Sized {
    fn target(&self) -> String;

    fn workspace(&self, host: &NativeHost<Self>) -> Result<u64, String> {
        databases::workspace(&host.data)
    }

    fn preparation_wait(&self, _host: &NativeHost<Self>, _workspace: u64) -> Result<Option<WaitReason>, String> {
        Ok(None)
    }
}

pub struct NativeHost<P> {
    pub root: PathBuf,
    pub coordinator: Coordinator<SqliteStore>,
    pub(crate) data: PathBuf,
    pub(crate) version: String,
    pub(crate) trial: bool,
    pub(crate) platform: P,
}

impl<P> Deref for NativeHost<P> {
    type Target = P;
    fn deref(&self) -> &P {
        &self.platform
    }
}

impl<P: Platform> NativeHost<P> {
    pub(crate) fn open_platform(data: &Path, version: &str, config: DiscoveryConfig, platform: P) -> Result<Self, String> {
        Self::open_for_startup(data, version, config, platform).map(|(host, _, _)| host)
    }

    pub(crate) fn open_for_startup(data: &Path, version: &str, config: DiscoveryConfig, platform: P) -> Result<(Self, UpdateLock, Option<ActiveTaxonomy>), String> {
        let root = data.join("updates");
        let lock = UpdateLock::acquire(&root)?;
        let taxonomy = active(&root)?;
        let mut installed = device(version, taxonomy.as_ref())?;
        installed.target = platform.target();
        let session = format!("{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos());
        let coordinator = Coordinator::open(SqliteStore::open(&root)?, config, installed, session)?;
        Ok((Self { root, data: data.into(), version: version.into(), coordinator, trial: false, platform }, lock, taxonomy))
    }

    pub(crate) fn installed(&self) -> Result<DeviceVersion, String> {
        self.installed_with_taxonomy(active(&self.root)?.as_ref())
    }

    pub(crate) fn installed_with_taxonomy(&self, taxonomy: Option<&ActiveTaxonomy>) -> Result<DeviceVersion, String> {
        let mut installed = device(&self.version, taxonomy)?;
        installed.target = self.platform.target();
        Ok(installed)
    }

    pub fn is_trial(&self) -> bool {
        self.trial
    }

    pub(crate) fn accept_trial(&mut self, journal: &Journal, trial: Option<&str>) -> Result<bool, String> {
        if journal.state != State::Trial || trial != Some(journal.plan_id.as_str()) || journal.next_version != self.version {
            return Ok(false);
        }
        self.coordinator.approved_manifest()?;
        if !self.coordinator.record().approved.as_ref().is_some_and(|plan| plan.id == journal.plan_id && plan.phase == Phase::Activating) {
            return Err("startup trial does not match approved activation".into());
        }
        self.trial = true;
        Ok(true)
    }

    /// A committed receipt remains available until the supervisor observes it.
    /// Bookkeeping errors after commit must never take down a usable app.
    pub fn healthy(&mut self) -> Result<(), String> {
        if !self.trial {
            return Ok(());
        }
        let _lock = UpdateLock::acquire(&self.root)?;
        let mut journal = Journal::load(&self.root)?.ok_or("missing update startup journal")?;
        let installed = self.installed()?;
        journal.commit(&self.root)?;
        if let Err(error) = self.coordinator.activated(installed) {
            log::warn!("committed update acknowledgement will retry: {error}");
        }
        self.trial = false;
        Ok(())
    }

    pub fn approve(&mut self) -> Result<(), String> {
        let _lock = self.reload_locked()?;
        self.coordinator.approve(now())
    }

    pub fn view(&self) -> UpdateView {
        self.coordinator.view(now())
    }

    pub fn check_for_updates(&mut self) -> Result<(), String> {
        let lock = self.reload_locked()?;
        self.acknowledge_committed()?;
        self.coordinator.check_for_updates(&self.root, &lock, &blocking::client(concat!("Bokheim-update/", env!("CARGO_PKG_VERSION")))?, now())
    }

    pub fn prepare(&mut self) -> Result<UpdateView, String> {
        let lock = self.reload_locked()?;
        if self.coordinator.needs_preparation() {
            let workspace = self.workspace_bytes()?;
            if let Some(reason) = self.platform.preparation_wait(self, workspace)? {
                self.coordinator.preparation_failed(now(), reason, "update needs storage or installation permission".into())?;
                return Ok(self.view());
            }
            let client = blocking::client(concat!("Bokheim-update/", env!("CARGO_PKG_VERSION")))?;
            if let Err(error) = self.coordinator.prepare(&self.root, &lock, &client, now(), workspace, 0) {
                self.coordinator.preparation_failed(now(), WaitReason::Retry, error.clone())?;
                return Err(error);
            }
        }
        Ok(self.view())
    }

    pub(crate) fn reload_locked(&mut self) -> Result<UpdateLock, String> {
        let lock = UpdateLock::acquire(&self.root)?;
        self.coordinator.reload()?;
        Ok(lock)
    }

    pub(crate) fn acknowledge(&mut self, journal: &Journal) -> Result<(), String> {
        if self.coordinator.record().approved.as_ref().is_some_and(|plan| plan.id == journal.plan_id) {
            self.coordinator.activated(self.installed()?)?;
        }
        Ok(())
    }

    pub(crate) fn acknowledge_committed(&mut self) -> Result<(), String> {
        if let Some(journal) = Journal::load(&self.root)? {
            if journal.state == State::Committed {
                self.acknowledge(&journal)?;
            }
        }
        Ok(())
    }

    pub(crate) fn recover(&mut self, journal: &Journal, reason: String) -> Result<(), String> {
        if journal.state == State::Committed {
            return Ok(());
        }
        journal.recover(&self.root)?;
        if self.coordinator.record().approved.as_ref().is_some_and(|plan| plan.id == journal.plan_id) {
            self.coordinator.quarantine_after_recovery(reason)?;
        }
        Journal::clear(&self.root)
    }

    pub(crate) fn failed_activation(&mut self, reason: String) -> Result<(), String> {
        if let Some(journal) = Journal::load(&self.root)? {
            self.recover(&journal, reason)
        } else {
            match self.coordinator.record().approved.as_ref().map(|plan| &plan.phase) {
                Some(Phase::Activating) => self.coordinator.quarantine_after_recovery(reason),
                Some(Phase::Ready) => self.coordinator.activation_recovered_for_retry(now(), WaitReason::Retry, reason),
                _ => Ok(()),
            }
        }
    }

    pub(crate) fn finish_committed(&mut self, journal: &Journal) -> Result<(), String> {
        self.acknowledge(journal)?;
        Journal::clear(&self.root)?;
        self.cleanup_committed()
    }

    pub(crate) fn workspace_bytes(&self) -> Result<u64, String> {
        self.platform.workspace(self)
    }

    pub(crate) fn data_workspace(&self) -> Result<u64, String> {
        let mut bytes = databases::workspace(&self.data)?;
        if let Some(plan) = &self.coordinator.record().approved {
            for artifact in &plan.artifacts {
                bytes = bytes.checked_add(artifact.artifact.bytes).ok_or("workspace overflow")?;
            }
        }
        Ok(bytes)
    }

    pub(crate) fn select_taxonomy(&self) -> Result<(), String> {
        activate_taxonomy(active(&self.root)?.as_ref())
    }

    pub(crate) fn cleanup_committed(&self) -> Result<(), String> {
        let active = active(&self.root)?.map(|taxonomy| taxonomy.path);
        let pending = self.coordinator.record().approved.as_ref().map(|plan| plan.id.as_str());
        for directory in ["generations", "staging"] {
            let parent = self.root.join(directory);
            if !parent.try_exists().map_err(|error| error.to_string())? {
                continue;
            }
            for entry in fs::read_dir(parent).map_err(|error| error.to_string())? {
                let entry = entry.map_err(|error| error.to_string())?;
                let name = entry.file_name();
                if !entry.file_type().map_err(|error| error.to_string())?.is_dir() || pending == name.to_str() || update_client::decode_hex::<32>(&name.to_string_lossy()).is_err() {
                    continue;
                }
                let path = entry.path();
                if active.as_ref().and_then(|path| path.parent()) == Some(path.as_path()) {
                    for file in fs::read_dir(path).map_err(|error| error.to_string())? {
                        let file = file.map_err(|error| error.to_string())?;
                        if active.as_ref() == Some(&file.path()) {
                            continue;
                        }
                        if file.file_type().map_err(|error| error.to_string())?.is_dir() { fs::remove_dir_all(file.path()) } else { fs::remove_file(file.path()) }.map_err(|error| error.to_string())?;
                    }
                } else {
                    fs::remove_dir_all(path).map_err(|error| error.to_string())?;
                }
            }
        }
        Ok(())
    }
}

pub(crate) fn activate_taxonomy(taxonomy: Option<&ActiveTaxonomy>) -> Result<(), String> {
    if let Some(taxonomy) = taxonomy {
        subject_projection::installed::activate(&taxonomy.path)?;
    }
    Ok(())
}
