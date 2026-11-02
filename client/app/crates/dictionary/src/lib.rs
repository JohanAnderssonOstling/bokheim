//! Device-local lookup using the original StarDict indexes and definition files.
//! Definitions are read directly from the original plain or dictzip body.
#[cfg(feature = "client")]
mod client;
#[cfg(feature = "client")]
mod model;
mod stardict;
#[cfg(feature = "store")]
mod store;
#[cfg(feature = "client")]
pub use client::{Client, ClientFuture};
#[cfg(feature = "client")]
pub use model::{DictionaryDownloads, DictionaryInfo, DictionaryLookup, DictionaryMatches};
#[cfg(all(feature = "store", not(target_arch = "wasm32")))]
pub use store::native;
#[cfg(feature = "store")]
pub use store::{DictionaryStore, Storage};

use std::io::{Read, Seek, SeekFrom};

use serde::{Deserialize, Serialize};

pub use stardict::{Body, DICT_FILE, DICTZIP_FILE, Dictionary, DictionaryFile, IDX_FILE, IFO_FILE, SYN_FILE, prepare_import};

pub type Result<T> = std::result::Result<T, String>;
pub const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_IMPORT_BYTES: u64 = 512 * 1024 * 1024;
const MAX_INDEX_BYTES: u64 = 128 * 1024 * 1024;
const MAX_ENTRIES: usize = 2_000_000;
const MAX_ALIASES: usize = 4_000_000;
const MAX_ENTRY_BYTES: u32 = 4 * 1024 * 1024;
const MAX_QUERY_CHARACTERS: usize = 256;

pub fn can_lookup(query: &str) -> bool {
    let query = query.trim();
    !query.is_empty() && query.chars().take(MAX_QUERY_CHARACTERS + 1).count() <= MAX_QUERY_CHARACTERS
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Metadata {
    pub name: String,
    pub word_count: usize,
    pub description: String,
    pub author: String,
    pub website: String,
    pub language: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Definition {
    pub word: String,
    pub text: String,
}

fn bounded_read(reader: impl Read, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(limit + 1).read_to_end(&mut bytes).map_err(|error| error.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("Dictionary file exceeds the size limit".into());
    }
    Ok(bytes)
}

fn read_at<R: Read + Seek>(reader: &mut R, offset: u64, length: usize) -> Result<Vec<u8>> {
    reader.seek(SeekFrom::Start(offset)).map_err(|error| error.to_string())?;
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes).map_err(|error| error.to_string())?;
    Ok(bytes)
}
