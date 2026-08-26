use super::*;
use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;

impl CatalogClient {
    pub async fn cover(&self, source_id: &CatalogSourceId, image_url: &str) -> Result<Vec<u8>, CatalogProviderError> {
        self.covers.cover_with(source_id, image_url, self.server.catalog_cover(source_id, image_url)).await
    }

    pub async fn download_import(&self, request: &CatalogImportRequest, temporary_directory: &Path) -> Result<CatalogDownload, CatalogProviderError> {
        let response = self.server.catalog_acquisition(request).await?;
        if !response.status().is_success() {
            return Err(CatalogProviderError::new(format!("catalog server returned HTTP {}", response.status())));
        }
        if response.content_length().is_some_and(|length| length > MAX_CATALOG_ACQUISITION_BYTES) {
            return Err(CatalogProviderError::new("catalog server acquisition is too large"));
        }
        let expected_media_type = request.book_format.media_type();
        let media_type = response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).and_then(|value| value.split(';').next());
        if media_type != Some(expected_media_type) {
            return Err(CatalogProviderError::new("catalog server acquisition has an unexpected media type"));
        }

        tokio::fs::create_dir_all(temporary_directory).await.map_err(|error| CatalogProviderError::new(error.to_string()))?;
        let temporary = tempfile::NamedTempFile::new_in(temporary_directory).map_err(|error| CatalogProviderError::new(error.to_string()))?;
        let (file, path) = temporary.into_parts();
        let mut file = tokio::fs::File::from_std(file);
        let mut stream = response.bytes_stream();
        let mut written = 0_u64;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| CatalogProviderError::new(error.to_string()))?;
            written = written.checked_add(chunk.len() as u64).ok_or_else(|| CatalogProviderError::new("catalog acquisition size overflow"))?;
            if written > MAX_CATALOG_ACQUISITION_BYTES {
                return Err(CatalogProviderError::new("catalog server acquisition is too large"));
            }
            file.write_all(&chunk).await.map_err(|error| CatalogProviderError::new(error.to_string()))?;
        }
        file.flush().await.map_err(|error| CatalogProviderError::new(error.to_string()))?;
        if written == 0 {
            return Err(CatalogProviderError::new("catalog server acquisition is empty"));
        }
        Ok(CatalogDownload { path, file_name: request.suggested_file_name() })
    }
}

impl CatalogServerClient {
    async fn catalog_acquisition(&self, request: &CatalogImportRequest) -> Result<reqwest::Response, CatalogProviderError> {
        let url = self.endpoint(&format!("api/opds/sources/{}/acquire", request.source_id.as_str()))?;
        let bytes = wire::encode(request).map_err(|error| CatalogProviderError::new(error.to_string()))?;
        self.http
            .post(url)
            .timeout(CATALOG_ACQUISITION_TIMEOUT)
            .header(reqwest::header::CONTENT_TYPE, wire::MEDIA_TYPE)
            .header(reqwest::header::ACCEPT, request.book_format.media_type())
            .body(bytes)
            .send()
            .await
            .map_err(|error| CatalogProviderError::new(error.to_string()))
    }

    pub(crate) async fn catalog_cover(&self, source_id: &CatalogSourceId, image_url: &str) -> Result<Vec<u8>, CatalogProviderError> {
        let url = if catalog_core::catalog_cover_asset_digest(image_url).is_some() {
            self.endpoint(image_url.trim_start_matches('/'))?
        } else {
            let mut url = self.endpoint(&format!("api/opds/sources/{}/cover", source_id.as_str()))?;
            url.query_pairs_mut().append_pair("image_url", image_url);
            url
        };
        let response = self.http.get(url).send().await.map_err(|error| CatalogProviderError::new(error.to_string()))?;
        if !response.status().is_success() {
            return Err(CatalogProviderError::new(format!("catalog server returned HTTP {}", response.status())));
        }
        if response.content_length().is_some_and(|length| length > MAX_CATALOG_COVER_SOURCE_BYTES) {
            return Err(CatalogProviderError::new("catalog server cover is too large"));
        }
        let media_type = response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).and_then(|value| value.split(';').next());
        if media_type != Some("image/jpeg") {
            return Err(CatalogProviderError::new("catalog server cover is not a JPEG"));
        }
        let bytes = response.bytes().await.map_err(|error| CatalogProviderError::new(error.to_string()))?;
        if bytes.is_empty() || bytes.len() as u64 > MAX_CATALOG_COVER_SOURCE_BYTES {
            return Err(CatalogProviderError::new("catalog server cover is empty or too large"));
        }
        tokio::task::spawn_blocking(move || validate_server_cover(bytes.to_vec())).await.map_err(|error| CatalogProviderError::new(error.to_string()))?
    }
}
