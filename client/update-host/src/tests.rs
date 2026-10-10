use super::*;
use ring::{
    digest::{digest, SHA256},
    signature::{Ed25519KeyPair, KeyPair},
};
use std::os::unix::fs::PermissionsExt;
use update_client::{ApplicationRelease, Artifact, Manifest, SignedManifest, TaxonomyRelease, SIGNATURE_CONTEXT};
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn artifact(url: &str, bytes: &[u8]) -> Artifact {
    Artifact { url: url.into(), bytes: bytes.len() as u64, sha256: hex(digest(&SHA256, bytes).as_ref()) }
}
fn approve(host: &mut Host, key: &Ed25519KeyPair, manifest: &Manifest, app: &[u8], taxonomy: &[u8]) {
    let payload = serde_json::to_string(manifest).unwrap();
    let mut message = SIGNATURE_CONTEXT.to_vec();
    message.extend_from_slice(payload.as_bytes());
    let bytes = serde_json::to_vec(&SignedManifest { key_id: "fixture".into(), signature: hex(key.sign(&message).as_ref()), payload }).unwrap();
    host.coordinator.discovered(bytes, now()).unwrap();
    host.approve().unwrap();
    // The UI must be able to report preparation before any download completes.
    assert_eq!(host.view(), UpdateView::Preparing);
    assert!(host.coordinator.record().approved.as_ref().unwrap().completed.is_empty());
    let staging = host.root.join("staging").join(&host.coordinator.record().approved.as_ref().unwrap().id);
    fs::create_dir_all(&staging).unwrap();
    fs::write(staging.join("application.bin"), app).unwrap();
    fs::write(staging.join("taxonomy.sqlite3"), taxonomy).unwrap();
    host.coordinator.artifact_prepared(ArtifactKind::Application).unwrap();
    host.coordinator.artifact_prepared(ArtifactKind::Taxonomy).unwrap();
}

#[test]
fn candidate_process() {
    if let Some(path) = std::env::var_os("BOKHEIM_FIXTURE_JOB") {
        prepare_candidate(Path::new(&path), "2.0.0").unwrap();
    }
}


#[test]
fn registry_backup_does_not_require_a_library_directory() {
    let temp = tempfile::tempdir().unwrap();
    let registry = temp.path().join("app-state-v1.sqlite3");
    rusqlite::Connection::open(&registry).unwrap().execute_batch("CREATE TABLE setting(v)").unwrap();
    let config = DiscoveryConfig { endpoint: "https://updates.example.org/v1/stable".into(), channel: "stable".into(), trusted_keys: [("test".into(), [1; 32])].into() };
    let host = Host::open(temp.path(), "1.0.0", config, None, std::env::current_exe().unwrap()).unwrap();
    assert_eq!(host.databases().unwrap(), vec![registry]);
}
