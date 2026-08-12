use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fmt;

pub const MEDIA_TYPE: &str = "application/vnd.bokheim.metadata+bincode; version=1";
pub const MEDIA_TYPE_BASE: &str = "application/vnd.bokheim.metadata+bincode";
pub const MAX_REQUEST_BYTES: usize = 64 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_ISBNS_PER_REQUEST: usize = 256;

const MAGIC: &[u8; 4] = b"BKMD";
const VERSION: u8 = 1;
const HEADER_LEN: usize = MAGIC.len() + 1;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ClassificationRequest {
    pub isbns: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ClassificationResponse {
    pub snapshot: SnapshotDescription,
    pub results: Vec<IsbnClassificationResult>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SnapshotDescription {
    pub dump_date: String,
    pub imported_at_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct IsbnClassificationResult {
    pub requested_isbn: String,
    pub canonical_isbn13: Option<String>,
    pub status: LookupStatus,
    pub matches: Vec<WorkClassificationMatch>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LookupStatus {
    Matched,
    NoMatch,
    InvalidIsbn,
    Ambiguous,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WorkClassificationMatch {
    pub open_library_work_id: Option<String>,
    pub exact_edition_ids: Vec<String>,
    pub classifications: Vec<Classification>,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Classification {
    pub scheme: ClassificationScheme,
    pub notation: String,
    pub source: ClassificationSource,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClassificationScheme {
    LibraryOfCongress,
    DeweyDecimal,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClassificationSource {
    ExactEdition,
    Work,
}

#[derive(Debug, Eq, PartialEq)]
pub enum WireError {
    InvalidHeader,
    UnsupportedVersion(u8),
    TooLarge { limit: usize },
    Encode(String),
    Decode(String),
}

impl fmt::Display for WireError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidHeader => formatter.write_str("invalid metadata wire header"),
            Self::UnsupportedVersion(version) => write!(formatter, "unsupported metadata wire version {version}"),
            Self::TooLarge { limit } => write!(formatter, "metadata body exceeds the {limit}-byte limit"),
            Self::Encode(reason) => write!(formatter, "could not encode metadata body: {reason}"),
            Self::Decode(reason) => write!(formatter, "could not decode metadata body: {reason}"),
        }
    }
}

impl std::error::Error for WireError {}

pub fn encode<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, WireError> {
    let payload = bincode::serde::encode_to_vec(value, bincode::config::standard()).map_err(|error| WireError::Encode(error.to_string()))?;
    let mut body = Vec::with_capacity(HEADER_LEN + payload.len());
    body.extend_from_slice(MAGIC);
    body.push(VERSION);
    body.extend_from_slice(&payload);
    Ok(body)
}

pub fn decode<T: DeserializeOwned>(body: &[u8], limit: usize) -> Result<T, WireError> {
    if body.len() > limit {
        return Err(WireError::TooLarge { limit });
    }
    if body.len() < HEADER_LEN || &body[..MAGIC.len()] != MAGIC {
        return Err(WireError::InvalidHeader);
    }
    if body[MAGIC.len()] != VERSION {
        return Err(WireError::UnsupportedVersion(body[MAGIC.len()]));
    }
    let (decoded, consumed) = bincode::serde::decode_from_slice(&body[HEADER_LEN..], bincode::config::standard()).map_err(|error| WireError::Decode(error.to_string()))?;
    if consumed != body.len() - HEADER_LEN {
        return Err(WireError::Decode("trailing bytes after metadata value".to_owned()));
    }
    Ok(decoded)
}

pub fn is_binary_media_type(value: &str) -> bool {
    let mut fields = value.split(';');
    if !fields.next().is_some_and(|value| value.trim().eq_ignore_ascii_case(MEDIA_TYPE_BASE)) {
        return false;
    }
    fields.any(|field| field.split_once('=').is_some_and(|(name, value)| name.trim().eq_ignore_ascii_case("version") && value.trim() == VERSION.to_string()))
}
