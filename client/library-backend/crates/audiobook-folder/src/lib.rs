//! Folder-based audiobooks: finds a book's MP3 tracks and sidecars on disk,
//! orders them for playback, and reads MP3 tags. A device keeps ordinary MP3
//! files; `audiobook_archive` packs them for transfer and server storage.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io;
use std::path::Path;
use symphonia::core::io::MediaSource;
use symphonia::core::units::TimeBase;

use audiobook_archive::{Cover, Cue, MAX_COVER_BYTES, MAX_CUE_BYTES, MAX_NFO_BYTES, MAX_TRACKS, Nfo, Track, audio_file, natural_cmp, valid_sidecar_name};

mod local_names;
mod mp3_frames;
pub use local_names::{PATH_MAPPING_FILE, copy_path_mapping, extract_download, resolve_track_path};
#[cfg(not(target_arch = "wasm32"))]
mod archive_reader;
#[cfg(not(target_arch = "wasm32"))]
pub use archive_reader::{FolderArchive, folder_archive_reader};

#[derive(Default)]
pub struct Mp3Facts {
    pub duration_ms: u64,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub artist: Option<String>,
    pub track_title: Option<String>,
    pub description: Option<String>,
    pub narrator: Option<String>,
    pub publisher: Option<String>,
    pub release_date: Option<String>,
    pub asin: Option<String>,
    pub isbn: Option<String>,
    pub artwork: Option<Vec<u8>>,
}

fn set_missing(slot: &mut Option<String>, value: String) {
    let value = value.trim();
    if slot.is_none() && !value.is_empty() {
        *slot = Some(value.to_owned());
    }
}

impl Mp3Facts {
    fn absorb(&mut self, revision: &symphonia::core::meta::MetadataRevision, artwork: bool) {
        use symphonia::core::meta::{StandardTagKey as Key, StandardVisualKey};
        for tag in revision.tags() {
            let value = tag.value.to_string();
            match tag.std_key {
                Some(Key::Album) => set_missing(&mut self.album, value),
                Some(Key::AlbumArtist) => set_missing(&mut self.album_artist, value),
                Some(Key::Artist) => set_missing(&mut self.artist, value),
                Some(Key::TrackTitle) => set_missing(&mut self.track_title, value),
                Some(Key::Description | Key::Comment) => set_missing(&mut self.description, value),
                Some(Key::Performer) => set_missing(&mut self.narrator, value),
                Some(Key::Label) => set_missing(&mut self.publisher, value),
                Some(Key::ReleaseDate | Key::Date) => set_missing(&mut self.release_date, value),
                Some(Key::IdentAsin) => set_missing(&mut self.asin, value),
                _ => {
                    let key = tag.key.to_ascii_lowercase();
                    if key.contains("narrator") || key == "read by" {
                        set_missing(&mut self.narrator, value);
                    } else if key.contains("publisher") {
                        set_missing(&mut self.publisher, value);
                    } else if key.contains("isbn") {
                        set_missing(&mut self.isbn, value);
                    } else if key.contains("asin") {
                        set_missing(&mut self.asin, value);
                    }
                }
            }
        }
        if artwork
            && self.artwork.is_none()
            && let Some(visual) = revision.visuals().iter().find(|visual| visual.usage == Some(StandardVisualKey::FrontCover)).or_else(|| revision.visuals().first())
            && visual.data.len() as u64 <= MAX_COVER_BYTES
        {
            self.artwork = Some(visual.data.to_vec());
        }
    }
}

/// Counts MP3 samples without decoding audio. The scan uses the
/// same result for chapter boundaries and the book-wide playback clock.
pub fn duration_ms<R: MediaSource + 'static>(reader: R) -> io::Result<u64> {
    Ok(inspect_mp3(reader, false)?.duration_ms)
}

