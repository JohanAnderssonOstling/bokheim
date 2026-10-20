//! Device-local filenames. Logical archive names and ordering remain immutable.
use audiobook_archive::{EntryFormat, Track, audio_file, valid_archive_name};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};

pub const PATH_MAPPING_FILE: &str = ".bokheim-audiobook-paths.json";
const MAX_MAPPING_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PathMapping {
    version: u32,
    entries: Vec<NameMapping>,
    #[serde(default)]
    pub(crate) formats: Vec<EntryFormat>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct NameMapping {
    original: String,
    local: String,
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn portable_component(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or_default().trim_end_matches(' ').to_uppercase();
    let reserved =
        matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") || stem.strip_prefix("COM").or_else(|| stem.strip_prefix("LPT")).is_some_and(|number| matches!(number, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"));
    !name.is_empty() && name.len() <= 240 && !name.starts_with('.') && !name.ends_with([' ', '.']) && !reserved && !name.chars().any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
}

fn portable_path(name: &str) -> bool {
    valid_archive_name(name) && name.split('/').all(portable_component)
}

fn regular_mapped_track(folder: &Path, local: &str) -> io::Result<bool> {
    let mut path = folder.to_path_buf();
    let mut components = local.split('/').peekable();
    while let Some(component) = components.next() {
        path.push(component);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if !metadata.file_type().is_symlink() && if components.peek().is_some() { metadata.is_dir() } else { metadata.is_file() } => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
            _ => return Err(invalid("mapped audiobook track must use regular files and directories")),
        }
    }
    Ok(true)
}

pub(crate) fn load(folder: &Path) -> io::Result<Option<PathMapping>> {
    let file = match File::open(folder.join(PATH_MAPPING_FILE)) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(MAX_MAPPING_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_MAPPING_BYTES {
        return Err(invalid("audiobook filename mapping is too large"));
    }
    let mapping: PathMapping = serde_json::from_slice(&bytes).map_err(|error| invalid(error.to_string()))?;
    let mut originals = HashSet::new();
    let mut locals = HashSet::new();
    if mapping.version != 1 || mapping.entries.is_empty() || mapping.entries.len() > audiobook_archive::MAX_TRACKS + 3 {
        return Err(invalid("unsupported audiobook filename mapping"));
    }
    for entry in &mapping.entries {
        if !valid_archive_name(&entry.original)
            || !valid_archive_name(&entry.local)
            || cfg!(windows) && !portable_path(&entry.local)
            || Path::new(&entry.original).extension().map(|ext| ext.to_ascii_lowercase()) != Path::new(&entry.local).extension().map(|ext| ext.to_ascii_lowercase())
            || !originals.insert(entry.original.to_lowercase())
            || !locals.insert(entry.local.to_lowercase())
        {
            return Err(invalid("invalid audiobook filename mapping"));
        }
    }
    if !mapping.formats.is_empty() && (mapping.formats.len() != mapping.entries.len() || !mapping.formats.iter().zip(&mapping.entries).all(|(format, entry)| format.name == entry.original)) {
        return Err(invalid("audiobook archive metadata does not match its filename mapping"));
    }
    Ok(Some(mapping))
}

impl PathMapping {
    fn write(&self, folder: &Path) -> io::Result<()> {
        let bytes = serde_json::to_vec(self).map_err(|error| invalid(error.to_string()))?;
        if bytes.len() as u64 > MAX_MAPPING_BYTES {
            return Err(invalid("audiobook filename mapping is too large"));
        }
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.custom_flags(0x2); // FILE_ATTRIBUTE_HIDDEN
        }
        let mut file = options.open(folder.join(PATH_MAPPING_FILE))?;
        file.write_all(&bytes)?;
        file.sync_all()
    }

    pub(crate) fn logical_name<'a>(&'a self, local: &'a str) -> &'a str {
        self.entries.iter().find(|entry| entry.local.to_lowercase() == local.to_lowercase()).map_or(local, |entry| &entry.original)
    }

    pub(crate) fn apply_tracks(&self, folder: &Path, tracks: &mut Vec<Track>) -> io::Result<()> {
        let mut ordered = Vec::new();
        let mut physical = HashSet::new();
        let mut logical = HashSet::new();
        for entry in self.entries.iter().filter(|entry| audio_file(Path::new(&entry.original))) {
            let path = folder.join(&entry.local);
            if !regular_mapped_track(folder, &entry.local)? {
                continue;
            }
            physical.insert(entry.local.to_lowercase());
            logical.insert(entry.original.to_lowercase());
            ordered.push(Track { name: entry.original.clone(), path });
        }
        // Extra files added by the user still change the audiobook identity.
        // The mapping must never hide new content from scanning or uploads.
        for track in tracks.drain(..) {
            let local = track.path.strip_prefix(folder).map_err(|error| invalid(error.to_string()))?;
            let local = local.iter().map(|part| part.to_str()).collect::<Option<Vec<_>>>().ok_or_else(|| invalid("MP3 filename is not UTF-8"))?.join("/");
            if physical.contains(&local.to_lowercase()) {
                continue;
            }
            if !logical.insert(track.name.to_lowercase()) {
                return Err(invalid("an added track conflicts with a mapped archive name"));
            }
            ordered.push(track);
        }
        if ordered.len() > audiobook_archive::MAX_TRACKS {
            return Err(invalid("audiobook has too many MP3 tracks"));
        }
        *tracks = ordered;
        Ok(())
    }
}

