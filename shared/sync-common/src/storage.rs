//! Library storage selection and provider-owned file references.
//!
//! These values contain no credentials. A device resolves a provider account
//! to its own connection; metadata synchronization never authorizes file access.

use content_address::{BookId, ContentHash};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CloudProvider {
    GoogleDrive,
    Dropbox,
    OneDrive,
    WebDav,
    S3,
}

/// Provider IDs are opaque, case-sensitive values, never filesystem paths.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(try_from = "String", into = "String")]
pub struct ProviderId(String);

impl ProviderId {
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidStorage> {
        let value = value.into();
        if value.trim().is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
            return Err(InvalidStorage("provider ID must be nonempty, contain no control characters, and fit within 4096 bytes"));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str { &self.0 }
}

impl TryFrom<String> for ProviderId {
    type Error = InvalidStorage;
    fn try_from(value: String) -> Result<Self, Self::Error> { Self::new(value) }
}

impl From<ProviderId> for String {
    fn from(value: ProviderId) -> Self { value.0 }
}

/// Stable account and container scope shared across devices. `container_id`
/// identifies a OneDrive drive, Dropbox namespace, Nextcloud instance, or S3
/// bucket. Google Drive connections use the selected drive or `my_drive`.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub struct ProviderScope {
    pub provider: CloudProvider,
    pub account_id: ProviderId,
    pub container_id: ProviderId,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct CloudLibraryRoot {
    pub scope: ProviderScope,
    pub folder_id: ProviderId,
}

/// A library has exactly one authority for remote book and cover bytes.
/// Metadata/progress synchronization has its own account and scheduling.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LibraryStorage {
    Local,
    Bokheim,
    External(CloudLibraryRoot),
}

impl Default for LibraryStorage {
    fn default() -> Self { Self::Bokheim }
}

impl LibraryStorage {
    pub fn uses_bokheim_assets(&self) -> bool { matches!(self, Self::Bokheim) }
    pub fn requires_remote_upload(&self) -> bool { !matches!(self, Self::Local) }
    pub fn external(&self) -> Option<&CloudLibraryRoot> {
        match self { Self::External(root) => Some(root), _ => None }
    }
}

/// Identity survives rename/move when the provider supports stable IDs.
/// A revision is deliberately separate: replacing bytes must invalidate caches
/// without silently assigning existing reading state to another file.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub struct ProviderFileIdentity {
    pub scope: ProviderScope,
    pub file_id: ProviderId,
}

impl ProviderFileIdentity {
    pub fn book_id(&self) -> BookId {
        let provider = match self.scope.provider {
            CloudProvider::GoogleDrive => "google_drive",
            CloudProvider::Dropbox => "dropbox",
            CloudProvider::OneDrive => "one_drive",
            CloudProvider::WebDav => "web_dav",
            CloudProvider::S3 => "s3",
        };
        BookId::for_provider(provider, self.scope.account_id.as_str(), self.scope.container_id.as_str(), self.file_id.as_str())
    }
}

/// A complete file confirmed by the provider, either discovered by scanning or
/// returned after the final upload commit. Partial uploads cannot produce this
/// value. Provider revision tokens are not assumed to be content hashes.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ProviderFile {
    pub identity: ProviderFileIdentity,
    pub revision: ProviderId,
    pub size_bytes: u64,
    pub content_hash: Option<ContentHash>,
}

/// Publish the pair only after both provider uploads have committed. These
/// references never name Bokheim thumbnail endpoints.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ProviderThumbnails {
    pub full: ProviderFile,
    pub browse: ProviderFile,
}

impl ProviderFile {
    pub fn validate(&self) -> Result<(), InvalidStorage> {
        if self.size_bytes == 0 || self.size_bytes > 9_007_199_254_740_991 {
            return Err(InvalidStorage("provider file size is outside the supported range"));
        }
        Ok(())
    }
}

impl ProviderThumbnails {
    pub fn validate_for(&self, scope: &ProviderScope) -> Result<(), InvalidStorage> {
        for file in [&self.full, &self.browse] {
            file.validate()?;
            if &file.identity.scope != scope || file.size_bytes > crate::MAX_THUMBNAIL_BYTES {
                return Err(InvalidStorage("thumbnail must belong to the library's provider and fit its size limit"));
            }
        }
        if self.full.identity == self.browse.identity { return Err(InvalidStorage("thumbnail variants require distinct provider files")); }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidStorage(&'static str);

impl fmt::Display for InvalidStorage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result { formatter.write_str(self.0) }
}
impl std::error::Error for InvalidStorage {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_provider_ids_are_rejected_when_deserialized() {
        for value in ["", "  ", "file\nname"] {
            let json = serde_json::to_string(value).unwrap();
            assert!(serde_json::from_str::<ProviderId>(&json).is_err());
        }
        assert!(ProviderId::new("x".repeat(4097)).is_err());
    }

    #[test]
    fn external_storage_requires_upload_but_never_uses_bokheim_assets() {
        let storage = LibraryStorage::External(CloudLibraryRoot {
            scope: ProviderScope {
                provider: CloudProvider::GoogleDrive,
                account_id: ProviderId::new("account").unwrap(),
                container_id: ProviderId::new("my_drive").unwrap(),
            },
            folder_id: ProviderId::new("folder").unwrap(),
        });
        assert!(storage.requires_remote_upload());
        assert!(!storage.uses_bokheim_assets());
        assert_eq!(serde_json::from_slice::<LibraryStorage>(&serde_json::to_vec(&storage).unwrap()).unwrap(), storage);
    }
}
