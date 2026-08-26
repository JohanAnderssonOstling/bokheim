use crate::OutboundNetworkPolicy;
use crate::{
    CollectionKind, OpdsAcquisition, OpdsAcquisitionKind, OpdsAgentCredit, OpdsAuthorCredit, OpdsAuthorLink, OpdsBookFormat, OpdsCatalog, OpdsCatalogFreshness, OpdsCatalogLink, OpdsContributorCredit, OpdsDirectAcquisition, OpdsError,
    OpdsFacet, OpdsIdentifierScope, OpdsInteractionStatistic, OpdsLinkTarget, OpdsPagination, OpdsPublication, OpdsPublicationCollection, OpdsPublicationDate, OpdsPublicationIdentifier, OpdsPublicationMetadata, OpdsPublicationProperty,
    OpdsPublicationSubject, OpdsPublicationTitle, OpdsSearch, OpdsVersion,
};
use reqwest::Url;
use std::time::Duration;

pub(crate) fn is_catalog_url(url: &Url) -> bool {
    ::litteraturbanken::is_catalog_url(url)
}

pub(crate) fn decorate_download_headers(headers: &mut reqwest::header::HeaderMap, url: &Url) {
    ::litteraturbanken::decorate_download_headers(headers, url);
}

pub(crate) async fn fetch_catalog(policy: OutboundNetworkPolicy, source_url: Url, byte_limit: u64) -> Result<OpdsCatalog, OpdsError> {
    let client = crate::outbound::client_for_url(&source_url, policy, &format!("Bokheim/{} OPDS", env!("CARGO_PKG_VERSION")), Duration::from_secs(10)).await.map_err(|error| OpdsError::InvalidUrl(error.to_string()))?;
    let catalog = ::litteraturbanken::fetch_catalog(&client, source_url, byte_limit).await.map_err(provider_error)?;
    let pagination = OpdsPagination {
        first: catalog.first_page.as_ref().map(ToString::to_string),
        previous: catalog.previous_page.as_ref().map(ToString::to_string),
        next: catalog.next_page.as_ref().map(ToString::to_string),
        last: catalog.last_page.as_ref().map(ToString::to_string),
        number_of_items: Some(catalog.total_items),
        items_per_page: Some(::litteraturbanken::PAGE_SIZE),
        current_page: Some(catalog.current_page),
    };
    Ok(OpdsCatalog {
        version: OpdsVersion::V2_0,
        freshness: OpdsCatalogFreshness::Current,
        title: catalog.title,
        source_url: catalog.source_url.to_string(),
        self_url: Some(catalog.source_url.to_string()),
        start_url: Some(catalog.root_url.to_string()),
        navigation: vec![OpdsCatalogLink { title: "Litteraturbanken on the web".to_owned(), url: catalog.root_url.to_string(), target: OpdsLinkTarget::Web, relations: Vec::new(), number_of_items: Some(catalog.total_items), active: false }],
        publications: catalog.publications.into_iter().map(publication).collect(),
        groups: Vec::new(),
        facets: catalog
            .facets
            .into_iter()
            .map(|facet| OpdsFacet {
                title: facet.title,
                links: facet
                    .links
                    .into_iter()
                    .map(|link| OpdsCatalogLink { title: link.title, url: link.url.to_string(), target: OpdsLinkTarget::Catalog, relations: vec!["http://opds-spec.org/facet".to_owned()], number_of_items: None, active: link.active })
                    .collect(),
            })
            .collect(),
        pagination,
        search: Some(OpdsSearch::UriTemplate { base_url: catalog.source_url.to_string(), template: catalog.search_template }),
    })
}

