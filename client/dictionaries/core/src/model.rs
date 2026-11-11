//! Values shared by dictionary clients and storage.
use crate::{Definition, Metadata, OperationError};
use dictionary_catalog::CuratedDictionary;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DictionaryInfo {
    #[serde(default)]
    pub curated_id: Option<String>,
    pub id: String,
    pub metadata: Metadata,
    pub enabled: bool,
    pub size_bytes: u64,
    pub files: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DictionaryMatches {
    pub dictionary: String,
    pub definitions: Vec<Definition>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DictionaryLookup {
    pub dictionaries: Vec<DictionaryMatches>,
    pub enabled_count: usize,
    pub errors: Vec<OperationError>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DictionaryDownloads {
    pub available: Vec<CuratedDictionary>,
    pub recommended: Vec<String>,
    pub active: Option<String>,
    pub error: Option<OperationError>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DictionarySnapshot {
    pub dictionaries: Vec<DictionaryInfo>,
    pub downloads: DictionaryDownloads,
}