/// Inspect an owned seekable source with a known byte length, including
/// instrumented filesystem readers that do not implement MediaSource.
pub fn inspect_mp3_reader<R: io::Read + io::Seek + Send + Sync + 'static>(reader: R, length: u64, artwork: bool) -> io::Result<Mp3Facts> {
    struct Source<R> {
        reader: R,
        length: u64,
    }
    impl<R: io::Read> io::Read for Source<R> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.reader.read(buffer)
        }
    }
    impl<R: io::Seek> io::Seek for Source<R> {
        fn seek(&mut self, position: io::SeekFrom) -> io::Result<u64> {
            self.reader.seek(position)
        }
    }
    impl<R: io::Read + io::Seek + Send + Sync> symphonia::core::io::MediaSource for Source<R> {
        fn is_seekable(&self) -> bool {
            true
        }

        fn byte_len(&self) -> Option<u64> {
            Some(self.length)
        }
    }
    inspect_mp3(Source { reader, length }, artwork)
}

// Trust an explicit CBR frame count only when its declared audio extent fits
// the file exactly, optionally followed by a validated ID3v1 footer. VBR frame
// counts can include the header frame itself, so count actual audio frames.
fn cbr_sample_count(reader: &mut impl MediaSource, length: u64, start: u64) -> io::Result<Option<u64>> {
    use io::SeekFrom;
    let mut tag = [0; 10];
    reader.read_exact(&mut tag)?;
    let audio_start = if tag.starts_with(b"ID3") {
        if !matches!(tag[3], 2..=4) || tag[5] & 0x10 != 0 || tag[6..].iter().any(|byte| byte & 0x80 != 0) {
            return Ok(None);
        }
        let size = tag[6..].iter().fold(0_u64, |size, byte| (size << 7) | u64::from(*byte));
        start.checked_add(10).and_then(|start| start.checked_add(size)).ok_or_else(|| io::Error::other("MP3 tag size overflow"))?
    } else {
        start
    };
    if audio_start >= length {
        return Ok(None);
    }
    reader.seek(SeekFrom::Start(audio_start))?;
    let mut frame = [0; 60];
    reader.read_exact(&mut frame)?;
    let Some(header) = mp3_frames::FrameHeader::parse(u32::from_be_bytes([frame[0], frame[1], frame[2], frame[3]])) else { return Ok(None) };
    let offset = 4 + header.side_info_len;
    let number = |offset: usize| u32::from_be_bytes([frame[offset], frame[offset + 1], frame[offset + 2], frame[offset + 3]]);
    if &frame[offset..offset + 4] != b"Info" || number(offset + 4) & 3 != 3 {
        return Ok(None);
    }
    let frames = number(offset + 8);
    let bytes = number(offset + 12);
    let end = audio_start.checked_add(u64::from(bytes)).ok_or_else(|| io::Error::other("MP3 audio size overflow"))?;
    if frames == 0 || bytes < frame.len() as u32 {
        return Ok(None);
    }
    let frame_bytes = u64::from(header.bytes - u32::from((frame[2] >> 1) & 1));
    let frame_count = u64::from(frames) + 1; // Includes the Info header frame.
    if u64::from(bytes) < frame_count * frame_bytes || u64::from(bytes) > frame_count * (frame_bytes + 1) {
        return Ok(None);
    }
    if end != length {
        if end.checked_add(128) != Some(length) {
            return Ok(None);
        }
        reader.seek(SeekFrom::Start(end))?;
        let mut footer = [0; 3];
        reader.read_exact(&mut footer)?;
        if &footer != b"TAG" {
            return Ok(None);
        }
    }
    Ok(Some(u64::from(frames) * u64::from(header.samples)))
}

