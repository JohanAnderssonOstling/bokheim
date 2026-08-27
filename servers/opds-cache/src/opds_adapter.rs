use catalog_core::CatalogProviderError;
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
        capabilities: vec![CatalogCapability::Browse, CatalogCapability::Search, CatalogCapability::Facets, CatalogCapability::Import, CatalogCapability::ExternalFulfillment],
    }
}

pub fn opds_catalog_source_id(endpoint_id: &OpdsEndpointId) -> CatalogSourceId {
    CatalogSourceId::parse(&format!("{OPDS_SOURCE_PREFIX}{}", endpoint_id.as_str())).expect("namespaced OPDS endpoint id is a catalog source id")
}

#[doc(hidden)]
pub fn opds_endpoint_id(source_id: &CatalogSourceId) -> Result<OpdsEndpointId, CatalogProviderError> {
    let value = source_id.as_str().strip_prefix(OPDS_SOURCE_PREFIX).ok_or_else(|| CatalogProviderError::new("catalog source does not belong to the OPDS provider"))?;
    OpdsEndpointId::parse(value).map_err(|error| CatalogProviderError::new(error.to_string()))
}

pub fn canonicalize_opds_catalog_locator(endpoint: &OpdsEndpoint, locator: &str) -> Result<String, CatalogProviderError> {
    let expected = url::Url::parse(&endpoint.url).map_err(|error| CatalogProviderError::new(error.to_string()))?;
    let mut actual = url::Url::parse(locator).map_err(|error| CatalogProviderError::new(error.to_string()))?;
    if !matches!(actual.scheme(), "http" | "https") || !actual.username().is_empty() || actual.password().is_some() {
        return Err(CatalogProviderError::new("catalog page locator must be an HTTP URL without embedded credentials"));
    }
    // OpenEdition is served over HTTPS but currently publishes absolute HTTP
    // links back to its own OPDS host. Preserve the origin boundary while
    // refusing to follow that downgrade.
    if expected.scheme() == "https" && actual.scheme() == "http" && expected.host_str() == actual.host_str() && expected.port_or_known_default() == Some(443) && actual.port_or_known_default() == Some(80) {
        actual.set_scheme("https").map_err(|()| CatalogProviderError::new("could not upgrade the OPDS catalog locator to HTTPS"))?;
        actual.set_port(expected.port()).map_err(|()| CatalogProviderError::new("could not restore the registered OPDS HTTPS port"))?;
    }
    if actual.origin() != expected.origin() {
        return Err(CatalogProviderError::new("catalog page locator does not belong to the registered OPDS origin"));
    }
    Ok(actual.into())
}

pub fn validate_opds_catalog_locator(endpoint: &OpdsEndpoint, locator: &str) -> Result<(), CatalogProviderError> {
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
        publications: catalog.publications.into_iter().map(|publication| opds_publication(&source_id, &catalog_locator, publication)).collect(),
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
        search: catalog.search.map(opds_search),
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
    prefer_illustrated_gutenberg_editions(&mut page.publications);
    for group in &mut page.groups {
        prefer_illustrated_gutenberg_editions(&mut group.publications);
    }
}

fn prefer_illustrated_gutenberg_editions(publications: &mut Vec<CatalogPublication>) {
    let mut work_positions = HashMap::<String, usize>::new();
    let mut canonical = Vec::with_capacity(publications.len());
    for publication in publications.drain(..) {
        let Some(work_id) = publication.identity.work_id.as_deref().filter(|work_id| work_id.starts_with("urn:gutenberg:")) else {
            canonical.push(publication);
            continue;
        };
        if let Some(&position) = work_positions.get(work_id) {
            if gutenberg_edition_has_images(&publication) && !gutenberg_edition_has_images(&canonical[position]) {
                canonical[position] = publication;
            }
        } else {
            work_positions.insert(work_id.to_owned(), canonical.len());
            canonical.push(publication);
        }
    }
    *publications = canonical;
}

fn gutenberg_edition_has_images(publication: &CatalogPublication) -> bool {
    publication.actions.iter().any(|action| {
        let CatalogPublicationAction::Import { request } = action else { return false };
        let download_url = request.download_url.to_ascii_lowercase();
        download_url.contains(".images") && !download_url.contains(".noimages")
    })
}

