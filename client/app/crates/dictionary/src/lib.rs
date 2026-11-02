//! Device-local lookup using the original StarDict indexes and definition files.
//! Definitions are read directly from the original plain or dictzip body.
#[cfg(feature = "client")]
mod client;
#[cfg(feature = "client")]
mod import_source;
#[cfg(feature = "client")]
pub use import_source::{DictionaryImport, DictionarySource};
#[cfg(all(feature = "client", not(target_arch = "wasm32")))]
pub use import_source::{ImportReader, ImportReaderFuture};
#[cfg(feature = "client")]
mod model;
#[cfg(feature = "client")]
mod protocol;
mod stardict;
#[cfg(feature = "store")]
mod store;
#[cfg(feature = "client")]
pub use client::{Client, ClientFuture};
#[cfg(feature = "client")]
pub use model::{DictionaryDownloads, DictionaryInfo, DictionaryLookup, DictionaryMatches, DictionarySnapshot};
#[cfg(feature = "client")]
pub use protocol::{Request, Response};
#[cfg(feature = "store")]
pub use store::{DictionaryStore, LocalStorage, Storage};
#[cfg(feature = "store")]
mod service;
#[cfg(feature = "store")]
pub use service::LocalClient;

use std::future::Future;
use std::io::{Read, Seek, SeekFrom};

use serde::{Deserialize, Serialize};

pub use stardict::{Body, DICT_FILE, DICTZIP_FILE, Dictionary, DictionaryFile, IDX_FILE, IFO_FILE, SYN_FILE, prepare_import};

pub type Result<T> = std::result::Result<T, String>;

/// Random reads work with native readers and asynchronous browser file slices.
pub trait ReadAt {
    fn size(&mut self) -> impl Future<Output = Result<u64>>;
    fn read_at(&mut self, offset: u64, length: usize) -> impl Future<Output = Result<Vec<u8>>>;
}

impl<R: Read + Seek> ReadAt for R {
    async fn size(&mut self) -> Result<u64> {
        self.seek(SeekFrom::End(0)).map_err(|error| error.to_string())
    }

    async fn read_at(&mut self, offset: u64, length: usize) -> Result<Vec<u8>> {
        self.seek(SeekFrom::Start(offset)).map_err(|error| error.to_string())?;
        let mut bytes = vec![0; length];
        self.read_exact(&mut bytes).map_err(|error| error.to_string())?;
        Ok(bytes)
    }
}
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