pub fn inspect_mp3<R: MediaSource + 'static>(mut reader: R, artwork: bool) -> io::Result<Mp3Facts> {
    use io::Seek;
    use symphonia::core::errors::Error;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let length = if reader.is_seekable() { reader.byte_len() } else { None };
    let start = if length.is_some() { Some(reader.stream_position()?) } else { None };
    let samples = if let (Some(start), Some(length)) = (start, length) {
        let samples = cbr_sample_count(&mut reader, length, start).ok().flatten();
        reader.seek(io::SeekFrom::Start(start))?;
        samples
    } else {
        None
    };
    let source = MediaSourceStream::new(Box::new(reader), MediaSourceStreamOptions::default());
    let mut hint = Hint::new();
    hint.with_extension("mp3");
    let mut probed = symphonia::default::get_probe().format(&hint, source, &FormatOptions::default(), &MetadataOptions::default()).map_err(io::Error::other)?;
    let mut facts = Mp3Facts::default();
    if let Some(metadata) = probed.metadata.get()
        && let Some(revision) = metadata.current()
    {
        facts.absorb(revision, artwork);
    }
    let mut format = probed.format;
    if let Some(revision) = format.metadata().current() {
        facts.absorb(revision, artwork);
    }
    let track = format.default_track().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "MP3 has no audio track"))?;
    let track_id = track.id;
    let time_base = track.codec_params.time_base.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "MP3 has no time base"))?;
    let sample_rate = track.codec_params.sample_rate;
    let mp3 = track.codec_params.codec == symphonia::core::codecs::CODEC_TYPE_MP3;
    let end = match samples.filter(|samples| track.codec_params.n_frames == Some(*samples)) {
        Some(samples) => samples,
        None => {
            if let (true, Some(start), Some(length), Some(sample_rate)) = (mp3, start, length, sample_rate) {
                let mut source = format.into_inner();
                if let Ok(Some(samples)) = mp3_frames::sample_count(&mut source, length, sample_rate) {
                    return finish_duration(facts, time_base, samples);
                }
                source.seek(io::SeekFrom::Start(start))?;
                format = symphonia::default::get_probe().format(&hint, source, &FormatOptions::default(), &MetadataOptions::default()).map_err(io::Error::other)?.format;
            }
            let mut end = 0u64;
            loop {
                match format.next_packet() {
                    Ok(packet) if packet.track_id() == track_id => end = end.max(packet.ts().saturating_add(packet.dur())),
                    Ok(_) => {}
                    Err(Error::IoError(error)) if error.kind() == io::ErrorKind::UnexpectedEof => break,
                    Err(error) => return Err(io::Error::other(error)),
                }
            }
            end
        }
    };
    if let Some(revision) = format.metadata().current() {
        facts.absorb(revision, artwork);
    }
    finish_duration(facts, time_base, end)
}

fn finish_duration(mut facts: Mp3Facts, time_base: TimeBase, samples: u64) -> io::Result<Mp3Facts> {
    let duration = time_base.calc_time(samples);
    let millis = duration.seconds.saturating_mul(1000).saturating_add((duration.frac * 1000.0).round() as u64);
    if millis == 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "MP3 has no audio duration"));
    }
    facts.duration_ms = millis;
    Ok(facts)
}

fn discover_sidecar(folder: &Path, accepts: impl Fn(&str) -> bool, maximum: u64) -> io::Result<Option<Nfo>> {
    let mapping = local_names::load(folder)?;
    let mut files = Vec::new();
    for entry in fs::read_dir(folder)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let Ok(name) = entry.file_name().into_string() else { continue };
        if name.starts_with('.') || !accepts(&name) {
            continue;
        }
        if !valid_sidecar_name(&name) || entry.metadata()?.len() > maximum {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid or oversized audiobook sidecar"));
        }
        let name = mapping.as_ref().map_or(name.clone(), |mapping| mapping.logical_name(&name).to_owned());
        files.push(Nfo { name, path: entry.path() });
    }
    if files.len() > 1 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "audiobook has multiple matching sidecars"));
    }
    Ok(files.pop())
}

pub fn discover_nfo(folder: &Path) -> io::Result<Option<Nfo>> {
    discover_sidecar(folder, |name| name.to_ascii_lowercase().ends_with(".nfo"), MAX_NFO_BYTES)
}

pub fn discover_cue(folder: &Path) -> io::Result<Option<Cue>> {
    discover_sidecar(folder, |name| name.to_ascii_lowercase().ends_with(".cue"), MAX_CUE_BYTES)
}

pub fn discover_cover(folder: &Path) -> io::Result<Option<Cover>> {
    for stem in ["cover", "folder"] {
        for extension in ["jpg", "jpeg", "png", "webp"] {
            if let Some(cover) = discover_sidecar(folder, |name| name.eq_ignore_ascii_case(&format!("{stem}.{extension}")), MAX_COVER_BYTES)? {
                return Ok(Some(cover));
            }
        }
    }
    Ok(None)
}

