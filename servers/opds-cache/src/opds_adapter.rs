use crate::CatalogServiceError;
use catalog_model::*;
use opds::*;
use std::collections::HashMap;

const OPDS_SOURCE_PREFIX: &str = "opds-";

pub fn opds_source(endpoint: &OpdsEndpoint) -> catalog_model::CatalogSource {
    catalog_model::CatalogSource {
        id: opds_catalog_source_id(&endpoint.id),
        title: endpoint.title.clone(),
        description: endpoint.description.clone(),
        languages: endpoint.languages.clone(),
        authentication: CatalogAuthentication::None,
        capabilities: vec![CatalogCapability::Browse, CatalogCapability::Search, CatalogCapability::Facets, CatalogCapability::Import],
    }
}

pub fn opds_catalog_source_id(endpoint_id: &OpdsEndpointId) -> CatalogSourceId {
    CatalogSourceId::parse(&format!("{OPDS_SOURCE_PREFIX}{}", endpoint_id.as_str())).expect("namespaced OPDS endpoint id is a catalog source id")
}

pub fn canonicalize_opds_catalog_locator(endpoint: &OpdsEndpoint, locator: &str) -> Result<String, CatalogServiceError> {
    let expected = url::Url::parse(&endpoint.url).map_err(|error| CatalogServiceError(error.to_string()))?;
    let mut actual = url::Url::parse(locator).map_err(|error| CatalogServiceError(error.to_string()))?;
    if !matches!(actual.scheme(), "http" | "https") || !actual.username().is_empty() || actual.password().is_some() {
        return Err(CatalogServiceError("catalog page locator must be an HTTP URL without embedded credentials".to_owned()));
    }
    // OpenEdition is served over HTTPS but currently publishes absolute HTTP
    // links back to its own OPDS host. Preserve the origin boundary while
    // refusing to follow that downgrade.
    if expected.scheme() == "https" && actual.scheme() == "http" && expected.host_str() == actual.host_str() && expected.port_or_known_default() == Some(443) && actual.port_or_known_default() == Some(80) {
        actual.set_scheme("https").map_err(|()| CatalogServiceError("could not upgrade the OPDS catalog locator to HTTPS".to_owned()))?;
        actual.set_port(expected.port()).map_err(|()| CatalogServiceError("could not restore the registered OPDS HTTPS port".to_owned()))?;
    }
    if actual.origin() != expected.origin() {
        return Err(CatalogServiceError("catalog page locator does not belong to the registered OPDS origin".to_owned()));
    }
    Ok(actual.into())
}

pub fn validate_opds_catalog_locator(endpoint: &OpdsEndpoint, locator: &str) -> Result<(), CatalogServiceError> {
    canonicalize_opds_catalog_locator(endpoint, locator).map(|_| ())
}

pub fn opds_page(source_id: CatalogSourceId, catalog: OpdsCatalog) -> CatalogPage {
    let catalog_locator = catalog.source_url.clone();
    let project_gutenberg = source_id.as_str() == "opds-project-gutenberg";
    let mut page = CatalogPage {
        source_id: source_id.clone(),
        freshness: match catalog.freshness {
            opds::OpdsCatalogFreshness::Current => CatalogFreshness::Current,
            opds::OpdsCatalogFreshness::Stale => CatalogFreshness::Stale,
        },
        title: catalog.title,
        locator: catalog.source_url,
        navigation: catalog.navigation.into_iter().map(catalog_link).collect(),
        books: catalog.books.into_iter().map(|book| opds_book(&source_id, &catalog_locator, book)).collect(),
        groups: catalog.groups.into_iter().map(|group| opds_group(&source_id, &catalog_locator, group)).collect(),
        facets: catalog.facets.into_iter().filter_map(catalog_facet).collect(),
        pagination: CatalogPagination {
            first: catalog.pagination.first,
            previous: catalog.pagination.previous,
            next: catalog.pagination.next,
            last: catalog.pagination.last,
            number_of_items: catalog.pagination.number_of_items,
            items_per_page: catalog.pagination.items_per_page,
            current_page: catalog.pagination.current_page,
        },
    };
    if project_gutenberg {
        canonicalize_gutenberg_page(&mut page);
    }
    page
}

pub(crate) fn canonicalize_gutenberg_page(page: &mut CatalogPage) {
    if page.source_id.as_str() != "opds-project-gutenberg" {
        return;
    }
    prefer_illustrated_gutenberg_editions(&mut page.books);
    for group in &mut page.groups {
        prefer_illustrated_gutenberg_editions(&mut group.books);
    }
}