pub(crate) fn opds_publication_popularity(publication: &CatalogPublication) -> u64 {
    publication
        .metadata
        .interactions
        .iter()
        .find(|statistic| statistic.source == "litteraturbanken:epub_popularity" || statistic.interaction_type == "litteraturbanken:epub-popularity")
        .map(|statistic| statistic.count)
        .or_else(|| project_gutenberg_download_count(publication))
        .unwrap_or_default()
}

fn project_gutenberg_download_count(publication: &CatalogPublication) -> Option<u64> {
    if !publication.identity.edition_id.starts_with("urn:gutenberg:") {
        return None;
    }
    let CatalogDescription::PlainText { text } = publication.description.as_ref()? else {
        return None;
    };
    let value = text.rsplit_once("Downloads:")?.1.trim_start();
    let digits = value.chars().take_while(char::is_ascii_digit).collect::<String>();
    (!digits.is_empty()).then(|| digits.parse().ok()).flatten()
}

pub fn opds_search_definition(search: &CatalogSearch) -> Result<OpdsSearch, CatalogProviderError> {
    match &search.definition {
        CatalogSearchDefinition::OpdsUriTemplate { base_url, template } => Ok(OpdsSearch::UriTemplate { base_url: base_url.clone(), template: template.clone() }),
        CatalogSearchDefinition::OpdsOpenSearchDescription { url } => Ok(OpdsSearch::OpenSearchDescription { url: url.clone() }),
    }
}

pub fn opds_search_query(query: CatalogSearchQuery) -> OpdsSearchQuery {
    OpdsSearchQuery { query: query.query, title: query.title, author: query.author }
}

/// Revalidates a client-selected catalogue acquisition against a freshly
/// fetched OPDS page. Callers must separately validate that `catalog` belongs
/// to the registered endpoint before invoking this helper.
pub fn validated_opds_import(request: &CatalogImportRequest, catalog: &OpdsCatalog) -> Result<(OpdsPublicationIdentity, OpdsDirectAcquisition), CatalogProviderError> {
    let requested_identity = OpdsPublicationIdentity { publication_id: request.publication.edition_id.clone(), work_id: request.publication.work_id.clone() };
    requested_identity.validate().map_err(|error| CatalogProviderError::new(error.to_string()))?;
    if catalog.source_url != request.catalog_locator {
        return Err(CatalogProviderError::new("catalog import does not belong to the verified catalog page"));
    }
    let publication = catalog
        .publications
        .iter()
        .chain(catalog.groups.iter().flat_map(|group| group.publications.iter()))
        .find(|publication| publication.id == requested_identity.publication_id && publication.work_id == requested_identity.work_id)
        .ok_or_else(|| CatalogProviderError::new("catalog publication is not present on the verified catalog page"))?;
    let requested_format = opds_book_format(request.book_format);
    let acquisition = publication
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
        .ok_or_else(|| CatalogProviderError::new("catalog acquisition is not an importable link on the verified catalog page"))?;
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

#[doc(hidden)]
pub fn search_terms(query: &CatalogSearchQuery) -> Result<String, CatalogProviderError> {
    let terms = [Some(query.query.trim()).filter(|value| !value.is_empty()), query.title.as_deref().map(str::trim).filter(|value| !value.is_empty()), query.author.as_deref().map(str::trim).filter(|value| !value.is_empty())]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
    if terms.is_empty() || terms.len() > 500 || terms.chars().any(char::is_control) {
        return Err(CatalogProviderError::new("catalog search must contain between 1 and 500 characters without control characters"));
    }
    Ok(terms)
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
        publications: group.publications.into_iter().map(|publication| opds_publication(source_id, catalog_locator, publication)).collect(),
    }
}

