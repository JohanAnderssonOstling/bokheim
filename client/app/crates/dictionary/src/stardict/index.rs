//! In-memory record offsets for the original StarDict indexes. Nothing is serialized.
use std::{
    cmp::Ordering,
    io::{Read, Seek},
};

use super::{Body, SourceMetadata, integer, string_at, text};
use crate::{Definition, MAX_ENTRY_BYTES, MAX_INDEX_BYTES, Result, read_at};

pub const IFO_FILE: &str = "dictionary.ifo";
pub const IDX_FILE: &str = "dictionary.idx";
pub const SYN_FILE: &str = "dictionary.syn";
pub const DICT_FILE: &str = "dictionary.dict";
pub const DICTZIP_FILE: &str = "dictionary.dict.dz";

// StarDict indexes are sorted by ASCII-insensitive comparison, with a raw
// spelling tie-break. Ignoring that tie-break finds all case variants together.
fn compare(left: &str, right: &str) -> Ordering {
    left.bytes().map(|byte| byte.to_ascii_lowercase()).cmp(right.bytes().map(|byte| byte.to_ascii_lowercase()))
}

/// Record boundaries in the original StarDict order.
struct Index {
    offsets: Vec<u32>,
}
impl Index {
    fn parse(bytes: &[u8], count: usize, offset64: Option<bool>, limit: u64) -> Result<Self> {
        if bytes.len() as u64 > MAX_INDEX_BYTES {
            return Err("Dictionary index is too large".into());
        }
        let mut offsets = Vec::with_capacity(count + 1);
        let mut cursor = 0;
        let mut previous = "";
        while cursor < bytes.len() {
            if offsets.len() == count {
                return Err("Too many dictionary index entries".into());
            }
            offsets.push(cursor as u32);
            let word = string_at(bytes, &mut cursor)?;
            if compare(previous, word).is_gt() {
                return Err("Dictionary index is not in StarDict order".into());
            }
            previous = word;
            let offset = integer(bytes, &mut cursor, if offset64 == Some(true) { 8 } else { 4 })?;
            if offset64.is_some() {
                let size = integer(bytes, &mut cursor, 4)?;
                if size > u64::from(MAX_ENTRY_BYTES) || offset.checked_add(size).is_none_or(|end| end > limit) {
                    return Err("Dictionary entry points outside its definition file or is too large".into());
                }
            } else if offset >= limit {
                return Err("Dictionary alias points outside its index".into());
            }
        }
        if offsets.len() != count {
            return Err("Dictionary entry count does not match its metadata".into());
        }
        offsets.push(bytes.len() as u32);
        Ok(Self { offsets })
    }
    fn record(&self, reader: &mut (impl Read + Seek), ordinal: usize) -> Result<Vec<u8>> {
        let start = *self.offsets.get(ordinal).ok_or("Dictionary alias points outside its index")?;
        let end = *self.offsets.get(ordinal + 1).ok_or("Dictionary alias points outside its index")?;
        read_at(reader, u64::from(start), (end - start) as usize)
    }
    fn matches(&self, reader: &mut (impl Read + Seek), query: &str, synonyms: bool) -> Result<Vec<usize>> {
        let count = self.offsets.len() - 1;
        let (mut low, mut high) = (0, count);
        while low < high {
            let middle = low + (high - low) / 2;
            let bytes = self.record(reader, middle)?;
            let mut cursor = 0;
            if compare(string_at(&bytes, &mut cursor)?, query).is_lt() {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        let mut found = Vec::new();
        for ordinal in low..count {
            let bytes = self.record(reader, ordinal)?;
            let mut cursor = 0;
            if compare(string_at(&bytes, &mut cursor)?, query).is_ne() {
                break;
            }
            let target = if synonyms { integer(&bytes, &mut cursor, 4)? as usize } else { ordinal };
            if !found.contains(&target) {
                found.push(target);
            }
            if found.len() == 32 {
                break;
            }
        }
        Ok(found)
    }
}

/// Cached offsets only; each lookup reads headwords, aliases and definitions
/// from the original files supplied by the caller.
pub struct Dictionary {
    metadata: SourceMetadata,
    words: Index,
    synonyms: Option<Index>,
}
impl Dictionary {
    pub fn open(ifo: &[u8], index: &[u8], synonyms: Option<&[u8]>, body_size: u64) -> Result<Self> {
        let metadata = SourceMetadata::parse(ifo)?;
        if index.len() != metadata.index_size {
            return Err("Dictionary index size does not match its metadata".into());
        }
        let words = Index::parse(index, metadata.info.word_count, Some(metadata.offset64), body_size)?;
        if metadata.synonym_count > 0 && synonyms.is_none() {
            return Err("Dictionary is missing its .syn file".into());
        }
        let synonyms = synonyms.map(|bytes| Index::parse(bytes, metadata.synonym_count, None, metadata.info.word_count as u64)).transpose()?;
        Ok(Self { metadata, words, synonyms })
    }
    pub(crate) fn validate(ifo: &[u8], index: &[u8], synonyms: Option<&[u8]>, body_size: u64) -> Result<crate::Metadata> {
        Ok(Self::open(ifo, index, synonyms, body_size)?.metadata.info)
    }
    pub fn lookup<R: Read + Seek, D: Read + Seek>(&self, query: &str, index: &mut R, synonyms: Option<&mut R>, body: &mut Body<D>) -> Result<Vec<Definition>> {
        let query = query.trim();
        let mut entries = self.words.matches(index, query, false)?;
        if let Some(table) = &self.synonyms {
            entries.extend(table.matches(synonyms.ok_or("Dictionary is missing its .syn file")?, query, true)?);
        }
        entries.sort_unstable();
        entries.dedup();
        let mut definitions = Vec::new();
        for entry in entries.into_iter().take(32) {
            let bytes = self.words.record(index, entry)?;
            let mut cursor = 0;
            let word = string_at(&bytes, &mut cursor)?.to_owned();
            let offset = integer(&bytes, &mut cursor, if self.metadata.offset64 { 8 } else { 4 })?;
            let size = integer(&bytes, &mut cursor, 4)? as u32;
            let text = text::definition(&body.read_entry(offset, size)?, &self.metadata.types)?;
            if !text.is_empty() {
                definitions.push(Definition { word, text });
            }
        }
        Ok(definitions)
    }
}