fn publication(publication: ::litteraturbanken::Publication) -> OpdsPublication {
    let acquisition = OpdsDirectAcquisition { url: publication.download_url.to_string(), kind: OpdsAcquisitionKind::OpenAccess, book_format: OpdsBookFormat::Epub, suggested_file_name: publication.file_name, price: None };
    // lbworkid identifies a digitized source and is reused by some separately
    // listed texts within that source. titleid makes the catalogue identity
    // unique without pretending work_titleid is a FRBR work identity.
    let id = format!("{}:{}", publication.id, publication.title_id);
    let collections = publication.series.as_deref().and_then(series_collection).into_iter().collect();
    let mut identifiers = vec![OpdsPublicationIdentifier { value: publication.id.clone(), scheme: Some("litteraturbanken".to_owned()), source: "litteraturbanken:lbworkid".to_owned(), scope: OpdsIdentifierScope::CatalogEntry }];
    identifiers.push(OpdsPublicationIdentifier { value: publication.title_id.clone(), scheme: Some("litteraturbanken-title".to_owned()), source: "litteraturbanken:titleid".to_owned(), scope: OpdsIdentifierScope::CatalogEntry });
    if let Some(value) = publication.isbn.clone() {
        identifiers.push(OpdsPublicationIdentifier { value, scheme: Some("isbn".to_owned()), source: "litteraturbanken:isbn_download".to_owned(), scope: OpdsIdentifierScope::Edition });
    }
    if let Some(value) = publication.urn.clone() {
        identifiers.push(OpdsPublicationIdentifier { value, scheme: Some("urn:nbn".to_owned()), source: "litteraturbanken:urn".to_owned(), scope: OpdsIdentifierScope::Edition });
    }
    if let Some(value) = publication.libris_id.clone() {
        identifiers.push(OpdsPublicationIdentifier { value, scheme: Some("libris".to_owned()), source: "litteraturbanken:librisid".to_owned(), scope: OpdsIdentifierScope::Edition });
    }
    let publishers = publication
        .publisher
        .first()
        .and_then(|name| provider_plain_text(name))
        .map(|name| vec![OpdsAgentCredit { name, name_localizations: Vec::new(), sort_name: None, sort_name_localizations: Vec::new(), identifiers: Vec::new(), provider_link: None }])
        .unwrap_or_default();
    let mut dates = Vec::new();
    push_date(&mut dates, publication.imprint_year.as_deref(), "published", "litteraturbanken:imprintyear");
    push_date(&mut dates, publication.imported.as_deref(), "imported", "litteraturbanken:imported");
    push_date(&mut dates, publication.work_updated.as_deref(), "modified", "litteraturbanken:work_updated");
    push_date(&mut dates, publication.epub_updated.as_deref(), "epub-export", "litteraturbanken:export.epub.updated");
    let subjects = publication.keywords.iter().filter_map(|keyword| OpdsPublicationSubject::new(None, keyword, "litteraturbanken:keyword", Some("litteraturbanken:keyword".to_owned()), None).ok()).collect();
    let contributors = publication
        .contributors
        .iter()
        .map(|contributor| OpdsContributorCredit {
            name: contributor.name.clone(),
            name_localizations: Vec::new(),
            sort_name: contributor.sort_name.clone(),
            sort_name_localizations: Vec::new(),
            // Litteraturbanken author IDs are provider-local route keys, not
            // globally meaningful contributor identifiers.
            identifiers: Vec::new(),
            provider_link: contributor.provider_url.as_ref().map(|url| OpdsAuthorLink { url: url.to_string(), target: OpdsLinkTarget::Web }),
            roles: contributor.role.clone().into_iter().collect(),
        })
        .collect();
    let mut properties = Vec::new();
    push_property(&mut properties, "litteraturbanken:edition", publication.edition.as_deref(), "litteraturbanken:edition");
    push_property(&mut properties, "litteraturbanken:work-title-id", publication.work_title_id.as_deref(), "litteraturbanken:work_titleid");
    push_property(&mut properties, "litteraturbanken:description-attribution", publication.description_attribution.as_deref(), "litteraturbanken:workintro_download_author");
    push_property(&mut properties, "litteraturbanken:cover-attribution", publication.cover_attribution.as_deref(), "litteraturbanken:cover_download_info");
    push_property(&mut properties, "litteraturbanken:mediatype", publication.media_type.as_deref(), "litteraturbanken:mediatype");
    if let Some(value) = publication.modernized {
        push_property(&mut properties, "litteraturbanken:modernized", Some(if value { "true" } else { "false" }), "litteraturbanken:modernized");
    }
    if let Some(value) = publication.fraktur {
        push_property(&mut properties, "litteraturbanken:fraktur", Some(if value { "true" } else { "false" }), "litteraturbanken:fraktur");
    }
    for (property, value) in publisher_properties(&publication.publisher) {
        push_property(&mut properties, property, Some(&value), "litteraturbanken:publisher");
    }
    let mut interactions = Vec::new();
    if let Some(count) = publication.popularity {
        interactions.push(OpdsInteractionStatistic { interaction_type: "litteraturbanken:popularity".to_owned(), count, service: Some("Litteraturbanken".to_owned()), source: "litteraturbanken:popularity".to_owned() });
    }
    if let Some(count) = publication.epub_popularity {
        interactions.push(OpdsInteractionStatistic { interaction_type: "litteraturbanken:epub-popularity".to_owned(), count, service: Some("Litteraturbanken".to_owned()), source: "litteraturbanken:epub_popularity".to_owned() });
    }
    let modified = publication.epub_updated.clone().or_else(|| publication.work_updated.clone());
    let metadata = OpdsPublicationMetadata {
        published: publication.imprint_year.clone(),
        modified,
        dates,
        publishers,
        identifiers,
        subjects,
        alternate_titles: publication.alternate_titles.iter().filter_map(|title| OpdsPublicationTitle::new(None, title, Some("expanded".to_owned()), None, "litteraturbanken:title").ok()).collect(),
        collections,
        contributors,
        rights: publication.rights.clone(),
        sources: publication.sources.clone(),
        types: publication.text_type.clone().into_iter().collect(),
        formats: vec!["application/epub+zip".to_owned()],
        relations: publication.provider_url.as_ref().map(ToString::to_string).into_iter().collect(),
        coverage: publication.geographic_keywords.clone(),
        properties,
        interactions,
        ..OpdsPublicationMetadata::default()
    };
    OpdsPublication {
        id,
        work_id: None,
        title: publication.title,
        authors: publication
            .authors
            .into_iter()
            .map(|author| OpdsAuthorCredit {
                name: author.name,
                name_localizations: Vec::new(),
                sort_name: author.sort_name,
                sort_name_localizations: Vec::new(),
                // Keep the provider-local author ID confined to the provider
                // model and its link; do not publish it as identity evidence.
                identifiers: Vec::new(),
                provider_link: author.provider_url.map(|url| OpdsAuthorLink { url: url.to_string(), target: OpdsLinkTarget::Web }),
            })
            .collect(),
        languages: publication.language.into_iter().collect(),
        description: publication.description_html.as_deref().and_then(crate::inferred_description),
        image_url: None,
        metadata,
        preferred_download: Some(acquisition.clone()),
        acquisitions: vec![OpdsAcquisition::Direct(acquisition)],
    }
}

