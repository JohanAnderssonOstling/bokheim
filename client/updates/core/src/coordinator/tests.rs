use super::*;
use crate::{ApplicationRelease, SIGNATURE_CONTEXT, SignedManifest, TaxonomyRelease};
use ring::signature::{Ed25519KeyPair, KeyPair};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

#[derive(Clone, Default)]
struct Memory(Rc<RefCell<Record>>);
impl Store for Memory {
    fn load(&self) -> Result<Record, UpdateError> {
        Ok(self.0.borrow().clone())
    }
    fn save(&mut self, expected: u64, next: &Record) -> Result<(), UpdateError> {
        if self.0.borrow().revision != expected {
            return Err("conflict".into());
        }
        *self.0.borrow_mut() = next.clone();
        Ok(())
    }
}
fn fixture() -> (DiscoveryConfig, DeviceVersion, Manifest) {
    let key = Ed25519KeyPair::from_seed_unchecked(&[9; 32]).unwrap();
    let config = DiscoveryConfig { endpoint: crate::DEFAULT_ENDPOINT.into(), channel: "stable".into(), trusted_keys: BTreeMap::from([("test".into(), key.public_key().as_ref().try_into().unwrap())]) };
    let device = DeviceVersion { application: "1.0.0".parse().unwrap(), schema: 75, taxonomy_formats: vec![5], taxonomy_release: 1, target: "linux-x86_64-appimage".into() };
    let artifact = Artifact { url: "https://files.example.org/update".into(), bytes: 3, sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into() };
    let manifest = Manifest {
        protocol_version: 1,
        channel: "stable".into(),
        sequence: 1,
        issued_at_unix: 100,
        expires_at_unix: 1000,
        application: Some(ApplicationRelease { version: "2.0.0".parse().unwrap(), name: "Release".into(), schema_version: 76, taxonomy_format_versions: vec![5], artifacts: BTreeMap::from([(device.target.clone(), artifact.clone())]) }),
        taxonomy: Some(TaxonomyRelease { release_id: 2, format_version: 5, minimum_application_version: "2.0.0".parse().unwrap(), minimum_schema_version: 76, artifact }),
        catalog: None,
    };
    (config, device, manifest)
}
fn signed(manifest: &Manifest) -> Vec<u8> {
    let payload = serde_json::to_string(manifest).unwrap();
    let key = Ed25519KeyPair::from_seed_unchecked(&[9; 32]).unwrap();
    let mut message = SIGNATURE_CONTEXT.to_vec();
    message.extend_from_slice(payload.as_bytes());
    let signature = key.sign(&message).as_ref().iter().map(|b| format!("{b:02x}")).collect();
    serde_json::to_vec(&SignedManifest { key_id: "test".into(), payload, signature }).unwrap()
}

#[test]
fn taxonomy_only_can_publish_in_the_background_without_waiting_for_another_launch() {
    let (config, device, mut manifest) = fixture();
    manifest.application = None;
    let taxonomy = manifest.taxonomy.as_mut().unwrap();
    taxonomy.minimum_application_version = device.application.clone();
    taxonomy.minimum_schema_version = device.schema;
    let mut coordinator = Coordinator::open(Memory::default(), config, device.clone(), "current".into(), ApplicationActivation::Restart).unwrap();
    coordinator.discovered(signed(&manifest), 150).unwrap();
    coordinator.approve(150).unwrap();
    assert!(coordinator.is_taxonomy_only());
    assert!(coordinator.begin_activation().is_err(), "unprepared references cannot activate");
    coordinator.artifact_prepared(ArtifactKind::Taxonomy).unwrap();
    assert!(!coordinator.activation_due());
    coordinator.begin_activation().unwrap();
    let mut prepared = device;
    prepared.taxonomy_release = 2;
    coordinator.activated(prepared).unwrap();
    assert!(coordinator.record().approved.is_none());
}

