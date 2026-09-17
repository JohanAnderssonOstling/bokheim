use super::super::{
    fmt, get, header, post, Any, ApiError, AsyncReadExt, AxumPath, Body, Bytes, CatalogBookFormat, CatalogImportRequest, CatalogPageRequest, CatalogSearchRequest, CatalogService, CatalogServiceError, CatalogSourceId,
    CatalogSubjectBrowseRequest, CatalogSubjectBookPayloadRequest, CorsLayer, CoverQuery, HeaderMap, HeaderValue, IntoResponse, Json, Method, Path, Query, Response, Router, State, StatusCode, COVER_CACHE_CONTROL, DATA_CACHE_CONTROL,
    MAX_REGISTRY_BYTES, REGISTRY_CACHE_CONTROL,
};
use super::operations::catalog_operations;

pub fn app(service: CatalogService) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/opds/catalogs", get(catalogs))
        .route("/api/catalog/sources", get(catalog_source_registry))
        .route("/api/opds/sources/:source_id/page", post(catalog_page))
        .route("/api/opds/sources/:source_id/search", post(catalog_search))
        .route("/api/opds/subjects", post(catalog_subjects))
        .route("/api/opds/subject-books", post(catalog_subject_books))
        .route("/api/opds/sources/:source_id/acquire", post(catalog_acquire))
        .route("/api/opds/sources/:source_id/cover", get(catalog_cover))
        .route("/api/opds/covers/:asset", get(catalog_cover_asset))
        .route("/api/web/catalogs", get(browser_catalogs))
        .route("/api/web/stats", get(browser_catalog_stats))
        .route("/api/web/sources/:source_id/page", post(browser_catalog_page))
        .route("/api/web/sources/:source_id/search", post(browser_catalog_search))
        .route("/api/web/subjects", post(browser_catalog_subjects))
        .route("/api/web/sources/:source_id/acquire", post(browser_catalog_acquire))
        .layer(CorsLayer::new().allow_origin(Any).allow_methods([Method::GET, Method::POST, Method::OPTIONS]).allow_headers([header::ACCEPT, header::CONTENT_TYPE, header::IF_NONE_MATCH]).expose_headers([
            header::CONTENT_DISPOSITION,
            header::CONTENT_LENGTH,
            header::CONTENT_TYPE,
            header::ETAG,
        ]))
        .with_state(service)
}

pub(crate) async fn health() -> (StatusCode, &'static str) {
    (StatusCode::OK, "ok")
}

pub(crate) async fn catalogs(State(service): State<CatalogService>, headers: HeaderMap) -> Result<Response, ApiError> {
    require_wire_accept(&headers)?;
    let registry = service.registry().await;
    let etag = format!("\"opds-{}\"", registry.revision);
    if headers.get(header::IF_NONE_MATCH).and_then(|value| value.to_str().ok()).is_some_and(|value| value.split(',').any(|candidate| candidate.trim() == etag)) {
        let mut response = StatusCode::NOT_MODIFIED.into_response();
        add_common_headers(&mut response, REGISTRY_CACHE_CONTROL);
        response.headers_mut().insert(header::ETAG, HeaderValue::from_str(&etag).expect("ASCII ETag"));
        return Ok(response);
    }
    let mut response = wire_response(&registry, REGISTRY_CACHE_CONTROL)?;
    response.headers_mut().insert(header::ETAG, HeaderValue::from_str(&etag).expect("ASCII ETag"));
    Ok(response)
}

pub(crate) async fn catalog_source_registry(State(service): State<CatalogService>, headers: HeaderMap) -> Result<Response, ApiError> {
    require_wire_accept(&headers)?;
    let registry = service.source_registry().await.map_err(ApiError::internal)?;
    let etag = format!("\"catalog-sources-{}\"", registry.revision);
    if headers.get(header::IF_NONE_MATCH).and_then(|value| value.to_str().ok()).is_some_and(|value| value.split(',').any(|candidate| candidate.trim() == etag)) {
        let mut response = StatusCode::NOT_MODIFIED.into_response();
        add_common_headers(&mut response, REGISTRY_CACHE_CONTROL);
        response.headers_mut().insert(header::ETAG, HeaderValue::from_str(&etag).expect("ASCII ETag"));
        return Ok(response);
    }
    let mut response = wire_response(&registry, REGISTRY_CACHE_CONTROL)?;
    response.headers_mut().insert(header::ETAG, HeaderValue::from_str(&etag).expect("ASCII ETag"));
    Ok(response)
}

