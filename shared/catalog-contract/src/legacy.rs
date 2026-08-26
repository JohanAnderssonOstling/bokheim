//! Compatibility helpers for the deployed human-readable subject endpoint.
//!
//! Keeping this translation beside the catalog contract prevents transports
//! from growing their own partially compatible v14 models.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{
    CatalogBookFormat, CatalogBookFormatFacetValue, CatalogLanguageFacetValue, CatalogLink, CatalogPagination, CatalogPublication, CatalogPublicationAction, CatalogResultRangeResponse, CatalogSourcedPublication,
    CatalogSubjectBrowseCapabilities, CatalogSubjectBrowseRequest, CatalogSubjectBrowseResponse, CatalogSubjectSort,
};

#[derive(Serialize)]
struct LegacySubjectRequest<'a> {
    locator: &'a Option<String>,
    supported_languages: &'a [String],
    source_ids: &'a [crate::CatalogSourceId],
    query: &'a str,
    limit: u32,
}

#[derive(Deserialize)]
struct LegacySubjectResponse {
    navigation: Vec<CatalogLink>,
    publications: Vec<CatalogSourcedPublication>,
    pagination: CatalogPagination,
}

pub fn encode_subject_browse_request(request: &CatalogSubjectBrowseRequest) -> Result<String, serde_json::Error> {
    let limit = request.range.offset.saturating_add(request.range.limit).clamp(1, 100);
    serde_json::to_string(&LegacySubjectRequest { locator: &request.locator, supported_languages: &request.supported_languages, source_ids: &request.source_ids, query: &request.query, limit })
}

pub fn decode_subject_browse_response(json: &str, request: &CatalogSubjectBrowseRequest, supported_formats: &[CatalogBookFormat]) -> Result<CatalogSubjectBrowseResponse, String> {
    let mut value = serde_json::from_str::<serde_json::Value>(json).map_err(|error| format!("invalid legacy catalogue response: {error}"))?;
    let publications = value.get_mut("publications").and_then(serde_json::Value::as_array_mut).ok_or_else(|| "invalid legacy catalogue response: publications was not an array".to_owned())?;
    for item in publications {
        // v14 identifiers used enum variants removed from the current metadata
        // contract. Subject cards do not consume metadata, so discard only
        // that incompatible optional field before typed deserialization.
        item.get_mut("publication").and_then(serde_json::Value::as_object_mut).ok_or_else(|| "invalid legacy catalogue response: publication was not an object".to_owned())?.insert("metadata".to_owned(), serde_json::json!({}));
    }
    let mut legacy = serde_json::from_value::<LegacySubjectResponse>(value).map_err(|error| format!("invalid legacy catalogue response: {error}"))?;
    legacy.publications.retain_mut(|item| {
        item.publication.actions.retain_mut(|action| match action {
            CatalogPublicationAction::Import { request } => supported_formats.contains(&request.book_format),
            CatalogPublicationAction::External { available_formats, .. } => {
                available_formats.retain(|format| supported_formats.contains(format));
                true
            }
        });
        item.publication.actions.iter().any(|action| matches!(action, CatalogPublicationAction::Import { .. }))
    });
    let capabilities = subject_capabilities(&legacy.publications);
    if !request.book_formats.is_empty() {
        legacy.publications.retain(|item| publication_formats(&item.publication).iter().any(|format| request.book_formats.contains(format)));
    }
    if request.sort == CatalogSubjectSort::Title {
        legacy.publications.sort_by_key(|item| item.publication.title.to_lowercase());
    }
    let offset = usize::try_from(request.range.offset).unwrap_or(usize::MAX).min(legacy.publications.len());
    let limit = usize::try_from(request.range.limit).unwrap_or(usize::MAX).clamp(1, 100);
    let total_items = legacy.pagination.number_of_items.unwrap_or_else(|| u32::try_from(legacy.publications.len()).unwrap_or(u32::MAX));
    let publications = legacy.publications.into_iter().skip(offset).take(limit).collect::<Vec<_>>();
    let range = CatalogResultRangeResponse { offset: request.range.offset, returned_items: u32::try_from(publications.len()).unwrap_or(u32::MAX), total_items };
    Ok(CatalogSubjectBrowseResponse { epoch: "legacy-json-v14".to_owned(), navigation: legacy.navigation, publications, range, capabilities })
}

