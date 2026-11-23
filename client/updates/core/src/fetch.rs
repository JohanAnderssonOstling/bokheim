//! Async HTTP adapter, native and browser, for artifacts small enough to
//! verify in memory: the browser taxonomy and the catalog.
use crate::UpdateError;
use crate::{Artifact, DiscoveryConfig, MAX_MANIFEST_BYTES, VerifiedManifest, verify};
use update_protocol::copy_verified;

pub async fn discover(config: &DiscoveryConfig, now_unix: u64, minimum_sequence: u64) -> Result<VerifiedManifest, UpdateError> {
    verify(&manifest_bytes(config).await?, config, now_unix, minimum_sequence)
}

/// HTTP adapter only: the shared coordinator authenticates and pins these bytes.
pub async fn manifest_bytes(config: &DiscoveryConfig) -> Result<Vec<u8>, UpdateError> {
    config.validate()?;
    let response = http_response::client().get(&config.endpoint).header("Accept", "application/json").send().await.and_then(|r| r.error_for_status()).map_err(|e| UpdateError::operation(e).context("update discovery failed"))?;
    let bytes = response.bytes().await.map_err(|e| UpdateError::operation(e).context("cannot read update metadata"))?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err("update manifest exceeds size limit".into());
    }
    Ok(bytes.to_vec())
}

pub async fn download(artifact: &Artifact) -> Result<Vec<u8>, UpdateError> {
    artifact.validate()?;
    // Native reqwest and browser fetch decode HTTP compression before exposing
    // the body, so the signed size and digest describe the original artifact.
    let response = http_response::get(&artifact.url).await.and_then(|r| r.error_for_status()).map_err(|e| UpdateError::operation(e).context("update download failed"))?;
    let bytes = response.bytes().await.map_err(|e| UpdateError::operation(e).context("cannot read update"))?;
    let mut verified = Vec::with_capacity(bytes.len());
    copy_verified(&bytes[..], &mut verified, artifact)?;
    Ok(verified)
}