pub(crate) async fn catalog_page(State(service): State<CatalogService>, AxumPath(source_id): AxumPath<String>, headers: HeaderMap, body: Bytes) -> Result<Response, ApiError> {
    require_wire_accept(&headers)?;
    let request: CatalogPageRequest = decode_wire_request(&headers, &body)?;
    let source_id = CatalogSourceId::parse(&source_id).map_err(ApiError::bad_request)?;
    let response = service.page_response(&source_id, request).await.map_err(ApiError::unavailable)?;
    wire_response(&response, DATA_CACHE_CONTROL)
}

pub(crate) async fn catalog_search(State(service): State<CatalogService>, AxumPath(source_id): AxumPath<String>, headers: HeaderMap, body: Bytes) -> Result<Response, ApiError> {
    require_wire_accept(&headers)?;
    let request: CatalogSearchRequest = decode_wire_request(&headers, &body)?;
    let source_id = CatalogSourceId::parse(&source_id).map_err(ApiError::bad_request)?;
    let response = service.search_response(&source_id, request).await.map_err(ApiError::unavailable)?;
    wire_response(&response, DATA_CACHE_CONTROL)
}

pub(crate) async fn catalog_subjects(State(service): State<CatalogService>, headers: HeaderMap, body: Bytes) -> Result<Response, ApiError> {
    require_wire_accept(&headers)?;
    let request: CatalogSubjectBrowseRequest = decode_wire_request(&headers, &body)?;
    let response = service.subject_index(&request).await.map_err(|error| ApiError::bad_request(error.to_string()))?;
    wire_response(&response, DATA_CACHE_CONTROL)
}

pub(crate) async fn catalog_subject_books(State(service): State<CatalogService>, headers: HeaderMap, body: Bytes) -> Result<Response, ApiError> {
    require_wire_accept(&headers)?;
    let request: CatalogSubjectBookPayloadRequest = decode_wire_request(&headers, &body)?;
    let response = service.subject_book_payloads(&request).await.map_err(|error| ApiError::bad_request(error.to_string()))?;
    wire_response(&response, DATA_CACHE_CONTROL)
}

pub(crate) async fn catalog_acquire(State(service): State<CatalogService>, AxumPath(source_id): AxumPath<String>, headers: HeaderMap, body: Bytes) -> Result<Response, ApiError> {
    let request: CatalogImportRequest = decode_wire_request(&headers, &body)?;
    catalog_acquisition_response(&service, &source_id, request).await
}

pub(crate) async fn browser_catalogs(State(service): State<CatalogService>) -> Result<Response, ApiError> {
    let _ = service.registry().await;
    json_response(&service.browser_sources().await, REGISTRY_CACHE_CONTROL)
}

pub(crate) async fn browser_catalog_stats(State(service): State<CatalogService>) -> Result<Response, ApiError> {
    catalog_operations(State(service)).await
}

pub(crate) async fn browser_catalog_page(State(service): State<CatalogService>, AxumPath(source_id): AxumPath<String>, Json(mut request): Json<CatalogPageRequest>) -> Result<Response, ApiError> {
    let source_id = CatalogSourceId::parse(&source_id).map_err(ApiError::bad_request)?;
    request.epoch = None;
    request.known_books.clear();
    let page = service.browser_page(&source_id, request).await.map_err(ApiError::unavailable)?;
    json_response(&page, DATA_CACHE_CONTROL)
}