fn publication_formats(publication: &CatalogPublication) -> Vec<CatalogBookFormat> {
    let mut formats = publication
        .actions
        .iter()
        .filter_map(|action| match action {
            CatalogPublicationAction::Import { request } => Some(request.book_format),
            CatalogPublicationAction::External { .. } => None,
        })
        .collect::<Vec<_>>();
    formats.sort_by_key(|format| *format as u8);
    formats.dedup();
    formats
}

fn subject_capabilities(publications: &[CatalogSourcedPublication]) -> CatalogSubjectBrowseCapabilities {
    let mut format_counts = HashMap::<CatalogBookFormat, u32>::new();
    let mut language_counts = HashMap::<String, u32>::new();
    for item in publications {
        for format in publication_formats(&item.publication) {
            *format_counts.entry(format).or_default() += 1;
        }
        for language in &item.publication.languages {
            let language = language.as_str().split('-').next().unwrap_or(language.as_str()).to_lowercase();
            *language_counts.entry(language).or_default() += 1;
        }
    }
    let mut book_formats = format_counts.into_iter().map(|(value, number_of_items)| CatalogBookFormatFacetValue { value, number_of_items }).collect::<Vec<_>>();
    book_formats.sort_by_key(|facet| facet.value as u8);
    let mut languages = language_counts.into_iter().map(|(value, number_of_items)| CatalogLanguageFacetValue { value, number_of_items }).collect::<Vec<_>>();
    languages.sort_by(|left, right| left.value.cmp(&right.value));
    CatalogSubjectBrowseCapabilities { sorts: CatalogSubjectSort::ALL.to_vec(), book_formats, languages }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CatalogResultRange;

    fn publication(title: &str, format: &str) -> serde_json::Value {
        serde_json::json!({
            "source_id": "opds-test",
            "publication": {
                "identity": {"edition_id": format!("edition-{format}"), "work_id": null},
                "title": title,
                "authors": [],
                "languages": ["en-US"],
                "description": null,
                "image_url": null,
                "metadata": {
                    "identifiers": [{
                        "value": "legacy-id",
                        "scheme": null,
                        "identifier_types": [],
                        "source": "provider",
                        "scope": "catalog_entry"
                    }]
                },
                "actions": [{
                    "kind": "import",
                    "request": {
                        "source_id": "opds-test",
                        "catalog_locator": "https://catalog.example/root",
                        "publication": {"edition_id": format!("edition-{format}"), "work_id": null},
                        "download_url": format!("https://catalog.example/book.{format}"),
                        "book_format": format
                    }
                }]
            }
        })
    }

    #[test]
    fn legacy_subjects_are_normalized_and_limited_to_platform_formats() {
        let json = serde_json::json!({
            "navigation": [],
            "publications": [publication("EPUB book", "epub"), publication("PDF book", "pdf")],
            "pagination": {
                "first": null,
                "previous": null,
                "next": null,
                "last": null,
                "number_of_items": 2,
                "items_per_page": 100,
                "current_page": 1
            }
        })
        .to_string();
        let request = CatalogSubjectBrowseRequest { book_formats: vec![CatalogBookFormat::Epub], ..Default::default() };

        let response = decode_subject_browse_response(&json, &request, &[CatalogBookFormat::Epub, CatalogBookFormat::Mobi]).unwrap();

        assert_eq!(response.publications.len(), 1);
        assert_eq!(response.publications[0].publication.title, "EPUB book");
        assert_eq!(response.capabilities.book_formats.len(), 1);
        assert_eq!(response.capabilities.book_formats[0].value, CatalogBookFormat::Epub);
    }

    #[test]
    fn legacy_request_limit_is_bounded() {
        let request = CatalogSubjectBrowseRequest { range: CatalogResultRange::new(90, 40), ..Default::default() };
        let value: serde_json::Value = serde_json::from_str(&encode_subject_browse_request(&request).unwrap()).unwrap();

        assert_eq!(value["limit"], 100);
    }
}