/// A matching sibling belongs to this M4B. A generic cover is unambiguous
/// only when this is the directory's sole M4B file.
pub fn discover_m4b_cover(path: &Path) -> io::Result<Option<Cover>> {
    let folder = path.parent().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "M4B has no parent directory"))?;
    let stem = path.file_stem().and_then(|stem| stem.to_str()).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "M4B filename is not UTF-8"))?;
    for extension in ["jpg", "jpeg", "png", "webp"] {
        if let Some(cover) = discover_sidecar(folder, |name| name.eq_ignore_ascii_case(&format!("{stem}.{extension}")), MAX_COVER_BYTES)? {
            return Ok(Some(cover));
        }
    }
    let m4b_count = fs::read_dir(folder)?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()) && entry.path().extension().and_then(|extension| extension.to_str()).is_some_and(|extension| extension.eq_ignore_ascii_case("m4b")))
        .count();
    if m4b_count == 1 { discover_cover(folder) } else { Ok(None) }
}

fn disc_number(name: &str) -> Option<u32> {
    let lower = name.to_ascii_lowercase();
    let rest = ["disc", "disk", "cd"].iter().find_map(|prefix| lower.strip_prefix(prefix))?.trim();
    (!rest.is_empty() && rest.chars().all(|character| character.is_ascii_digit())).then(|| rest.parse().ok()).flatten()
}

pub fn is_disc_folder_name(name: &str) -> bool {
    disc_number(name).is_some()
}

#[derive(Default)]
struct TrackNumbers {
    disc: Option<u32>,
    track: Option<u32>,
}

fn positive_number(value: &str) -> Option<u32> {
    let first = value.trim().split('/').next()?.trim();
    first.parse::<u32>().ok().filter(|number| *number > 0)
}

fn read_track_numbers(path: &Path) -> io::Result<TrackNumbers> {
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
    use symphonia::core::meta::{MetadataOptions, MetadataRevision, StandardTagKey};
    use symphonia::core::probe::Hint;

    fn absorb(numbers: &mut TrackNumbers, revision: &MetadataRevision) {
        for tag in revision.tags() {
            match tag.std_key {
                Some(StandardTagKey::DiscNumber) if numbers.disc.is_none() => numbers.disc = positive_number(&tag.value.to_string()),
                Some(StandardTagKey::TrackNumber) if numbers.track.is_none() => numbers.track = positive_number(&tag.value.to_string()),
                _ => {}
            }
        }
    }

    let source = MediaSourceStream::new(Box::new(File::open(path)?), MediaSourceStreamOptions::default());
    let mut hint = Hint::new();
    hint.with_extension("mp3");
    let mut probed = symphonia::default::get_probe().format(&hint, source, &FormatOptions::default(), &MetadataOptions::default()).map_err(io::Error::other)?;
    let mut numbers = TrackNumbers::default();
    if let Some(metadata) = probed.metadata.get()
        && let Some(revision) = metadata.current()
    {
        absorb(&mut numbers, revision);
    }
    if let Some(revision) = probed.format.metadata().current() {
        absorb(&mut numbers, revision);
    }
    Ok(numbers)
}

fn order_from_tags(tracks: &mut [Track], use_disc_tags: bool) {
    let Some(mut tagged) = tracks.iter().map(|track| read_track_numbers(&track.path).ok()).collect::<Option<Vec<_>>>() else { return };
    if tagged.iter().any(|numbers| numbers.track.is_none()) {
        return;
    }
    let disc_tags_complete = use_disc_tags && tagged.iter().all(|numbers| numbers.disc.is_some());
    let mut seen = HashSet::new();
    if tagged.iter().any(|numbers| !seen.insert((if disc_tags_complete { numbers.disc } else { None }, numbers.track))) {
        return;
    }
    let mut indexed = tracks.iter().cloned().zip(tagged.drain(..)).collect::<Vec<_>>();
    indexed.sort_by(|(left, left_numbers), (right, right_numbers)| {
        (if disc_tags_complete { left_numbers.disc } else { None }, left_numbers.track).cmp(&(if disc_tags_complete { right_numbers.disc } else { None }, right_numbers.track)).then_with(|| natural_cmp(&left.name, &right.name))
    });
    for (destination, (track, _)) in tracks.iter_mut().zip(indexed) {
        *destination = track;
    }
}

