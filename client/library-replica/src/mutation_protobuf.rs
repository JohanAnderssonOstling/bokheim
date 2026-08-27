//! Explicit protobuf mapping for client-owned mutation payloads.
//!
//! The service deliberately treats the resulting bytes as opaque. Compatibility
//! is governed by the stable field numbers in `bokheim_wire.proto`, not by the
//! Rust/Serde representation used for local persistence.

use super::{catalog_publication_state_subkey, MutationBody};
use catalog_model::{CatalogPublicationIdentity, CatalogSourceId};
use library_model::{
    AnnotationAnchor, AnnotationState, AnnotationStyle, BookFormat, BookLifecycleState, BookMetadataState, Collection, CollectionKind, Contributor, DirectoryLifecycleState, Identifier, LanguageTag, LocalizedText, MarcRelatorCode,
    PdfAnnotationAnchor, PdfAnnotationRect, PublicationDate, PublicationMetadata, PublicationProperty, PublicationSubject, PublicationTitle, PublisherCredit, ReadingPosition, ReadingPositionState, Scheme, Scope,
};
use sync_common::transport::{self, protobuf as pb};
use sync_common::{ContentHash, WireMutation};

fn decode_error(message: impl Into<String>) -> transport::WireError {
    transport::WireError::Decode(message.into())
}

fn required<T>(value: Option<T>, field: &str) -> Result<T, transport::WireError> {
    value.ok_or_else(|| decode_error(format!("mutation protobuf is missing {field}")))
}

pub(super) fn encode(body: &MutationBody) -> Result<Vec<u8>, transport::WireError> {
    use pb::mutation_value::Kind;
    let kind = match body {
        MutationBody::DirectoryName { name, .. } => Kind::DirectoryName(name.clone()),
        MutationBody::DirectoryParent { parent_id, .. } => Kind::DirectoryParent(parent_id.to_string()),
        MutationBody::DirectoryLifecycle { value, .. } => Kind::DirectoryLifecycle(encode_directory_lifecycle(*value) as i32),
        MutationBody::BookLifecycle { value, .. } => Kind::BookLifecycle(encode_book_lifecycle(value)),
        MutationBody::Placement { present, origin_folder_id, .. } => Kind::Placement(pb::Placement { present: *present, origin_folder_id: origin_folder_id.clone() }),
        MutationBody::ReadingPosition { value, .. } => Kind::ReadingPosition(encode_reading_position(value)),
        MutationBody::Annotation { value, .. } => Kind::Annotation(encode_annotation(value)),
        MutationBody::CatalogPublication { publication, content_hash, .. } => {
            Kind::CatalogPublication(pb::CatalogPublication { edition_id: publication.edition_id.clone(), work_id: publication.work_id.clone(), content_hash: content_hash.to_string() })
        }
        MutationBody::Metadata { value, .. } => Kind::Metadata(encode_book_metadata(value)),
    };
    transport::encode_mutation_value(&pb::MutationValue { kind: Some(kind) })
}