#[doc(hidden)]
pub fn opds_publication(source_id: &CatalogSourceId, catalog_locator: &str, publication: OpdsPublication) -> CatalogPublication {
    let identity = CatalogPublicationIdentity { edition_id: publication.id, work_id: publication.work_id };
    let mut import_formats = Vec::new();
    let actions = publication
        .acquisitions
        .into_iter()
        .filter_map(|acquisition| opds_action(source_id, catalog_locator, &identity, acquisition))
        .filter(|action| match action {
            CatalogPublicationAction::Import { request } if import_formats.contains(&request.book_format) => false,
            CatalogPublicationAction::Import { request } => {
                import_formats.push(request.book_format);
                true
            }
            CatalogPublicationAction::External { .. } => true,
        })
        .collect();
    let description = publication.description.map(|description| match description {
        OpdsDescription::PlainText { text } => CatalogDescription::PlainText { text },
        OpdsDescription::Html { markup } | OpdsDescription::Xhtml { markup } => CatalogDescription::RichHtml { html: markup },
    });
    let authors = publication
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
    let metadata = CatalogPublicationMetadata {
        title_localizations: publication.metadata.title_localizations.into_iter().map(localized_text).collect(),
        sort_name: publication.metadata.sort_name,
        sort_name_localizations: publication.metadata.sort_name_localizations.into_iter().map(localized_text).collect(),
        published: publication.metadata.published,
        modified: publication.metadata.modified,
        dates: publication.metadata.dates,
        publishers: publication
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
        identifiers: publication
            .metadata
            .identifiers
            .into_iter()
            .filter_map(|identifier| {
                let scope = catalog_identifier_scope(identifier.scope)?;
                Identifier::new(identifier.value, Scheme::from_label(identifier.scheme.as_deref()), scope, identifier.source).ok()
            })
            .collect(),
        subjects: publication.metadata.subjects,
        subtitle: publication.metadata.subtitle,
        subtitle_localizations: publication.metadata.subtitle_localizations.into_iter().map(localized_text).collect(),
        alternate_titles: publication.metadata.alternate_titles,
        collections: publication.metadata.collections,
        contributors: publication
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
        rights: publication.metadata.rights,
        sources: publication.metadata.sources,
        types: publication.metadata.types,
        formats: publication.metadata.formats,
        relations: publication.metadata.relations,
        coverage: publication.metadata.coverage,
        properties: publication.metadata.properties,
        audiences: publication.metadata.audiences,
        age_ranges: publication.metadata.age_ranges,
        images: publication.metadata.images,
        ratings: publication.metadata.ratings,
        interactions: publication.metadata.interactions,
        popularity_ranks: publication.metadata.popularity_ranks,
    };
    let languages = publication.languages.into_iter().filter_map(|language| LanguageTag::parse(language).ok()).collect();
    CatalogPublication { identity, title: publication.title, authors, languages, description, image_url: publication.image_url, metadata, actions }
}

fn localized_text(value: opds::OpdsLocalizedText) -> LocalizedText {
    LocalizedText { language: value.language, value: value.value }
}

