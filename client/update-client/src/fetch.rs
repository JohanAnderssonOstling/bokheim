//! Async HTTP adapter, native and browser, for artifacts small enough to
//! verify in memory: the browser taxonomy and the catalog.
use crate::{verify, Artifact, DiscoveryConfig, VerifiedManifest, MAX_MANIFEST_BYTES};
use update_protocol::copy_verified;

pub async fn discover(config: &DiscoveryConfig, now_unix: u64, minimum_sequence: u64) -> Result<VerifiedManifest, String> {
    config.validate()?;
    let response = reqwest::Client::new().get(&config.endpoint).header("Accept", "application/json").send().await.and_then(|r| r.error_for_status()).map_err(|e| format!("update discovery failed: {e}"))?;
    let bytes = response.bytes().await.map_err(|e| format!("cannot read update metadata: {e}"))?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err("update manifest exceeds size limit".into());
    }
    verify(&bytes, config, now_unix, minimum_sequence)
}

pub async fn download(artifact: &Artifact) -> Result<Vec<u8>, String> {
    artifact.validate()?;
    let response = reqwest::get(&artifact.url).await.and_then(|r| r.error_for_status()).map_err(|e| format!("update download failed: {e}"))?;
    let bytes = response.bytes().await.map_err(|e| format!("cannot read update: {e}"))?;
    let mut verified = Vec::with_capacity(bytes.len());
    copy_verified(&bytes[..], &mut verified, artifact)?;
    Ok(verified)
}
