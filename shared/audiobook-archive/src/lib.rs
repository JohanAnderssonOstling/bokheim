//! The audiobook transport archive: an ordinary ZIP of stored MP3 entries in
//! playback order, followed by optional NFO, CUE and cover sidecars. Clients
//! write it; the server reads its layout to serve each track by byte range.

use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, Cursor, Read, Seek, Write};
use std::path::{Path, PathBuf};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, System, ZipArchive, ZipWriter};

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

/// Header fields that differ between platforms but participate in content identity.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntryFormat {
    pub name: String,
    pub system: u8,
    pub modified: [u16; 6],
    pub permissions: u32,
}

impl EntryFormat {
    pub fn is_native_default(&self) -> bool {
        self.system == if cfg!(windows) { 0 } else { 3 } && self.modified == [1980, 1, 1, 0, 0, 0] && self.permissions == 0o100644
    }

    fn options(&self) -> io::Result<SimpleFileOptions> {
        let [year, month, day, hour, minute, second] = self.modified;
        let component = |value: u16| u8::try_from(value).map_err(io::Error::other);
        let modified = DateTime::from_date_and_time(year, component(month)?, component(day)?, component(hour)?, component(minute)?, component(second)?).map_err(io::Error::other)?;
        Ok(SimpleFileOptions::default().compression_method(CompressionMethod::Stored).system(System::from(self.system)).last_modified_time(modified).unix_permissions(self.permissions))
    }
}

/// Read transport header metadata before extracting, including the ZIP origin
/// OS byte, which ZipFile's portable file options do not currently expose.
pub fn entry_formats<R: Read + Seek>(reader: &mut R) -> io::Result<Vec<EntryFormat>> {
    use std::io::SeekFrom;
    let mut headers = Vec::new();
    {
        let mut archive = ZipArchive::new(&mut *reader).map_err(io::Error::other)?;
        if archive.len() > MAX_TRACKS + 3 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "audiobook ZIP has too many entries"));
        }
        for index in 0..archive.len() {
            let file = archive.by_index(index).map_err(io::Error::other)?;
            let modified = file.last_modified().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid audiobook ZIP timestamp"))?;
            headers.push((
                file.central_header_start(),
                EntryFormat {
                    name: file.name().to_owned(),
                    system: 0,
                    modified: [modified.year(), modified.month().into(), modified.day().into(), modified.hour().into(), modified.minute().into(), modified.second().into()],
                    permissions: file.unix_mode().unwrap_or(0o100644),
                },
            ));
        }
    }
    for (offset, format) in &mut headers {
        reader.seek(SeekFrom::Start(offset.checked_add(4).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "ZIP offset overflow"))?))?;
        let mut version = [0; 2];
        reader.read_exact(&mut version)?;
        format.system = version[1];
    }
    Ok(headers.into_iter().map(|(_, format)| format).collect())
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
    write_archive_with_formats(writer, tracks, nfo, cue, cover, &[])
}

/// Rebuild a materialized archive with its original entry order and platform metadata.
pub fn write_archive_with_formats<W: Write + Seek>(writer: W, tracks: &[Track], nfo: Option<&Nfo>, cue: Option<&Cue>, cover: Option<&Cover>, formats: &[EntryFormat]) -> io::Result<W> {
    if tracks.is_empty() || tracks.len() > MAX_TRACKS {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "audiobook has no MP3 tracks"));
    }
    let mut zip = ZipWriter::new(writer);
    let options = |name: &str| formats.iter().find(|format| format.name == name).map_or_else(|| Ok(SimpleFileOptions::default().compression_method(CompressionMethod::Stored)), EntryFormat::options);
    let mut names = HashSet::new();
    let mut entries = Vec::new();
    for track in tracks {
        if !valid_entry_name(&track.name) || !names.insert(track.name.to_lowercase()) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid or duplicate audiobook track name"));
        }
        entries.push((&track.name, &track.path, None));
    }
    for (sidecar, maximum, kind) in [(nfo, MAX_NFO_BYTES, SidecarKind::Nfo), (cue, MAX_CUE_BYTES, SidecarKind::Cue), (cover, MAX_COVER_BYTES, SidecarKind::Cover)] {
        if let Some(sidecar) = sidecar {
            if sidecar_kind(&sidecar.name) != Some(kind) || !names.insert(sidecar.name.to_lowercase()) || fs::metadata(&sidecar.path)?.len() > maximum {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid or oversized audiobook sidecar"));
            }
            entries.push((&sidecar.name, &sidecar.path, Some(maximum)));
        }
    }
    entries.sort_by_key(|(name, _, maximum)| (maximum.is_some(), formats.iter().position(|format| &format.name == *name).unwrap_or(formats.len())));
    for (name, path, maximum) in entries {
        zip.start_file(name, options(name)?).map_err(io::Error::other)?;
        let mut file = File::open(path)?;
        if let Some(maximum) = maximum {
            if io::copy(&mut file.take(maximum + 1), &mut zip)? > maximum {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "audiobook sidecar changed while archiving"));
            }
        } else {
            io::copy(&mut file, &mut zip)?;
        }
    }
    zip.finish().map_err(io::Error::other)
}