fn order_from_cue(folder: &Path, tracks: &mut Vec<Track>) -> io::Result<()> {
    let Some(cue) = discover_cue(folder)? else { return Ok(()) };
    let bytes = fs::read(cue.path)?;
    let text = match std::str::from_utf8(&bytes) {
        Ok(text) => text.trim_start_matches('\u{feff}').to_owned(),
        Err(_) => encoding_rs::WINDOWS_1252.decode(&bytes).0.into_owned(),
    };
    let mut order = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let Some((command, rest)) = line.split_once(char::is_whitespace) else { continue };
        if !command.eq_ignore_ascii_case("FILE") {
            continue;
        }
        let rest = rest.trim();
        let file = if let Some(quoted) = rest.strip_prefix('"') { quoted.split_once('"').map(|(name, _)| name) } else { rest.split_whitespace().next() };
        let Some(file) = file else { return Ok(()) };
        let file = file.replace('\\', "/");
        let exact = tracks.iter().enumerate().filter(|(_, track)| track.name.eq_ignore_ascii_case(&file)).map(|(index, _)| index).collect::<Vec<_>>();
        let matching = if exact.is_empty() { tracks.iter().enumerate().filter(|(_, track)| track.name.rsplit('/').next().is_some_and(|name| name.eq_ignore_ascii_case(&file))).map(|(index, _)| index).collect::<Vec<_>>() } else { exact };
        if matching.len() != 1 {
            return Ok(());
        }
        if !order.contains(&matching[0]) {
            order.push(matching[0]);
        }
    }
    if order.len() == tracks.len() {
        let previous = tracks.clone();
        *tracks = order.into_iter().map(|index| previous[index].clone()).collect();
    }
    Ok(())
}

/// Finds the tracks belonging to this book folder. Only numbered disc folders
/// are folded into the book; other child folders remain separate scan targets.
pub fn discover(folder: &Path) -> io::Result<Vec<Track>> {
    let mapping = local_names::load(folder)?;
    let mut tracks = discover_physical(folder)?;
    if let Some(mapping) = mapping {
        mapping.apply_tracks(folder, &mut tracks)?;
    }
    Ok(tracks)
}

fn discover_physical(folder: &Path) -> io::Result<Vec<Track>> {
    let mut tracks = Vec::new();
    let mut discs = Vec::new();
    for entry in fs::read_dir(folder)? {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_file() && audio_file(&path) && !entry.file_name().to_string_lossy().starts_with('.') {
            let name = entry.file_name().into_string().map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "MP3 filename is not UTF-8"))?;
            tracks.push(Track { name, path });
        } else if kind.is_dir() {
            let Ok(name) = entry.file_name().into_string() else { continue };
            if let Some(number) = disc_number(&name) {
                discs.push((number, name, path));
            }
        }
    }
    tracks.sort_by(|left, right| natural_cmp(&left.name, &right.name));
    order_from_tags(&mut tracks, true);
    discs.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| natural_cmp(&left.1, &right.1)));
    for (_, disc_name, path) in discs {
        let mut disc_tracks = Vec::new();
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_file() && audio_file(&path) && !entry.file_name().to_string_lossy().starts_with('.') {
                let filename = entry.file_name().into_string().map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "MP3 filename is not UTF-8"))?;
                disc_tracks.push(Track { name: format!("{disc_name}/{filename}"), path });
            }
        }
        disc_tracks.sort_by(|left, right| natural_cmp(&left.name, &right.name));
        order_from_tags(&mut disc_tracks, false);
        tracks.extend(disc_tracks);
    }
    if !tracks.is_empty() {
        order_from_cue(folder, &mut tracks)?;
    }
    if tracks.len() > MAX_TRACKS {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "audiobook has too many MP3 tracks"));
    }
    Ok(tracks)
}

/// Hashes the reproducible archive, including playback order and sidecars.
#[cfg(not(target_arch = "wasm32"))]
pub fn folder_hash(folder: &Path) -> io::Result<blake3::Hash> {
    let tracks = discover(folder)?;
    let archive = write_folder_archive(folder, tempfile::tempfile()?, &tracks)?;
    audiobook_archive::archive_hash(archive)
}

