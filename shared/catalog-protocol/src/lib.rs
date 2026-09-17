//! Wire DTOs for page and search exchange between the catalog client and
//! cache server.

use catalog_model::{CatalogFacet, CatalogFreshness, CatalogGroup, CatalogLink, CatalogPage, CatalogPagination, CatalogBook, CatalogBookIdentity, CatalogSource, CatalogSourceId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogSourceRegistration {
    pub source: CatalogSource,
    pub root_locator: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogSourceRegistry {
    pub revision: u64,
    pub sources: Vec<CatalogSourceRegistration>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogBookReference {
    pub identity: CatalogBookIdentity,
    pub sequence: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogBookPayload {
    pub book: CatalogBook,
    pub sequence: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogPageRequest {
    pub locator: Option<String>,
    pub supported_languages: Vec<String>,
    pub epoch: Option<String>,
    pub known_books: Vec<CatalogBookReference>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogGroupLayout {
    pub title: String,
    pub page_locator: Option<String>,
    pub number_of_items: Option<u32>,
    pub navigation: Vec<CatalogLink>,
    pub books: Vec<CatalogBookReference>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogPageLayout {
    pub source_id: CatalogSourceId,
    pub title: String,
    pub locator: String,
    pub navigation: Vec<CatalogLink>,
    pub books: Vec<CatalogBookReference>,
    pub groups: Vec<CatalogGroupLayout>,
    pub facets: Vec<CatalogFacet>,
    pub pagination: CatalogPagination,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogPageResponse {
    pub epoch: String,
    pub page: CatalogPageLayout,
    pub payloads: Vec<CatalogBookPayload>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogSearchRequest {
    pub query: String,
    pub supported_languages: Vec<String>,
    pub epoch: Option<String>,
    pub known_books: Vec<CatalogBookReference>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogSearchResponse {
    pub epoch: String,
    pub search: CatalogSearchLayout,
    pub payloads: Vec<CatalogBookPayload>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogSearchLayout {
    pub source_id: CatalogSourceId,
    pub query: String,
    pub title_matches: Vec<CatalogBookReference>,
    pub author_matches: Vec<CatalogBookReference>,
    pub toc_matches: Vec<CatalogBookReference>,
}

pub fn page_layout(page: &CatalogPage, mut reference: impl FnMut(&CatalogBook) -> CatalogBookReference) -> CatalogPageLayout {
    CatalogPageLayout {
        source_id: page.source_id.clone(),
        title: page.title.clone(),
        locator: page.locator.clone(),
        navigation: page.navigation.clone(),
        books: page.books.iter().map(&mut reference).collect(),
        groups: page
            .groups
            .iter()
            .map(|group| CatalogGroupLayout {
                title: group.title.clone(),
                page_locator: group.page_locator.clone(),
                number_of_items: group.number_of_items,
                navigation: group.navigation.clone(),
                books: group.books.iter().map(&mut reference).collect(),
            })
            .collect(),
        facets: page.facets.clone(),
        pagination: page.pagination.clone(),
    }
}

pub fn materialize_page(layout: CatalogPageLayout, mut book: impl FnMut(&CatalogBookReference) -> Option<CatalogBook>) -> Option<CatalogPage> {
    let books = layout.books.iter().map(&mut book).collect::<Option<Vec<_>>>()?;
    let groups = layout
        .groups
        .into_iter()
        .map(|group| {
            Some(CatalogGroup {
                title: group.title,
                page_locator: group.page_locator,
                number_of_items: group.number_of_items,
                navigation: group.navigation,
                books: group.books.iter().map(&mut book).collect::<Option<Vec<_>>>()?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(CatalogPage {
        source_id: layout.source_id,
        freshness: CatalogFreshness::Current,
        title: layout.title,
        locator: layout.locator,
        navigation: layout.navigation,
        books,
        groups,
        facets: layout.facets,
        pagination: layout.pagination,
    })
}
