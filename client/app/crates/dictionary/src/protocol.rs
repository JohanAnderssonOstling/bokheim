//! Dictionary lifecycle requests and replies shared by clients and the store.
use crate::{DictionaryDownloads, DictionaryInfo, DictionarySnapshot, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Request {
    List,
    Snapshot { languages: Vec<String> },
    Downloads { languages: Vec<String> },
    Download { id: String },
    Configure { id: String, enabled: bool, direction: i8 },
    Remove { id: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Response {
    Dictionaries(Vec<DictionaryInfo>),
    Snapshot(DictionarySnapshot),
    Downloads(DictionaryDownloads),
}

impl Response {
    pub(crate) fn into_dictionaries(self) -> Result<Vec<DictionaryInfo>> {
        match self {
            Self::Dictionaries(value) => Ok(value),
            _ => Err("Unexpected dictionary response".into()),
        }
    }

    pub(crate) fn into_snapshot(self) -> Result<DictionarySnapshot> {
        match self {
            Self::Snapshot(value) => Ok(value),
            _ => Err("Unexpected dictionary response".into()),
        }
    }

    pub(crate) fn into_downloads(self) -> Result<DictionaryDownloads> {
        match self {
            Self::Downloads(value) => Ok(value),
            _ => Err("Unexpected dictionary response".into()),
        }
    }
}