fn prefer_illustrated_gutenberg_editions(books: &mut Vec<CatalogBook>) {
    let mut work_positions = HashMap::<String, usize>::new();
    let mut canonical = Vec::with_capacity(books.len());
    for book in books.drain(..) {
        let Some(work_id) = book.identity.work_id.as_deref().filter(|work_id| work_id.starts_with("urn:gutenberg:")) else {
            canonical.push(book);
            continue;
        };
        if let Some(&position) = work_positions.get(work_id) {
            if gutenberg_edition_has_images(&book) && !gutenberg_edition_has_images(&canonical[position]) {
                canonical[position] = book;
            }
        } else {
            work_positions.insert(work_id.to_owned(), canonical.len());
            canonical.push(book);
        }
    }
    *books = canonical;
}

fn gutenberg_edition_has_images(book: &CatalogBook) -> bool {
    book.actions.iter().any(|action| {
        let CatalogBookAction::Import { request } = action;
        let download_url = request.download_url.to_ascii_lowercase();
        download_url.contains(".images") && !download_url.contains(".noimages")
    })
}

pub(crate) fn opds_book_popularity(book: &CatalogBook) -> u64 {
    book
        .metadata
        .interactions
        .iter()
        .find(|statistic| statistic.source == "litteraturbanken:epub_popularity" || statistic.interaction_type == "litteraturbanken:epub-popularity")
        .map(|statistic| statistic.count)
        .or_else(|| project_gutenberg_download_count(book))
        .unwrap_or_default()
}

fn project_gutenberg_download_count(book: &CatalogBook) -> Option<u64> {
    if !book.identity.edition_id.starts_with("urn:gutenberg:") {
        return None;
    }
    let CatalogDescription::PlainText { text } = book.description.as_ref()? else {
        return None;
    };
    let value = text.rsplit_once("Downloads:")?.1.trim_start();
    let digits = value.chars().take_while(char::is_ascii_digit).collect::<String>();
    (!digits.is_empty()).then(|| digits.parse().ok()).flatten()
}

/// Revalidates a client-selected catalogue acquisition against a freshly
/// fetched OPDS page. Callers must separately validate that `catalog` belongs
/// to the registered endpoint before invoking this helper.
pub fn validated_opds_import(request: &CatalogImportRequest, catalog: &OpdsCatalog) -> Result<(OpdsBookIdentity, OpdsDirectAcquisition), CatalogServiceError> {
    let requested_identity = OpdsBookIdentity { book_id: request.book.edition_id.clone(), work_id: request.book.work_id.clone() };
    requested_identity.validate().map_err(|error| CatalogServiceError(error.to_string()))?;
    if catalog.source_url != request.catalog_locator {
        return Err(CatalogServiceError("catalog import does not belong to the verified catalog page".to_owned()));
    }
    let book = catalog
        .books
        .iter()
        .chain(catalog.groups.iter().flat_map(|group| group.books.iter()))
        .find(|book| book.id == requested_identity.book_id && book.work_id == requested_identity.work_id)
        .ok_or_else(|| CatalogServiceError("catalog book is not present on the verified catalog page".to_owned()))?;
    let requested_format = opds_book_format(request.book_format);
    let acquisition = book
        .acquisitions
        .iter()
        .find_map(|acquisition| match acquisition {
            OpdsAcquisition::Direct(acquisition)
                if acquisition.url == request.download_url && acquisition.book_format == requested_format && acquisition.price.is_none() && matches!(acquisition.kind, OpdsAcquisitionKind::OpenAccess | OpdsAcquisitionKind::Generic) =>
            {
                Some(acquisition.clone())
            }
            _ => None,
        })
        .ok_or_else(|| CatalogServiceError("catalog acquisition is not an importable link on the verified catalog page".to_owned()))?;
    Ok((requested_identity, acquisition))
}

fn opds_book_format(format: CatalogBookFormat) -> OpdsBookFormat {
    match format {
        CatalogBookFormat::Epub => OpdsBookFormat::Epub,
        CatalogBookFormat::Pdf => OpdsBookFormat::Pdf,
        CatalogBookFormat::Mobi => OpdsBookFormat::Mobi,
        CatalogBookFormat::M4b => OpdsBookFormat::M4b,
    }
}

fn catalog_link(link: OpdsCatalogLink) -> CatalogLink {
    let target = match link.target {
        OpdsLinkTarget::Catalog => CatalogLinkTarget::Page { locator: link.url },
        OpdsLinkTarget::Web => CatalogLinkTarget::External { url: link.url },
    };
    CatalogLink { title: link.title, target, relations: link.relations, number_of_items: link.number_of_items, active: link.active }
}

