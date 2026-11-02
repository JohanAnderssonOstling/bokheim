//! Library sources and book storage policy.

use serde::{Deserialize, Serialize};
use url::Url;

/// How a library obtains its contents, independently of its book storage policy.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LibraryKind {
    #[default]
    Standard,
    Opds {
        url: Url,
    },
}

impl LibraryKind {
    pub fn is_opds(&self) -> bool {
        matches!(self, Self::Opds { .. })
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LibraryStorage {
    Local,
    #[default]
    Bokheim,
}

impl LibraryStorage {
    pub fn uses_bokheim_assets(&self) -> bool {
        matches!(self, Self::Bokheim)
    }
}

#[cfg(test)]
mod tests {
    use super::LibraryStorage;

    #[test]
    fn storage_policies_round_trip_and_control_uploads() {
        for (storage, upload) in [(LibraryStorage::Local, false), (LibraryStorage::Bokheim, true)] {
            let json = serde_json::to_vec(&storage).unwrap();
            assert_eq!(serde_json::from_slice::<LibraryStorage>(&json).unwrap(), storage);
            assert_eq!(storage.uses_bokheim_assets(), upload);
        }
    }

    #[test]
    fn unknown_storage_is_rejected() {
        assert!(serde_json::from_str::<LibraryStorage>(r#"{"external":{}}"#).is_err());
    }
}
