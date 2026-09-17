use crate::OutboundNetworkPolicy;
use crate::{
    CollectionKind, OpdsAcquisition, OpdsAcquisitionKind, OpdsAgentCredit, OpdsAuthorCredit, OpdsAuthorLink, OpdsBookFormat, OpdsCatalog, OpdsCatalogFreshness, OpdsCatalogLink, OpdsContributorCredit, OpdsDirectAcquisition, OpdsError,
    OpdsFacet, OpdsIdentifierScope, OpdsInteractionStatistic, OpdsLinkTarget, OpdsPagination, OpdsBook, OpdsBookCollection, OpdsBookDate, OpdsBookIdentifier, OpdsBookMetadata, OpdsBookProperty,
    OpdsBookSubject, OpdsBookTitle, OpdsVersion,
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
        books: catalog.books.into_iter().map(book).collect(),
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
    })
}

fn book(book: ::litteraturbanken::Book) -> OpdsBook {
    let acquisition = OpdsDirectAcquisition { url: book.download_url.to_string(), kind: OpdsAcquisitionKind::OpenAccess, book_format: OpdsBookFormat::Epub, suggested_file_name: book.file_name, price: None };
    // lbworkid identifies a digitized source and is reused by some separately
    // listed texts within that source. titleid makes the catalogue identity
    // unique without pretending work_titleid is a FRBR work identity.
    let id = format!("{}:{}", book.id, book.title_id);
    let collections = book.series.as_deref().and_then(series_collection).into_iter().collect();
    let mut identifiers = vec![OpdsBookIdentifier { value: book.id.clone(), scheme: Some("litteraturbanken".to_owned()), source: "litteraturbanken:lbworkid".to_owned(), scope: OpdsIdentifierScope::CatalogEntry }];
    identifiers.push(OpdsBookIdentifier { value: book.title_id.clone(), scheme: Some("litteraturbanken-title".to_owned()), source: "litteraturbanken:titleid".to_owned(), scope: OpdsIdentifierScope::CatalogEntry });
    if let Some(value) = book.isbn.clone() {
        identifiers.push(OpdsBookIdentifier { value, scheme: Some("isbn".to_owned()), source: "litteraturbanken:isbn_download".to_owned(), scope: OpdsIdentifierScope::Edition });
    }
    if let Some(value) = book.urn.clone() {
        identifiers.push(OpdsBookIdentifier { value, scheme: Some("urn:nbn".to_owned()), source: "litteraturbanken:urn".to_owned(), scope: OpdsIdentifierScope::Edition });
    }
    if let Some(value) = book.libris_id.clone() {
        identifiers.push(OpdsBookIdentifier { value, scheme: Some("libris".to_owned()), source: "litteraturbanken:librisid".to_owned(), scope: OpdsIdentifierScope::Edition });
    }
    let publishers = book
        .publisher
        .first()
        .and_then(|name| provider_plain_text(name))
        .map(|name| vec![OpdsAgentCredit { name, name_localizations: Vec::new(), sort_name: None, sort_name_localizations: Vec::new(), identifiers: Vec::new(), provider_link: None }])
        .unwrap_or_default();
    let mut dates = Vec::new();
    push_date(&mut dates, book.imprint_year.as_deref(), "published", "litteraturbanken:imprintyear");
    push_date(&mut dates, book.imported.as_deref(), "imported", "litteraturbanken:imported");
    push_date(&mut dates, book.work_updated.as_deref(), "modified", "litteraturbanken:work_updated");
    push_date(&mut dates, book.epub_updated.as_deref(), "epub-export", "litteraturbanken:export.epub.updated");
    let subjects = book.keywords.iter().filter_map(|keyword| OpdsBookSubject::new(None, keyword, "litteraturbanken:keyword", Some("litteraturbanken:keyword".to_owned()), None).ok()).collect();
    let contributors = book
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
    push_property(&mut properties, "litteraturbanken:edition", book.edition.as_deref(), "litteraturbanken:edition");
    push_property(&mut properties, "litteraturbanken:work-title-id", book.work_title_id.as_deref(), "litteraturbanken:work_titleid");
    push_property(&mut properties, "litteraturbanken:description-attribution", book.description_attribution.as_deref(), "litteraturbanken:workintro_download_author");
    push_property(&mut properties, "litteraturbanken:cover-attribution", book.cover_attribution.as_deref(), "litteraturbanken:cover_download_info");
    push_property(&mut properties, "litteraturbanken:mediatype", book.media_type.as_deref(), "litteraturbanken:mediatype");
    if let Some(value) = book.modernized {
        push_property(&mut properties, "litteraturbanken:modernized", Some(if value { "true" } else { "false" }), "litteraturbanken:modernized");
    }
    if let Some(value) = book.fraktur {
        push_property(&mut properties, "litteraturbanken:fraktur", Some(if value { "true" } else { "false" }), "litteraturbanken:fraktur");
    }
    for (property, value) in publisher_properties(&book.publisher) {
        push_property(&mut properties, property, Some(&value), "litteraturbanken:publisher");
    }
    let mut interactions = Vec::new();
    if let Some(count) = book.popularity {
        interactions.push(OpdsInteractionStatistic { interaction_type: "litteraturbanken:popularity".to_owned(), count, service: Some("Litteraturbanken".to_owned()), source: "litteraturbanken:popularity".to_owned() });
    }
    if let Some(count) = book.epub_popularity {
        interactions.push(OpdsInteractionStatistic { interaction_type: "litteraturbanken:epub-popularity".to_owned(), count, service: Some("Litteraturbanken".to_owned()), source: "litteraturbanken:epub_popularity".to_owned() });
    }
    let modified = book.epub_updated.clone().or_else(|| book.work_updated.clone());
    let metadata = OpdsBookMetadata {
        published: book.imprint_year.clone(),
        modified,
        dates,
        publishers,
        identifiers,
        subjects,
        alternate_titles: book.alternate_titles.iter().filter_map(|title| OpdsBookTitle::new(None, title, Some("expanded".to_owned()), None, "litteraturbanken:title").ok()).collect(),
        collections,
        contributors,
        rights: book.rights.clone(),
        sources: book.sources.clone(),
        types: book.text_type.clone().into_iter().collect(),
        formats: vec!["application/epub+zip".to_owned()],
        relations: book.provider_url.as_ref().map(ToString::to_string).into_iter().collect(),
        coverage: book.geographic_keywords.clone(),
        properties,
        interactions,
        ..OpdsBookMetadata::default()
    };
    OpdsBook {
        id,
        work_id: None,
        title: book.title,
        authors: book
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
        languages: book.language.into_iter().collect(),
        description: book.description_html.as_deref().and_then(crate::inferred_description),
        image_url: None,
        metadata,
        preferred_download: Some(acquisition.clone()),
        acquisitions: vec![OpdsAcquisition::Direct(acquisition)],
    }
}

