use super::*;

impl CatalogClient {
    pub async fn cover(&self, source_id: &CatalogSourceId, image_url: &str) -> Result<Vec<u8>, CatalogClientError> {
        let fetched = async { self.catalog_cover(source_id, image_url).await.map_err(CatalogCoverError::new) };
        self.covers.cover_with(source_id, image_url, fetched).await.map_err(Into::into)
    }

    pub async fn download_import(&self, request: &CatalogImportRequest) -> Result<Vec<u8>, CatalogClientError> {
        let url = self.endpoint(&format!("api/opds/sources/{}/acquire", request.source_id.as_str()))?;
        let bytes = wire::encode(request).map_err(CatalogClientError::new)?;
        let response = tokio::time::timeout(CATALOG_ACQUISITION_TIMEOUT, self.http.post(url).header(reqwest::header::CONTENT_TYPE, wire::MEDIA_TYPE).header(reqwest::header::ACCEPT, request.book_format.media_type()).body(bytes).send())
            .await
            .map_err(|_| CatalogClientError::new("catalog acquisition timed out"))?
            .map_err(CatalogClientError::new)?;
        if !response.status().is_success() {
            return Err(CatalogClientError::new(format!("catalog server returned HTTP {}", response.status())));
        }
        let expected_media_type = request.book_format.media_type();
        let media_type = response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).and_then(|value| value.split(';').next());
        if media_type != Some(expected_media_type) {
            return Err(CatalogClientError::new("catalog server acquisition has an unexpected media type"));
        }

        read_response_body(response, MAX_CATALOG_ACQUISITION_BYTES, "acquisition").await
    }
    async fn catalog_cover(&self, source_id: &CatalogSourceId, image_url: &str) -> Result<Vec<u8>, CatalogClientError> {
        let url = if catalog_core::catalog_cover_asset_digest(image_url).is_some() {
            self.endpoint(image_url.trim_start_matches('/'))?
        } else {
            let mut url = self.endpoint(&format!("api/opds/sources/{}/cover", source_id.as_str()))?;
            url.query_pairs_mut().append_pair("image_url", image_url);
            url
        };
        let response = tokio::time::timeout(Duration::from_secs(15), self.http.get(url).send()).await.map_err(|_| CatalogClientError::new("catalog server cover request timed out"))?.map_err(CatalogClientError::new)?;
        if !response.status().is_success() {
            return Err(CatalogClientError::new(format!("catalog server returned HTTP {}", response.status())));
        }
        let media_type = response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).and_then(|value| value.split(';').next());
        if media_type != Some("image/jpeg") {
            return Err(CatalogClientError::new("catalog server cover is not a JPEG"));
        }
        let bytes = read_response_body(response, MAX_CATALOG_COVER_SOURCE_BYTES, "cover").await?;
        validate_server_cover(bytes).map_err(Into::into)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn streamed_response_bodies_enforce_size_limits_without_content_length() {
        use axum::{body::Body, routing::get, Router};
        let app = Router::new()
            .route("/exact", get(|| async { Body::from_stream(futures_util::stream::iter([Ok::<_, std::io::Error>("1234"), Ok("5678")])) }))
            .route("/large", get(|| async { Body::from_stream(futures_util::stream::iter([Ok::<_, std::io::Error>("1234"), Ok("56789")])) }))
            .route("/empty", get(|| async { Body::empty() }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::new();
        let response = client.get(format!("http://{address}/exact")).send().await.unwrap();
        assert!(response.content_length().is_none());
        assert_eq!(read_response_body(response, 8, "cover").await.unwrap(), b"12345678");
        for (path, expected) in [("large", "too large"), ("empty", "empty")] {
            let response = client.get(format!("http://{address}/{path}")).send().await.unwrap();
            assert!(read_response_body(response, 8, "acquisition").await.unwrap_err().to_string().contains(expected));
        }
        server.abort();
    }
}