/// Inspect content identity without creating or checksumming ZIP payloads.
pub fn folder_content_with_reader<R: io::Read>(folder: &Path, tracks: &[Track], open: impl FnMut(&Path) -> io::Result<R>) -> io::Result<audiobook_archive::ContentManifest> {
    let nfo = discover_nfo(folder)?;
    let cue = discover_cue(folder)?;
    let cover = discover_cover(folder)?;
    let mapping = local_names::load(folder)?;
    let formats = mapping.as_ref().map_or(&[][..], |mapping| mapping.formats.as_slice());
    audiobook_archive::folder_content_with_reader(tracks, nfo.as_ref(), cue.as_ref(), cover.as_ref(), formats, open)
}

pub fn folder_content_hash(folder: &Path) -> io::Result<blake3::Hash> {
    let tracks = discover(folder)?;
    Ok(folder_content_with_reader(folder, &tracks, |path| fs::File::open(path))?.checksum)
}

/// Retain the original transport's metadata precedence for mapped downloads.
pub fn metadata_sidecars(folder: &Path) -> io::Result<Vec<Nfo>> {
    let mut sidecars = discover_nfo(folder)?.into_iter().chain(discover_cue(folder)?).collect::<Vec<_>>();
    let mapping = local_names::load(folder)?;
    let formats = mapping.as_ref().map_or(&[][..], |mapping| mapping.formats.as_slice());
    sidecars.sort_by_key(|sidecar| formats.iter().position(|format| format.name == sidecar.name).unwrap_or(formats.len()));
    Ok(sidecars)
}

/// Use logical member names and source ZIP metadata for mapped native files.
pub fn write_folder_archive<W: io::Write + io::Seek>(folder: &Path, writer: W, tracks: &[Track]) -> io::Result<W> {
    write_folder_archive_with_reader(folder, writer, tracks, |path| fs::File::open(path))
}

/// Archive through caller-supplied member readers, preserving identical bytes.
pub fn write_folder_archive_with_reader<W: io::Write + io::Seek, R: io::Read>(folder: &Path, writer: W, tracks: &[Track], open: impl FnMut(&Path) -> io::Result<R>) -> io::Result<W> {
    let nfo = discover_nfo(folder)?;
    let cue = discover_cue(folder)?;
    let cover = discover_cover(folder)?;
    let mapping = local_names::load(folder)?;
    let formats = mapping.as_ref().map_or(&[][..], |mapping| mapping.formats.as_slice());
    audiobook_archive::write_archive_with_formats_and_reader(writer, tracks, nfo.as_ref(), cue.as_ref(), cover.as_ref(), formats, open)
}

#[cfg(test)]
mod tests {
    use super::*;
    use audiobook_archive::{archive_hash, archived_tracks, extract_archive_with_names, write_archive, write_archive_with_sidecars};

    fn tagged_mp3(frames: &[(&str, &str)]) -> Vec<u8> {
        let mut body = Vec::new();
        for (key, value) in frames {
            body.extend_from_slice(key.as_bytes());
            body.extend_from_slice(&((value.len() + 1) as u32).to_be_bytes());
            body.extend_from_slice(&[0, 0, 0]);
            body.extend_from_slice(value.as_bytes());
        }
        let size = body.len() as u32;
        let mut bytes = vec![b'I', b'D', b'3', 3, 0, 0, ((size >> 21) & 0x7f) as u8, ((size >> 14) & 0x7f) as u8, ((size >> 7) & 0x7f) as u8, (size & 0x7f) as u8];
        bytes.extend(body);
        bytes.extend_from_slice(include_bytes!("../../book-inspection/tests/fixtures/silence.mp3"));
        bytes
    }