pub(super) fn decode(wire: &WireMutation) -> Result<MutationBody, transport::WireError> {
    use pb::mutation_value::Kind;
    let value = transport::decode_mutation_value(&wire.value, transport::MAX_DECODED_REQUEST_BYTES)?;
    let kind = required(value.kind, "value")?;
    let key = wire.entity_key.as_str();
    match (wire.kind.as_str(), kind) {
        (sync_common::mutation_kind::DIRECTORY_NAME, Kind::DirectoryName(name)) => Ok(MutationBody::DirectoryName { dir_id: parse_uuid(key, "directory id")?, name }),
        (sync_common::mutation_kind::DIRECTORY_PARENT, Kind::DirectoryParent(parent_id)) => Ok(MutationBody::DirectoryParent { dir_id: parse_uuid(key, "directory id")?, parent_id: parse_uuid(&parent_id, "parent directory id")? }),
        (sync_common::mutation_kind::DIRECTORY_LIFECYCLE, Kind::DirectoryLifecycle(value)) => Ok(MutationBody::DirectoryLifecycle { dir_id: parse_uuid(key, "directory id")?, value: decode_directory_lifecycle(value)? }),
        (sync_common::mutation_kind::BOOK_LIFECYCLE, Kind::BookLifecycle(value)) => Ok(MutationBody::BookLifecycle { content_hash: ContentHash::new(key), value: decode_book_lifecycle(value)? }),
        (sync_common::mutation_kind::PLACEMENT, Kind::Placement(value)) => {
            Ok(MutationBody::Placement { dir_id: parse_uuid(&wire.entity_subkey, "placement directory id")?, content_hash: ContentHash::new(key), present: value.present, origin_folder_id: value.origin_folder_id })
        }
        (sync_common::mutation_kind::READING_POSITION, Kind::ReadingPosition(value)) => Ok(MutationBody::ReadingPosition { content_hash: ContentHash::new(key), value: decode_reading_position(value)? }),
        (sync_common::mutation_kind::ANNOTATION, Kind::Annotation(value)) => Ok(MutationBody::Annotation { annotation_id: key.to_owned(), value: decode_annotation(value)? }),
        (sync_common::mutation_kind::CATALOG_PUBLICATION, Kind::CatalogPublication(value)) => {
            let source_id = CatalogSourceId::parse(key).map_err(decode_error)?;
            let publication = CatalogPublicationIdentity { edition_id: value.edition_id, work_id: value.work_id };
            let expected_subkey = catalog_publication_state_subkey(&publication.edition_id, publication.work_id.as_deref());
            if wire.entity_subkey != expected_subkey {
                return Err(decode_error("catalog publication identity does not match its register subkey"));
            }
            Ok(MutationBody::CatalogPublication { source_id, publication, content_hash: ContentHash::new(&value.content_hash) })
        }
        (sync_common::mutation_kind::METADATA, Kind::Metadata(value)) => Ok(MutationBody::Metadata { content_hash: ContentHash::new(key), value: decode_book_metadata(value)? }),
        _ => Err(decode_error("mutation kind does not match its protobuf value")),
    }
}

fn parse_uuid(value: &str, field: &str) -> Result<sync_common::DirId, transport::WireError> {
    sync_common::DirId::parse_str(value).map_err(|error| decode_error(format!("invalid {field}: {error}")))
}

fn encode_directory_lifecycle(value: DirectoryLifecycleState) -> pb::DirectoryLifecycle {
    match value {
        DirectoryLifecycleState::Present => pb::DirectoryLifecycle::Present,
        DirectoryLifecycleState::Deleted => pb::DirectoryLifecycle::Deleted,
        DirectoryLifecycleState::Purged => pb::DirectoryLifecycle::Purged,
    }
}

fn decode_directory_lifecycle(value: i32) -> Result<DirectoryLifecycleState, transport::WireError> {
    match pb::DirectoryLifecycle::try_from(value).map_err(|_| decode_error("unknown directory lifecycle"))? {
        pb::DirectoryLifecycle::Present => Ok(DirectoryLifecycleState::Present),
        pb::DirectoryLifecycle::Deleted => Ok(DirectoryLifecycleState::Deleted),
        pb::DirectoryLifecycle::Purged => Ok(DirectoryLifecycleState::Purged),
        pb::DirectoryLifecycle::Unspecified => Err(decode_error("unspecified directory lifecycle")),
    }
}

fn encode_book_lifecycle(value: &BookLifecycleState) -> pb::BookLifecycle {
    use pb::book_lifecycle::State;
    let state = match value {
        BookLifecycleState::Present { metadata, added_at, format } => State::Present(pb::PresentBook { metadata: Some(encode_book_metadata(metadata)), added_at: *added_at, format: encode_book_format(*format) as i32 }),
        BookLifecycleState::Deleted { origin_folder_id } => State::Deleted(pb::DeletedBook { origin_folder_id: origin_folder_id.clone() }),
        BookLifecycleState::Purged => State::Purged(pb::PurgedBook {}),
    };
    pb::BookLifecycle { state: Some(state) }
}

