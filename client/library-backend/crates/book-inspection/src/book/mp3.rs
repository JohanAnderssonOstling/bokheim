//! MP3 track metadata shared by archive imports and native folder inspection.

use super::{author_credit, cue, decode_sidecar, nfo, recording_book, AudiobookMetadata};
use crate::MetadataResult;
use audiobook_archive::ArchivedTrack;
use audiobook_folder::Mp3Facts;
use book_model::{AudiobookChapter, AudiobookTrack, BookRecord, ChapterOrigin};
use content_address::{hash_file, BookId};
#[cfg(not(target_arch = "wasm32"))]
use std::collections::HashMap;
#[cfg(target_arch = "wasm32")]
use std::io::Cursor;
use std::io::{Read, Seek};
use std::path::Path;

pub(super) fn inspect_archive<R: Read + Seek>(source_name: &Path, reader: R, book_id: Option<BookId>) -> MetadataResult<(BookRecord, AudiobookMetadata)> {
    inspect(source_name, reader, book_id, |archive, index, _| {
        #[cfg(not(target_arch = "wasm32"))]
        let facts = {
            let mut audio = tempfile::tempfile()?;
            std::io::copy(&mut archive.by_index(index)?, &mut audio)?;
            audio.rewind()?;
            audiobook_folder::inspect_mp3(audio, false)?
        };
        #[cfg(target_arch = "wasm32")]
        let facts = {
            let mut bytes = Vec::new();
            archive.by_index(index)?.read_to_end(&mut bytes)?;
            audiobook_folder::inspect_mp3(Cursor::new(bytes), false)?
        };
        Ok(facts)
    })
}

/// Read native tracks directly. The archive supplies only the canonical
/// playback order, byte offsets and sidecars, preserving transfer identity.
#[cfg(not(target_arch = "wasm32"))]
pub fn inspect_folder_reader<R: Read + Seek, M: Read + Seek + Send + Sync + 'static>(
    source_name: &Path, reader: R, book_id: BookId, tracks: &[audiobook_archive::Track], mut open: impl FnMut(&Path) -> std::io::Result<M>,
) -> MetadataResult<super::InspectedBook> {
    let paths = tracks.iter().map(|track| (track.name.as_str(), track.path.as_path())).collect::<HashMap<_, _>>();
    let (mut metadata, audiobook) = inspect(source_name, reader, Some(book_id), |_, _, track| {
        let path = paths.get(track.name.as_str()).ok_or("archived MP3 is missing from the native folder")?;
        Ok(audiobook_folder::inspect_mp3_reader(open(path)?, track.length, false)?)
    })?;
    crate::normalize_with_filename(&mut metadata, &source_name.to_string_lossy())?;
    Ok(super::InspectedBook { metadata, audiobook: Some(audiobook), toc: Vec::new(), pdf: None })
}