/// Repacks an ordinary ZIP of MP3s, such as a source site's download, into
/// the transport archive: its MP3s in natural filename order, then `cover`.
/// A supplied JPEG is saved separately as `cover.jpg`. Every MP3 is copied
/// byte for byte, preserving audio, tags, embedded artwork and chapters.
/// Folders are flattened; NFO and CUE sidecars are retained. CUE file paths
/// follow the flattened tracks. Other files are left out.
pub fn repack_zip<R: Read + Seek>(reader: R, cover: Option<&[u8]>) -> io::Result<Vec<u8>> {
    if cover.is_some_and(|bytes| bytes.len() as u64 > MAX_COVER_BYTES || !bytes.starts_with(&[0xff, 0xd8])) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "the replacement cover must be a bounded JPEG"));
    }
    let mut source = ZipArchive::new(reader).map_err(io::Error::other)?;
    let mut tracks = Vec::new();
    let mut sidecars = Vec::new();
    let mut sidecar_kinds = HashSet::new();
    let mut source_cover = None;
    for index in 0..source.len() {
        let entry = source.by_index(index).map_err(io::Error::other)?;
        let name = entry.name().rsplit('/').next().unwrap_or_default().to_owned();
        if entry.is_file() && !name.starts_with('.') && valid_entry_name(&name) {
            tracks.push((name, index));
        } else if entry.is_file() {
            match sidecar_kind(&name) {
                Some(kind @ (SidecarKind::Nfo | SidecarKind::Cue)) => {
                    if !sidecar_kinds.insert(kind) {
                        return Err(io::Error::new(io::ErrorKind::InvalidData, "the download has multiple sidecars of the same kind"));
                    }
                    sidecars.push((name, index));
                }
                Some(SidecarKind::Cover) if cover.is_none() && source_cover.is_none() => source_cover = Some((name, index)),
                _ => {}
            }
        }
    }
    tracks.sort_by(|left, right| natural_cmp(&left.0, &right.0));
    if tracks.is_empty() || tracks.len() > MAX_TRACKS {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "the download has no MP3 tracks, or too many"));
    }
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let mut names = HashSet::new();
    for (name, index) in tracks {
        if !names.insert(name.to_lowercase()) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "the download has duplicate MP3 names"));
        }
        zip.start_file(&name, options).map_err(io::Error::other)?;
        let mut track = source.by_index(index).map_err(io::Error::other)?;
        io::copy(&mut track, &mut zip)?;
    }
    if let Some(source_cover) = source_cover {
        sidecars.push(source_cover);
    }
    for (name, index) in sidecars {
        if !names.insert(name.to_lowercase()) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "the download has duplicate sidecar names"));
        }
        let maximum = sidecar_limit(sidecar_kind(&name).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "unsupported audiobook sidecar"))?);
        let entry = source.by_index(index).map_err(io::Error::other)?;
        if entry.size() > maximum {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "oversized audiobook sidecar"));
        }
        zip.start_file(&name, options).map_err(io::Error::other)?;
        if sidecar_kind(&name) == Some(SidecarKind::Cue) {
            let mut bytes = Vec::new();
            entry.take(maximum + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 > maximum {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "oversized audiobook sidecar"));
            }
            zip.write_all(&flatten_cue_paths(&bytes))?;
        } else if io::copy(&mut entry.take(maximum + 1), &mut zip)? > maximum {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "oversized audiobook sidecar"));
        }
    }
    if let Some(cover) = cover.filter(|cover| cover.len() as u64 <= MAX_COVER_BYTES) {
        zip.start_file("cover.jpg", options).map_err(io::Error::other)?;
        zip.write_all(cover)?;
    }
    Ok(zip.finish().map_err(io::Error::other)?.into_inner())
}