fn decode_book_lifecycle(value: pb::BookLifecycle) -> Result<BookLifecycleState, transport::WireError> {
    use pb::book_lifecycle::State;
    match required(value.state, "book lifecycle state")? {
        State::Present(value) => Ok(BookLifecycleState::Present { metadata: decode_book_metadata(required(value.metadata, "present-book metadata")?)?, added_at: value.added_at, format: decode_book_format(value.format)? }),
        State::Deleted(value) => Ok(BookLifecycleState::Deleted { origin_folder_id: value.origin_folder_id }),
        State::Purged(_) => Ok(BookLifecycleState::Purged),
    }
}

fn encode_book_format(value: BookFormat) -> pb::BookFormat {
    match value {
        BookFormat::Epub => pb::BookFormat::Epub,
        BookFormat::Pdf => pb::BookFormat::Pdf,
        BookFormat::Mobi => pb::BookFormat::Mobi,
        BookFormat::Fb2 => pb::BookFormat::Fb2,
        BookFormat::Cbz => pb::BookFormat::Cbz,
        BookFormat::Html => pb::BookFormat::Html,
        BookFormat::WebPub => pb::BookFormat::Webpub,
        BookFormat::M4b => pb::BookFormat::M4b,
    }
}

fn decode_book_format(value: i32) -> Result<BookFormat, transport::WireError> {
    match pb::BookFormat::try_from(value).map_err(|_| decode_error("unknown book format"))? {
        pb::BookFormat::Epub => Ok(BookFormat::Epub),
        pb::BookFormat::Pdf => Ok(BookFormat::Pdf),
        pb::BookFormat::Mobi => Ok(BookFormat::Mobi),
        pb::BookFormat::Fb2 => Ok(BookFormat::Fb2),
        pb::BookFormat::Cbz => Ok(BookFormat::Cbz),
        pb::BookFormat::Html => Ok(BookFormat::Html),
        pb::BookFormat::Webpub => Ok(BookFormat::WebPub),
        pb::BookFormat::M4b => Ok(BookFormat::M4b),
        pb::BookFormat::Unspecified => Err(decode_error("unspecified book format")),
    }
}

fn encode_reading_position(value: &ReadingPositionState) -> pb::ReadingPosition {
    use pb::reading_position::Location;
    let location = match &value.location {
        ReadingPosition::Epub(_) => Location::EpubCfi(value.location.as_str()),
        ReadingPosition::Pdf(page) => Location::PdfPage(*page),
        ReadingPosition::PdfAtPoint { page, full_page_position } => {
            return pb::ReadingPosition { location: Some(Location::PdfPage(*page)), pdf_full_page_position: Some(full_page_position.max(0.0)), progress: value.progress };
        }
        ReadingPosition::Audiobook(millis) => Location::AudiobookMillis(*millis),
    };
    pb::ReadingPosition { location: Some(location), progress: value.progress, pdf_full_page_position: None }
}

fn decode_reading_position(value: pb::ReadingPosition) -> Result<ReadingPositionState, transport::WireError> {
    use pb::reading_position::Location;
    let full_page_position = value.pdf_full_page_position;
    let location = match required(value.location, "reading location")? {
        Location::EpubCfi(value) => ReadingPosition::epub_cfi(&value).map_err(|_| decode_error("invalid EPUB reading position"))?,
        Location::PdfPage(page) => match full_page_position {
            Some(full_page_position) if full_page_position.is_finite() => ReadingPosition::pdf_page_at_point(page, full_page_position.max(0.0)),
            Some(_) => return Err(decode_error("invalid PDF full-page position")),
            None => ReadingPosition::pdf_page(page),
        },
        Location::AudiobookMillis(value) => ReadingPosition::audiobook_millis(value),
    };
    Ok(ReadingPositionState { location, progress: value.progress })
}

