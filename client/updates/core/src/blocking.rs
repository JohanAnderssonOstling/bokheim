//! Native HTTP adapter; the protocol itself has no transport dependency.
use crate::UpdateError;
use crate::{Artifact, DiscoveryConfig, MAX_MANIFEST_BYTES, VerifiedManifest, verify};
use reqwest::blocking::Client;
use std::path::Path;
use std::time::Duration;
use update_protocol::stage_verified;

pub fn client(user_agent: &str) -> Result<Client, UpdateError> {
    http_response::blocking_client_builder()
        .user_agent(user_agent)
        .https_only(true)
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|e| UpdateError::operation(e).context("cannot initialize update HTTP client"))
}

pub fn fetch_manifest(client: &Client, config: &DiscoveryConfig) -> Result<Vec<u8>, UpdateError> {
    config.validate()?;
    let response = client.get(&config.endpoint).header("Accept", "application/json").send().and_then(|r| r.error_for_status()).map_err(|e| UpdateError::operation(e).context("update discovery failed"))?;
    update_files::read_bounded(response, MAX_MANIFEST_BYTES).map_err(|e| UpdateError::operation(e).context("cannot read update metadata"))
}

pub fn discover(client: &Client, config: &DiscoveryConfig, now_unix: u64, minimum_sequence: u64) -> Result<VerifiedManifest, UpdateError> {
    verify(&fetch_manifest(client, config)?, config, now_unix, minimum_sequence)
}

/// Publish a verified staging file, removing partial output on failure.
pub fn download(client: &Client, artifact: &Artifact, destination: &Path) -> Result<(), UpdateError> {
    artifact.validate()?;
    // Native artifacts belong to a durably approved user update, even when
    // preparation resumes after a restart. Automatic catalog updates use fetch.
    let response = client.get(&artifact.url).timeout(Duration::from_secs(30 * 60)).send().and_then(|r| r.error_for_status()).map_err(|error| UpdateError::operation(error).context("update download failed"))?;
    stage_verified(response, destination, &destination.with_extension("part"), artifact, |_, _| {})
}
