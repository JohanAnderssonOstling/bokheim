//! StarDict import, metadata, indexes, and definition decoding.
use std::collections::HashMap;

use crate::{MAX_ALIASES, MAX_ENTRIES, MAX_INDEX_BYTES, Metadata, Result};

mod dictzip;
mod import;
mod index;
mod text;
pub use dictzip::Body;
pub use import::{DictionaryFile, prepare_import};
pub use index::{DICT_FILE, DICTZIP_FILE, Dictionary, IDX_FILE, IFO_FILE, SYN_FILE};

#[cfg(test)]
mod tests;

pub(crate) struct SourceMetadata {
    pub(crate) info: Metadata,
    pub(crate) index_size: usize,
    pub(crate) offset64: bool,
    pub(crate) types: String,
    pub(crate) synonym_count: usize,
}

impl SourceMetadata {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > 1024 * 1024 {
            return Err("Dictionary metadata is too large".into());
        }
        let text = std::str::from_utf8(bytes).map_err(|_| "Dictionary metadata is not UTF-8")?;
        let mut lines = text.trim_start_matches('\u{feff}').lines();
        if lines.next() != Some("StarDict's dict ifo file") {
            return Err("This is not a StarDict .ifo file".into());
        }
        let fields: HashMap<_, _> = lines.filter_map(|line| line.split_once('=')).collect();
        if !matches!(fields.get("version"), Some(&"2.4.2" | &"3.0.0")) {
            return Err("Supported StarDict versions are 2.4.2 and 3.0.0".into());
        }
        let number = |name| fields.get(name).ok_or_else(|| format!("Dictionary metadata is missing {name}"))?.parse::<usize>().map_err(|_| format!("Invalid {name}"));
        let word_count = number("wordcount")?;
        if word_count == 0 || word_count > MAX_ENTRIES {
            return Err("Dictionary word count is outside the supported range".into());
        }
        let offset64 = match fields.get("idxoffsetbits").copied().unwrap_or("32") {
            "32" => false,
            "64" if fields.get("version") == Some(&"3.0.0") => true,
            _ => return Err("Invalid StarDict index offset width".into()),
        };
        let name = fields.get("bookname").filter(|name| !name.trim().is_empty()).ok_or("Dictionary has no name")?.to_string();
        let types = fields.get("sametypesequence").copied().unwrap_or("").to_owned();
        if !types.bytes().all(|byte| byte.is_ascii_alphabetic()) {
            return Err("Invalid dictionary entry types".into());
        }
        let synonym_count = fields.get("synwordcount").map(|value| value.parse::<usize>().map_err(|_| "Invalid synonym count")).transpose()?.unwrap_or(0);
        if synonym_count > MAX_ALIASES {
            return Err("Too many dictionary aliases".into());
        }
        let index_size = number("idxfilesize")?;
        if index_size as u64 > MAX_INDEX_BYTES {
            return Err("Dictionary index is too large".into());
        }
        Ok(Self {
            info: Metadata {
                name,
                word_count,
                description: text::html_text(fields.get("description").copied().unwrap_or("")),
                author: fields.get("author").copied().unwrap_or("").to_owned(),
                website: fields.get("website").copied().unwrap_or("").to_owned(),
                language: fields.get("idxlang").or_else(|| fields.get("lang")).filter(|value| !value.trim().is_empty()).map(|value| value.to_string()),
            },
            index_size,
            offset64,
            types,
            synonym_count,
        })
    }
}

pub(crate) fn string_at<'a>(bytes: &'a [u8], cursor: &mut usize) -> Result<&'a str> {
    let rest = bytes.get(*cursor..).ok_or("Truncated dictionary index")?;
    let end = rest.iter().position(|byte| *byte == 0).ok_or("Unterminated dictionary headword")?;
    if end == 0 || end > 4096 {
        return Err("Invalid dictionary headword length".into());
    }
    let word = std::str::from_utf8(&rest[..end]).map_err(|_| "Dictionary headword is not UTF-8")?;
    *cursor += end + 1;
    Ok(word)
}

pub(crate) fn integer(bytes: &[u8], cursor: &mut usize, width: usize) -> Result<u64> {
    let value = bytes.get(*cursor..*cursor + width).ok_or("Truncated dictionary index")?;
    *cursor += width;
    Ok(value.iter().fold(0, |value, byte| value << 8 | u64::from(*byte)))
}
