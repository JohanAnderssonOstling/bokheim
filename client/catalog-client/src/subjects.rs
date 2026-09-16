use super::*;

impl CatalogClient {
    pub async fn refresh_subjects(&self, request: CatalogSubjectBrowseRequest) -> Result<CatalogSubjectBrowseResponse, CatalogClientError> {
        // The server is authoritative for every filtered and sorted range.
        // Fetch its ordered IDs first, resolve the immutable payloads retained
        // locally, and ask for details only when an ID is missing.
        let index: CatalogSubjectBrowseIndexResponse = self.post_wire("api/opds/subjects", &request).await?;
        if index.range.returned_items as usize != index.entry_ids.len() {
            return Err(CatalogClientError::new("catalogue returned an inconsistent subject result range"));
        }
        let missing_entry_ids = self.store.missing_entry_ids(&index.epoch, &index.entry_ids)?;
        let new_payloads = if missing_entry_ids.is_empty() {
            Vec::new()
        } else {
            let payloads: CatalogSubjectPublicationPayloadResponse = self.post_wire("api/opds/subject-publications", &CatalogSubjectPublicationPayloadRequest { epoch: index.epoch.clone(), entry_ids: missing_entry_ids }).await?;
            if payloads.epoch != index.epoch {
                return Err(CatalogClientError::new("catalogue changed while publication details were loading"));
            }
            payloads.payloads
        };
        let publications = self.store.resolve(&index.epoch, &index.entry_ids, &new_payloads)?.ok_or_else(|| CatalogClientError::new("catalog server omitted a missing publication payload"))?;
        let response = CatalogSubjectBrowseResponse { epoch: index.epoch, navigation: index.navigation, publications, range: index.range, capabilities: index.capabilities };
        self.covers.remember_subject_browse(&response);
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, Bytes};
    use axum::http::{header, Response};
    use axum::routing::post;
    use axum::Router;
    use catalog_model::{CatalogResultRangeResponse, CatalogSourcedPublication, CatalogSubjectBrowseCapabilities, CatalogSubjectBrowseEntryPayload, CatalogSubjectSort, LanguageTag};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    fn subject_payload(entry_id: u64, title: impl Into<String>) -> CatalogSubjectBrowseEntryPayload {
        CatalogSubjectBrowseEntryPayload {
            entry_id,
            publication: CatalogSourcedPublication {
                source_id: CatalogSourceId::parse("opds-test").unwrap(),
                publication: CatalogPublication {
                    identity: catalog_model::CatalogPublicationIdentity { edition_id: format!("edition-{entry_id}"), work_id: None },
                    title: title.into(),
                    authors: Vec::new(),
                    languages: vec![LanguageTag::parse("en").unwrap()],
                    description: None,
                    image_url: None,
                    metadata: Default::default(),
                    actions: Vec::new(),
                },
            },
        }
    }

    #[test]
    fn incomplete_payloads_remain_cached_for_the_next_attempt() {
        let temporary = tempfile::tempdir().unwrap();
        let client = CatalogClient::with_persistent_cache("http://localhost", temporary.path()).unwrap();
        assert!(client.store.resolve("epoch", &[2, 1], &[subject_payload(1, "First")]).unwrap().is_none());
        assert_eq!(client.store.missing_entry_ids("epoch", &[2, 1]).unwrap(), vec![2]);
        let publications = client.store.resolve("epoch", &[2, 1], &[subject_payload(2, "Second")]).unwrap().unwrap();
        assert_eq!(publications.iter().map(|item| item.publication.title.as_str()).collect::<Vec<_>>(), ["Second", "First"]);
    }

    #[tokio::test]
    async fn fresh_client_browses_without_registry_and_reuses_publication_payloads() {
        let request_count = Arc::new(AtomicU64::new(0));
        let handler_request_count = request_count.clone();
        let subject_application = Router::new().route(
            "/api/opds/subjects",
            post(move |body: Bytes| {
                let request_count = handler_request_count.clone();
                async move {
                    request_count.fetch_add(1, Ordering::SeqCst);
                    let request: CatalogSubjectBrowseRequest = wire::decode(&body, wire::MAX_DECODED_RESPONSE_BYTES).unwrap();
                    assert_eq!(request.supported_languages, ["en"]);
                    let entry_ids = vec![2, 1];
                    let index = CatalogSubjectBrowseIndexResponse {
                        epoch: "epoch".to_owned(),
                        navigation: Vec::new(),
                        entry_ids,
                        range: CatalogResultRangeResponse { offset: 0, returned_items: 2, has_more: false },
                        capabilities: CatalogSubjectBrowseCapabilities { sorts: vec![CatalogSubjectSort::Popularity, CatalogSubjectSort::Title], ..Default::default() },
                    };
                    Response::builder().header(header::CONTENT_TYPE, wire::MEDIA_TYPE).body(Body::from(wire::encode(&index).unwrap())).unwrap()
                }
            }),
        );
        let payload_request_count = request_count.clone();
        let application = subject_application.route(
            "/api/opds/subject-publications",
            post(move |body: Bytes| {
                let request_count = payload_request_count.clone();
                async move {
                    request_count.fetch_add(1, Ordering::SeqCst);
                    let request: CatalogSubjectPublicationPayloadRequest = wire::decode(&body, wire::MAX_DECODED_RESPONSE_BYTES).unwrap();
                    let response = CatalogSubjectPublicationPayloadResponse { epoch: request.epoch, payloads: request.entry_ids.into_iter().map(|entry_id| subject_payload(entry_id, format!("Book {entry_id}"))).collect() };
                    Response::builder().header(header::CONTENT_TYPE, wire::MEDIA_TYPE).body(Body::from(wire::encode(&response).unwrap())).unwrap()
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, application).await.unwrap() });
        let temporary = tempfile::tempdir().unwrap();
        let client = CatalogClient::with_persistent_cache(&format!("http://{address}"), temporary.path()).unwrap();
        // No registry endpoint is installed: browsing must work on a fresh client.

        let request = CatalogSubjectBrowseRequest { supported_languages: vec!["en".to_owned()], range: CatalogResultRange::new(0, 10), ..Default::default() };
        let first = client.refresh_subjects(request.clone()).await.unwrap();
        let second = client.refresh_subjects(request).await.unwrap();

        assert_eq!(first.publications.iter().map(|item| item.publication.title.as_str()).collect::<Vec<_>>(), ["Book 2", "Book 1"]);
        assert_eq!(second.publications.iter().map(|item| item.publication.title.as_str()).collect::<Vec<_>>(), ["Book 2", "Book 1"]);
        assert_eq!(first.capabilities.sorts, [CatalogSubjectSort::Popularity, CatalogSubjectSort::Title]);
        assert_eq!(second.capabilities.sorts, [CatalogSubjectSort::Popularity, CatalogSubjectSort::Title]);
        assert_eq!(request_count.load(Ordering::SeqCst), 3, "the second navigation needs only the fresh ordered-ID request");
        server.abort();
    }
}
