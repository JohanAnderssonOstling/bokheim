//! One immutable taxonomy selection contract for native and browser storage.
//! Storage adapters persist this record atomically after staging the referenced image.
use crate::{Artifact, TaxonomyRelease, UpdateError};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::path::Path;
use subject_projection::{installed, runtime_format};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Reference {
    pub path: String,
    pub release: u64,
    pub format: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<Artifact>,
}
/// Current and previous immutable snapshots selected by the update coordinator.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Selection {
    pub current: Reference,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous: Option<Reference>,
}

impl Reference {
    pub fn validate(&self) -> Result<(), UpdateError> {
        installed::validate(Path::new(&self.path), (self.release, self.format))
    }
}

impl Selection {
    pub fn new(current: Reference) -> Self {
        Self { current, previous: None }
    }

    /// A broken current image recovers to the last compatible image before workers open.
    pub fn recover(&self) -> Result<Self, UpdateError> {
        if self.current.validate().is_ok() {
            return Ok(self.clone());
        }
        let previous = self.previous.as_ref().ok_or("no valid previous taxonomy image")?;
        previous.validate()?;
        Ok(Self::new(previous.clone()))
    }

    pub fn stage(&self, next: Reference) -> Result<Self, UpdateError> {
        next.validate()?;
        if next.release <= self.current.release {
            return Err("taxonomy release must increase".into());
        }
        Ok(Self { current: next, previous: Some(self.current.clone()) })
    }
}

pub fn compatible(release: &TaxonomyRelease, application: &Version, schema: u32, formats: &[u32]) -> bool {
    formats.contains(&release.format_version) && application >= &release.minimum_application_version && schema >= release.minimum_schema_version
}

pub fn offered(release: &TaxonomyRelease, installed: u64, application: &Version, schema: u32) -> bool {
    release.release_id > installed && compatible(release, application, schema, runtime_format::DOWNLOAD_FORMATS)
}

pub fn validate_download(image: &[u8], release: &TaxonomyRelease) -> Result<(), UpdateError> {
    installed::validate_image(image, (release.release_id, release.format_version))
}
