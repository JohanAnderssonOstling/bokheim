//! Update discovery, offer selection, and client update lifecycle.

use semver::Version;
use std::collections::BTreeMap;
pub use update_protocol::{decode_hex, validate_https_url, ApplicationRelease, Artifact, CatalogRelease, Manifest, SignedManifest, TaxonomyRelease, VerifiedManifest, MAX_MANIFEST_BYTES, SIGNATURE_CONTEXT};

#[cfg(all(feature = "blocking", not(target_arch = "wasm32")))]
pub mod blocking;

pub mod coordinator;

pub mod fetch;

#[cfg(all(feature = "native-state", any(unix, windows)))]
pub mod activation;
#[cfg(all(feature = "native-state", any(unix, windows)))]
pub mod native_files;
#[cfg(all(feature = "native-state", not(target_arch = "wasm32")))]
pub mod native_state;
#[cfg(all(feature = "native-state", any(unix, windows)))]
pub mod supervisor;
#[cfg(all(feature = "native-state", any(unix, windows)))]
pub mod windows_package;

pub const DEFAULT_ENDPOINT: &str = "https://updates.bokheim.se/v1/stable";

/// Trust comes from the host configuration, never from the downloaded envelope.
#[derive(Clone, Debug)]
pub struct DiscoveryConfig {
    pub endpoint: String,
    pub channel: String,
    pub trusted_keys: BTreeMap<String, [u8; 32]>,
}

impl DiscoveryConfig {
    pub fn validate(&self) -> Result<(), String> {
        validate_https_url(&self.endpoint)?;
        if self.channel.is_empty() || self.trusted_keys.is_empty() {
            return Err("update discovery requires a channel and trusted signing key".into());
        }
        Ok(())
    }

    /// Production builds inject a public key; no test or generated key is trusted
    /// by default. Only the endpoint may be overridden at runtime.
    pub fn bundled() -> Result<Self, String> {
        let key = option_env!("BOKHEIM_UPDATE_PUBLIC_KEY_HEX").ok_or("no update signing public key was bundled")?;
        let key_id = option_env!("BOKHEIM_UPDATE_KEY_ID").unwrap_or("release-1");
        let endpoint = option_env!("BOKHEIM_UPDATE_ENDPOINT").unwrap_or(DEFAULT_ENDPOINT).to_owned();
        let config = Self { endpoint, channel: "stable".into(), trusted_keys: [(key_id.into(), decode_hex(key)?)].into() };
        config.validate()?;
        Ok(config)
    }
}

pub struct Installed<'a> {
    pub application_version: &'a Version,
    pub schema_version: u32,
    pub taxonomy_format_versions: &'a [u32],
    pub taxonomy_release_id: u64,
    pub target: &'a str,
}

/// A snapshot of the offer. Persist the authenticated source envelope alongside
/// an approved plan so subsequent publications cannot silently change it.
#[derive(Debug)]
pub struct UpdateOffer<'a> {
    pub application: Option<(&'a ApplicationRelease, &'a Artifact)>,
    pub taxonomy: Option<&'a TaxonomyRelease>,
}

/// Select compatible updates from an authenticated publication for this device.
pub fn offer<'a>(verified: &'a VerifiedManifest, installed: &Installed<'_>) -> UpdateOffer<'a> {
    let manifest = verified.manifest();
    let application = manifest.application.as_ref().filter(|release| release.version > *installed.application_version).and_then(|release| release.artifacts.get(installed.target).map(|artifact| (release, artifact)));
    let (version, schema, formats) =
        application.map_or((installed.application_version, installed.schema_version, installed.taxonomy_format_versions), |(release, _)| (&release.version, release.schema_version, release.taxonomy_format_versions.as_slice()));
    let taxonomy = manifest
        .taxonomy
        .as_ref()
        .filter(|release| release.release_id > installed.taxonomy_release_id && version >= &release.minimum_application_version && schema >= release.minimum_schema_version && formats.contains(&release.format_version));
    UpdateOffer { application, taxonomy }
}

/// Verify fetched metadata using the client's configured endpoint, channel, and trust keys.
pub fn verify(bytes: &[u8], config: &DiscoveryConfig, now_unix: u64, minimum_sequence: u64) -> Result<VerifiedManifest, String> {
    config.validate()?;
    update_protocol::verify(bytes, &config.channel, &config.trusted_keys, now_unix, minimum_sequence)
}
