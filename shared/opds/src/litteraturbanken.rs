use crate::{
    OpdsAcquisition, OpdsAcquisitionKind, OpdsAuthorCredit, OpdsAuthorLink, OpdsBookFormat, OpdsCatalog, OpdsCatalogFreshness, OpdsCatalogLink, OpdsDirectAcquisition, OpdsError, OpdsFacet, OpdsLinkTarget, OpdsPagination, OpdsPublication,
    OpdsSearch, OpdsVersion,
};
use reqwest::{Client, RequestBuilder, Url};

pub(crate) fn is_catalog_url(url: &Url) -> bool {
    ::litteraturbanken::is_catalog_url(url)
}

pub(crate) fn download_request(request: RequestBuilder, url: &Url) -> RequestBuilder {
    ::litteraturbanken::decorate_download_request(request, url)
}

pub(crate) async fn fetch_catalog(client: &Client, source_url: Url, byte_limit: u64) -> Result<OpdsCatalog, OpdsError> {
    let catalog = ::litteraturbanken::fetch_catalog(client, source_url, byte_limit).await.map_err(provider_error)?;
    Ok(OpdsCatalog {
        version: OpdsVersion::V2_0,
        freshness: OpdsCatalogFreshness::Current,
        title: catalog.title,
        source_url: catalog.source_url.to_string(),
        self_url: Some(catalog.source_url.to_string()),
        start_url: Some(catalog.root_url.to_string()),
        navigation: vec![OpdsCatalogLink { title: "Litteraturbanken on the web".to_owned(), url: catalog.root_url.to_string(), target: OpdsLinkTarget::Web, number_of_items: Some(catalog.total_items), active: false }],
        publications: catalog.publications.into_iter().map(publication).collect(),
        groups: Vec::new(),
        facets: catalog
            .facets
            .into_iter()
            .map(|facet| OpdsFacet {
                title: facet.title,
                links: facet.links.into_iter().map(|link| OpdsCatalogLink { title: link.title, url: link.url.to_string(), target: OpdsLinkTarget::Catalog, number_of_items: None, active: link.active }).collect(),
            })
            .collect(),
        pagination: OpdsPagination { number_of_items: Some(catalog.total_items), items_per_page: Some(catalog.total_items), current_page: Some(1), ..OpdsPagination::default() },
        search: Some(OpdsSearch::UriTemplate { base_url: catalog.source_url.to_string(), template: catalog.search_template }),
    })
}

fn publication(publication: ::litteraturbanken::Publication) -> OpdsPublication {
    let acquisition = OpdsDirectAcquisition { url: publication.download_url.to_string(), kind: OpdsAcquisitionKind::OpenAccess, book_format: OpdsBookFormat::Epub, suggested_file_name: publication.file_name, price: None };
    OpdsPublication {
        id: publication.id,
        work_id: None,
        title: publication.title,
        authors: publication
            .authors
            .into_iter()
            .map(|author| OpdsAuthorCredit { name: author.name, sort_name: author.sort_name, identifiers: vec![author.id], provider_link: author.provider_url.map(|url| OpdsAuthorLink { url: url.to_string(), target: OpdsLinkTarget::Web }) })
            .collect(),
        languages: publication.language.into_iter().collect(),
        description: None,
        image_url: None,
        preferred_download: Some(acquisition.clone()),
        acquisitions: vec![OpdsAcquisition::Direct(acquisition)],
    }
}

fn provider_error(error: ::litteraturbanken::Error) -> OpdsError {
    match error {
        ::litteraturbanken::Error::InvalidUrl(error) => OpdsError::InvalidUrl(error),
        ::litteraturbanken::Error::Network(error) => OpdsError::Network(error),
        ::litteraturbanken::Error::TimedOut => OpdsError::TimedOut,
        ::litteraturbanken::Error::HttpStatus(status) => OpdsError::HttpStatus(status),
        ::litteraturbanken::Error::ResponseTooLarge { limit } => OpdsError::ResponseTooLarge { limit },
        ::litteraturbanken::Error::InvalidResponse(error) => OpdsError::InvalidCatalog(error),
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    #[ignore = "requires the live Litteraturbanken API"]
    async fn live_catalogue_search_and_download_are_available() {
        let client = crate::OpdsClient::new().unwrap();
        let root = client.fetch_catalog(::litteraturbanken::CATALOG_URL).await.unwrap();
        assert!(root.publications.len() > 1_000);
        let search = client.search(&root, "Doktor Glas").await.unwrap();
        let publication = search.publications.iter().find(|publication| publication.title == "Doktor Glas").unwrap();
        let bytes = client.download_book_bytes(publication.preferred_download.as_ref().unwrap()).await.unwrap();
        assert!(bytes.starts_with(b"PK"));
    }
}