fn catalog_identifier_scope(scope: opds::OpdsIdentifierScope) -> Option<Scope> {
    match scope {
        opds::OpdsIdentifierScope::Publication => Some(Scope::Publication),
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

fn opds_action(source_id: &CatalogSourceId, catalog_locator: &str, identity: &CatalogPublicationIdentity, acquisition: OpdsAcquisition) -> Option<CatalogPublicationAction> {
    match acquisition {
        OpdsAcquisition::Direct(acquisition) if acquisition.price.is_none() && matches!(acquisition.kind, OpdsAcquisitionKind::OpenAccess | OpdsAcquisitionKind::Generic) => Some(CatalogPublicationAction::Import {
            request: CatalogImportRequest { source_id: source_id.clone(), catalog_locator: catalog_locator.to_owned(), publication: identity.clone(), download_url: acquisition.url, book_format: book_format(acquisition.book_format) },
        }),
        OpdsAcquisition::Direct(acquisition) => {
            Some(CatalogPublicationAction::External { purpose: external_purpose(acquisition.kind), url: acquisition.url, price: acquisition.price.map(price), available_formats: vec![book_format(acquisition.book_format)] })
        }
        OpdsAcquisition::Indirect(acquisition) => Some(CatalogPublicationAction::External {
            purpose: external_purpose(acquisition.kind),
            url: acquisition.url,
            price: acquisition.price.map(price),
            available_formats: acquisition.available_formats.into_iter().map(book_format).collect(),
        }),
        OpdsAcquisition::Unsupported(acquisition) => Some(CatalogPublicationAction::External { purpose: external_purpose(acquisition.kind), url: acquisition.url, price: acquisition.price.map(price), available_formats: Vec::new() }),
    }
}

#[doc(hidden)]
pub fn opds_search(search: OpdsSearch) -> CatalogSearch {
    let mut fields = vec![CatalogSearchField::Query];
    if let OpdsSearch::UriTemplate { template, .. } = &search {
        if template_has_variable(template, "title") {
            fields.push(CatalogSearchField::Title);
        }
        if template_has_variable(template, "author") {
            fields.push(CatalogSearchField::Author);
        }
    }
    let definition = match search {
        OpdsSearch::UriTemplate { base_url, template } => CatalogSearchDefinition::OpdsUriTemplate { base_url, template },
        OpdsSearch::OpenSearchDescription { url } => CatalogSearchDefinition::OpdsOpenSearchDescription { url },
    };
    CatalogSearch { definition, fields }
}

fn template_has_variable(template: &str, expected: &str) -> bool {
    template
        .split('{')
        .skip(1)
        .filter_map(|suffix| suffix.split_once('}').map(|(expression, _)| expression))
        .flat_map(|expression| expression.trim_start_matches(['+', '#', '.', '/', ';', '?', '&']).split(','))
        .any(|variable| variable.trim_end_matches(['?', '*']).split(':').next() == Some(expected))
}

fn book_format(format: OpdsBookFormat) -> CatalogBookFormat {
    match format {
        OpdsBookFormat::Epub => CatalogBookFormat::Epub,
        OpdsBookFormat::Pdf => CatalogBookFormat::Pdf,
        OpdsBookFormat::Mobi => CatalogBookFormat::Mobi,
        OpdsBookFormat::M4b => CatalogBookFormat::M4b,
    }
}

fn price(value: OpdsPrice) -> CatalogPrice {
    CatalogPrice { currency: value.currency, value: value.value }
}

fn external_purpose(kind: OpdsAcquisitionKind) -> CatalogExternalActionKind {
    match kind {
        OpdsAcquisitionKind::Preview => CatalogExternalActionKind::Preview,
        OpdsAcquisitionKind::Buy => CatalogExternalActionKind::Buy,
        OpdsAcquisitionKind::Borrow => CatalogExternalActionKind::Borrow,
        OpdsAcquisitionKind::Subscribe => CatalogExternalActionKind::Subscribe,
        OpdsAcquisitionKind::OpenAccess | OpdsAcquisitionKind::Generic => CatalogExternalActionKind::Open,
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
        assert_eq!(opds_endpoint_id(&CatalogSourceId::parse("opds-example").unwrap()).unwrap().as_str(), "example");
        assert!(opds_endpoint_id(&CatalogSourceId::parse("unknown").unwrap()).is_err());
        assert_eq!(canonicalize_opds_catalog_locator(&endpoint(), "http://catalog.example/page").unwrap(), "https://catalog.example/page");
        assert!(validate_opds_catalog_locator(&endpoint(), "https://attacker.example/page").is_err());
        assert!(validate_opds_catalog_locator(&endpoint(), "http://catalog.example:8080/page").is_err());
        assert!(validate_opds_catalog_locator(&endpoint(), "https://user:secret@catalog.example/page").is_err());
    }

    #[test]
    fn search_exposes_only_template_supported_fields() {
        let simple = opds_search(OpdsSearch::UriTemplate { base_url: "https://example.com".to_owned(), template: "search{?searchTerms}".to_owned() });
        assert_eq!(simple.fields, vec![CatalogSearchField::Query]);
        let advanced = opds_search(OpdsSearch::UriTemplate { base_url: "https://example.com".to_owned(), template: "search{?query,title,author}".to_owned() });
        assert_eq!(advanced.fields, vec![CatalogSearchField::Query, CatalogSearchField::Title, CatalogSearchField::Author]);
    }

    #[test]
    fn source_ids_are_namespaced_for_the_server_adapter() {
        let source = opds_source(&endpoint());
        assert_eq!(source.id.as_str(), "opds-example");
        assert_eq!(source.title, "Example");
        assert!(source.capabilities.contains(&CatalogCapability::Import));
    }
}