fn encode_annotation(value: &AnnotationState) -> pb::Annotation {
    use pb::annotation_anchor::Anchor;
    let anchor = match &value.anchor {
        AnnotationAnchor::EpubCfi { cfi } => Anchor::EpubCfi(cfi.clone()),
        AnnotationAnchor::Pdf { page_index, rects, fallback_cfi } => Anchor::Pdf(pb::PdfAnnotationAnchor {
            page_index: *page_index,
            rects: rects.iter().map(|rect| pb::PdfAnnotationRect { left: rect.left, top: rect.top, width: rect.width, height: rect.height }).collect(),
            fallback_cfi: fallback_cfi.clone(),
        }),
    };
    pb::Annotation {
        content_hash: value.content_hash.to_string(),
        anchor: Some(pb::AnnotationAnchor { anchor: Some(anchor) }),
        exact_text: value.exact_text.clone(),
        style: encode_annotation_style(value.style.clone()) as i32,
        color: value.color.clone(),
        note: value.note.clone(),
        created_at: value.created_at,
        modified_at: value.modified_at,
        deleted: value.deleted,
    }
}

fn decode_annotation(value: pb::Annotation) -> Result<AnnotationState, transport::WireError> {
    use pb::annotation_anchor::Anchor;
    let anchor = match required(required(value.anchor, "annotation anchor")?.anchor, "annotation anchor value")? {
        Anchor::EpubCfi(cfi) => AnnotationAnchor::epub_cfi(cfi),
        Anchor::Pdf(value) => AnnotationAnchor::pdf(PdfAnnotationAnchor::new(value.page_index, value.rects.into_iter().map(|rect| PdfAnnotationRect::new(rect.left, rect.top, rect.width, rect.height)).collect(), value.fallback_cfi)),
    };
    Ok(AnnotationState {
        content_hash: ContentHash::new(&value.content_hash),
        anchor,
        exact_text: value.exact_text,
        style: decode_annotation_style(value.style)?,
        color: value.color,
        note: value.note,
        created_at: value.created_at,
        modified_at: value.modified_at,
        deleted: value.deleted,
    })
}

fn encode_annotation_style(value: AnnotationStyle) -> pb::AnnotationStyle {
    match value {
        AnnotationStyle::Highlight => pb::AnnotationStyle::Highlight,
        AnnotationStyle::Underline => pb::AnnotationStyle::Underline,
        AnnotationStyle::Squiggly => pb::AnnotationStyle::Squiggly,
        AnnotationStyle::Strikethrough => pb::AnnotationStyle::Strikethrough,
    }
}

fn decode_annotation_style(value: i32) -> Result<AnnotationStyle, transport::WireError> {
    match pb::AnnotationStyle::try_from(value).map_err(|_| decode_error("unknown annotation style"))? {
        pb::AnnotationStyle::Highlight => Ok(AnnotationStyle::Highlight),
        pb::AnnotationStyle::Underline => Ok(AnnotationStyle::Underline),
        pb::AnnotationStyle::Squiggly => Ok(AnnotationStyle::Squiggly),
        pb::AnnotationStyle::Strikethrough => Ok(AnnotationStyle::Strikethrough),
        pb::AnnotationStyle::Unspecified => Err(decode_error("unspecified annotation style")),
    }
}

fn encode_book_metadata(value: &BookMetadataState) -> pb::BookMetadata {
    pb::BookMetadata { title: value.title.clone(), contributors: value.contributors.iter().map(encode_contributor).collect(), description: value.description.clone(), publication: Some(encode_publication_metadata(&value.publication)) }
}

fn decode_book_metadata(value: pb::BookMetadata) -> Result<BookMetadataState, transport::WireError> {
    Ok(BookMetadataState {
        title: value.title,
        contributors: value.contributors.into_iter().map(decode_contributor).collect::<Result<_, _>>()?,
        description: value.description,
        publication: value.publication.map(decode_publication_metadata).transpose()?.unwrap_or_default(),
    })
}

fn encode_contributor(value: &Contributor) -> pb::Contributor {
    pb::Contributor { contributor_id: value.contributor_id().to_string(), name: value.name().to_owned(), sort_name: value.sort_name().map(str::to_owned), role: value.role_code().0.to_vec() }
}

