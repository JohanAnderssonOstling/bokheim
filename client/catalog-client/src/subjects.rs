use super::*;

impl CatalogClient {
    pub async fn refresh_subjects(&self, mut request: CatalogSubjectBrowseRequest) -> Result<CatalogSubjectBrowseResponse, CatalogProviderError> {
        if request.supported_languages.is_empty() {
            request.supported_languages = self.language_filter().supported().to_vec();
        }
        if self.catalog_sources().is_empty() {
            self.refresh_sources().await?;
        }
        // The server is authoritative for every filtered and sorted range.
        // Fetch its ordered IDs first, resolve the immutable payloads retained
        // locally, and ask for details only when an ID is missing.
        let index = self.server.subject_index(&request).await?;
        if index.range.returned_items as usize != index.entry_ids.len() {
            return Err(CatalogProviderError::new("catalogue returned an inconsistent subject result range"));
        }
        let known_entry_ids = self.store.known_entry_ids(&index.epoch, &index.entry_ids)?;
        let known = known_entry_ids.into_iter().collect::<HashSet<_>>();
        let missing_entry_ids = index.entry_ids.iter().copied().filter(|entry_id| !known.contains(entry_id)).collect::<Vec<_>>();
        let mut new_payloads = Vec::new();
        if !missing_entry_ids.is_empty() {
            let payloads = self.server.subject_publication_payloads(&CatalogSubjectPublicationPayloadRequest { epoch: index.epoch.clone(), entry_ids: missing_entry_ids }).await?;
            if payloads.epoch != index.epoch {
                return Err(CatalogProviderError::new("catalogue changed while publication details were loading"));
            }
            new_payloads = payloads.payloads;
        }
        let publications = self.store.resolve(&index.epoch, &index.entry_ids, &new_payloads)?.ok_or_else(|| CatalogProviderError::new("catalog server omitted a missing publication payload"))?;
        let response = CatalogSubjectBrowseResponse { epoch: index.epoch, navigation: index.navigation, publications, range: index.range, capabilities: index.capabilities };
        self.covers.remember_subject_browse(&response);
        Ok(response)
    }
}

impl CatalogServerClient {
    async fn subject_index(&self, request: &CatalogSubjectBrowseRequest) -> Result<CatalogSubjectBrowseIndexResponse, CatalogProviderError> {
        self.post_wire(self.endpoint("api/opds/subjects")?, request).await
    }

    async fn subject_publication_payloads(&self, request: &CatalogSubjectPublicationPayloadRequest) -> Result<CatalogSubjectPublicationPayloadResponse, CatalogProviderError> {
        self.post_wire(self.endpoint("api/opds/subject-publications")?, request).await
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
    use catalog_protocol::{CatalogSourceRegistration, CatalogSourceRegistry};
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

    #[tokio::test]
    async fn server_order_is_refetched_while_publication_payloads_are_reused() {
        let request_count = Arc::new(AtomicU64::new(0));
        let handler_request_count = request_count.clone();
        let subject_application = Router::new().route(
            "/api/opds/subjects",
            post(move |body: Bytes| {
                let request_count = handler_request_count.clone();
                async move {
                    request_count.fetch_add(1, Ordering::SeqCst);
                    let _request: CatalogSubjectBrowseRequest = wire::decode(&body, wire::MAX_DECODED_RESPONSE_BYTES).unwrap();
                    let entry_ids = vec![2, 1];
                    let index = CatalogSubjectBrowseIndexResponse {
                        epoch: "epoch".to_owned(),
                        navigation: Vec::new(),
                        entry_ids,
                        range: CatalogResultRangeResponse { offset: 0, returned_items: 2, total_items: 2 },
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
        let root_locator = format!("http://{address}/catalog");
        let source = CatalogSource {
            id: CatalogSourceId::parse("opds-test").unwrap(),
            title: "Test".to_owned(),
            description: "Test".to_owned(),
            languages: vec!["English".to_owned()],
            authentication: catalog_model::CatalogAuthentication::None,
            capabilities: vec![catalog_model::CatalogCapability::Browse],
        };
        let registry = CatalogSourceRegistry { revision: 1, sources: vec![CatalogSourceRegistration { source, root_locator }] };
        std::fs::write(temporary.path().join("catalog-sources.json"), serde_json::to_vec(&registry).unwrap()).unwrap();
        let client = CatalogClient::with_supported_languages(&format!("http://{address}"), temporary.path(), ["en"]).unwrap();

        let request = CatalogSubjectBrowseRequest { range: CatalogResultRange::new(0, 10), ..Default::default() };
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
