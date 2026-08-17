//! Extracts typed book and audiobook metadata without application or database state.

use epub_provider::EpubProvider;
use library_model::{marc_relator_code_from_metadata_role, BookFormat, BookMetadataState, AUTHOR_MARC_RELATOR_CODE};
use library_model::{
    Contributor, LanguageTag, PublicationCollection, PublicationDate, PublicationIdentifier, PublicationIdentifierScheme, PublicationIdentifierType, PublicationMetadata, PublicationMetadataError, PublicationProperty, PublicationSubject,
    PublicationTitle, PublicationTocEntry, PublisherCredit,
};
use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

mod pdf;

pub mod external;
pub mod library_of_congress;

pub use audiobook_library::ChapterOrigin;

pub type MetadataResult<T> = Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Debug, serde::Serialize)]
pub struct AudiobookChapterMetadata {
    pub title: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

pub struct AudiobookMetadata {
    pub title: String,
    pub authors: Vec<Contributor>,
    pub description: String,
    pub duration_ms: u64,
    pub chapters: Vec<AudiobookChapterMetadata>,
    pub chapter_origin: ChapterOrigin,
}

pub struct InspectedBook {
    pub metadata: BookMetadataState,
    pub audiobook: Option<AudiobookMetadata>,
}

pub struct InspectedEpub {
    pub metadata: BookMetadataState,
    pub toc: Vec<PublicationTocEntry>,
}

pub use pdf::{inspect_pdf, InspectedPdf};

/// Reads package metadata and navigation from one EPUB provider instance.
/// Catalogue ingestion uses this instead of maintaining a second EPUB
/// metadata parser; ordinary library scans can continue to omit the TOC.
pub fn inspect_epub(path: &Path) -> MetadataResult<InspectedEpub> {
    let provider = std::fs::File::open(path).and_then(EpubProvider::try_from_reader)?;
    let metadata = epub_metadata_from_provider(path, &provider)?;
    let mut count = 0;
    let toc = provider.toc()?.unwrap_or_default().into_iter().filter_map(|entry| publication_toc_entry(entry, 0, &mut count)).collect();
    Ok(InspectedEpub { metadata, toc })
}

fn publication_toc_entry(entry: epub_provider::TocEntry, depth: usize, count: &mut usize) -> Option<PublicationTocEntry> {
    const MAX_TOC_DEPTH: usize = 32;
    const MAX_TOC_ENTRIES: usize = 8_192;
    const MAX_TOC_TEXT_BYTES: usize = 4_096;
    if depth >= MAX_TOC_DEPTH || *count >= MAX_TOC_ENTRIES {
        return None;
    }
    let valid_text = |value: String| {
        let value = value.trim();
        (!value.is_empty() && value.len() <= MAX_TOC_TEXT_BYTES && !value.chars().any(char::is_control)).then(|| value.split_whitespace().collect::<Vec<_>>().join(" "))
    };
    let title = valid_text(entry.title)?;
    let target = valid_text(entry.link).unwrap_or_default();
    *count += 1;
    let children = entry.children.into_iter().filter_map(|entry| publication_toc_entry(entry, depth + 1, count)).collect();
    Some(PublicationTocEntry { title, target, children })
}

/// Reads all metadata relevant to indexing. Formats without useful embedded
/// metadata receive a title derived from the file name.
pub fn inspect_book(path: &Path, format: BookFormat) -> MetadataResult<InspectedBook> {
    let fallback = || fallback_metadata(path);
    let (metadata, audiobook) = match format {
        BookFormat::Epub => (epub_metadata(path)?, None),
        BookFormat::Mobi => (mobi_metadata(path).unwrap_or_else(|_| fallback()), None),
        BookFormat::Fb2 | BookFormat::Cbz | BookFormat::Html | BookFormat::WebPub => (publication_metadata(path).unwrap_or_else(|_| fallback()), None),
        BookFormat::Pdf => (inspect_pdf(path).map(|pdf| pdf.metadata).unwrap_or_else(|_| fallback()), None),
        BookFormat::M4b => match audiobook_metadata(path) {
            Ok(audiobook) => {
                let metadata = BookMetadataState { title: audiobook.title.clone(), contributors: audiobook.authors.clone(), description: audiobook.description.clone(), publication: Default::default() };
                (metadata, Some(audiobook))
            }
            Err(_) => (fallback(), None),
        },
    };
    Ok(InspectedBook { metadata, audiobook })
}

fn fallback_metadata(path: &Path) -> BookMetadataState {
    let title = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or("Untitled").to_owned();
    BookMetadataState { title, contributors: Vec::new(), description: String::new(), publication: Default::default() }
}

fn author_credit(name: String, sort_name: Option<String>) -> Result<Contributor, PublicationMetadataError> {
    Contributor::new(name, sort_name, AUTHOR_MARC_RELATOR_CODE)
}

fn publication_metadata(path: &Path) -> MetadataResult<BookMetadataState> {
    let source = html_publication::PublicationSource::from_path(path.to_path_buf())?;
    let mut authors = source
        .metadata
        .creators
        .iter()
        .filter(|creator| marc_relator_code_from_metadata_role(creator.role.as_str()) == AUTHOR_MARC_RELATOR_CODE)
        .map(|creator| author_credit(creator.name.clone(), creator.file_as.clone()))
        .collect::<Result<Vec<_>, _>>()?;
    if authors.is_empty() {
        authors.extend(source.metadata.creators.first().map(|creator| author_credit(creator.name.clone(), creator.file_as.clone())).transpose()?);
    }
    let title = if source.metadata.title.trim().is_empty() { fallback_metadata(path).title } else { source.metadata.title };
    Ok(BookMetadataState { title, contributors: authors, description: String::new(), publication: Default::default() })
}

fn mobi_metadata(path: &Path) -> MetadataResult<BookMetadataState> {
    // Metadata inspection must not construct the renderer publication: doing so
    // decompresses every text record merely to read the title and author. Some
    // valid legacy MOBI files also use 0xffff for optional content bounds, which
    // mobi 0.8.0 mishandles while decoding text even though its EXTH metadata is
    // readable.
    let book = mobi::MobiMetadata::from_path(path)?;
    let fallback = fallback_metadata(path);
    let title = book.title();
    let title = if title.trim().is_empty() { fallback.title } else { title };
    let contributors = book.author().filter(|author| !author.trim().is_empty()).map(|author| author_credit(author, None)).into_iter().collect::<Result<Vec<_>, _>>()?;
    Ok(BookMetadataState { title, contributors, description: book.description().unwrap_or_default(), publication: Default::default() })
}

fn audiobook_metadata(path: &Path) -> MetadataResult<AudiobookMetadata> {
    let tag = mp4ameta::Tag::read_from_path(path)?;
    let fallback_title = || path.file_stem().and_then(|stem| stem.to_str()).unwrap_or("Untitled").to_owned();
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

fn duration_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn non_empty_optional(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

fn publication_properties(properties: Vec<epub_provider::EpubMetadataProperty>) -> Vec<PublicationProperty> {
    // EPUB package metadata is extensible. A malformed, oversized, or
    // unsupported optional property must not make core metadata such as title
    // and authors unusable. Keep every property that fits the synchronized
    // model and omit only the property that does not.
    properties
        .into_iter()
        .filter_map(|property| {
            PublicationProperty::new(non_empty_optional(property.id), property.property, property.value, non_empty_optional(property.refines), non_empty_optional(property.scheme), non_empty_optional(property.language)).ok()
        })
        .collect()
}

fn epub_metadata(path: &Path) -> MetadataResult<BookMetadataState> {
    let provider = std::fs::File::open(path).and_then(EpubProvider::try_from_reader)?;
    epub_metadata_from_provider(path, &provider)
}

fn epub_metadata_from_provider(path: &Path, provider: &EpubProvider) -> MetadataResult<BookMetadataState> {
    let metadata = provider.metadata()?;
    let mut authors = metadata
        .creators
        .iter()
        .filter(|creator| marc_relator_code_from_metadata_role(creator.role.as_str()) == AUTHOR_MARC_RELATOR_CODE)
        .map(|creator| author_credit(creator.name.clone(), creator.file_as.clone()))
        .collect::<Result<Vec<_>, _>>()?;
    if authors.is_empty() {
        authors.extend(metadata.creators.first().map(|creator| author_credit(creator.name.clone(), creator.file_as.clone())).transpose()?);
    }

    let mut identifiers = metadata
        .identifiers
        .into_iter()
        .map(|identifier| {
            let mut scheme = match identifier.scheme {
                epub_provider::IdentifierScheme::Unspecified => PublicationIdentifierScheme::Unspecified,
                epub_provider::IdentifierScheme::ISBN => PublicationIdentifierScheme::Isbn,
                epub_provider::IdentifierScheme::ASIN => PublicationIdentifierScheme::Asin,
                epub_provider::IdentifierScheme::DOI => PublicationIdentifierScheme::Doi,
                epub_provider::IdentifierScheme::ISSN => PublicationIdentifierScheme::Issn,
                epub_provider::IdentifierScheme::OCLC => PublicationIdentifierScheme::Oclc,
                epub_provider::IdentifierScheme::LCCN => PublicationIdentifierScheme::Lccn,
                epub_provider::IdentifierScheme::UUID => PublicationIdentifierScheme::Uuid,
                epub_provider::IdentifierScheme::CALIBRE => PublicationIdentifierScheme::Calibre,
                epub_provider::IdentifierScheme::GOOG => PublicationIdentifierScheme::Google,
                epub_provider::IdentifierScheme::GOODREADS => PublicationIdentifierScheme::Goodreads,
                epub_provider::IdentifierScheme::KOBO => PublicationIdentifierScheme::Kobo,
                epub_provider::IdentifierScheme::Other(value) => PublicationIdentifierScheme::Other(value),
            };
            let mut identifier_types = identifier.identifier_types.into_iter().map(|kind| PublicationIdentifierType::new(kind.value, kind.scheme)).collect::<Result<Vec<_>, _>>()?;
            // Some producers emit a bare, checksum-valid ISBN without a
            // scheme, or attach an incorrect proprietary scheme. The value is
            // stronger evidence than that missing/mislabelled declaration.
            if library_model::canonical_isbn(&identifier.value).is_some() && scheme != PublicationIdentifierScheme::Isbn {
                scheme = PublicationIdentifierScheme::Isbn;
                identifier_types.push(PublicationIdentifierType::new("checksum-inferred", Some("bokheim:provenance".to_owned()))?);
            }
            PublicationIdentifier::new(identifier.value, scheme, identifier_types)
        })
        .collect::<Result<Vec<_>, PublicationMetadataError>>()?;
    let contributor = |creator: epub_provider::EpubCreator| Contributor::new(creator.name, creator.file_as, marc_relator_code_from_metadata_role(creator.role.as_str()));
    let non_author_contributors =
        metadata.contributors.into_iter().chain(metadata.creators.into_iter().filter(|creator| marc_relator_code_from_metadata_role(creator.role.as_str()) != AUTHOR_MARC_RELATOR_CODE)).map(contributor).collect::<Result<Vec<_>, _>>()?;
    let properties = publication_properties(metadata.properties);
    if let Some(isbn) = isbn_from_opf_path(provider.package_document_path()) {
        push_epub_isbn(&mut identifiers, isbn, "opf-filename")?;
    }
    for property in &properties {
        let name = property.property().rsplit(':').next().unwrap_or_default();
        if name.eq_ignore_ascii_case("isbn") || name.eq_ignore_ascii_case("identifier") {
            if let Some(isbn) = library_model::canonical_isbn(property.value()) {
                push_epub_isbn(&mut identifiers, isbn, "opf-property")?;
            }
        }
    }
    let collections = properties
        .iter()
        .filter(|property| property.property().rsplit(':').next() == Some("belongs-to-collection"))
        .filter_map(|property| {
            let target = property.id().map(|id| format!("#{id}"));
            let refinement = |name: &str| properties.iter().find(|candidate| candidate.refines() == target.as_deref() && candidate.property().rsplit(':').next() == Some(name)).map(|candidate| candidate.value().to_owned());
            PublicationCollection::new(property.value(), refinement("collection-type"), refinement("group-position")).ok()
        })
        .collect();
    let title = if metadata.title.trim().is_empty() { fallback_metadata(path).title } else { metadata.title };
    let publication = PublicationMetadata {
        identifiers,
        publishers: metadata.publishers.into_iter().map(PublisherCredit::new).collect::<Result<Vec<_>, _>>()?,
        languages: metadata.languages.into_iter().map(LanguageTag::parse).collect::<Result<Vec<_>, _>>()?,
        dates: metadata.dates.into_iter().filter_map(|date| PublicationDate::new(non_empty_optional(date.id), date.value, non_empty_optional(date.event)).ok()).collect(),
        subjects: metadata.subjects.into_iter().map(|subject| PublicationSubject::new(subject.id, subject.value, subject.source, subject.authority, subject.code)).collect::<Result<Vec<_>, _>>()?,
        collections,
        alternate_titles: metadata.alternate_titles.into_iter().map(|title| PublicationTitle::new(title.id, title.value, title.title_type, title.sort_as)).collect::<Result<Vec<_>, _>>()?,
        rights: metadata.rights,
        sources: metadata.sources,
        types: metadata.types,
        formats: metadata.formats,
        relations: metadata.relations,
        coverage: metadata.coverage,
        properties,
    };
    let contributors = authors.into_iter().chain(non_author_contributors).collect();
    Ok(BookMetadataState { title, contributors, description: metadata.description, publication })
}

fn isbn_from_opf_path(path: &str) -> Option<String> {
    let file_name = path.rsplit('/').next()?;
    let stem = file_name.strip_suffix(".opf").or_else(|| file_name.strip_suffix(".OPF"))?;
    let stem = stem.trim().trim_start_matches(|character: char| matches!(character, '_' | '-'));
    let stem = stem.strip_prefix("ISBN").or_else(|| stem.strip_prefix("isbn")).map(|value| value.trim_start_matches(|character: char| matches!(character, '_' | '-' | ':' | ' '))).unwrap_or(stem);
    library_model::canonical_isbn(stem)
}

fn push_epub_isbn(identifiers: &mut Vec<PublicationIdentifier>, isbn: String, provenance: &str) -> Result<(), PublicationMetadataError> {
    if identifiers.iter().any(|identifier| identifier.scheme() == &PublicationIdentifierScheme::Isbn && identifier.canonical_value().as_deref() == Some(isbn.as_str())) {
        return Ok(());
    }
    let evidence = PublicationIdentifierType::new(provenance, Some("bokheim:provenance".to_owned()))?;
    identifiers.push(PublicationIdentifier::new(isbn, PublicationIdentifierScheme::Isbn, vec![evidence])?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn property(value: String) -> epub_provider::EpubMetadataProperty {
        epub_provider::EpubMetadataProperty { id: None, property: "schema:accessibilitySummary".to_owned(), value, refines: None, scheme: None, language: None }
    }

    #[test]
    fn invalid_optional_epub_properties_are_omitted_individually() {
        let properties = publication_properties(vec![property("textual".to_owned()), property("multiline\nvalue".to_owned()), property("x".repeat(4097))]);

        assert_eq!(properties.len(), 1);
        assert_eq!(properties[0].value(), "textual");
    }

    #[test]
    fn empty_optional_epub_attributes_are_absent_not_errors() {
        let properties =
            publication_properties(vec![epub_provider::EpubMetadataProperty { id: Some("".to_owned()), property: "schema:accessMode".to_owned(), value: "textual".to_owned(), refines: Some("  ".to_owned()), scheme: None, language: None }]);

        assert_eq!(properties.len(), 1);
        assert_eq!(properties[0].id(), None);
        assert_eq!(properties[0].refines(), None);
    }

    #[test]
    fn opf_filename_isbn_requires_a_valid_checksum() {
        assert_eq!(isbn_from_opf_path("OPS/9781501168697.opf").as_deref(), Some("9781501168697"));
        assert_eq!(isbn_from_opf_path("OPS/ISBN_978-1-5011-6869-7.OPF").as_deref(), Some("9781501168697"));
        assert_eq!(isbn_from_opf_path("OPS/9781501168698.opf"), None);
        assert_eq!(isbn_from_opf_path("OPS/package.opf"), None);
    }
}