fn repaired_name(original: &str, title: Option<&str>, number: usize, attempt: usize) -> String {
    let extension = Path::new(original).extension().and_then(|ext| ext.to_str()).unwrap_or("mp3").to_ascii_lowercase();
    let title = title.unwrap_or("Track");
    let title = title.chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect::<String>();
    let title = title.trim_matches(|c: char| c == '.' || c.is_whitespace());
    let title = if title.is_empty() { "Track" } else { title };
    let mut end = title.len().min(180);
    while !title.is_char_boundary(end) {
        end -= 1;
    }
    let suffix = if attempt == 0 { String::new() } else { format!(" ({attempt})") };
    format!("{number:03} - {}{suffix}.{extension}", title[..end].trim_end_matches([' ', '.']))
}

/// Extract a native download, preserving source ZIP metadata on every platform.
/// Windows additionally repairs filenames using the supplied chapter titles.
pub fn extract_download<R: Read + Seek>(reader: R, destination: &Path, titles: &[(String, String)]) -> io::Result<Vec<String>> {
    extract_with_local_names(reader, destination, titles, cfg!(windows))
}

/// Windows downloads may use chapter titles for filenames that cannot be
/// materialized as written. The original names and entry order stay in a local
/// sidecar, so rebuilding the transport archive keeps the same content hash.
pub fn extract_for_windows<R: Read + Seek>(reader: R, destination: &Path, titles: &[(String, String)]) -> io::Result<Vec<String>> {
    extract_with_local_names(reader, destination, titles, true)
}

fn extract_with_local_names<R: Read + Seek>(mut reader: R, destination: &Path, titles: &[(String, String)], repair_names: bool) -> io::Result<Vec<String>> {
    let mut mapping = PathMapping { version: 1, entries: Vec::new(), formats: audiobook_archive::entry_formats(&mut reader)? };
    let mut used = HashSet::new();
    let mut track_number = 0;
    let names = audiobook_archive::extract_archive_with_names(reader, destination, |original| {
        let track = audio_file(Path::new(original));
        if track {
            track_number += 1;
        }
        let title = titles.iter().find(|(name, _)| name == original).map(|(_, title)| title.as_str());
        let mut local = original.to_owned();
        let mut attempt = 0;
        while repair_names && (!portable_path(&local) || used.contains(&local.to_lowercase())) {
            local = repaired_name(original, if track { title } else { Some("Sidecar") }, if track { track_number } else { mapping.entries.len() + 1 }, attempt);
            attempt += 1;
        }
        used.insert(local.to_lowercase());
        mapping.entries.push(NameMapping { original: original.to_owned(), local: local.clone() });
        Ok(local)
    })?;
    let mut needs_mapping = mapping.entries.iter().any(|entry| entry.original != entry.local) || mapping.formats.iter().any(|format| !format.is_native_default());
    if !needs_mapping {
        let mut discovered = super::discover_physical(destination)?.into_iter().map(|track| track.name).collect::<Vec<_>>();
        for sidecar in [crate::discover_nfo(destination)?, crate::discover_cue(destination)?, crate::discover_cover(destination)?].into_iter().flatten() {
            discovered.push(sidecar.name);
        }
        needs_mapping = discovered != names;
    }
    if needs_mapping {
        mapping.write(destination)?;
    }
    Ok(names)
}

pub fn resolve_track_path(folder: &Path, original: &str) -> io::Result<PathBuf> {
    if !valid_archive_name(original) || !audio_file(Path::new(original)) {
        return Err(invalid("invalid audiobook track name"));
    }
    let mapping = load(folder)?;
    let entry = mapping.as_ref().and_then(|mapping| mapping.entries.iter().find(|entry| entry.original == original));
    let local = entry.map_or(original, |entry| entry.local.as_str());
    if entry.is_some() {
        regular_mapped_track(folder, local)?;
    }
    #[cfg(windows)]
    if !portable_path(local) {
        return Err(invalid("this audiobook needs its local filename mapping; download it again to repair the files"));
    }
    Ok(folder.join(local))
}