    #[test]
    fn complete_id3_numbers_control_playback_order_and_archive_hash() {
        let source = tempfile::tempdir().unwrap();
        fs::write(source.path().join("A.mp3"), tagged_mp3(&[("TPOS", "2/2"), ("TRCK", "1/2")])).unwrap();
        fs::write(source.path().join("B.mp3"), tagged_mp3(&[("TPOS", "1/2"), ("TRCK", "2/2")])).unwrap();
        let tracks = discover(source.path()).unwrap();
        assert_eq!(tracks.iter().map(|track| track.name.as_str()).collect::<Vec<_>>(), ["B.mp3", "A.mp3"]);
        let legacy = vec![tracks[1].clone(), tracks[0].clone()];
        let book_identity = folder_hash(source.path()).unwrap();
        let legacy_archive = write_archive(io::Cursor::new(Vec::new()), &legacy).unwrap().into_inner();
        assert_ne!(book_identity, blake3::hash(&legacy_archive));
        let archive = write_archive(io::Cursor::new(Vec::new()), &tracks).unwrap().into_inner();
        assert_eq!(archive_hash(io::Cursor::new(&archive)).unwrap(), book_identity);
        assert_eq!(archived_tracks(io::Cursor::new(&archive)).unwrap().iter().map(|track| track.name.as_str()).collect::<Vec<_>>(), ["B.mp3", "A.mp3"]);
    }

    #[test]
    fn incomplete_id3_numbers_leave_filename_order_intact() {
        let source = tempfile::tempdir().unwrap();
        fs::write(source.path().join("A.mp3"), tagged_mp3(&[("TRCK", "2")])).unwrap();
        fs::write(source.path().join("B.mp3"), tagged_mp3(&[])).unwrap();
        assert_eq!(discover(source.path()).unwrap().iter().map(|track| track.name.as_str()).collect::<Vec<_>>(), ["A.mp3", "B.mp3"]);
    }

    #[test]
    fn cue_file_order_overrides_conflicting_id3_numbers() {
        let source = tempfile::tempdir().unwrap();
        fs::write(source.path().join("A.mp3"), tagged_mp3(&[("TRCK", "2")])).unwrap();
        fs::write(source.path().join("B.mp3"), tagged_mp3(&[("TRCK", "1")])).unwrap();
        fs::write(source.path().join("book.cue"), b"FILE \"A.mp3\" MP3\nTRACK 01 AUDIO\nINDEX 01 00:00:00\nFILE \"B.mp3\" MP3\nTRACK 02 AUDIO\nINDEX 01 00:00:00\n").unwrap();
        let tracks = discover(source.path()).unwrap();
        assert_eq!(tracks.iter().map(|track| track.name.as_str()).collect::<Vec<_>>(), ["A.mp3", "B.mp3"]);
        let archive = write_archive_with_sidecars(io::Cursor::new(Vec::new()), &tracks, None, discover_cue(source.path()).unwrap().as_ref(), None).unwrap().into_inner();
        assert_eq!(archive_hash(io::Cursor::new(&archive)).unwrap(), folder_hash(source.path()).unwrap());
    }

    #[test]
    fn cue_full_paths_disambiguate_matching_disc_filenames() {
        let source = tempfile::tempdir().unwrap();
        fs::create_dir(source.path().join("Disc 1")).unwrap();
        fs::create_dir(source.path().join("Disc 2")).unwrap();
        fs::write(source.path().join("Disc 1/01.mp3"), tagged_mp3(&[])).unwrap();
        fs::write(source.path().join("Disc 2/01.mp3"), tagged_mp3(&[])).unwrap();
        fs::write(source.path().join("book.cue"), b"FILE \"Disc 2/01.mp3\" MP3\nTRACK 01 AUDIO\nINDEX 01 00:00:00\nFILE \"Disc 1/01.mp3\" MP3\nTRACK 02 AUDIO\nINDEX 01 00:00:00\n").unwrap();
        assert_eq!(discover(source.path()).unwrap().iter().map(|track| track.name.as_str()).collect::<Vec<_>>(), ["Disc 2/01.mp3", "Disc 1/01.mp3"]);
    }

