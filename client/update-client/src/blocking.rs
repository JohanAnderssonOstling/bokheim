//! Native HTTP adapter; the protocol itself has no transport dependency.
use crate::{Artifact, DiscoveryConfig, MAX_MANIFEST_BYTES, VerifiedManifest, verify};
use reqwest::blocking::Client;
use std::path::Path;
use std::time::Duration;
use update_protocol::stage_verified;

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
    update_files::read_bounded(response, MAX_MANIFEST_BYTES).map_err(|e| format!("cannot read update metadata: {e}"))
}

pub fn discover(client: &Client, config: &DiscoveryConfig, now_unix: u64, minimum_sequence: u64) -> Result<VerifiedManifest, String> {
    verify(&fetch_manifest(client, config)?, config, now_unix, minimum_sequence)
}

/// Publish a verified staging file, removing partial output on failure.
pub fn download(client: &Client, artifact: &Artifact, destination: &Path) -> Result<(), String> {
    artifact.validate()?;
    // Native artifacts belong to a durably approved user update, even when
    // preparation resumes after a restart. Automatic catalog updates use fetch.
    let response = client.get(&artifact.url).timeout(Duration::from_secs(30 * 60)).send().and_then(|r| r.error_for_status()).map_err(|error| format!("update download failed: {error}"))?;
    stage_verified(response, destination, &destination.with_extension("part"), artifact, |_, _| {})
}
