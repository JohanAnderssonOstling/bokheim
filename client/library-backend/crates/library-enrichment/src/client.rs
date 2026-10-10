use std::fmt;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;
use url::Url;

#[cfg(target_arch = "wasm32")]
async fn browser_delay(milliseconds: u32) {
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(catch, js_namespace = globalThis, js_name = setTimeout)]
        fn set_timeout(callback: &js_sys::Function, milliseconds: u32) -> Result<i32, JsValue>;
    }
    let delay = js_sys::Promise::new(&mut |resolve, reject| {
        if let Err(error) = set_timeout(&resolve, milliseconds) {
            let _ = reject.call1(&JsValue::UNDEFINED, &error);
        }
    });
    let _ = wasm_bindgen_futures::JsFuture::from(delay).await;
}

#[derive(Clone)]
pub struct MetadataClient {
    http: reqwest::Client,
    base_url: Url,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetadataClientError(String);

impl MetadataClientError {
    fn new(message: impl fmt::Display) -> Self {
        Self(message.to_string())
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
        let base_url = Self::parse_origin(base_url)?;
        let http = reqwest::Client::new();
        Ok(Self { http, base_url })
    }

    fn parse_origin(origin: &str) -> Result<Url, MetadataClientError> {
        let url = Url::parse(origin).map_err(MetadataClientError::new)?;
        if !matches!(url.scheme(), "http" | "https") || !url.username().is_empty() || url.password().is_some() || url.query().is_some() || url.fragment().is_some() {
            return Err(MetadataClientError::new("server URL must be an HTTP origin or base path without credentials, query, or fragment"));
        }
        Ok(url)
    }

    /// Fetch the artwork already resolved and cached by the metadata server.
    pub async fn resolved_cover(&self, id: &str) -> Result<Option<Vec<u8>>, MetadataClientError> {
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err(MetadataClientError::new("invalid cover reference"));
        }
        Self::with_deadline(35_000, async {
            let response = self.http.get(self.endpoint(&format!("v3/covers/{id}"))?).send().await.map_err(MetadataClientError::new)?;
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                return Ok(None);
            }
            Self::response_bytes(response.error_for_status().map_err(MetadataClientError::new)?, 4 * 1024 * 1024).await.map(Some)
        })
        .await
    }

    /// Every lookup and merge rule for a batch of books runs on the server.
    pub async fn books(&self, request: &metadata_contract::books::BookEnrichmentRequest) -> Result<metadata_contract::books::BookEnrichmentResponse, MetadataClientError> {
        let body = serde_json::to_vec(request).map_err(MetadataClientError::new)?;
        if body.len() > metadata_contract::books::MAX_BOOKS_REQUEST_BYTES {
            return Err(MetadataClientError::new("book request exceeds size limit"));
        }
        // The server resolves books concurrently, but one may wait on Audible
        // and the Library of Congress fallback.
        Self::with_deadline(120_000, async {
            let response =
                self.http.post(self.endpoint("v3/books")?).header(reqwest::header::CONTENT_TYPE, "application/json").body(body).send().await.map_err(MetadataClientError::new)?.error_for_status().map_err(MetadataClientError::new)?;
            let bytes = Self::response_bytes(response, metadata_contract::MAX_RESPONSE_BYTES).await?;
            serde_json::from_slice(&bytes).map_err(MetadataClientError::new)
        })
        .await
    }

    async fn with_deadline<T>(milliseconds: u32, operation: impl std::future::Future<Output = Result<T, MetadataClientError>>) -> Result<T, MetadataClientError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            tokio::time::timeout(Duration::from_millis(u64::from(milliseconds)), operation).await.map_err(|_| MetadataClientError::new("metadata request timed out"))?
        }
        #[cfg(target_arch = "wasm32")]
        {
            use futures_util::future::{select, Either};
            match select(Box::pin(operation), Box::pin(browser_delay(milliseconds))).await {
                Either::Left((result, _)) => result,
                Either::Right(_) => Err(MetadataClientError::new("metadata request timed out")),
            }
        }
    }

    async fn response_bytes(response: reqwest::Response, max_bytes: usize) -> Result<Vec<u8>, MetadataClientError> {
        if response.content_length().is_some_and(|length| length > max_bytes as u64) {
            return Err(MetadataClientError::new("metadata response is too large"));
        }
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(MetadataClientError::new)?;
            if bytes.len().saturating_add(chunk.len()) > max_bytes {
                return Err(MetadataClientError::new("metadata response is too large"));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }

    fn endpoint(&self, path: &str) -> Result<Url, MetadataClientError> {
        self.base_url.join(path).map_err(MetadataClientError::new)
    }
}

use futures_util::StreamExt;