/// Placement copies must carry device-local names with the physical files.
pub fn copy_path_mapping(source: &Path, destination: &Path) -> io::Result<()> {
    if let Some(mapping) = load(source)? {
        mapping.write(destination)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

    fn archive() -> Vec<u8> {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in [
            ("NUL.mp3", b"opening".as_slice()),
            ("Disc?/track?.mp3", b"middle".as_slice()),
            ("001 - Introduction.mp3", b"ending".as_slice()),
            ("CON.nfo", b"Title: Story\n".as_slice()),
            ("chapters?.cue", b"FILE \"Disc?/track?.mp3\" MP3\nTRACK 01 AUDIO\nINDEX 01 00:00:00\n".as_slice()),
            ("cover.png", b"image".as_slice()),
        ] {
            // Model an existing Linux-origin archive, even on the Windows runner.
            zip.start_file(name, SimpleFileOptions::default().compression_method(CompressionMethod::Stored).system(zip::System::Unix)).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn repairs_names_from_titles_without_changing_archive_identity_or_order() {
        let bytes = archive();
        let destination = tempfile::tempdir().unwrap();
        let titles = vec![("NUL.mp3".into(), "Introduction".into()), ("Disc?/track?.mp3".into(), "Introduction".into()), ("001 - Introduction.mp3".into(), "End: Why?".into())];
        extract_for_windows(Cursor::new(&bytes), destination.path(), &titles).unwrap();
        let mapping = load(destination.path()).unwrap().unwrap();
        assert_eq!(mapping.entries[0].local, "001 - Introduction.mp3");
        assert_eq!(mapping.entries[1].local, "002 - Introduction.mp3");
        assert_eq!(mapping.entries[2].local, "003 - End_ Why_.mp3");
        assert!(mapping.entries.iter().all(|entry| portable_path(&entry.local)));
        let tracks = crate::discover(destination.path()).unwrap();
        assert_eq!(tracks.iter().map(|track| track.name.as_str()).collect::<Vec<_>>(), ["NUL.mp3", "Disc?/track?.mp3", "001 - Introduction.mp3"]);
        assert_eq!(fs::read(resolve_track_path(destination.path(), "NUL.mp3").unwrap()).unwrap(), b"opening");
        assert_eq!(crate::folder_hash(destination.path()).unwrap(), blake3::hash(&bytes));
        assert_eq!(crate::discover_nfo(destination.path()).unwrap().unwrap().name, "CON.nfo");
        assert_eq!(fs::read(crate::discover_cue(destination.path()).unwrap().unwrap().path).unwrap(), b"FILE \"Disc?/track?.mp3\" MP3\nTRACK 01 AUDIO\nINDEX 01 00:00:00\n");
        // Reopening uses the durable mapping, without the database title hints.
        assert_eq!(crate::folder_hash(destination.path()).unwrap(), blake3::hash(&bytes));
        fs::write(destination.path().join("extra.mp3"), b"extra").unwrap();
        assert_ne!(crate::folder_hash(destination.path()).unwrap(), blake3::hash(&bytes));
        let tracks = crate::discover(destination.path()).unwrap();
        let changed = crate::write_folder_archive(destination.path(), Cursor::new(Vec::new()), &tracks).unwrap();
        assert_eq!(audiobook_archive::archived_tracks(changed).unwrap().len(), 4);
    }

    #[test]
    fn missing_titles_have_portable_numbered_fallbacks() {
        let bytes = archive();
        let destination = tempfile::tempdir().unwrap();
        extract_for_windows(Cursor::new(&bytes), destination.path(), &[]).unwrap();
        let mapping = load(destination.path()).unwrap().unwrap();
        assert_eq!(mapping.entries[0].local, "001 - Track.mp3");
        assert_eq!(mapping.entries[1].local, "002 - Track.mp3");
        assert_eq!(crate::folder_hash(destination.path()).unwrap(), blake3::hash(&bytes));
    }

    #[test]
    fn native_downloads_preserve_foreign_headers_and_sidecar_order() {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Stored)
            .system(if cfg!(windows) { zip::System::Unix } else { zip::System::Dos })
            .last_modified_time(zip::DateTime::from_date_and_time(2024, 2, 3, 4, 5, 6).unwrap())
            .unix_permissions(0o600);
        for (name, bytes) in [("01.mp3", b"audio".as_slice()), ("chapters.cue", b"FILE 01.mp3 MP3\n".as_slice()), ("book.nfo", b"Title: Story\n".as_slice())] {
            zip.start_file(name, options).unwrap();
            zip.write_all(bytes).unwrap();
        }
        let bytes = zip.finish().unwrap().into_inner();
        let destination = tempfile::tempdir().unwrap();
        extract_download(Cursor::new(&bytes), destination.path(), &[]).unwrap();
        let mapping = load(destination.path()).unwrap().unwrap();
        assert!(mapping.entries.iter().all(|entry| entry.original == entry.local));
        assert_eq!(crate::folder_hash(destination.path()).unwrap(), blake3::hash(&bytes));
    }

    #[test]
    fn safe_names_preserve_archive_order_when_it_differs_from_filename_order() {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        for name in ["02.mp3", "01.mp3"] {
            zip.start_file(name, SimpleFileOptions::default().compression_method(CompressionMethod::Stored)).unwrap();
            zip.write_all(b"audio").unwrap();
        }
        let bytes = zip.finish().unwrap().into_inner();
        let destination = tempfile::tempdir().unwrap();
        extract_for_windows(Cursor::new(&bytes), destination.path(), &[]).unwrap();
        assert_eq!(crate::discover(destination.path()).unwrap().iter().map(|track| track.name.as_str()).collect::<Vec<_>>(), ["02.mp3", "01.mp3"]);
        assert_eq!(crate::folder_hash(destination.path()).unwrap(), blake3::hash(&bytes));
    }

    #[test]
    fn rejects_tampered_mappings_without_reading_outside_the_folder() {
        let destination = tempfile::tempdir().unwrap();
        let mut invalid_names = vec!["../escape.mp3", "C:/escape.mp3", "/escape.mp3", "folder/../escape.mp3", "track.nfo"];
        if cfg!(windows) {
            invalid_names.extend(["NUL.mp3", "track?.mp3"]);
        }
        for local in invalid_names {
            let bytes = serde_json::to_vec(&serde_json::json!({"version": 1, "entries": [{"original": "NUL.mp3", "local": local}]})).unwrap();
            fs::write(destination.path().join(PATH_MAPPING_FILE), bytes).unwrap();
            assert!(crate::discover(destination.path()).is_err(), "accepted mapping: {local}");
            assert!(resolve_track_path(destination.path(), "NUL.mp3").is_err());
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn unix_downloads_keep_native_names_with_foreign_headers() {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        for name in ["NUL.mp3", "Disc?/track?.mp3"] {
            zip.start_file(name, SimpleFileOptions::default().compression_method(CompressionMethod::Stored).system(zip::System::Dos)).unwrap();
            zip.write_all(b"audio").unwrap();
        }
        let bytes = zip.finish().unwrap().into_inner();
        let destination = tempfile::tempdir().unwrap();
        extract_download(Cursor::new(&bytes), destination.path(), &[("NUL.mp3".into(), "Introduction".into())]).unwrap();
        assert_eq!(fs::read(destination.path().join("NUL.mp3")).unwrap(), b"audio");
        assert_eq!(fs::read(destination.path().join("Disc?/track?.mp3")).unwrap(), b"audio");
        assert_eq!(fs::read(resolve_track_path(destination.path(), "NUL.mp3").unwrap()).unwrap(), b"audio");
        assert_eq!(crate::folder_hash(destination.path()).unwrap(), blake3::hash(&bytes));
    }

    #[cfg(unix)]
    #[test]
    fn mapped_tracks_cannot_follow_a_symlink_outside_the_folder() {
        let destination = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("01.mp3"), b"outside").unwrap();
        std::os::unix::fs::symlink(outside.path(), destination.path().join("Disc 1")).unwrap();
        let bytes = serde_json::to_vec(&serde_json::json!({"version": 1, "entries": [{"original": "NUL.mp3", "local": "Disc 1/01.mp3"}]})).unwrap();
        fs::write(destination.path().join(PATH_MAPPING_FILE), bytes).unwrap();
        assert!(crate::discover(destination.path()).is_err());
        assert!(resolve_track_path(destination.path(), "NUL.mp3").is_err());
    }

    #[test]
    fn safe_archives_keep_their_existing_names_without_a_mapping_file() {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file("Disc 1/01.mp3", SimpleFileOptions::default().compression_method(CompressionMethod::Stored)).unwrap();
        zip.write_all(b"audio").unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        let destination = tempfile::tempdir().unwrap();
        extract_for_windows(Cursor::new(&bytes), destination.path(), &[]).unwrap();
        assert!(!destination.path().join(PATH_MAPPING_FILE).exists());
        assert_eq!(fs::read(destination.path().join("Disc 1/01.mp3")).unwrap(), b"audio");
        assert_eq!(crate::folder_hash(destination.path()).unwrap(), blake3::hash(&bytes));
    }
}