fn decode_contributor(value: pb::Contributor) -> Result<Contributor, transport::WireError> {
    let contributor_id = library_model::ContributorId::parse_str(&value.contributor_id).map_err(|error| decode_error(format!("invalid contributor id: {error}")))?;
    let role: [u8; 3] = value.role.try_into().map_err(|_| decode_error("contributor role must contain exactly three bytes"))?;
    Contributor::with_id(contributor_id, value.name, value.sort_name, MarcRelatorCode(role)).map_err(|error| decode_error(error.to_string()))
}

fn encode_localized_text(value: &LocalizedText) -> pb::LocalizedText {
    pb::LocalizedText { language: value.language.clone(), value: value.value.clone() }
}

fn decode_localized_text(value: pb::LocalizedText) -> Result<LocalizedText, transport::WireError> {
    LocalizedText::new(value.language, value.value).map_err(|error| decode_error(error.to_string()))
}

fn decode_collection(value: pb::PublicationCollection) -> Result<Collection, transport::WireError> {
    let kind = value.collection_type.map(CollectionKind::from_label).transpose().map_err(|error| decode_error(error.to_string()))?;
    let name_localizations = value.name_localizations.into_iter().map(decode_localized_text).collect::<Result<_, _>>()?;
    let sort_name_localizations = value.sort_name_localizations.into_iter().map(decode_localized_text).collect::<Result<_, _>>()?;
    Collection::with_details(value.name, name_localizations, value.sort_name, sort_name_localizations, value.identifiers, kind, value.position, value.source.unwrap_or_else(|| "legacy:protobuf-collection".to_owned()))
        .map_err(|error| decode_error(error.to_string()))
}

fn encode_publication_metadata(value: &PublicationMetadata) -> pb::PublicationMetadata {
    pb::PublicationMetadata {
        identifiers: value.identifiers.iter().map(encode_publication_identifier).collect(),
        publishers: value.publishers.iter().map(|publisher| pb::Publisher { publisher_id: publisher.publisher_id().to_string(), name: publisher.name().as_str().to_owned() }).collect(),
        languages: value.languages.iter().map(|language| language.as_str().to_owned()).collect(),
        dates: value.dates.iter().map(|date| pb::PublicationDate { id: date.id().map(str::to_owned), value: date.value().to_owned(), event: date.event().map(str::to_owned), source: Some(date.source().to_owned()) }).collect(),
        subjects: value
            .subjects
            .iter()
            .map(|subject| pb::PublicationSubject {
                id: subject.id().map(str::to_owned),
                name: subject.name().to_owned(),
                authority: subject.authority().map(str::to_owned),
                code: subject.code().map(str::to_owned),
                source: Some(subject.source().to_owned()),
                name_localizations: subject.name_localizations.iter().map(encode_localized_text).collect(),
                sort_name: subject.sort_name.clone(),
                sort_name_localizations: subject.sort_name_localizations.iter().map(encode_localized_text).collect(),
            })
            .collect(),
        collections: value
            .collections
            .iter()
            .map(|collection| pb::PublicationCollection {
                name: collection.name.clone(),
                collection_type: collection.kind.as_ref().map(|kind| kind.label().to_owned()),
                position: collection.position.clone(),
                name_localizations: collection.name_localizations.iter().map(encode_localized_text).collect(),
                sort_name: collection.sort_name.clone(),
                sort_name_localizations: collection.sort_name_localizations.iter().map(encode_localized_text).collect(),
                identifiers: collection.identifiers.clone(),
                source: Some(collection.source.clone()),
            })
            .collect(),
        alternate_titles: value
            .alternate_titles
            .iter()
            .map(|title| pb::PublicationTitle {
                id: title.id().map(str::to_owned),
                value: title.value().to_owned(),
                title_type: title.title_type().map(str::to_owned),
                sort_as: title.sort_as().map(str::to_owned),
                source: Some(title.source().to_owned()),
            })
            .collect(),
        rights: value.rights.clone(),
        sources: value.sources.clone(),
        types: value.types.clone(),
        formats: value.formats.clone(),
        relations: value.relations.clone(),
        coverage: value.coverage.clone(),
        properties: value
            .properties
            .iter()
            .map(|property| pb::PublicationProperty {
                id: property.id().map(str::to_owned),
                property: property.property().to_owned(),
                value: property.value().to_owned(),
                refines: property.refines().map(str::to_owned),
                scheme: property.scheme().map(str::to_owned),
                language: property.language().map(str::to_owned),
                source: Some(property.source().to_owned()),
            })
            .collect(),
    }
}

