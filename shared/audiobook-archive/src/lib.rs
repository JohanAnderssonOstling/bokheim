//! The audiobook transport archive: an ordinary ZIP of stored MP3 entries in
//! playback order, followed by optional NFO, CUE and cover sidecars. Clients
//! write it; the server reads its layout to serve each track by byte range.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

pub const MAX_TRACKS: usize = 1024;
pub const MAX_NFO_BYTES: u64 = 256 * 1024;
pub const MAX_CUE_BYTES: u64 = 256 * 1024;
pub const MAX_COVER_BYTES: u64 = 20 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Track {
    pub name: String,
    pub path: PathBuf,
}

/// A single metadata sidecar at the root of an audiobook folder.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Nfo {
    pub name: String,
    pub path: PathBuf,
}

pub type Cue = Nfo;
pub type Cover = Nfo;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchivedTrack {
    pub name: String,
    pub offset: u64,
    pub length: u64,
}

pub fn audio_file(path: &Path) -> bool {
    path.extension().and_then(|extension| extension.to_str()).is_some_and(|extension| extension.eq_ignore_ascii_case("mp3"))
}

/// Hashes all archive bytes without interpreting an embedded identity.
pub fn archive_hash<R: Read + Seek>(mut reader: R) -> io::Result<blake3::Hash> {
    reader.rewind()?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hasher.finalize())
}

/// Writes an ordinary ZIP with uncompressed MP3 entries. Stored entries can
/// be served directly by byte range without decoding the archive on the server.
pub fn write_archive<W: Write + Seek>(writer: W, tracks: &[Track]) -> io::Result<W> {
    write_archive_with_sidecars(writer, tracks, None, None, None)
}

pub fn write_archive_with_sidecars<W: Write + Seek>(writer: W, tracks: &[Track], nfo: Option<&Nfo>, cue: Option<&Cue>, cover: Option<&Cover>) -> io::Result<W> {
    if tracks.is_empty() || tracks.len() > MAX_TRACKS {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "audiobook has no MP3 tracks"));
    }
    let mut zip = ZipWriter::new(writer);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let mut names = HashSet::new();
    for track in tracks {
        if !valid_entry_name(&track.name) || !names.insert(track.name.to_lowercase()) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid or duplicate audiobook track name"));
        }
        zip.start_file(&track.name, options).map_err(io::Error::other)?;
        io::copy(&mut File::open(&track.path)?, &mut zip)?;
    }
    for (sidecar, maximum, kind) in [(nfo, MAX_NFO_BYTES, SidecarKind::Nfo), (cue, MAX_CUE_BYTES, SidecarKind::Cue), (cover, MAX_COVER_BYTES, SidecarKind::Cover)] {
        if let Some(sidecar) = sidecar {
            if sidecar_kind(&sidecar.name) != Some(kind) || !names.insert(sidecar.name.to_lowercase()) || fs::metadata(&sidecar.path)?.len() > maximum {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid or oversized audiobook sidecar"));
            }
            zip.start_file(&sidecar.name, options).map_err(io::Error::other)?;
            let size = io::copy(&mut File::open(&sidecar.path)?.take(maximum + 1), &mut zip)?;
            if size > maximum {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "audiobook sidecar changed while archiving"));
            }
        }
    }
    zip.finish().map_err(io::Error::other)
}

pub fn valid_sidecar_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('.') && !name.contains('/') && !name.contains('\\') && !name.contains(':') && !name.chars().any(char::is_control)
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
enum SidecarKind {
    Nfo,
    Cue,
    Cover,
}

fn sidecar_kind(name: &str) -> Option<SidecarKind> {
    if !valid_sidecar_name(name) {
        return None;
    }
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".nfo") {
        Some(SidecarKind::Nfo)
    } else if lower.ends_with(".cue") {
        Some(SidecarKind::Cue)
    } else if ["cover.jpg", "cover.jpeg", "cover.png", "cover.webp", "folder.jpg", "folder.jpeg", "folder.png", "folder.webp"].contains(&lower.as_str()) {
        Some(SidecarKind::Cover)
    } else {
        None
    }
}

fn sidecar_limit(kind: SidecarKind) -> u64 {
    match kind {
        SidecarKind::Nfo => MAX_NFO_BYTES,
        SidecarKind::Cue => MAX_CUE_BYTES,
        SidecarKind::Cover => MAX_COVER_BYTES,
    }
}