#[test]
fn discovery_failures_are_silent_and_approved_plan_is_pinned() {
    let (config, device, mut manifest) = fixture();
    let mut coordinator = Coordinator::open(Memory::default(), config, device, "first".into(), ApplicationActivation::Restart).unwrap();
    coordinator.discovery_failed("offline".into()).unwrap();
    assert_eq!(coordinator.view(100), UpdateView::Hidden);
    coordinator.discovered(signed(&manifest), 150).unwrap();
    coordinator.approve(150).unwrap();
    let pinned = coordinator.record().approved.as_ref().unwrap().id.clone();
    manifest.sequence = 2;
    manifest.application.as_mut().unwrap().version = "3.0.0".parse().unwrap();
    coordinator.discovered(signed(&manifest), 160).unwrap();
    assert_eq!(coordinator.record().approved.as_ref().unwrap().id, pinned);
    assert_eq!(coordinator.approved_manifest().unwrap().application.unwrap().version.to_string(), "2.0.0");
    coordinator.preparation_failed(WaitReason::Retry, "temporary failure".into()).unwrap();
    assert_eq!(coordinator.view(160), UpdateView::Preparing);
    assert!(coordinator.needs_preparation());
    coordinator.artifact_prepared(ArtifactKind::Application).unwrap();
    assert!(coordinator.record().approved.as_ref().unwrap().last_error.is_none());
}

#[test]
fn expired_offer_cannot_be_approved_but_existing_authorization_survives_expiry() {
    let (config, device, manifest) = fixture();
    let mut coordinator = Coordinator::open(Memory::default(), config.clone(), device.clone(), "first".into(), ApplicationActivation::Restart).unwrap();
    coordinator.discovered(signed(&manifest), 150).unwrap();
    assert_eq!(coordinator.view(1001), UpdateView::Hidden);
    assert!(coordinator.approve(1001).is_err());
    coordinator.approve(150).unwrap();
    assert!(coordinator.approved_manifest().is_ok());
    assert_eq!(coordinator.view(1001), UpdateView::Preparing);
}

#[test]
fn rejects_stale_writers_and_equal_sequence_mutation() {
    let (config, device, mut manifest) = fixture();
    let store = Memory::default();
    let mut a = Coordinator::open(store.clone(), config.clone(), device.clone(), "a".into(), ApplicationActivation::Restart).unwrap();
    let mut b = Coordinator::open(store, config, device, "b".into(), ApplicationActivation::Restart).unwrap();
    a.discovered(signed(&manifest), 150).unwrap();
    assert!(b.discovered(signed(&manifest), 150).is_err());
    b.reload().unwrap();
    b.approve(150).unwrap();
    assert!(a.approve(150).is_err());
    a.reload().unwrap();
    manifest.expires_at_unix += 10;
    assert!(a.discovered(signed(&manifest), 151).is_err());
}

#[cfg(all(feature = "native-state", not(target_arch = "wasm32")))]
#[test]
fn sqlite_state_and_verified_staging_survive_restart_without_network() {
    use crate::native_state::{SqliteStore, UpdateLock};
    let root = tempfile::tempdir().unwrap();
    let (config, device, manifest) = fixture();
    let mut a = Coordinator::open(SqliteStore::open(root.path()).unwrap(), config.clone(), device.clone(), "a".into(), ApplicationActivation::Restart).unwrap();
    a.discovered(signed(&manifest), 150).unwrap();
    a.approve(150).unwrap();
    let id = a.record().approved.as_ref().unwrap().id.clone();
    let folder = root.path().join("staging").join(id);
    std::fs::create_dir_all(&folder).unwrap();
    for kind in [ArtifactKind::Application, ArtifactKind::Taxonomy] {
        std::fs::write(folder.join(kind.filename()), b"abc").unwrap();
    }
    let lock = UpdateLock::acquire(root.path()).unwrap();
    assert!(UpdateLock::acquire(root.path()).is_err());
    let client = crate::blocking::client("test").unwrap();
    a.prepare(root.path(), &lock, &client, 0, 0).unwrap();
    assert_eq!(a.view(150), UpdateView::Ready { action: UpdateAction::Restart });
    assert!(!a.activation_due());
    assert_eq!(a.record().approved.as_ref().unwrap().completed.len(), 2);
    drop(a);
    let mut b = Coordinator::open(SqliteStore::open(root.path()).unwrap(), config.clone(), device.clone(), "b".into(), ApplicationActivation::Restart).unwrap();
    b.prepare(root.path(), &lock, &client, 0, 0).unwrap();
    assert!(b.activation_due());
    assert_eq!(b.staged_paths(root.path(), &lock).unwrap().len(), 2);
    std::fs::write(folder.join(ArtifactKind::Taxonomy.filename()), b"bad").unwrap();
    assert!(b.staged_paths(root.path(), &lock).is_err());
    assert_eq!(b.view(170), UpdateView::Preparing);
    assert!(!b.activation_due());
}