pub(crate) async fn browser_catalog_search(State(service): State<CatalogService>, AxumPath(source_id): AxumPath<String>, Json(mut request): Json<CatalogSearchRequest>) -> Result<Response, ApiError> {
    let source_id = CatalogSourceId::parse(&source_id).map_err(ApiError::bad_request)?;
    request.epoch = None;
    request.known_books.clear();
    let page = service.browser_search(&source_id, request).await.map_err(ApiError::unavailable)?;
    json_response(&page, DATA_CACHE_CONTROL)
}

pub(crate) async fn browser_catalog_subjects(State(service): State<CatalogService>, Json(request): Json<CatalogSubjectBrowseRequest>) -> Result<Response, ApiError> {
    let response = service.browse_subjects(&request).await.map_err(|error| ApiError::bad_request(error.to_string()))?;
    json_response(&response, DATA_CACHE_CONTROL)
}

pub(crate) async fn browser_catalog_acquire(State(service): State<CatalogService>, AxumPath(source_id): AxumPath<String>, Json(request): Json<CatalogImportRequest>) -> Result<Response, ApiError> {
    catalog_acquisition_response(&service, &source_id, request).await
}

async fn catalog_acquisition_response(service: &CatalogService, source_id: &str, request: CatalogImportRequest) -> Result<Response, ApiError> {
    let source_id = CatalogSourceId::parse(&source_id).map_err(ApiError::bad_request)?;
    let downloaded = service.acquisition(&source_id, &request).await.map_err(ApiError::unavailable)?;
    let length = tokio::fs::metadata(downloaded.path()).await.map_err(ApiError::internal)?.len();
    let file = tokio::fs::File::open(downloaded.path()).await.map_err(ApiError::internal)?;
    let stream = futures_util::stream::try_unfold((file, downloaded), |(mut file, guard)| async move {
        let mut buffer = vec![0_u8; 64 * 1024];
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            Ok::<_, std::io::Error>(None)
        } else {
            buffer.truncate(read);
            Ok(Some((Bytes::from(buffer), (file, guard))))
        }
    });
    let mut response = Response::new(Body::from_stream(stream));
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(book_media_type(request.book_format)));
    response.headers_mut().insert(header::CONTENT_LENGTH, HeaderValue::from_str(&length.to_string()).map_err(ApiError::internal)?);
    response.headers_mut().insert(header::CONTENT_DISPOSITION, HeaderValue::from_static("attachment"));
    response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    response.headers_mut().insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    Ok(response)
}

pub(crate) fn book_media_type(format: CatalogBookFormat) -> &'static str {
    match format {
        CatalogBookFormat::Epub => "application/epub+zip",
        CatalogBookFormat::Pdf => "application/pdf",
        CatalogBookFormat::Mobi => "application/x-mobipocket-ebook",
        CatalogBookFormat::M4b => "audio/mp4",
    }
}

pub(crate) async fn catalog_cover(State(service): State<CatalogService>, AxumPath(source_id): AxumPath<String>, Query(query): Query<CoverQuery>, request_headers: HeaderMap) -> Result<Response, ApiError> {
    let source_id = CatalogSourceId::parse(&source_id).map_err(ApiError::bad_request)?;
    let bytes = service.catalog_cover(&source_id, &query.image_url).await.map_err(ApiError::unavailable)?;
    let digest = blake3::hash(&bytes).to_hex().to_string();
    cover_response(bytes, &digest, &request_headers)
}

pub(crate) async fn catalog_cover_asset(State(service): State<CatalogService>, AxumPath(asset): AxumPath<String>, request_headers: HeaderMap) -> Result<Response, ApiError> {
    let digest = asset.strip_suffix(".jpg").ok_or_else(|| ApiError::bad_request("catalog cover asset must end in .jpg"))?;
    if catalog_core::catalog_cover_asset_digest(&format!("/api/opds/covers/{asset}")).is_none() {
        return Err(ApiError::bad_request("catalog cover asset digest is invalid"));
    }
    let bytes = service.catalog_cover_asset(digest).await.map_err(ApiError::unavailable)?;
    cover_response(bytes, digest, &request_headers)
}

