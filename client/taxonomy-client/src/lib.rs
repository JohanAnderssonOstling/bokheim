use library_model::{project_subject_codes, unified_has_exact_source_code, PublicationSubject, UnifiedConceptDefinition};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::Duration;
use taxonomy_contract::{TaxonomyBatchRequest, TaxonomyBatchResponse, TaxonomyCode, TaxonomyConcept, TaxonomySystem, MAX_CODES_PER_REQUEST, MAX_CODE_BYTES, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES};
use url::Url;

#[derive(Clone)]
pub struct TaxonomyClient {
    http: reqwest::Client,
    base_url: Url,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaxonomyClientError(String);

impl TaxonomyClientError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for TaxonomyClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for TaxonomyClientError {}

impl TaxonomyClient {
    pub fn new(base_url: &str) -> Result<Self, TaxonomyClientError> {
        let base_url = Url::parse(base_url).map_err(|error| TaxonomyClientError::new(error.to_string()))?;
        if !matches!(base_url.scheme(), "http" | "https") || !base_url.username().is_empty() || base_url.password().is_some() || base_url.query().is_some() || base_url.fragment().is_some() {
            return Err(TaxonomyClientError::new("taxonomy server URL must be an HTTP origin or base path without credentials, query, or fragment"));
        }
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .user_agent(format!("Bokheim/{} taxonomy-client", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| TaxonomyClientError::new(error.to_string()))?;
        Ok(Self { http, base_url })
    }

    /// Collects classifications not covered by the currently installed local
    /// taxonomy. Multiple books can be passed and duplicate codes are folded
    /// before any network request is made.
    pub fn unsupported_codes<'a>(&self, books: impl IntoIterator<Item = &'a [PublicationSubject]>) -> Vec<TaxonomyCode> {
        books
            .into_iter()
            .flat_map(project_subject_codes)
            .filter(|classification| !unified_has_exact_source_code(classification.system_id(), classification.code()))
            .filter_map(|classification| {
                let system = match classification.system_id() {
                    library_model::BISAC_SYSTEM_ID => TaxonomySystem::Bisac,
                    library_model::DDC_SYSTEM_ID => TaxonomySystem::Ddc,
                    library_model::LCC_SYSTEM_ID => TaxonomySystem::Lcc,
                    _ => return None,
                };
                Some(TaxonomyCode { system, code: classification.code().to_owned() })
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// Sends one bounded request for normal operations and deterministic
    /// consecutive requests when an operation exceeds the server batch limit.
    /// An empty local-miss set performs no request and returns `None`.
    pub async fn resolve_codes(&self, codes: impl IntoIterator<Item = TaxonomyCode>) -> Result<Option<TaxonomyBatchResponse>, TaxonomyClientError> {
        let requests = prepare_batches(codes)?;
        if requests.is_empty() {
            return Ok(None);
        }
        let mut combined: Option<TaxonomyBatchResponse> = None;
        let mut concept_positions = BTreeMap::<String, usize>::new();
        for request in requests {
            let response = self.resolve_batch(request).await?;
            combine_batch_response(&mut combined, &mut concept_positions, response)?;
        }
        Ok(combined)
    }

    async fn resolve_batch(&self, request: TaxonomyBatchRequest) -> Result<TaxonomyBatchResponse, TaxonomyClientError> {
        let expected = request.codes.iter().cloned().collect::<BTreeSet<_>>();
        let encoded = serde_json::to_vec(&request).map_err(|error| TaxonomyClientError::new(error.to_string()))?;
        if encoded.len() > MAX_REQUEST_BYTES {
            return Err(TaxonomyClientError::new("taxonomy batch request is too large"));
        }
        let mut response = self
            .http
            .post(self.endpoint("v1/resolve")?)
            .header(reqwest::header::ACCEPT, "application/json")
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(encoded)
            .send()
            .await
            .map_err(|error| TaxonomyClientError::new(error.to_string()))?;
        if !response.status().is_success() {
            let status = response.status();
            let detail = response.text().await.map_err(|error| TaxonomyClientError::new(error.to_string()))?;
            let detail = detail.chars().take(4_096).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ");
            return Err(TaxonomyClientError::new(if detail.is_empty() { format!("taxonomy server returned HTTP {status}") } else { format!("taxonomy server returned HTTP {status}: {detail}") }));
        }
        if response.content_length().is_some_and(|length| length > MAX_RESPONSE_BYTES as u64) {
            return Err(TaxonomyClientError::new("taxonomy response is too large"));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|error| TaxonomyClientError::new(error.to_string()))? {
            if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                return Err(TaxonomyClientError::new("taxonomy response is too large"));
            }
            bytes.extend_from_slice(&chunk);
        }
        let response: TaxonomyBatchResponse = serde_json::from_slice(&bytes).map_err(|error| TaxonomyClientError::new(error.to_string()))?;
        let actual = response.results.iter().map(|result| TaxonomyCode { system: result.system, code: result.code.clone() }).collect::<BTreeSet<_>>();
        if actual != expected || actual.len() != response.results.len() {
            return Err(TaxonomyClientError::new("taxonomy response did not contain exactly one result for every requested code"));
        }
        let concept_ids = response.concepts.iter().map(|concept| concept.concept_id.as_str()).collect::<BTreeSet<_>>();
        if concept_ids.len() != response.concepts.len() {
            return Err(TaxonomyClientError::new("taxonomy response contained duplicate concept records"));
        }
        if response.results.iter().flat_map(|result| &result.matched_concept_ids).any(|concept_id| !concept_ids.contains(concept_id.as_str())) {
            return Err(TaxonomyClientError::new("taxonomy response omitted a matched concept from its ancestor slice"));
        }
        Ok(response)
    }

    pub async fn resolve_unsupported<'a>(&self, books: impl IntoIterator<Item = &'a [PublicationSubject]>) -> Result<Option<TaxonomyBatchResponse>, TaxonomyClientError> {
        self.resolve_codes(self.unsupported_codes(books)).await
    }

    fn endpoint(&self, path: &str) -> Result<Url, TaxonomyClientError> {
        self.base_url.join(path).map_err(|error| TaxonomyClientError::new(error.to_string()))
    }
}

fn prepare_batches(codes: impl IntoIterator<Item = TaxonomyCode>) -> Result<Vec<TaxonomyBatchRequest>, TaxonomyClientError> {
    let codes = codes
        .into_iter()
        .map(|mut code| {
            code.code = code.code.trim().to_owned();
            code
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let empty_size = serde_json::to_vec(&TaxonomyBatchRequest { codes: Vec::new() }).map_err(|error| TaxonomyClientError::new(error.to_string()))?.len();
    let mut requests = Vec::new();
    let mut current = Vec::new();
    let mut current_size = empty_size;
    for code in codes {
        if code.code.trim().is_empty() || code.code.len() > MAX_CODE_BYTES {
            return Err(TaxonomyClientError::new(format!("taxonomy code must contain between 1 and {MAX_CODE_BYTES} bytes")));
        }
        let encoded_size = serde_json::to_vec(&code).map_err(|error| TaxonomyClientError::new(error.to_string()))?.len();
        let separator_size = usize::from(!current.is_empty());
        if !current.is_empty() && (current.len() == MAX_CODES_PER_REQUEST || current_size + separator_size + encoded_size > MAX_REQUEST_BYTES) {
            requests.push(TaxonomyBatchRequest { codes: std::mem::take(&mut current) });
            current_size = empty_size;
        }
        current_size += usize::from(!current.is_empty()) + encoded_size;
        current.push(code);
    }
    if !current.is_empty() {
        requests.push(TaxonomyBatchRequest { codes: current });
    }
    Ok(requests)
}

fn combine_batch_response(combined: &mut Option<TaxonomyBatchResponse>, concept_positions: &mut BTreeMap<String, usize>, response: TaxonomyBatchResponse) -> Result<(), TaxonomyClientError> {
    match combined.as_mut() {
        None => {
            concept_positions.extend(response.concepts.iter().enumerate().map(|(position, concept)| (concept.concept_id.clone(), position)));
            *combined = Some(response);
        }
        Some(combined) => {
            if combined.release_id != response.release_id {
                return Err(TaxonomyClientError::new("taxonomy release changed while resolving a multi-batch operation"));
            }
            combined.results.extend(response.results);
            for concept in response.concepts {
                if let Some(position) = concept_positions.get(&concept.concept_id).copied() {
                    if combined.concepts[position] != concept {
                        return Err(TaxonomyClientError::new(format!("taxonomy concept {} changed while resolving a multi-batch operation", concept.concept_id)));
                    }
                } else {
                    concept_positions.insert(concept.concept_id.clone(), combined.concepts.len());
                    combined.concepts.push(concept);
                }
            }
        }
    }
    Ok(())
}

/// Converts the server's parent-first slice into the library model used by the
/// local runtime and SQLite merger.
pub fn concept_definitions(concepts: &[TaxonomyConcept]) -> Vec<UnifiedConceptDefinition> {
    concepts
        .iter()
        .map(|concept| {
            let (parent_ids, parent_sort_orders) = concept.parents.iter().map(|parent| (parent.concept_id.clone(), parent.display_order)).unzip();
            UnifiedConceptDefinition::new_with_parent_sort_orders(concept.concept_id.clone(), concept.preferred_label.clone(), parent_ids, parent_sort_orders, concept.source_selectors.clone(), concept.source_labels.clone())
        })
        .collect()
}

pub fn code_resolutions(response: &TaxonomyBatchResponse) -> Vec<library_model::UnifiedCodeResolution> {
    response.results.iter().map(|result| library_model::UnifiedCodeResolution::new(result.system.as_str().to_owned(), result.code.clone(), result.matched_concept_ids.clone())).collect()
}

/// Produces a complete replacement runtime by overlaying a fetched slice on
/// the currently installed local definitions.
pub fn merged_runtime_definitions(concepts: &[TaxonomyConcept]) -> Vec<UnifiedConceptDefinition> {
    let mut merged = library_model::unified_concept_definitions().into_iter().map(|concept| (concept.concept_id().to_owned(), concept)).collect::<BTreeMap<_, _>>();
    for concept in concept_definitions(concepts) {
        merged.insert(concept.concept_id().to_owned(), concept);
    }
    merged.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_unique_local_misses_form_one_batch() {
        let accounting = TaxonomyCode { system: TaxonomySystem::Lcc, code: "HF5601".to_owned() };
        let requests = prepare_batches([accounting.clone(), accounting, TaxonomyCode { system: TaxonomySystem::Ddc, code: "515".to_owned() }]).unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].codes.len(), 2);
    }

    #[test]
    fn oversized_operations_are_split_without_losing_or_repeating_codes() {
        let codes = (0..=MAX_CODES_PER_REQUEST).map(|number| TaxonomyCode { system: TaxonomySystem::Lcc, code: format!("QA{number}") }).collect::<Vec<_>>();
        let requests = prepare_batches(codes.clone()).unwrap();
        assert_eq!(requests.iter().map(|request| request.codes.len()).collect::<Vec<_>>(), vec![MAX_CODES_PER_REQUEST, 1]);
        assert_eq!(requests.into_iter().flat_map(|request| request.codes).collect::<BTreeSet<_>>(), codes.into_iter().collect());
    }

    #[test]
    fn batches_also_respect_the_encoded_request_limit() {
        let codes = (0..400).map(|number| TaxonomyCode { system: TaxonomySystem::Lcc, code: format!("{number:04}{}", "X".repeat(MAX_CODE_BYTES - 4)) }).collect::<Vec<_>>();
        let requests = prepare_batches(codes).unwrap();
        assert!(requests.len() > 1);
        assert!(requests.iter().all(|request| request.codes.len() <= MAX_CODES_PER_REQUEST && serde_json::to_vec(request).unwrap().len() <= MAX_REQUEST_BYTES));
    }

    #[test]
    fn responses_from_different_releases_cannot_be_combined() {
        let response = |release: &str, code: &str| TaxonomyBatchResponse {
            release_id: release.to_owned(),
            results: vec![taxonomy_contract::TaxonomyCodeResolution { system: TaxonomySystem::Lcc, code: code.to_owned(), matched_concept_ids: Vec::new() }],
            concepts: Vec::new(),
        };
        let mut combined = None;
        let mut positions = BTreeMap::new();
        combine_batch_response(&mut combined, &mut positions, response("one", "QA1")).unwrap();
        assert!(combine_batch_response(&mut combined, &mut positions, response("two", "QA2")).unwrap_err().to_string().contains("release changed"));
    }

    #[tokio::test]
    async fn empty_miss_set_does_not_contact_the_server() {
        let client = TaxonomyClient::new("https://taxonomy.example.test/").unwrap();
        assert!(client.resolve_codes(Vec::new()).await.unwrap().is_none());
    }
}