#[cfg(all(feature = "native-state", not(target_arch = "wasm32")))]
#[test]
fn preparation_stops_at_a_blocker_and_resumes_on_the_next_launch() {
    use crate::native_state::{SqliteStore, UpdateLock};

    let root = tempfile::tempdir().unwrap();
    let (config, device, manifest) = fixture();
    let mut first = Coordinator::open(SqliteStore::open(root.path()).unwrap(), config.clone(), device.clone(), "first".into(), ApplicationActivation::Restart).unwrap();
    first.discovered(signed(&manifest), 150).unwrap();
    first.approve(150).unwrap();
    let folder = root.path().join("staging").join(&first.record().approved.as_ref().unwrap().id);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join(ArtifactKind::Application.filename()), b"abc").unwrap();
    let lock = UpdateLock::acquire(root.path()).unwrap();
    let client = crate::blocking::client("test").unwrap();
    first.prepare(root.path(), &lock, &client, u64::MAX - 6, 0).unwrap();
    assert!(matches!(first.view(150), UpdateView::NeedsSpace { .. }));
    assert_eq!(first.record().approved.as_ref().unwrap().completed, vec![ArtifactKind::Application]);
    assert!(!folder.join(ArtifactKind::Taxonomy.filename()).exists());
    drop(first);

    std::fs::write(folder.join(ArtifactKind::Taxonomy.filename()), b"abc").unwrap();
    let mut next = Coordinator::open(SqliteStore::open(root.path()).unwrap(), config, device, "next".into(), ApplicationActivation::Restart).unwrap();
    // Old retry deadlines must not block startup or an explicit retry.
    let mut record = next.record().clone();
    next.commit(record).unwrap();
    next.prepare(root.path(), &lock, &client, 0, 0).unwrap();
    assert_eq!(next.view(151), UpdateView::Ready { action: UpdateAction::Restart });
    assert!(!next.activation_due());
}

#[test]
fn quarantined_release_requires_corrected_bytes_and_new_approval() {
    let (config, device, mut manifest) = fixture();
    let mut coordinator = Coordinator::open(Memory::default(), config, device, "a".into(), ApplicationActivation::Restart).unwrap();
    coordinator.discovered(signed(&manifest), 150).unwrap();
    coordinator.approve(150).unwrap();
    coordinator.quarantine_after_recovery("invalid package; old version retained".into()).unwrap();
    assert_eq!(coordinator.view(150), UpdateView::Hidden);
    manifest.sequence += 1;
    coordinator.discovered(signed(&manifest), 160).unwrap();
    assert_eq!(coordinator.view(160), UpdateView::Hidden);
    manifest.sequence += 1;
    manifest.application.as_mut().unwrap().artifacts.values_mut().next().unwrap().sha256 = "11".repeat(32);
    coordinator.discovered(signed(&manifest), 170).unwrap();
    assert_eq!(coordinator.view(170), UpdateView::Available);
    assert!(!coordinator.needs_preparation());
}

#[test]
fn modified_durable_plan_cannot_redirect_an_approved_download() {
    let (config, device, manifest) = fixture();
    let store = Memory::default();
    let mut coordinator = Coordinator::open(store.clone(), config, device, "a".into(), ApplicationActivation::Restart).unwrap();
    coordinator.discovered(signed(&manifest), 150).unwrap();
    coordinator.approve(150).unwrap();
    store.0.borrow_mut().approved.as_mut().unwrap().artifacts[0].artifact.url = "https://different.example.org/file".into();
    coordinator.reload().unwrap();
    assert!(coordinator.approved_manifest().is_err());
    assert!(coordinator.artifact_prepared(ArtifactKind::Application).is_err());
}

#[test]
fn adding_a_rotation_key_preserves_approval_and_replay_floor() {
    let (mut config, device, manifest) = fixture();
    let store = Memory::default();
    let mut a = Coordinator::open(store.clone(), config.clone(), device.clone(), "a".into(), ApplicationActivation::Restart).unwrap();
    a.discovered(signed(&manifest), 150).unwrap();
    a.approve(150).unwrap();
    let id = a.record().approved.as_ref().unwrap().id.clone();
    config.trusted_keys.insert("next-key".into(), [8; 32]);
    let b = Coordinator::open(store.clone(), config.clone(), device.clone(), "b".into(), ApplicationActivation::Restart).unwrap();
    assert_eq!(b.record().highest_sequence, manifest.sequence);
    assert_eq!(b.record().approved.as_ref().unwrap().id, id);
    config.trusted_keys.remove("test");
    assert!(Coordinator::open(store, config, device, "c".into(), ApplicationActivation::Restart).is_err());
}
