//! Native file observation shared by ingestion and local reader opening.
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::{fs, time::UNIX_EPOCH};
pub fn file_metadata_fingerprint(metadata: &fs::Metadata) -> std::io::Result<u64> {
    let modified = metadata.modified()?.duration_since(UNIX_EPOCH).map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(&modified.as_nanos().to_le_bytes());
    hasher.update(&metadata.len().to_le_bytes());
    #[cfg(unix)]
    {
        hasher.update(&metadata.ino().to_le_bytes());
        hasher.update(&metadata.dev().to_le_bytes());
    }
    Ok(u64::from_le_bytes(hasher.finalize().as_bytes()[..8].try_into().expect("BLAKE3 digest contains eight bytes")))
}