fn cover_response(bytes: Vec<u8>, digest: &str, request_headers: &HeaderMap) -> Result<Response, ApiError> {
    let etag = format!("\"{digest}\"");
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("image/jpeg"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(COVER_CACHE_CONTROL));
    headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    headers.insert(header::ETAG, HeaderValue::from_str(&etag).expect("ASCII ETag"));
    if request_headers.get(header::IF_NONE_MATCH).and_then(|value| value.to_str().ok()).is_some_and(|value| value.split(',').any(|candidate| candidate.trim() == etag)) {
        return Ok((StatusCode::NOT_MODIFIED, headers).into_response());
    }
    Ok((headers, bytes).into_response())
}

pub(crate) fn require_wire_accept(headers: &HeaderMap) -> Result<(), ApiError> {
    if !headers.get_all(header::ACCEPT).iter().filter_map(|value| value.to_str().ok()).flat_map(|value| value.split(',')).any(transport_wire::is_current_media_type) {
        return Err(ApiError { status: StatusCode::NOT_ACCEPTABLE, message: "request requires the current Bokheim binary media type".to_owned() });
    }
    Ok(())
}

pub(crate) fn decode_wire_request<T: serde::de::DeserializeOwned>(headers: &HeaderMap, body: &[u8]) -> Result<T, ApiError> {
    let media_type = headers.get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok());
    if !media_type.is_some_and(transport_wire::is_current_media_type) {
        return Err(ApiError::bad_request("request requires the current Bokheim binary content type"));
    }
    if headers.get(header::CONTENT_ENCODING).and_then(|value| value.to_str().ok()).is_some_and(|value| !value.eq_ignore_ascii_case("identity")) {
        return Err(ApiError::bad_request("compressed request bodies are not supported"));
    }
    transport_wire::decode(body, transport_wire::MAX_DECODED_REQUEST_BYTES).map_err(|error| ApiError::bad_request(error.to_string()))
}

pub(crate) fn wire_response(value: &impl serde::Serialize, cache_control: &'static str) -> Result<Response, ApiError> {
    let bytes = transport_wire::encode(value).map_err(ApiError::internal)?;
    let mut response = bytes.into_response();
    add_common_headers(&mut response, cache_control);
    Ok(response)
}

pub(crate) fn json_response(value: &impl serde::Serialize, cache_control: &'static str) -> Result<Response, ApiError> {
    let mut response = Json(value).into_response();
    response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static(cache_control));
    Ok(response)
}

pub(crate) fn add_common_headers(response: &mut Response, cache_control: &'static str) {
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(transport_wire::MEDIA_TYPE));
    response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static(cache_control));
    response.headers_mut().insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    response.headers_mut().insert(header::VARY, HeaderValue::from_static("Accept"));
}

impl ApiError {
    pub(crate) fn bad_request(message: impl Into<String>) -> Self {
        Self { status: StatusCode::BAD_REQUEST, message: message.into() }
    }

    pub(crate) fn unavailable(error: CatalogServiceError) -> Self {
        Self { status: StatusCode::SERVICE_UNAVAILABLE, message: error.to_string() }
    }

    fn internal(error: impl fmt::Display) -> Self {
        Self { status: StatusCode::INTERNAL_SERVER_ERROR, message: error.to_string() }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, [(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")], self.message).into_response()
    }
}

pub(crate) async fn read_registry_file(path: &Path) -> Result<Vec<u8>, CatalogServiceError> {
    let metadata = tokio::fs::metadata(path).await.map_err(|error| CatalogServiceError(format!("could not inspect catalog registry {}: {error}", path.display())))?;
    if metadata.len() > MAX_REGISTRY_BYTES as u64 {
        return Err(CatalogServiceError(format!("catalog registry {} exceeds {MAX_REGISTRY_BYTES} bytes", path.display())));
    }
    tokio::fs::read(path).await.map_err(|error| CatalogServiceError(format!("could not read catalog registry {}: {error}", path.display())))
}