fn decode_publication_metadata(value: pb::PublicationMetadata) -> Result<PublicationMetadata, transport::WireError> {
    Ok(PublicationMetadata {
        identifiers: value.identifiers.into_iter().map(decode_publication_identifier).collect::<Result<_, _>>()?,
        publishers: value
            .publishers
            .into_iter()
            .map(|publisher| {
                let publisher_id = library_model::PublisherId::parse_str(&publisher.publisher_id).map_err(|error| decode_error(format!("invalid publisher id: {error}")))?;
                PublisherCredit::with_id(publisher_id, publisher.name).map_err(|error| decode_error(error.to_string()))
            })
            .collect::<Result<_, _>>()?,
        languages: value.languages.into_iter().map(|language| LanguageTag::parse(language).map_err(|error| decode_error(error.to_string()))).collect::<Result<_, _>>()?,
        dates: value
            .dates
            .into_iter()
            .map(|date| PublicationDate::new(date.id, date.value, date.event, date.source.unwrap_or_else(|| "legacy:protobuf-date".to_owned())).map_err(|error| decode_error(error.to_string())))
            .collect::<Result<_, _>>()?,
        subjects: value
            .subjects
            .into_iter()
            .map(|subject| {
                PublicationSubject::with_details(
                    subject.id,
                    subject.name,
                    subject.name_localizations.into_iter().map(decode_localized_text).collect::<Result<_, _>>()?,
                    subject.sort_name,
                    subject.sort_name_localizations.into_iter().map(decode_localized_text).collect::<Result<_, _>>()?,
                    subject.source.unwrap_or_else(|| "dc:subject".to_owned()),
                    subject.authority,
                    subject.code,
                )
                .map_err(|error| decode_error(error.to_string()))
            })
            .collect::<Result<_, _>>()?,
        collections: value.collections.into_iter().map(decode_collection).collect::<Result<_, _>>()?,
        alternate_titles: value
            .alternate_titles
            .into_iter()
            .map(|title| PublicationTitle::new(title.id, title.value, title.title_type, title.sort_as, title.source.unwrap_or_else(|| "legacy:protobuf-title".to_owned())).map_err(|error| decode_error(error.to_string())))
            .collect::<Result<_, _>>()?,
        rights: value.rights,
        sources: value.sources,
        types: value.types,
        formats: value.formats,
        relations: value.relations,
        coverage: value.coverage,
        properties: value
            .properties
            .into_iter()
            .map(|property| {
                PublicationProperty::new(property.id, property.property, property.value, property.refines, property.scheme, property.language, property.source.unwrap_or_else(|| "legacy:protobuf-property".to_owned()))
                    .map_err(|error| decode_error(error.to_string()))
            })
            .collect::<Result<_, _>>()?,
    })
}

fn encode_publication_identifier(value: &Identifier) -> pb::PublicationIdentifier {
    let (scheme, other_scheme) = match value.scheme() {
        Scheme::Unspecified => (pb::IdentifierScheme::Unspecified, None),
        Scheme::Isbn => (pb::IdentifierScheme::Isbn, None),
        Scheme::Asin => (pb::IdentifierScheme::Asin, None),
        Scheme::Doi => (pb::IdentifierScheme::Doi, None),
        Scheme::Issn => (pb::IdentifierScheme::Issn, None),
        Scheme::Oclc => (pb::IdentifierScheme::Oclc, None),
        Scheme::Lccn => (pb::IdentifierScheme::Lccn, None),
        Scheme::Uuid => (pb::IdentifierScheme::Uuid, None),
        Scheme::Calibre => (pb::IdentifierScheme::Calibre, None),
        Scheme::Google => (pb::IdentifierScheme::Google, None),
        Scheme::Goodreads => (pb::IdentifierScheme::Goodreads, None),
        Scheme::Kobo => (pb::IdentifierScheme::Kobo, None),
        Scheme::Other(value) => (pb::IdentifierScheme::Other, Some(value.clone())),
    };
    let mut identifier_types = vec![pb::PublicationIdentifierType { value: value.source().to_owned(), scheme: Some("bokheim:source".to_owned()) }];
    if value.scope() != Scope::Publication {
        let scope = match value.scope() {
            Scope::Publication => "publication",
            Scope::Edition => "edition",
            Scope::Work => "work",
        };
        identifier_types.push(pb::PublicationIdentifierType { value: scope.to_owned(), scheme: Some("bokheim:scope".to_owned()) });
    }
    pb::PublicationIdentifier { value: value.value().to_owned(), scheme: scheme as i32, other_scheme, identifier_types }
}