fn inspect<R: Read + Seek>(
    source_name: &Path, mut reader: R, book_id: Option<BookId>, mut inspect_track: impl FnMut(&mut zip::ZipArchive<R>, usize, &ArchivedTrack) -> MetadataResult<Mp3Facts>,
) -> MetadataResult<(BookRecord, AudiobookMetadata)> {
    let book_id = match book_id {
        Some(book_id) => book_id,
        None => BookId::from(hash_file(&mut reader)?),
    };
    let tracks = audiobook_archive::archived_tracks(&mut reader)?;
    let mut archive = zip::ZipArchive::new(reader)?;
    let facts = tracks.iter().enumerate().map(|(index, track)| inspect_track(&mut archive, index, track)).collect::<MetadataResult<Vec<_>>>()?;
    let mut sidecars = Vec::new();
    for index in tracks.len()..archive.len() {
        let entry = archive.by_index(index)?;
        let name = entry.name().to_ascii_lowercase();
        let maximum = if name.ends_with(".nfo") {
            audiobook_archive::MAX_NFO_BYTES
        } else if name.ends_with(".cue") {
            audiobook_archive::MAX_CUE_BYTES
        } else {
            continue;
        };
        let mut bytes = Vec::new();
        entry.take(maximum + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > maximum {
            return Err("audiobook sidecar exceeds byte limit".into());
        }
        sidecars.push((name, bytes));
    }
    assemble(source_name, book_id, &tracks, facts, sidecars)
}

/// Inspect original files using the content manifest's inexpensive layout.
#[cfg(not(target_arch = "wasm32"))]
pub fn inspect_folder_content<M: Read + Seek + Send + Sync + 'static>(
    source_name: &Path, folder: &Path, book_id: BookId, members: &[ArchivedTrack], tracks: &[audiobook_archive::Track], mut open: impl FnMut(&Path) -> std::io::Result<M>,
) -> MetadataResult<super::InspectedBook> {
    let paths = tracks.iter().map(|track| (track.name.as_str(), track.path.as_path())).collect::<HashMap<_, _>>();
    let facts = members
        .iter()
        .map(|track| {
            let path = paths.get(track.name.as_str()).ok_or("MP3 is missing from the native folder")?;
            Ok(audiobook_folder::inspect_mp3_reader(open(path)?, track.length, false)?)
        })
        .collect::<MetadataResult<Vec<_>>>()?;
    let mut sidecars = Vec::new();
    for sidecar in audiobook_folder::metadata_sidecars(folder)? {
        let maximum = if sidecar.name.to_ascii_lowercase().ends_with(".nfo") { audiobook_archive::MAX_NFO_BYTES } else { audiobook_archive::MAX_CUE_BYTES };
        let mut bytes = Vec::new();
        open(&sidecar.path)?.take(maximum + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > maximum {
            return Err("audiobook sidecar exceeds byte limit".into());
        }
        sidecars.push((sidecar.name.to_ascii_lowercase(), bytes));
    }
    let (mut metadata, audiobook) = assemble(source_name, book_id, members, facts, sidecars)?;
    crate::normalize_with_filename(&mut metadata, &source_name.to_string_lossy())?;
    Ok(super::InspectedBook { metadata, audiobook: Some(audiobook), toc: Vec::new(), pdf: None })
}

fn assemble(source_name: &Path, book_id: BookId, tracks: &[ArchivedTrack], facts: Vec<Mp3Facts>, sidecars: Vec<(String, Vec<u8>)>) -> MetadataResult<(BookRecord, AudiobookMetadata)> {
    let mut chapters = Vec::with_capacity(tracks.len());
    let mut track_index = Vec::with_capacity(tracks.len());
    let mut embedded = Vec::with_capacity(tracks.len());
    let mut total = 0u64;
    for (index, (track, facts)) in tracks.iter().zip(facts).enumerate() {
        let duration = facts.duration_ms;
        let stem = Path::new(&track.name).file_stem().and_then(|value| value.to_str()).unwrap_or(&track.name);
        let title = facts.track_title.as_deref().filter(|title| !title.trim().is_empty()).map(str::to_owned).unwrap_or_else(|| book_model::audiobook_chapter_display_title(stem, index + 1));
        let end = total.checked_add(duration).ok_or("audiobook duration overflow")?;
        chapters.push(AudiobookChapter { title, start_ms: total, end_ms: end });
        track_index.push(AudiobookTrack { name: track.name.clone(), start_ms: total, end_ms: end, offset: track.offset, length: track.length, archive_hash: Some(book_id) });
        embedded.push(facts);
        total = end;
    }
    let default_title = source_name.file_stem().and_then(|value| value.to_str()).unwrap_or("Untitled").to_owned();
    let mut audiobook =
        AudiobookMetadata { title: default_title, authors: Vec::new(), description: String::new(), duration_ms: total, chapters, tracks: track_index, chapter_origin: ChapterOrigin::TrackFiles, recording: Default::default() };
    if let Some(album) = embedded.iter().find_map(|facts| facts.album.as_deref()) {
        audiobook.title = album.to_owned();
    }
    if let Some(author) = embedded.iter().find_map(|facts| facts.album_artist.as_deref()).or_else(|| embedded.iter().find_map(|facts| facts.artist.as_deref())) {
        audiobook.authors = author.split(';').filter_map(|name| author_credit(name.trim().to_owned()).ok()).collect();
    }
    if let Some(description) = embedded.iter().find_map(|facts| facts.description.as_deref()) {
        audiobook.description = description.to_owned();
    }
    if let Some(narrator) = embedded.iter().find_map(|facts| facts.narrator.as_deref()) {
        audiobook.recording.narrators.push(narrator.to_owned());
    }
    if let Some(publisher) = embedded.iter().find_map(|facts| facts.publisher.as_deref()) {
        audiobook.recording.publishers.push(publisher.to_owned());
    }
    if let Some(asin) = embedded.iter().find_map(|facts| facts.asin.as_deref()).filter(|value| value.len() == 10 && value.bytes().all(|byte| byte.is_ascii_alphanumeric())) {
        audiobook.recording.asins.push(asin.to_ascii_uppercase());
    }
    if let Some(isbn) = embedded.iter().find_map(|facts| facts.isbn.as_deref()).and_then(book_model::from_metadata_value) {
        audiobook.recording.isbns.push(isbn);
    }
    if let Some(date) = embedded.iter().find_map(|facts| facts.release_date.as_deref()) {
        if date.len() == 4 && date.bytes().all(|byte| byte.is_ascii_digit()) {
            audiobook.recording.recording_release_year = date.parse::<i32>().ok().filter(|year| (1000..=2100).contains(year));
        } else {
            audiobook.recording.recording_release_date = Some(date.to_owned());
        }
    }
    let mut book = recording_book(&audiobook.recording)?;
    for (name, bytes) in sidecars {
        if name.ends_with(".nfo") {
            if bytes.len() as u64 > audiobook_archive::MAX_NFO_BYTES {
                return Err("audiobook NFO is too large".into());
            }
            book = nfo::apply(&decode_sidecar(&bytes), &mut audiobook)?;
        } else if name.ends_with(".cue") {
            if bytes.len() as u64 > audiobook_archive::MAX_CUE_BYTES {
                return Err("audiobook CUE is too large".into());
            }
            let text = decode_sidecar(&bytes);
            cue::apply_credits(&text, &mut audiobook);
            match cue::chapters(&text, &audiobook.tracks) {
                Ok(chapters) => {
                    audiobook.chapters = chapters;
                    audiobook.chapter_origin = ChapterOrigin::CueSheet;
                }
                Err(error) => log::warn!("ignoring invalid audiobook CUE: {error}"),
            }
        }
    }
    let metadata =
        BookRecord { metadata: book_model::BookDetails { title: audiobook.title.clone(), subtitle: audiobook.recording.subtitle.clone(), contributors: audiobook.authors.clone(), book }, description: audiobook.description.clone() };
    Ok((metadata, audiobook))
}