/// Change only FILE paths, retaining the original encoding and cue timings.
fn flatten_cue_paths(bytes: &[u8]) -> Vec<u8> {
    let mut updated = Vec::with_capacity(bytes.len());
    for line in bytes.split_inclusive(|byte| *byte == b'\n') {
        let start = line.iter().position(|byte| !byte.is_ascii_whitespace()).unwrap_or(line.len());
        let instruction = &line[start..];
        if instruction.len() < 5 || !instruction[..4].eq_ignore_ascii_case(b"FILE") || !instruction[4].is_ascii_whitespace() {
            updated.extend_from_slice(line);
            continue;
        }
        let path_start = start + 4 + instruction[4..].iter().take_while(|byte| byte.is_ascii_whitespace()).count();
        let quoted = line.get(path_start) == Some(&b'"');
        let name_start = path_start + usize::from(quoted);
        let Some(length) = line[name_start..].iter().position(|byte| if quoted { *byte == b'"' } else { byte.is_ascii_whitespace() }) else {
            updated.extend_from_slice(line);
            continue;
        };
        let name_end = name_start + length;
        let basename = line[name_start..name_end].rsplit(|byte| *byte == b'/' || *byte == b'\\').next().unwrap_or_default();
        updated.extend_from_slice(&line[..name_start]);
        updated.extend_from_slice(basename);
        updated.extend_from_slice(&line[name_end..]);
    }
    updated
}

/// Orders names as people number tracks: "2" before "10", ignoring case.
pub fn natural_cmp(left: &str, right: &str) -> Ordering {
    let mut a = left.chars().peekable();
    let mut b = right.chars().peekable();
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let digits_a: String = std::iter::from_fn(|| a.next_if(|character| character.is_ascii_digit())).collect();
                let digits_b: String = std::iter::from_fn(|| b.next_if(|character| character.is_ascii_digit())).collect();
                let order = digits_a.trim_start_matches('0').len().cmp(&digits_b.trim_start_matches('0').len()).then_with(|| digits_a.trim_start_matches('0').cmp(digits_b.trim_start_matches('0')));
                if !order.is_eq() {
                    return order;
                }
            }
            (Some(x), Some(y)) => {
                a.next();
                b.next();
                let order = x.to_ascii_lowercase().cmp(&y.to_ascii_lowercase());
                if !order.is_eq() {
                    return order;
                }
            }
            (None, None) => return left.cmp(right),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
        }
    }
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

/// Logical names permitted in an audiobook transport archive, independent of
/// the filenames used to store its members on this device.
pub fn valid_archive_name(name: &str) -> bool {
    valid_entry_name(name) || sidecar_kind(name).is_some()
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
    extract_archive_with_names(reader, destination, |name| Ok(name.to_owned()))
}

