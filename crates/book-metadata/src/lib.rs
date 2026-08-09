//! Extracts typed book and audiobook metadata without application or database state.

use domain::{marc_relator_code_from_metadata_role, BookFormat, BookMetadataState, AUTHOR_MARC_RELATOR_CODE};
use epub_provider::EpubProvider;
use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;
use sync_common::{
    Contributor, LanguageTag, PublicationCollection, PublicationDate, PublicationIdentifier, PublicationIdentifierScheme, PublicationIdentifierType, PublicationMetadata, PublicationProperty,
    PublicationSubject, PublicationTitle, PublisherCredit,
};

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

/// Reads all metadata relevant to indexing. Formats without useful embedded
/// metadata receive a title derived from the file name.
pub fn inspect_book(path: &Path, format: BookFormat) -> MetadataResult<InspectedBook> {
    let fallback = || fallback_metadata(path);
    let (metadata, audiobook) = match format {
        BookFormat::Epub => (epub_metadata(path)?, None),
        BookFormat::Mobi | BookFormat::Fb2 | BookFormat::Cbz | BookFormat::Html | BookFormat::WebPub => (publication_metadata(path).unwrap_or_else(|_| fallback()), None),
        BookFormat::Pdf => (fallback(), None),
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

fn author_credit(name: String, sort_name: Option<String>) -> Result<Contributor, sync_common::PublicationMetadataError> {
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

fn epub_metadata(path: &Path) -> MetadataResult<BookMetadataState> {
    let provider = std::fs::File::open(path).and_then(EpubProvider::try_from_reader)?;
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

    let identifiers = metadata
        .identifiers
        .into_iter()
        .map(|identifier| {
            let scheme = match identifier.scheme {
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
            let identifier_types = identifier.identifier_types.into_iter().map(|kind| PublicationIdentifierType::new(kind.value, kind.scheme)).collect::<Result<Vec<_>, _>>()?;
            PublicationIdentifier::new(identifier.value, scheme, identifier_types)
        })
        .collect::<Result<Vec<_>, sync_common::PublicationMetadataError>>()?;
    let contributor = |creator: epub_provider::EpubCreator| Contributor::new(creator.name, creator.file_as, marc_relator_code_from_metadata_role(creator.role.as_str()));
    let non_author_contributors =
        metadata.contributors.into_iter().chain(metadata.creators.into_iter().filter(|creator| marc_relator_code_from_metadata_role(creator.role.as_str()) != AUTHOR_MARC_RELATOR_CODE)).map(contributor).collect::<Result<Vec<_>, _>>()?;
    let properties = metadata.properties.into_iter().map(|property| PublicationProperty::new(property.id, property.property, property.value, property.refines, property.scheme, property.language)).collect::<Result<Vec<_>, _>>()?;
    let collections = properties
        .iter()
        .filter(|property| property.property().rsplit(':').next() == Some("belongs-to-collection"))
        .map(|property| {
            let target = property.id().map(|id| format!("#{id}"));
            let refinement = |name: &str| properties.iter().find(|candidate| candidate.refines() == target.as_deref() && candidate.property().rsplit(':').next() == Some(name)).map(|candidate| candidate.value().to_owned());
            PublicationCollection::new(property.value(), refinement("collection-type"), refinement("group-position"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let title = if metadata.title.trim().is_empty() { fallback_metadata(path).title } else { metadata.title };
    let publication = PublicationMetadata {
        identifiers,
        publishers: metadata.publishers.into_iter().map(PublisherCredit::new).collect::<Result<Vec<_>, _>>()?,
        languages: metadata.languages.into_iter().map(LanguageTag::parse).collect::<Result<Vec<_>, _>>()?,
        dates: metadata.dates.into_iter().map(|date| PublicationDate::new(date.id, date.value, date.event)).collect::<Result<Vec<_>, _>>()?,
        subjects: metadata.subjects.into_iter().map(|subject| PublicationSubject::new(subject.id, subject.value, subject.authority, subject.code)).collect::<Result<Vec<_>, _>>()?,
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