fn provider_plain_text(value: &str) -> Option<String> {
    let value = crate::compact_description_whitespace(&html::parse::plain_text_from_fragment(value))?;
    (!value.is_empty()).then_some(value)
}

fn push_date(target: &mut Vec<OpdsPublicationDate>, value: Option<&str>, event: &str, source: &str) {
    if let Some(date) = value.and_then(|value| OpdsPublicationDate::new(None, value, Some(event.to_owned()), source).ok()) {
        target.push(date);
    }
}

fn push_property(target: &mut Vec<OpdsPublicationProperty>, property: &str, value: Option<&str>, source: &str) {
    if let Some(property) = value.and_then(|value| OpdsPublicationProperty::new(None, property, value, None, None, None, source).ok()) {
        target.push(property);
    }
}

fn publisher_properties(values: &[String]) -> Vec<(&'static str, String)> {
    let Some((_, details)) = values.split_first() else { return Vec::new() };
    let (places, country) = details.last().filter(|value| value.len() == 2 && value.chars().all(|character| character.is_ascii_alphabetic())).map_or((details, None), |_| (&details[..details.len() - 1], details.last()));
    let mut properties = places.iter().filter_map(|value| provider_plain_text(value).map(|value| ("litteraturbanken:publisher-place", value))).collect::<Vec<_>>();
    if let Some(country) = country {
        properties.push(("litteraturbanken:publisher-country", country.to_owned()));
    }
    properties
}