pub fn archived_cover<R: Read + Seek>(reader: R) -> io::Result<Option<Vec<u8>>> {
    let mut archive = ZipArchive::new(reader).map_err(io::Error::other)?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(io::Error::other)?;
        if sidecar_kind(entry.name()) != Some(SidecarKind::Cover) {
            continue;
        }
        if entry.size() > MAX_COVER_BYTES {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "oversized audiobook cover"));
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        return Ok(Some(bytes));
    }
    Ok(None)
}

fn valid_entry_name(name: &str) -> bool {
    !name.is_empty() && name.split('/').all(|part| !part.is_empty() && part != "." && part != ".." && !part.contains('\\') && !part.contains(':') && !part.chars().any(char::is_control)) && audio_file(Path::new(name))
}

/// Reads the byte positions needed to serve each MP3 as a separate resource.
pub fn archived_tracks<R: Read + Seek>(reader: R) -> io::Result<Vec<ArchivedTrack>> {
    let mut archive = ZipArchive::new(reader).map_err(io::Error::other)?;
    if archive.len() > MAX_TRACKS + 3 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "audiobook ZIP has too many entries"));
    }
    let mut tracks = Vec::with_capacity(archive.len());
    let mut names = HashSet::new();
    let mut sidecars = HashSet::new();
    let mut in_sidecars = false;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(io::Error::other)?;
        let name = entry.name().to_owned();
        if !names.insert(name.to_lowercase()) || entry.compression() != CompressionMethod::Stored {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid audiobook ZIP entry"));
        }
        if let Some(kind) = sidecar_kind(&name) {
            in_sidecars = true;
            if !sidecars.insert(kind) || entry.size() > sidecar_limit(kind) {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "duplicate or oversized audiobook sidecar"));
            }
            continue;
        }
        if in_sidecars {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "audio track follows audiobook sidecar"));
        }
        if !valid_entry_name(&name) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid audiobook ZIP entry"));
        }
        let offset = entry.data_start().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "audiobook ZIP entry has no byte offset"))?;
        tracks.push(ArchivedTrack { name, offset, length: entry.size() });
    }
    if tracks.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "audiobook ZIP has no MP3 tracks"));
    }
    Ok(tracks)
}

/// Extracts tracks into a caller-owned staging directory. The caller publishes
/// that directory only after this function returns successfully.
pub fn extract_archive<R: Read + Seek>(reader: R, destination: &Path) -> io::Result<Vec<String>> {
    let mut archive = ZipArchive::new(reader).map_err(io::Error::other)?;
    if archive.len() > MAX_TRACKS + 3 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "audiobook ZIP has too many entries"));
    }
    let mut names = Vec::with_capacity(archive.len());
    let mut seen = HashSet::new();
    let mut sidecars = HashSet::new();
    let mut in_sidecars = false;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(io::Error::other)?;
        let name = entry.name().to_owned();
        let sidecar = sidecar_kind(&name);
        if !(valid_entry_name(&name) || sidecar.is_some()) || !seen.insert(name.to_lowercase()) || entry.compression() != CompressionMethod::Stored || sidecar.is_some_and(|kind| entry.size() > sidecar_limit(kind) || !sidecars.insert(kind))
        {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid audiobook ZIP entry"));
        }
        if sidecar.is_some() {
            in_sidecars = true;
        } else if in_sidecars {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "audio track follows audiobook sidecar"));
        }
        let output = destination.join(&name);
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        io::copy(&mut entry, &mut File::create(output)?)?;
        names.push(name);
    }
    if !names.iter().any(|name| name.to_ascii_lowercase().ends_with(".mp3")) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "audiobook ZIP has no MP3 tracks"));
    }
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_archive_entries_outside_the_album() {
        assert!(!valid_entry_name("C:/escape.mp3"));
        let mut zip = ZipWriter::new(io::Cursor::new(Vec::new()));
        zip.start_file("../escape.mp3", SimpleFileOptions::default()).unwrap();
        zip.write_all(b"x").unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        let output = tempfile::tempdir().unwrap();
        assert!(extract_archive(io::Cursor::new(bytes), output.path()).is_err());
        assert!(!output.path().parent().unwrap().join("escape.mp3").exists());
    }
}
