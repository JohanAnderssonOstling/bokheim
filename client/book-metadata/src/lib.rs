//! Extracts typed book and audiobook metadata without application or database state.

#[cfg(feature = "native-formats")]
use library_model::BookFormat;
use library_model::{BookMetadataState, Contributor, PublicationMetadataError, AUTHOR_MARC_RELATOR_CODE};
#[cfg(feature = "native-formats")]
use std::collections::HashSet;
use std::path::Path;
#[cfg(feature = "native-formats")]
use std::time::Duration;

mod identity;

pub use identity::{edition_identity_evidence, isbn_from_filename_like_title, EditionIdentityEvidence};

#[cfg(feature = "external-metadata")]
pub mod external;
#[cfg(feature = "external-metadata")]
pub mod librarything;

#[cfg(feature = "native-formats")]
pub use audiobook_library::ChapterOrigin;

pub type MetadataResult<T> = Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Debug, serde::Serialize)]
#[cfg(feature = "native-formats")]
pub struct AudiobookChapterMetadata {
    pub title: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

#[cfg(feature = "native-formats")]
pub struct AudiobookMetadata {
    pub title: String,
    pub authors: Vec<Contributor>,
    pub description: String,
    pub duration_ms: u64,
    pub chapters: Vec<AudiobookChapterMetadata>,
    pub chapter_origin: ChapterOrigin,
}

#[cfg(feature = "native-formats")]
pub struct InspectedBook {
    pub metadata: BookMetadataState,
    pub audiobook: Option<AudiobookMetadata>,
}

pub use publication_inspection::{inspect_epub, inspect_epub_reader, inspect_pdf, inspect_pdf_reader, InspectedEpub, InspectedPdf};

/// Reads all metadata relevant to indexing. Formats without useful embedded
/// metadata receive a title derived from the file name.
#[cfg(feature = "native-formats")]
pub fn inspect_book(path: &Path, format: BookFormat) -> MetadataResult<InspectedBook> {
    let (metadata, audiobook) = match format {
        BookFormat::Epub => (epub_metadata(path)?, None),
        BookFormat::Mobi => (mobi_metadata(path)?, None),
        BookFormat::Fb2 | BookFormat::Cbz | BookFormat::Html | BookFormat::WebPub => return Err(format!("{} metadata is no longer supported", format.canonical_extension()).into()),
        BookFormat::Pdf => (inspect_pdf(path)?.metadata, None),
        BookFormat::M4b => {
            let audiobook = audiobook_metadata(path)?;
            let metadata = BookMetadataState { title: audiobook.title.clone(), contributors: audiobook.authors.clone(), description: audiobook.description.clone(), publication: Default::default() };
            (metadata, Some(audiobook))
        }
    };
    Ok(InspectedBook { metadata, audiobook })
}

/// Applies the same strict metadata validation to publication bytes acquired
/// from a browser picker, catalogue response, or synchronized blob.
#[cfg(feature = "native-formats")]
pub fn inspect_book_bytes(source_name: &str, format: BookFormat, bytes: Vec<u8>) -> MetadataResult<InspectedBook> {
    let source_name = Path::new(source_name);
    let (metadata, audiobook) = match format {
        BookFormat::Epub => (inspect_epub_reader(source_name, std::io::Cursor::new(bytes))?.metadata, None),
        BookFormat::Mobi => (mobi_metadata_from(source_name, mobi::MobiMetadata::new(bytes)?)?, None),
        BookFormat::Fb2 | BookFormat::Cbz | BookFormat::Html | BookFormat::WebPub => return Err(format!("{} metadata is no longer supported", format.canonical_extension()).into()),
        BookFormat::Pdf => (inspect_pdf_reader(source_name, std::io::Cursor::new(bytes))?.metadata, None),
        BookFormat::M4b => {
            let mut reader = std::io::Cursor::new(bytes);
            let audiobook = audiobook_metadata_from(source_name, mp4ameta::Tag::read_from(&mut reader)?)?;
            let metadata = BookMetadataState { title: audiobook.title.clone(), contributors: audiobook.authors.clone(), description: audiobook.description.clone(), publication: Default::default() };
            (metadata, Some(audiobook))
        }
    };
    Ok(InspectedBook { metadata, audiobook })
}

/// Best-effort metadata inspection for a local catalogue scan.
///
/// Import, upload, and reader-open paths use [`inspect_book`] and retain its
/// strict validation. A scanner must still index a supported local file when
/// its embedded metadata is malformed or unavailable, so it falls back to the
/// filename and leaves format validation to the eventual open operation.
#[cfg(feature = "native-formats")]
pub fn inspect_book_for_catalogue(path: &Path, format: BookFormat) -> InspectedBook {
    inspect_book(path, format).unwrap_or_else(|_| InspectedBook { metadata: fallback_metadata(path), audiobook: None })
}

fn fallback_metadata(path: &Path) -> BookMetadataState {
    let title = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or("Untitled").to_owned();
    BookMetadataState { title, contributors: Vec::new(), description: String::new(), publication: Default::default() }
}

fn author_credit(name: String, sort_name: Option<String>) -> Result<Contributor, PublicationMetadataError> {
    Contributor::new(name, sort_name, AUTHOR_MARC_RELATOR_CODE)
}

#[cfg(feature = "native-formats")]
fn mobi_metadata(path: &Path) -> MetadataResult<BookMetadataState> {
    mobi_metadata_from(path, mobi::MobiMetadata::from_path(path)?)
}

#[cfg(feature = "native-formats")]
fn mobi_metadata_from(source_name: &Path, publication: mobi::MobiMetadata) -> MetadataResult<BookMetadataState> {
    if !matches!(publication.encryption(), mobi::headers::Encryption::No) {
        return Err("DRM-protected MOBI publications are not supported".into());
    }
    let fallback = fallback_metadata(source_name);
    let title = publication.title();
    let title = if title.trim().is_empty() { fallback.title } else { title };
    let contributors = publication.author().filter(|author| !author.trim().is_empty()).map(|author| author_credit(author, None)).into_iter().collect::<Result<Vec<_>, _>>()?;
    Ok(BookMetadataState { title, contributors, description: publication.description().unwrap_or_default(), publication: Default::default() })
}

#[cfg(feature = "native-formats")]
fn audiobook_metadata(path: &Path) -> MetadataResult<AudiobookMetadata> {
    audiobook_metadata_from(path, mp4ameta::Tag::read_from_path(path)?)
}

#[cfg(feature = "native-formats")]
fn audiobook_metadata_from(source_name: &Path, tag: mp4ameta::Tag) -> MetadataResult<AudiobookMetadata> {
    let fallback_title = || source_name.file_stem().and_then(|stem| stem.to_str()).unwrap_or("Untitled").to_owned();
    let title = tag.title().map(str::trim).filter(|title| !title.is_empty()).map(str::to_owned).unwrap_or_else(fallback_title);
    let from_name = |name: &str| author_credit(name.to_owned(), None);
    let mut authors = tag.artists().map(str::trim).filter(|author| !author.is_empty()).map(from_name).collect::<Result<Vec<_>, _>>()?;
    if authors.is_empty() {
        authors.extend(tag.album_artists().map(str::trim).filter(|author| !author.is_empty()).map(from_name).collect::<Result<Vec<_>, _>>()?);
    }
    let mut seen = HashSet::new();
    authors.retain(|author| seen.insert((author.name().to_owned(), author.sort_name().map(str::to_owned))));

    let description = tag.description().map(str::trim).unwrap_or_default().to_owned();
    let duration = tag.duration();
    if duration.is_zero() {
        return Err("M4B publication contains no playable audio duration".into());
    }
    let total_duration_ms = duration_ms(duration);
    let embedded = tag.chapters();
    let mut chapters = Vec::with_capacity(embedded.len().max(1));
    for (index, chapter) in embedded.iter().enumerate() {
        let start = chapter.start.min(duration);
        let end = embedded.get(index + 1).map(|next| next.start).unwrap_or(duration).clamp(start, duration);
        chapters.push(AudiobookChapterMetadata { title: if chapter.title.trim().is_empty() { format!("Chapter {}", index + 1) } else { chapter.title.trim().to_owned() }, start_ms: duration_ms(start), end_ms: duration_ms(end) });
    }
    let chapter_origin = if chapters.is_empty() { ChapterOrigin::FullAudiobookFallback } else { ChapterOrigin::EmbeddedM4b };
    if chapters.is_empty() && !duration.is_zero() {
        chapters.push(AudiobookChapterMetadata { title: "Full audiobook".to_owned(), start_ms: 0, end_ms: total_duration_ms });
    }
    Ok(AudiobookMetadata { title, authors, description, duration_ms: total_duration_ms, chapters, chapter_origin })
}

#[cfg(feature = "native-formats")]
fn duration_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(feature = "native-formats")]
fn epub_metadata(path: &Path) -> MetadataResult<BookMetadataState> {
    Ok(publication_inspection::inspect_epub(path)?.metadata)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "native-formats")]
    #[test]
    fn catalogue_inspection_falls_back_when_embedded_metadata_is_invalid() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("Local title.pdf");
        std::fs::write(&path, b"not a valid PDF").unwrap();

        let inspected = inspect_book_for_catalogue(&path, BookFormat::Pdf);

        assert_eq!(inspected.metadata.title, "Local title");
        assert!(inspected.audiobook.is_none());
    }
}