fn series_collection(value: &str) -> Option<OpdsPublicationCollection> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let (name, position) = value
        .rsplit_once(char::is_whitespace)
        .filter(|(name, position)| !name.trim().is_empty() && !position.is_empty() && position.chars().all(|character| character.is_ascii_digit()))
        .map_or((value, None), |(name, position)| (name.trim(), Some(position.to_owned())));
    OpdsPublicationCollection::new(name, Some(CollectionKind::Series), position, "litteraturbanken:series").ok()
}

fn provider_error(error: ::litteraturbanken::Error) -> OpdsError {
    match error {
        ::litteraturbanken::Error::InvalidUrl(error) => OpdsError::InvalidUrl(error),
        ::litteraturbanken::Error::Network(error) => OpdsError::Network(error),
        ::litteraturbanken::Error::TimedOut => OpdsError::TimedOut,
        ::litteraturbanken::Error::HttpStatus { status, retry_after } => OpdsError::HttpStatus { status, retry_after },
        ::litteraturbanken::Error::ResponseTooLarge { limit } => OpdsError::ResponseTooLarge { limit },
        ::litteraturbanken::Error::InvalidResponse(error) => OpdsError::InvalidCatalog(error),
    }
}

#[cfg(test)]
mod tests {
    use super::{publication, series_collection};

    #[test]
    fn provider_series_becomes_shared_series_membership() {
        let collection = series_collection("Barnbiblioteket Saga 3").unwrap();
        assert_eq!(collection.name(), "Barnbiblioteket Saga");
        assert_eq!(collection.collection_type(), Some("series"));
        assert_eq!(collection.position(), Some("3"));
        assert_eq!(collection.source(), "litteraturbanken:series");
        assert!(collection.identifiers.is_empty());
    }

    #[test]
    fn provider_series_without_a_numeric_suffix_keeps_its_full_name() {
        let collection = series_collection("Svenska klassiker").unwrap();
        assert_eq!(collection.name(), "Svenska klassiker");
        assert_eq!(collection.position(), None);
    }