fn decode_publication_identifier(value: pb::PublicationIdentifier) -> Result<Identifier, transport::WireError> {
    let scheme = match pb::IdentifierScheme::try_from(value.scheme).map_err(|_| decode_error("unknown publication identifier scheme"))? {
        pb::IdentifierScheme::Unspecified => Scheme::Unspecified,
        pb::IdentifierScheme::Isbn => Scheme::Isbn,
        pb::IdentifierScheme::Asin => Scheme::Asin,
        pb::IdentifierScheme::Doi => Scheme::Doi,
        pb::IdentifierScheme::Issn => Scheme::Issn,
        pb::IdentifierScheme::Oclc => Scheme::Oclc,
        pb::IdentifierScheme::Lccn => Scheme::Lccn,
        pb::IdentifierScheme::Uuid => Scheme::Uuid,
        pb::IdentifierScheme::Calibre => Scheme::Calibre,
        pb::IdentifierScheme::Google => Scheme::Google,
        pb::IdentifierScheme::Goodreads => Scheme::Goodreads,
        pb::IdentifierScheme::Kobo => Scheme::Kobo,
        pb::IdentifierScheme::Other => Scheme::Other(required(value.other_scheme, "other identifier scheme")?),
    };
    let source = value
        .identifier_types
        .iter()
        .find(|value| value.scheme.as_deref() == Some("bokheim:source"))
        .or_else(|| value.identifier_types.iter().find(|value| value.scheme.as_deref() == Some("bokheim:provenance")))
        .map(|value| value.value.clone())
        .unwrap_or_else(|| "legacy:identifier".to_owned());
    let scope = value
        .identifier_types
        .iter()
        .find(|value| value.scheme.as_deref() == Some("bokheim:scope"))
        .and_then(|value| match value.value.as_str() {
            "edition" => Some(Scope::Edition),
            "work" => Some(Scope::Work),
            "publication" => Some(Scope::Publication),
            _ => None,
        })
        .unwrap_or(Scope::Publication);
    Identifier::new(value.value, scheme, scope, source).map_err(|error| decode_error(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_protobuf_publication_values_receive_explicit_provenance() {
        let encoded = pb::PublicationMetadata {
            dates: vec![pb::PublicationDate { id: None, value: "1965".to_owned(), event: Some("publication".to_owned()), source: None }],
            alternate_titles: vec![pb::PublicationTitle { id: None, value: "Dune World".to_owned(), title_type: Some("alternative".to_owned()), sort_as: None, source: None }],
            properties: vec![pb::PublicationProperty { id: None, property: "belongs-to-collection".to_owned(), value: "Dune".to_owned(), refines: None, scheme: None, language: None, source: None }],
            ..Default::default()
        };

        let decoded = decode_publication_metadata(encoded).unwrap();
        assert_eq!(decoded.dates[0].value(), "1965");
        assert_eq!(decoded.dates[0].event(), Some("publication"));
        assert_eq!(decoded.dates[0].source(), "legacy:protobuf-date");
        assert_eq!(decoded.alternate_titles[0].source(), "legacy:protobuf-title");
        assert_eq!(decoded.properties[0].source(), "legacy:protobuf-property");
    }
}