fn catalog_facet(mut facet: OpdsFacet) -> Option<CatalogFacet> {
    if matches!(facet.title.trim().to_ascii_lowercase().as_str(), "license" | "collection") {
        return None;
    }
    if facet.title.trim().eq_ignore_ascii_case("format") {
        facet.links = facet.links.into_iter().filter_map(compact_format_facet_link).collect();
    }
    (!facet.links.is_empty()).then(|| CatalogFacet { title: facet.title, links: facet.links.into_iter().map(catalog_link).collect() })
}

fn compact_format_facet_link(mut link: OpdsCatalogLink) -> Option<OpdsCatalogLink> {
    let words = link.title.split(|character: char| !character.is_ascii_alphanumeric()).filter(|word| !word.is_empty()).collect::<Vec<_>>();
    link.title = if words.iter().any(|word| word.eq_ignore_ascii_case("epub")) {
        "EPUB".to_owned()
    } else if words.iter().any(|word| word.eq_ignore_ascii_case("pdf")) {
        "PDF".to_owned()
    } else if words.iter().any(|word| matches!(word.to_ascii_lowercase().as_str(), "mobi" | "azw" | "azw3")) {
        "MOBI/AZW".to_owned()
    } else if words.iter().any(|word| word.eq_ignore_ascii_case("m4b")) {
        "M4B".to_owned()
    } else {
        return None;
    };
    Some(link)
}

fn opds_group(source_id: &CatalogSourceId, catalog_locator: &str, group: OpdsGroup) -> CatalogGroup {
    CatalogGroup {
        title: group.title,
        page_locator: group.self_url,
        number_of_items: group.number_of_items,
        navigation: group.navigation.into_iter().map(catalog_link).collect(),
        books: group.books.into_iter().map(|book| opds_book(source_id, catalog_locator, book)).collect(),
    }
}

#[doc(hidden)]
pub fn opds_book(source_id: &CatalogSourceId, catalog_locator: &str, book: OpdsBook) -> CatalogBook {
    let identity = CatalogBookIdentity { edition_id: book.id, work_id: book.work_id };
    let mut import_formats = Vec::new();
    let actions = book
        .acquisitions
        .into_iter()
        .filter_map(|acquisition| opds_action(source_id, catalog_locator, &identity, acquisition))
        .filter(|action| match action {
            CatalogBookAction::Import { request } if import_formats.contains(&request.book_format) => false,
            CatalogBookAction::Import { request } => {
                import_formats.push(request.book_format);
                true
            }
        })
        .collect();
    let description = book.description.map(|description| match description {
        OpdsDescription::PlainText { text } => CatalogDescription::PlainText { text },
        OpdsDescription::Html { markup } | OpdsDescription::Xhtml { markup } => CatalogDescription::RichHtml { html: markup },
    });
    let authors = book
        .authors
        .into_iter()
        .map(|author| CatalogAuthorCredit {
            name: author.name,
            name_localizations: author.name_localizations.into_iter().map(localized_text).collect(),
            sort_name: author.sort_name,
            sort_name_localizations: author.sort_name_localizations.into_iter().map(localized_text).collect(),
            identifiers: author.identifiers,
            provider_link: author.provider_link.map(|link| match link.target {
                OpdsLinkTarget::Catalog => CatalogLinkTarget::Page { locator: link.url },
                OpdsLinkTarget::Web => CatalogLinkTarget::External { url: link.url },
            }),
        })
        .collect();
    let metadata = CatalogBookMetadata {
        title_localizations: book.metadata.title_localizations.into_iter().map(localized_text).collect(),
        sort_name: book.metadata.sort_name,
        sort_name_localizations: book.metadata.sort_name_localizations.into_iter().map(localized_text).collect(),
        published: book.metadata.published,
        modified: book.metadata.modified,
        dates: book.metadata.dates,
        publishers: book
            .metadata
            .publishers
            .into_iter()
            .map(|publisher| CatalogPublisherCredit {
                name: publisher.name,
                name_localizations: publisher.name_localizations.into_iter().map(localized_text).collect(),
                sort_name: publisher.sort_name,
                sort_name_localizations: publisher.sort_name_localizations.into_iter().map(localized_text).collect(),
                identifiers: publisher.identifiers,
                provider_link: publisher.provider_link.map(catalog_author_link),
            })
            .collect(),
        identifiers: book
            .metadata
            .identifiers
            .into_iter()
            .filter_map(|identifier| {
                let scope = catalog_identifier_scope(identifier.scope)?;
                Identifier::new(identifier.value, Scheme::from_label(identifier.scheme.as_deref()), scope, identifier.source).ok()
            })
            .collect(),
        subjects: book.metadata.subjects,
        subtitle: book.metadata.subtitle,
        subtitle_localizations: book.metadata.subtitle_localizations.into_iter().map(localized_text).collect(),
        alternate_titles: book.metadata.alternate_titles,
        collections: book.metadata.collections,
        contributors: book
            .metadata
            .contributors
            .into_iter()
            .map(|contributor| CatalogContributorCredit {
                name: contributor.name,
                name_localizations: contributor.name_localizations.into_iter().map(localized_text).collect(),
                sort_name: contributor.sort_name,
                sort_name_localizations: contributor.sort_name_localizations.into_iter().map(localized_text).collect(),
                identifiers: contributor.identifiers,
                provider_link: contributor.provider_link.map(catalog_author_link),
                roles: contributor.roles,
            })
            .collect(),
        rights: book.metadata.rights,
        sources: book.metadata.sources,
        types: book.metadata.types,
        formats: book.metadata.formats,
        relations: book.metadata.relations,
        coverage: book.metadata.coverage,
        properties: book.metadata.properties,
        audiences: book.metadata.audiences,
        age_ranges: book.metadata.age_ranges,
        images: book.metadata.images,
        ratings: book.metadata.ratings,
        interactions: book.metadata.interactions,
        popularity_ranks: book.metadata.popularity_ranks,
    };
    let languages = book.languages.into_iter().filter_map(|language| LanguageTag::parse(language).ok()).collect();
    CatalogBook { identity, title: book.title, authors, languages, description, image_url: book.image_url, metadata, actions }
}

