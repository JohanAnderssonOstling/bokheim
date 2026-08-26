use crate::{
    CatalogFacet, CatalogGroup, CatalogLink, CatalogPage, CatalogPagination, CatalogPublication, CatalogPublicationIdentity, CatalogSearch, CatalogSource, CatalogSourceId, CatalogSourcedPublication, CatalogSubjectBrowseDeltaResponse,
    CatalogSubjectBrowseResponse,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
pub struct CatalogPublicationReference {
    pub identity: CatalogPublicationIdentity,
    pub sequence: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogPublicationPayload {
    pub publication: CatalogPublication,
    pub sequence: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogPageRequest {
    pub locator: Option<String>,
    pub supported_languages: Vec<String>,
    pub epoch: Option<String>,
    pub known_publications: Vec<CatalogPublicationReference>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogGroupLayout {
    pub title: String,
    pub page_locator: Option<String>,
    pub number_of_items: Option<u32>,
    pub navigation: Vec<CatalogLink>,
    pub publications: Vec<CatalogPublicationReference>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogPageLayout {
    pub source_id: CatalogSourceId,
    pub title: String,
    pub locator: String,
    pub navigation: Vec<CatalogLink>,
    pub publications: Vec<CatalogPublicationReference>,
    pub groups: Vec<CatalogGroupLayout>,
    pub facets: Vec<CatalogFacet>,
    pub pagination: CatalogPagination,
    pub search: Option<CatalogSearch>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogPageResponse {
    pub epoch: String,
    pub page: CatalogPageLayout,
    pub payloads: Vec<CatalogPublicationPayload>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogSearchRequest {
    pub query: String,
    pub supported_languages: Vec<String>,
    pub epoch: Option<String>,
    pub known_publications: Vec<CatalogPublicationReference>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogSearchResponse {
    pub source_id: CatalogSourceId,
    pub epoch: String,
    pub query: String,
    pub title_matches: Vec<CatalogPublicationReference>,
    pub author_matches: Vec<CatalogPublicationReference>,
    pub toc_matches: Vec<CatalogPublicationReference>,
    pub payloads: Vec<CatalogPublicationPayload>,
}

pub fn page_layout(page: &CatalogPage, mut reference: impl FnMut(&CatalogPublication) -> CatalogPublicationReference) -> CatalogPageLayout {
    CatalogPageLayout {
        source_id: page.source_id.clone(),
        title: page.title.clone(),
        locator: page.locator.clone(),
        navigation: page.navigation.clone(),
        publications: page.publications.iter().map(&mut reference).collect(),
        groups: page
            .groups
            .iter()
            .map(|group| CatalogGroupLayout {
                title: group.title.clone(),
                page_locator: group.page_locator.clone(),
                number_of_items: group.number_of_items,
                navigation: group.navigation.clone(),
                publications: group.publications.iter().map(&mut reference).collect(),
            })
            .collect(),
        facets: page.facets.clone(),
        pagination: page.pagination.clone(),
        search: page.search.clone(),
    }
}

pub fn materialize_page(layout: CatalogPageLayout, mut publication: impl FnMut(&CatalogPublicationReference) -> Option<CatalogPublication>) -> Option<CatalogPage> {
    let publications = layout.publications.iter().map(&mut publication).collect::<Option<Vec<_>>>()?;
    let groups = layout
        .groups
        .into_iter()
        .map(|group| {
            Some(CatalogGroup {
                title: group.title,
                page_locator: group.page_locator,
                number_of_items: group.number_of_items,
                navigation: group.navigation,
                publications: group.publications.iter().map(&mut publication).collect::<Option<Vec<_>>>()?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(CatalogPage {
        source_id: layout.source_id,
        freshness: crate::CatalogFreshness::Current,
        title: layout.title,
        locator: layout.locator,
        navigation: layout.navigation,
        publications,
        groups,
        facets: layout.facets,
        pagination: layout.pagination,
        search: layout.search,
    })
}

/// Resolves a delta response into the ordered subject-browse response consumed
/// by frontends. All transports share the same consistency checks.
pub fn materialize_subject_browse_delta(delta: CatalogSubjectBrowseDeltaResponse) -> Result<CatalogSubjectBrowseResponse, String> {
    if delta.range.returned_items as usize != delta.entry_ids.len() {
        return Err("catalogue returned an inconsistent subject result range".to_owned());
    }
    let mut payloads = HashMap::<u64, CatalogSourcedPublication>::new();
    for payload in delta.payloads {
        if payloads.insert(payload.entry_id, payload.publication).is_some() {
            return Err(format!("catalogue returned duplicate subject publication {}", payload.entry_id));
        }
    }
    let publications = delta.entry_ids.iter().map(|entry_id| payloads.remove(entry_id).ok_or_else(|| format!("catalogue omitted subject publication {entry_id}"))).collect::<Result<Vec<_>, _>>()?;
    Ok(CatalogSubjectBrowseResponse { epoch: delta.epoch, navigation: delta.navigation, publications, range: delta.range, capabilities: delta.capabilities })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CatalogResultRangeResponse, CatalogSubjectBrowseCapabilities};

    #[test]
    fn subject_delta_range_must_match_its_ordered_ids() {
        let delta = CatalogSubjectBrowseDeltaResponse {
            epoch: "epoch".to_owned(),
            revision: 1,
            navigation: Vec::new(),
            entry_ids: vec![1],
            exact_entry_ids: Vec::new(),
            payloads: Vec::new(),
            range: CatalogResultRangeResponse { offset: 0, returned_items: 0, total_items: 1 },
            capabilities: CatalogSubjectBrowseCapabilities::default(),
        };

        assert!(materialize_subject_browse_delta(delta).unwrap_err().contains("inconsistent"));
    }
}
