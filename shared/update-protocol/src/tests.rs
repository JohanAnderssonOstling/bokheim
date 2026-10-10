use super::*;
use ring::signature::{Ed25519KeyPair, KeyPair};

fn fixture() -> (Ed25519KeyPair, BTreeMap<String, [u8; 32]>, Manifest) {
    // Fixed fixture key, never a release trust anchor.
    let key = Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap();
    let config = [("test".into(), key.public_key().as_ref().try_into().unwrap())].into();
    let artifact = Artifact { url: "https://downloads.example.org/arbitrary-name.bin".into(), bytes: 3, sha256: "00".repeat(32) };
    let manifest = Manifest {
        protocol_version: 1,
        channel: "stable".into(),
        sequence: 10,
        issued_at_unix: 100,
        expires_at_unix: 200,
        application: Some(ApplicationRelease {
            version: "2.0.0".parse().unwrap(),
            name: "Example release".into(),
            schema_version: 76,
            taxonomy_format_versions: vec![5],
            artifacts: [("android-aarch64-apk".into(), artifact.clone())].into(),
        }),
        taxonomy: Some(TaxonomyRelease { release_id: 9, format_version: 5, minimum_application_version: "2.0.0".parse().unwrap(), minimum_schema_version: 76, artifact }),
        catalog: None,
    };
    (key, config, manifest)
}
fn signed(key: &Ed25519KeyPair, manifest: &Manifest) -> Vec<u8> {
    let payload = serde_json::to_string(manifest).unwrap();
    let mut message = SIGNATURE_CONTEXT.to_vec();
    message.extend(payload.as_bytes());
    let signature = key.sign(&message).as_ref().iter().map(|b| format!("{b:02x}")).collect();
    serde_json::to_vec(&SignedManifest { key_id: "test".into(), payload, signature }).unwrap()
}

#[test]
fn tampering_unknown_keys_unsigned_and_oversized_metadata_fail_closed() {
    let (key, mut config, manifest) = fixture();
    let raw = signed(&key, &manifest);
    let mut envelope: SignedManifest = serde_json::from_slice(&raw).unwrap();
    envelope.payload = envelope.payload.replace("2.0.0", "3.0.0");
    assert!(verify(&serde_json::to_vec(&envelope).unwrap(), "stable", &config, 150, 0).is_err());
    assert!(verify(&serde_json::to_vec(&manifest).unwrap(), "stable", &config, 150, 0).is_err());
    assert!(verify(&vec![b' '; MAX_MANIFEST_BYTES + 1], "stable", &config, 150, 0).is_err());
    config.clear();
    assert!(verify(&raw, "stable", &config, 150, 0).is_err());
    config.insert("other".into(), [1; 32]);
    assert!(verify(&raw, "stable", &config, 150, 0).is_err());
}

#[test]
fn rejects_expired_future_replayed_and_wrong_protocol_metadata() {
    let (key, config, mut manifest) = fixture();
    let raw = signed(&key, &manifest);
    for (now, floor) in [(99, 0), (200, 0), (150, 11)] {
        assert!(verify(&raw, "stable", &config, now, floor).is_err());
    }
    manifest.protocol_version = 2;
    assert!(verify(&signed(&key, &manifest), "stable", &config, 150, 0).is_err());
    manifest.protocol_version = 1;
    manifest.channel = "beta".into();
    assert!(verify(&signed(&key, &manifest), "stable", &config, 150, 0).is_err());
}

#[test]
fn rejects_unsafe_urls_hashes_and_prereleases() {
    let (key, config, mut manifest) = fixture();
    for url in ["http://host/file", "file:///etc/passwd", "https://user:pass@host/file", "https://host/file#fragment"] {
        manifest.taxonomy.as_mut().unwrap().artifact.url = url.into();
        assert!(verify(&signed(&key, &manifest), "stable", &config, 150, 0).is_err());
    }
    manifest.taxonomy = None;
    manifest.application.as_mut().unwrap().version = "2.0.0-beta.1".parse().unwrap();
    assert!(verify(&signed(&key, &manifest), "stable", &config, 150, 0).is_err());
    manifest.application.as_mut().unwrap().version = "2.0.0".parse().unwrap();
    manifest.application.as_mut().unwrap().artifacts.values_mut().next().unwrap().sha256 = "bad".into();
    assert!(verify(&signed(&key, &manifest), "stable", &config, 150, 0).is_err());
}