/// Extract with device-local filenames while retaining the archive's logical
/// names in the returned index. Callers persist their mapping before publishing.
pub fn extract_archive_with_names<R: Read + Seek>(reader: R, destination: &Path, mut local_name: impl FnMut(&str) -> io::Result<String>) -> io::Result<Vec<String>> {
    let mut archive = ZipArchive::new(reader).map_err(io::Error::other)?;
    if archive.len() > MAX_TRACKS + 3 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "audiobook ZIP has too many entries"));
    }
    let mut names = Vec::with_capacity(archive.len());
    let mut seen = HashSet::new();
    let mut local_names = HashSet::new();
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
        let local = local_name(&name)?;
        if !valid_archive_name(&local) || !local_names.insert(local.to_lowercase()) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid or duplicate local audiobook filename"));
        }
        let output = destination.join(&local);
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        io::copy(&mut entry, &mut fs::OpenOptions::new().write(true).create_new(true).open(output)?)?;
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

    const JPEG: &[u8] = &[0xff, 0xd8, 0xff, 0xe0, 0xff, 0xd9];

    #[test]
    fn mapped_extraction_rejects_escape_and_never_overwrites_an_aliased_track() {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in [("one.mp3", b"first"), ("two.mp3", b"other")] {
            zip.start_file(name, SimpleFileOptions::default().compression_method(CompressionMethod::Stored)).unwrap();
            zip.write_all(bytes).unwrap();
        }
        let bytes = zip.finish().unwrap().into_inner();
        let destination = tempfile::tempdir().unwrap();
        assert!(extract_archive_with_names(Cursor::new(&bytes), destination.path(), |_| Ok("../escape.mp3".into())).is_err());
        assert!(fs::read_dir(destination.path()).unwrap().next().is_none());
        assert!(extract_archive_with_names(Cursor::new(&bytes), destination.path(), |_| Ok("same.mp3".into())).is_err());
        assert_eq!(fs::read(destination.path().join("same.mp3")).unwrap(), b"first");
    }

    fn audio() -> &'static [u8] {
        include_bytes!("../../../client/library-backend/crates/book-inspection/tests/fixtures/silence.mp3")
    }

    #[test]
    fn repack_saves_separate_cover_and_preserves_complete_mp3_bytes() {
        let mut original = ZipWriter::new(Cursor::new(Vec::new()));
        for name in ["folder/10.mp3", "folder/2.mp3"] {
            original.start_file(name, SimpleFileOptions::default()).unwrap();
            original.write_all(audio()).unwrap();
        }
        original.start_file("folder/chapters.cue", SimpleFileOptions::default()).unwrap();
        original.write_all(b"FILE 2.mp3 MP3\nTRACK 01 AUDIO").unwrap();
        original.start_file("folder/cover.jpg", SimpleFileOptions::default()).unwrap();
        original.write_all(b"old cover").unwrap();
        let original = original.finish().unwrap().into_inner();
        let result = repack_zip(Cursor::new(original.clone()), Some(JPEG)).unwrap();
        let mut archive = ZipArchive::new(Cursor::new(result)).unwrap();
        assert_eq!(archive.by_index(0).unwrap().name(), "2.mp3");
        assert_eq!(archive.by_index(1).unwrap().name(), "10.mp3");
        for name in ["2.mp3", "10.mp3"] {
            let mut bytes = Vec::new();
            archive.by_name(name).unwrap().read_to_end(&mut bytes).unwrap();
            assert_eq!(bytes, audio());
        }
        let mut cue = String::new();
        archive.by_name("chapters.cue").unwrap().read_to_string(&mut cue).unwrap();
        assert_eq!(cue, "FILE 2.mp3 MP3\nTRACK 01 AUDIO");
        let mut image = Vec::new();
        archive.by_name("cover.jpg").unwrap().read_to_end(&mut image).unwrap();
        assert_eq!(image, JPEG);
        let unchanged = repack_zip(Cursor::new(original), None).unwrap();
        let mut archive = ZipArchive::new(Cursor::new(unchanged)).unwrap();
        let mut track = Vec::new();
        archive.by_name("2.mp3").unwrap().read_to_end(&mut track).unwrap();
        assert_eq!(track, audio());
        let mut old_cover = Vec::new();
        archive.by_name("cover.jpg").unwrap().read_to_end(&mut old_cover).unwrap();
        assert_eq!(old_cover, b"old cover");
    }

    #[test]
    fn repack_never_reads_or_rewrites_track_tags() {
        let tagged = b"ID3\x04\0\0\x7f\x7f\x7f\x7fsource artwork and chapter metadata";
        let untagged = b"raw MP3 audio payload";
        let mut input = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in [("tagged.mp3", tagged.as_slice()), ("untagged.mp3", untagged.as_slice())] {
            input.start_file(name, SimpleFileOptions::default()).unwrap();
            input.write_all(bytes).unwrap();
        }
        let input = input.finish().unwrap().into_inner();
        let output = repack_zip(Cursor::new(input), Some(JPEG)).unwrap();
        let mut archive = ZipArchive::new(Cursor::new(output)).unwrap();
        for (name, expected) in [("tagged.mp3", tagged.as_slice()), ("untagged.mp3", untagged.as_slice())] {
            let mut actual = Vec::new();
            archive.by_name(name).unwrap().read_to_end(&mut actual).unwrap();
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn cue_paths_follow_flattened_tracks_without_changing_timings_or_encoding() {
        let original = b"TITLE \"B\xe5k\"\r\n  FILE \"folder/chapter 1.mp3\" MP3\r\n TRACK 01 AUDIO\r\n INDEX 01 00:12:34\r\nFILE folder\\2.mp3 MP3\n";
        let expected = b"TITLE \"B\xe5k\"\r\n  FILE \"chapter 1.mp3\" MP3\r\n TRACK 01 AUDIO\r\n INDEX 01 00:12:34\r\nFILE 2.mp3 MP3\n";
        assert_eq!(flatten_cue_paths(original), expected);
    }

    #[test]
    fn rejects_archive_entries_outside_the_album() {
        assert!(!valid_entry_name("C:/escape.mp3"));
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file("../escape.mp3", SimpleFileOptions::default()).unwrap();
        zip.write_all(b"x").unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        let output = tempfile::tempdir().unwrap();
        assert!(extract_archive(Cursor::new(bytes), output.path()).is_err());
        assert!(!output.path().parent().unwrap().join("escape.mp3").exists());
    }
}
