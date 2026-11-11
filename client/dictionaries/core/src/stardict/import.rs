use std::collections::BTreeMap;
use std::io::{self, Cursor, Read};

use flate2::read::GzDecoder;

use serde::{Deserialize, Serialize};

use super::{Body, Dictionary};
use crate::{MAX_FILE_BYTES, MAX_IMPORT_BYTES, MAX_INDEX_BYTES, Metadata, Result, bounded_read};

const COMPONENTS: [&str; 6] = [".ifo", ".idx", ".idx.gz", ".dict", ".dict.dz", ".syn"];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DictionaryFile {
    pub name: String,
    #[serde(with = "serde_bytes")]
    pub bytes: Vec<u8>,
}

/// Canonical component names are chosen here; archive paths are never used as
/// filesystem paths. One import must contain exactly one complete dictionary.
pub fn prepare_import(files: Vec<DictionaryFile>) -> Result<(Metadata, Vec<DictionaryFile>)> {
    let mut expanded = Vec::new();
    let mut total = 0u64;
    for file in files {
        if file.bytes.len() as u64 > MAX_FILE_BYTES {
            return Err("Dictionary file exceeds 256 MiB".into());
        }
        if file.name.to_ascii_lowercase().ends_with(".zip") {
            let mut archive = zip::ZipArchive::new(Cursor::new(file.bytes)).map_err(|error| crate::OperationError::from_error(&error))?;
            if archive.len() > 1024 {
                return Err("Dictionary archive contains too many files".into());
            }
            for index in 0..archive.len() {
                let entry = archive.by_index(index).map_err(|error| crate::OperationError::from_error(&error))?;
                if entry.is_dir() || !component(entry.name()) {
                    continue;
                }
                if entry.size() > MAX_FILE_BYTES {
                    return Err("Dictionary file exceeds 256 MiB".into());
                }
                let name = entry.name().to_owned();
                let bytes = bounded_read(entry, MAX_FILE_BYTES.min(MAX_IMPORT_BYTES.saturating_sub(total)))?;
                total += bytes.len() as u64;
                expanded.push(DictionaryFile { name, bytes });
            }
        } else if component(&file.name) {
            total += file.bytes.len() as u64;
            expanded.push(file);
        }
        if total > MAX_IMPORT_BYTES {
            return Err("Dictionary import exceeds 512 MiB".into());
        }
    }
    let mut bases = expanded.iter().filter_map(|file| file.name.strip_suffix(".ifo"));
    let base = match (bases.next(), bases.next()) {
        (Some(base), None) => base.to_owned(),
        _ => return Err("Select one dictionary's .ifo, .idx and .dict files together, or a ZIP containing them".into()),
    };
    let parent = base.rsplit_once('/').map(|(parent, _)| parent).unwrap_or("");
    let mut selected = BTreeMap::new();
    for file in expanded {
        let (folder, leaf) = file.name.rsplit_once('/').unwrap_or(("", &file.name));
        if folder == parent {
            if let Some(suffix) = notice(leaf) {
                if file.bytes.len() > 128 * 1024 {
                    return Err("Dictionary attribution file is too large".into());
                }
                selected.insert(suffix.to_owned(), file.bytes);
                continue;
            }
        }
        let Some(suffix) = file.name.strip_prefix(&base).filter(|suffix| COMPONENTS.contains(suffix)) else {
            continue;
        };
        if selected.insert(suffix.to_owned(), file.bytes).is_some() {
            return Err("Duplicate dictionary component".into());
        }
    }
    for (plain, compressed) in [(".idx", ".idx.gz"), (".dict", ".dict.dz")] {
        if selected.contains_key(plain) && selected.contains_key(compressed) {
            return Err(format!("Select either {plain} or {compressed}, rather than both").into());
        }
    }
    if let Some(compressed) = selected.remove(".idx.gz") {
        selected.insert(".idx".into(), bounded_read(GzDecoder::new(compressed.as_slice()), MAX_INDEX_BYTES)?);
    }
    let ifo = selected.get(".ifo").ok_or("Missing .ifo file")?;
    let compressed = selected.contains_key(".dict.dz");
    let body = selected.get(".dict.dz").or_else(|| selected.get(".dict")).ok_or("Missing matching .dict or .dict.dz file")?;
    if compressed {
        let limit = 1024 * 1024 * 1024;
        let size = io::copy(&mut GzDecoder::new(body.as_slice()).take(limit + 1), &mut io::sink()).map_err(|error| crate::OperationError::from(error).context("Damaged dictzip definition file"))?;
        if size > limit {
            return Err("Expanded dictionary exceeds 1 GiB".into());
        }
    }
    let index = selected.get(".idx").ok_or("Missing matching .idx or .idx.gz file")?;
    let mut metadata = Dictionary::validate(ifo, index, selected.get(".syn").map(Vec::as_slice), futures_lite::future::block_on(Body::open(Cursor::new(body), compressed))?.len())?;
    for suffix in [".license.txt", ".attribution.txt"] {
        if let Some(bytes) = selected.get(suffix) {
            let text = std::str::from_utf8(bytes).map_err(|_| "Dictionary attribution is not UTF-8")?;
            metadata.description.push_str("\n\n");
            metadata.description.push_str(text);
        }
    }
    if selected.values().map(|bytes| bytes.len() as u64).sum::<u64>() > MAX_IMPORT_BYTES {
        return Err("Dictionary import exceeds 512 MiB".into());
    }
    let files = selected.into_iter().map(|(suffix, bytes)| DictionaryFile { name: format!("dictionary{suffix}"), bytes }).collect();
    Ok((metadata, files))
}

fn component(name: &str) -> bool {
    COMPONENTS.iter().any(|suffix| name.ends_with(suffix)) || notice(name.rsplit('/').next().unwrap_or(name)).is_some()
}

fn notice(name: &str) -> Option<&'static str> {
    match name.to_ascii_lowercase().as_str() {
        "license" | "license.txt" | "copying" | "copying.txt" | "dictionary.license.txt" => Some(".license.txt"),
        "attribution" | "attribution.txt" | "dictionary.attribution.txt" => Some(".attribution.txt"),
        _ => None,
    }
}
