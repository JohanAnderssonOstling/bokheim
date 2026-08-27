use metadata_contract::{EditionIdentityRequest, EditionIdentityResponse, MetadataEnrichmentRequest, RichMetadataEnrichmentResponse};
use std::fmt;
use std::time::Duration;
use url::Url;

#[derive(Clone)]
pub struct MetadataClient {
    http: reqwest::Client,
    base_url: Url,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetadataClientError(String);

impl MetadataClientError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for MetadataClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for MetadataClientError {}

impl MetadataClient {
    pub fn new(base_url: &str) -> Result<Self, MetadataClientError> {
        let base_url = Url::parse(base_url).map_err(|error| MetadataClientError::new(error.to_string()))?;
        if !matches!(base_url.scheme(), "http" | "https") || !base_url.username().is_empty() || base_url.password().is_some() || base_url.query().is_some() || base_url.fragment().is_some() {
            return Err(MetadataClientError::new("metadata server URL must be an HTTP origin or base path without credentials, query, or fragment"));
        }
        let http = reqwest::Client::new();
        Ok(Self { http, base_url })
    }

    pub async fn enrichment(&self, request: &MetadataEnrichmentRequest) -> Result<RichMetadataEnrichmentResponse, MetadataClientError> {
        self.rich_enrichment(request).await
    }

    pub async fn rich_enrichment(&self, request: &MetadataEnrichmentRequest) -> Result<RichMetadataEnrichmentResponse, MetadataClientError> {
        let body = metadata_contract::encode_v2_request(request).map_err(|error| MetadataClientError::new(error.to_string()))?;
        let bytes = self.post("v2/enrichment", metadata_contract::MEDIA_TYPE_V2, body).await?;
        metadata_contract::decode_v2_response(&bytes, metadata_contract::MAX_RESPONSE_BYTES).map_err(|error| MetadataClientError::new(error.to_string()))
    }

    pub async fn resolve_edition_identities(&self, request: &EditionIdentityRequest) -> Result<EditionIdentityResponse, MetadataClientError> {
        let body = metadata_contract::encode_v2_edition_identity_request(request).map_err(|error| MetadataClientError::new(error.to_string()))?;
        let bytes = self.post("v2/edition-identities", metadata_contract::MEDIA_TYPE_V2, body).await?;
        metadata_contract::decode_v2_edition_identity_response(&bytes, metadata_contract::MAX_RESPONSE_BYTES).map_err(|error| MetadataClientError::new(error.to_string()))
    }

    async fn post(&self, path: &str, media_type: &str, body: Vec<u8>) -> Result<Vec<u8>, MetadataClientError> {
        let response = tokio::time::timeout(Duration::from_secs(30), self.http.post(self.endpoint(path)?).header(reqwest::header::ACCEPT, media_type).header(reqwest::header::CONTENT_TYPE, media_type).body(body).send())
            .await
            .map_err(|_| MetadataClientError::new("metadata request timed out"))?
            .map_err(|error| MetadataClientError::new(error.to_string()))?;
        if !response.status().is_success() {
            let status = response.status();
            let mut detail = Vec::new();
            const MAX_ERROR_DETAIL_BYTES: usize = 4 * 1024;
            let mut stream = response.bytes_stream();
            while detail.len() < MAX_ERROR_DETAIL_BYTES {
                let Some(chunk) = stream.next().await else { break };
                let chunk = chunk.map_err(|error| MetadataClientError::new(error.to_string()))?;
                let remaining = MAX_ERROR_DETAIL_BYTES - detail.len();
                detail.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
            }
            let detail = String::from_utf8_lossy(&detail).split_whitespace().collect::<Vec<_>>().join(" ");
            let message = if detail.is_empty() { format!("metadata server returned HTTP {status}") } else { format!("metadata server returned HTTP {status}: {detail}") };
            return Err(MetadataClientError::new(message));
        }
        if response.content_length().is_some_and(|length| length > metadata_contract::MAX_RESPONSE_BYTES as u64) {
            return Err(MetadataClientError::new("metadata response is too large"));
        }
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| MetadataClientError::new(error.to_string()))?;
            if bytes.len().saturating_add(chunk.len()) > metadata_contract::MAX_RESPONSE_BYTES {
                return Err(MetadataClientError::new("metadata response is too large"));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }

    fn endpoint(&self, path: &str) -> Result<Url, MetadataClientError> {
        self.base_url.join(path).map_err(|error| MetadataClientError::new(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_are_resolved_against_the_metadata_origin() {
        let client = MetadataClient::new("https://metadata.example.test/base/").unwrap();
        assert_eq!(client.endpoint("v2/enrichment").unwrap().as_str(), "https://metadata.example.test/base/v2/enrichment");
    }

    #[test]
    fn rejects_non_http_origins_and_embedded_credentials() {
        assert!(MetadataClient::new("file:///tmp/metadata.sqlite").is_err());
        assert!(MetadataClient::new("https://user@example.test/").is_err());
    }
}
use futures_util::StreamExt;