    #[test]
    fn folder_round_trip_keeps_disc_order_and_audio_bytes() {
        let source = tempfile::tempdir().unwrap();
        fs::create_dir(source.path().join("Disc 2")).unwrap();
        fs::create_dir(source.path().join("Disc 1")).unwrap();
        fs::write(source.path().join("Disc 2/10 End.mp3"), b"last").unwrap();
        fs::write(source.path().join("Disc 1/2 Middle.mp3"), b"middle").unwrap();
        fs::write(source.path().join("Disc 1/1 Start.mp3"), b"first").unwrap();
        fs::write(source.path().join("book.nfo"), b"Title: A Book\n").unwrap();
        fs::write(source.path().join("book.cue"), b"FILE \"Disc 1/1 Start.mp3\" MP3\n").unwrap();
        fs::write(source.path().join("cover.jpg"), b"image").unwrap();
        let tracks = discover(source.path()).unwrap();
        assert_eq!(tracks.iter().map(|track| track.name.as_str()).collect::<Vec<_>>(), ["Disc 1/1 Start.mp3", "Disc 1/2 Middle.mp3", "Disc 2/10 End.mp3"]);
        let original_identity = folder_hash(source.path()).unwrap();
        let nfo = discover_nfo(source.path()).unwrap();
        let cue = discover_cue(source.path()).unwrap();
        let cover = discover_cover(source.path()).unwrap();
        let bytes = write_archive_with_sidecars(io::Cursor::new(Vec::new()), &tracks, nfo.as_ref(), cue.as_ref(), cover.as_ref()).unwrap().into_inner();
        assert_eq!(write_archive_with_sidecars(io::Cursor::new(Vec::new()), &tracks, nfo.as_ref(), cue.as_ref(), cover.as_ref()).unwrap().into_inner(), bytes);
        assert_eq!(archive_hash(io::Cursor::new(&bytes)).unwrap(), original_identity);
        let index = archived_tracks(io::Cursor::new(&bytes)).unwrap();
        assert_eq!(index.len(), 3);
        for (entry, expected) in index.iter().zip([b"first".as_slice(), b"middle".as_slice(), b"last".as_slice()]) {
            assert_eq!(&bytes[entry.offset as usize..][..entry.length as usize], expected);
        }
        let destination = tempfile::tempdir().unwrap();
        extract_archive_with_names(io::Cursor::new(&bytes), destination.path(), |name| Ok(name.to_owned())).unwrap();
        assert_eq!(folder_hash(destination.path()).unwrap(), original_identity);
        assert_eq!(fs::read(destination.path().join("book.nfo")).unwrap(), b"Title: A Book\n");
        assert_eq!(fs::read(destination.path().join("book.cue")).unwrap(), b"FILE \"Disc 1/1 Start.mp3\" MP3\n");
        assert_eq!(fs::read(destination.path().join("cover.jpg")).unwrap(), b"image");
        fs::write(destination.path().join("cover.jpg"), b"edited image").unwrap();
        assert_ne!(folder_hash(destination.path()).unwrap(), original_identity);

        {
            let mut picture = vec![0];
            picture.extend_from_slice(b"image/png\0");
            picture.push(3);
            picture.push(0);
            picture.extend_from_slice(b"picture bytes");
            let mut frame = Vec::new();
            frame.extend_from_slice(b"APIC");
            frame.extend_from_slice(&(picture.len() as u32).to_be_bytes());
            frame.extend_from_slice(&[0, 0]);
            frame.extend_from_slice(&picture);
            let size = frame.len() as u32;
            let mut bytes = vec![b'I', b'D', b'3', 3, 0, 0, ((size >> 21) & 0x7f) as u8, ((size >> 14) & 0x7f) as u8, ((size >> 7) & 0x7f) as u8, (size & 0x7f) as u8];
            bytes.extend(frame);
            bytes.extend_from_slice(include_bytes!("../../book-inspection/tests/fixtures/silence.mp3"));
            let facts = inspect_mp3(io::Cursor::new(bytes), true).unwrap();
            assert_eq!(facts.artwork.as_deref(), Some(b"picture bytes".as_slice()));
        }
    }

    #[test]
    fn m4b_cover_requires_a_matching_name_or_an_unambiguous_folder() {
        let folder = tempfile::tempdir().unwrap();
        let first = folder.path().join("First.m4b");
        fs::write(&first, b"audio").unwrap();
        fs::write(folder.path().join("cover.jpg"), b"generic").unwrap();
        assert_eq!(discover_m4b_cover(&first).unwrap().unwrap().name, "cover.jpg");
        fs::write(folder.path().join("Second.m4b"), b"audio").unwrap();
        assert!(discover_m4b_cover(&first).unwrap().is_none());
        fs::write(folder.path().join("First.png"), b"specific").unwrap();
        assert_eq!(discover_m4b_cover(&first).unwrap().unwrap().name, "First.png");
    }
}