fn localized_text(value: opds::OpdsLocalizedText) -> LocalizedText {
    LocalizedText { language: value.language, value: value.value }
}

fn catalog_identifier_scope(scope: opds::OpdsIdentifierScope) -> Option<Scope> {
    match scope {
        opds::OpdsIdentifierScope::Book => Some(Scope::Book),
        opds::OpdsIdentifierScope::CatalogEntry => None,
        opds::OpdsIdentifierScope::Edition => Some(Scope::Edition),
        opds::OpdsIdentifierScope::Work => Some(Scope::Work),
    }
}

fn catalog_author_link(link: opds::OpdsAuthorLink) -> CatalogLinkTarget {
    match link.target {
        OpdsLinkTarget::Catalog => CatalogLinkTarget::Page { locator: link.url },
        OpdsLinkTarget::Web => CatalogLinkTarget::External { url: link.url },
    }
}

fn opds_action(source_id: &CatalogSourceId, catalog_locator: &str, identity: &CatalogBookIdentity, acquisition: OpdsAcquisition) -> Option<CatalogBookAction> {
    match acquisition {
        OpdsAcquisition::Direct(acquisition) if acquisition.price.is_none() && matches!(acquisition.kind, OpdsAcquisitionKind::OpenAccess | OpdsAcquisitionKind::Generic) => Some(CatalogBookAction::Import {
            request: CatalogImportRequest { source_id: source_id.clone(), catalog_locator: catalog_locator.to_owned(), book: identity.clone(), download_url: acquisition.url, book_format: book_format(acquisition.book_format) },
        }),
        OpdsAcquisition::Direct(_) | OpdsAcquisition::Indirect(_) | OpdsAcquisition::Unsupported(_) => None,
    }
}

fn book_format(format: OpdsBookFormat) -> CatalogBookFormat {
    match format {
        OpdsBookFormat::Epub => CatalogBookFormat::Epub,
        OpdsBookFormat::Pdf => CatalogBookFormat::Pdf,
        OpdsBookFormat::Mobi => CatalogBookFormat::Mobi,
        OpdsBookFormat::M4b => CatalogBookFormat::M4b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint() -> OpdsEndpoint {
        OpdsEndpoint { id: OpdsEndpointId::parse("example").unwrap(), title: "Example".to_owned(), description: "Example".to_owned(), languages: vec!["English".to_owned()], url: "https://catalog.example/root".to_owned() }
    }

    #[test]
    fn provider_routing_stays_inside_the_registered_origin() {
        assert_eq!(canonicalize_opds_catalog_locator(&endpoint(), "http://catalog.example/page").unwrap(), "https://catalog.example/page");
        assert!(validate_opds_catalog_locator(&endpoint(), "https://attacker.example/page").is_err());
        assert!(validate_opds_catalog_locator(&endpoint(), "http://catalog.example:8080/page").is_err());
        assert!(validate_opds_catalog_locator(&endpoint(), "https://user:secret@catalog.example/page").is_err());
    }

    #[test]
    fn source_ids_are_namespaced_for_the_server_adapter() {
        let source = opds_source(&endpoint());
        assert_eq!(source.id.as_str(), "opds-example");
        assert_eq!(source.title, "Example");
        assert!(source.capabilities.contains(&CatalogCapability::Import));
    }
}