fn provider_plain_text(value: &str) -> Option<String> {
    let value = quick_xml::escape::unescape(value).ok()?;
    let value = crate::compact_description_whitespace(&value)?;
    (!value.is_empty()).then_some(value)
}

fn push_date(target: &mut Vec<OpdsBookDate>, value: Option<&str>, event: &str, source: &str) {
    if let Some(date) = value.and_then(|value| OpdsBookDate::new(None, value, Some(event.to_owned()), source).ok()) {
        target.push(date);
    }
}

fn push_property(target: &mut Vec<OpdsBookProperty>, property: &str, value: Option<&str>, source: &str) {
    if let Some(property) = value.and_then(|value| OpdsBookProperty::new(None, property, value, None, None, None, source).ok()) {
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

fn series_collection(value: &str) -> Option<OpdsBookCollection> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let (name, position) = value
        .rsplit_once(char::is_whitespace)
        .filter(|(name, position)| !name.trim().is_empty() && !position.is_empty() && position.chars().all(|character| character.is_ascii_digit()))
        .map_or((value, None), |(name, position)| (name.trim(), Some(position.to_owned())));
    OpdsBookCollection::new(name, Some(CollectionKind::Series), position, "litteraturbanken:series").ok()
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
    use super::{book, series_collection};

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
        let book = book(::litteraturbanken::Book {
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

        assert_eq!(book.id, "lb1728740:DoktorGlas");
        assert_eq!(book.work_id, None);
        assert_eq!(book.metadata.published.as_deref(), Some("1905"));
        assert_eq!(book.metadata.modified.as_deref(), Some("2026-05-21"));
        assert_eq!(book.metadata.publishers[0].name, "Almqvist & Wiksell förlag");
        assert!(book.metadata.identifiers.iter().any(|identifier| identifier.scheme.as_deref() == Some("isbn") && identifier.scope == super::OpdsIdentifierScope::Edition));
        assert_eq!(book.metadata.subjects.iter().map(|subject| subject.name()).collect::<Vec<_>>(), ["Brott", "Dagboksromaner"]);
        assert_eq!(book.metadata.coverage, ["Stockholm"]);
        assert_eq!(book.metadata.types, ["roman"]);
        assert_eq!(book.metadata.rights, ["cc-0"]);
        assert_eq!(book.metadata.sources, ["GUB: Litt. Sv."]);
        assert_eq!(book.metadata.alternate_titles[0].value(), "Doktor Glas. Roman");
        assert_eq!(book.metadata.alternate_titles[0].title_type(), Some("expanded"));
        assert_eq!(book.metadata.relations, ["https://litteraturbanken.se/f%C3%B6rfattare/S%C3%B6derbergH/titlar/DoktorGlas/info"]);
        assert_eq!(book.metadata.contributors[0].roles, ["editor"]);
        assert!(book.authors[0].identifiers.is_empty());
        assert!(book.metadata.contributors[0].identifiers.is_empty());
        assert_eq!(book.metadata.collections[0].position(), Some("3"));
        assert_eq!(book.metadata.interactions.len(), 2);
        assert!(book.metadata.properties.iter().any(|property| property.property() == "litteraturbanken:publisher-place" && property.value() == "Stockholm"));
        assert!(book.metadata.properties.iter().any(|property| property.property() == "litteraturbanken:publisher-country" && property.value() == "SE"));
        assert!(book.metadata.properties.iter().any(|property| property.property() == "litteraturbanken:description-attribution" && property.value() == "HelgesonP"));
        assert!(!book.metadata.properties.iter().any(|property| property.property() == "litteraturbanken:description-author"));
        assert_eq!(book.description.as_ref().unwrap().markup(), Some("<p>En kort <i>introduktion</i>.</p>"));
    }

    #[tokio::test]
    #[ignore = "requires the live Litteraturbanken API"]
    async fn live_catalogue_and_download_are_available() {
        let client = crate::OpdsClient::new().unwrap();
        let root = client.fetch_catalog(::litteraturbanken::CATALOG_URL).await.unwrap();
        assert_eq!(root.books.len(), ::litteraturbanken::PAGE_SIZE as usize);
        assert!(root.pagination.next.is_some());
        let book = &root.books[0];
        let bytes = client.download_book_bytes(book.preferred_download.as_ref().unwrap()).await.unwrap();
        assert!(bytes.starts_with(b"PK"));
    }
}