    #[test]
    fn rich_provider_metadata_reaches_existing_opds_evidence_fields() {
        let publication = publication(::litteraturbanken::Publication {
            id: "lb1728740".to_owned(),
            title_id: "DoktorGlas".to_owned(),
            work_title_id: Some("DoktorGlasSamlade".to_owned()),
            title: "Doktor Glas".to_owned(),
            alternate_titles: vec!["Doktor Glas. Roman".to_owned()],
            authors: vec![::litteraturbanken::Author {
                id: "SöderbergH".to_owned(),
                name: "Hjalmar Söderberg".to_owned(),
                sort_name: Some("Söderberg, Hjalmar".to_owned()),
                role: None,
                provider_url: Some(reqwest::Url::parse("https://litteraturbanken.se/författare/SöderbergH").unwrap()),
            }],
            contributors: vec![::litteraturbanken::Author { id: "EditorE".to_owned(), name: "Eva Redaktör".to_owned(), sort_name: Some("Redaktör, Eva".to_owned()), role: Some("editor".to_owned()), provider_url: None }],
            language: Some("sv".to_owned()),
            series: Some("Barnbiblioteket Saga 3".to_owned()),
            isbn: Some("978-91-8123-064-2".to_owned()),
            urn: Some("urn:nbn:se:lb-lb1728740-etext".to_owned()),
            libris_id: Some("1728740".to_owned()),
            publisher: vec!["Almqvist &amp; Wiksell förlag".to_owned(), "Stockholm".to_owned(), "SE".to_owned()],
            imprint_year: Some("1905".to_owned()),
            edition: Some("1".to_owned()),
            imported: Some("2012-01-17".to_owned()),
            work_updated: Some("2026-04-27".to_owned()),
            epub_updated: Some("2026-05-21".to_owned()),
            keywords: vec!["Brott".to_owned(), "Dagboksromaner".to_owned()],
            geographic_keywords: vec!["Stockholm".to_owned()],
            text_type: Some("roman".to_owned()),
            media_type: Some("etext".to_owned()),
            description_html: Some("<p>En kort <i>introduktion</i>.</p>".to_owned()),
            description_attribution: Some("HelgesonP".to_owned()),
            rights: vec!["cc-0".to_owned()],
            popularity: Some(2_059_021),
            epub_popularity: Some(9_705),
            modernized: Some(false),
            fraktur: Some(false),
            cover_attribution: Some("Omslag från GUB.".to_owned()),
            sources: vec!["GUB: Litt. Sv.".to_owned()],
            provider_url: Some(reqwest::Url::parse("https://litteraturbanken.se/författare/SöderbergH/titlar/DoktorGlas/info").unwrap()),
            download_url: reqwest::Url::parse("https://litteraturbanken.se/txt/epub/S%C3%B6derbergH_DoktorGlas.epub").unwrap(),
            file_name: "SöderbergH_DoktorGlas.epub".to_owned(),
        });

        assert_eq!(publication.id, "lb1728740:DoktorGlas");
        assert_eq!(publication.work_id, None);
        assert_eq!(publication.metadata.published.as_deref(), Some("1905"));
        assert_eq!(publication.metadata.modified.as_deref(), Some("2026-05-21"));
        assert_eq!(publication.metadata.publishers[0].name, "Almqvist & Wiksell förlag");
        assert!(publication.metadata.identifiers.iter().any(|identifier| identifier.scheme.as_deref() == Some("isbn") && identifier.scope == super::OpdsIdentifierScope::Edition));
        assert_eq!(publication.metadata.subjects.iter().map(|subject| subject.name()).collect::<Vec<_>>(), ["Brott", "Dagboksromaner"]);
        assert_eq!(publication.metadata.coverage, ["Stockholm"]);
        assert_eq!(publication.metadata.types, ["roman"]);
        assert_eq!(publication.metadata.rights, ["cc-0"]);
        assert_eq!(publication.metadata.sources, ["GUB: Litt. Sv."]);
        assert_eq!(publication.metadata.alternate_titles[0].value(), "Doktor Glas. Roman");
        assert_eq!(publication.metadata.alternate_titles[0].title_type(), Some("expanded"));
        assert_eq!(publication.metadata.relations, ["https://litteraturbanken.se/f%C3%B6rfattare/S%C3%B6derbergH/titlar/DoktorGlas/info"]);
        assert_eq!(publication.metadata.contributors[0].roles, ["editor"]);
        assert!(publication.authors[0].identifiers.is_empty());
        assert!(publication.metadata.contributors[0].identifiers.is_empty());
        assert_eq!(publication.metadata.collections[0].position(), Some("3"));
        assert_eq!(publication.metadata.interactions.len(), 2);
        assert!(publication.metadata.properties.iter().any(|property| property.property() == "litteraturbanken:publisher-place" && property.value() == "Stockholm"));
        assert!(publication.metadata.properties.iter().any(|property| property.property() == "litteraturbanken:publisher-country" && property.value() == "SE"));
        assert!(publication.metadata.properties.iter().any(|property| property.property() == "litteraturbanken:description-attribution" && property.value() == "HelgesonP"));
        assert!(!publication.metadata.properties.iter().any(|property| property.property() == "litteraturbanken:description-author"));
        assert_eq!(publication.description.as_ref().unwrap().plain_text(), "En kort introduktion.");
    }

    #[tokio::test]
    #[ignore = "requires the live Litteraturbanken API"]
    async fn live_catalogue_search_and_download_are_available() {
        let client = crate::OpdsClient::new().unwrap();
        let root = client.fetch_catalog(::litteraturbanken::CATALOG_URL).await.unwrap();
        assert_eq!(root.publications.len(), ::litteraturbanken::PAGE_SIZE as usize);
        assert!(root.pagination.next.is_some());
        let search = client.search(&root, "Doktor Glas").await.unwrap();
        let publication = search.publications.iter().find(|publication| publication.title == "Doktor Glas").unwrap();
        let bytes = client.download_book_bytes(publication.preferred_download.as_ref().unwrap()).await.unwrap();
        assert!(bytes.starts_with(b"PK"));
    }
}
