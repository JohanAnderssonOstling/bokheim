//! Native HTTP adapter; the protocol itself has no transport dependency.
use crate::{verify, Artifact, DiscoveryConfig, VerifiedManifest, MAX_MANIFEST_BYTES};
use reqwest::blocking::Client;
use std::io::{Read, Write};
use std::time::Duration;
use update_protocol::copy_verified;

pub fn client(user_agent: &str) -> Result<Client, String> {
    Client::builder()
        .user_agent(user_agent)
        .https_only(true)
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|e| format!("cannot initialize update HTTP client: {e}"))
}

pub fn fetch_manifest(client: &Client, config: &DiscoveryConfig) -> Result<Vec<u8>, String> {
    config.validate()?;
    let response = client.get(&config.endpoint).header("Accept", "application/json").send().and_then(|r| r.error_for_status()).map_err(|e| format!("update discovery failed: {e}"))?;
    let mut bytes = Vec::new();
    response.take((MAX_MANIFEST_BYTES + 1) as u64).read_to_end(&mut bytes).map_err(|e| format!("cannot read update metadata: {e}"))?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err("update manifest exceeds size limit".into());
    }
    Ok(bytes)
}

pub fn discover(client: &Client, config: &DiscoveryConfig, now_unix: u64, minimum_sequence: u64) -> Result<VerifiedManifest, String> {
    verify(&fetch_manifest(client, config)?, config, now_unix, minimum_sequence)
}

/// Caller owns a staging file and removes it on error. Never writes more than
/// the authenticated length, even if a server streams an unbounded response.
pub fn download(client: &Client, artifact: &Artifact, output: &mut impl Write) -> Result<(), String> {
    artifact.validate()?;
    // Packages can be hundreds of megabytes; discovery's short request budget
    // would cause a slow connection to restart the same download indefinitely.
    let response = client.get(&artifact.url).timeout(Duration::from_secs(30 * 60)).send().and_then(|r| r.error_for_status()).map_err(|e| format!("update download failed: {e}"))?;
    copy_verified(response, output, artifact)
}
