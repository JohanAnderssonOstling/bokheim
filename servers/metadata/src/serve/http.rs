use crate::query::run_database;
use crate::serve::ServiceState;
use crate::*;
pub fn app(service: MetadataService) -> Router {
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/health/descriptions", get(description_capability))
        .route("/internal/operations", get(operational_stats))
        .route("/v3/books", post(books).layer::<_, std::convert::Infallible>(DefaultBodyLimit::max(metadata_contract::books::MAX_BOOKS_REQUEST_BYTES)).layer(public_cors()))
        .route("/v3/covers/:id", get(resolved_cover).layer(public_cors()))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES))
        .layer(middleware::from_fn_with_state(service.clone(), record_request_metrics))
        .with_state(service)
}

async fn description_capability(State(service): State<MetadataService>) -> (StatusCode, &'static str) {
    if service.state.descriptions_available {
        (StatusCode::OK, "available")
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "snapshot has no descriptions")
    }
}

async fn record_request_metrics(State(service): State<MetadataService>, request: Request, next: Next) -> Response {
    let endpoint = match request.uri().path() {
        "/v3/books" => Some(&service.state.books),
        _ => None,
    };
    let started = Instant::now();
    if let Some(endpoint) = endpoint {
        endpoint.in_flight.fetch_add(1, Ordering::Relaxed);
    }
    let response = next.run(request).await;
    if let Some(endpoint) = endpoint {
        endpoint.in_flight.fetch_sub(1, Ordering::Relaxed);
        endpoint.requests.fetch_add(1, Ordering::Relaxed);
        if !response.status().is_success() {
            endpoint.errors.fetch_add(1, Ordering::Relaxed);
        }
        let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        endpoint.total_duration_ms.fetch_add(duration_ms, Ordering::Relaxed);
    }
    response
}

async fn operational_stats(State(service): State<MetadataService>) -> Response {
    match run_database(service, |service| load_operational_stats(&service.state)).await {
        Ok(Ok(stats)) => {
            let mut response = axum::Json(stats).into_response();
            response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            response.headers_mut().insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
            response
        }
        Ok(Err(error)) => api_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()),
        Err(error) => api_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("statistics worker failed: {error}")),
    }
}

fn load_operational_stats(state: &ServiceState) -> Result<serde_json::Value, MetadataError> {
    let pool = state.pool.state();
    Ok(serde_json::json!({
        "status":"available","uptime_seconds":state.started_at.elapsed().as_secs(),
        "schema_version":state.schema_version,"dump_date":state.snapshot.dump_date,
        "built_at_ms":state.snapshot.imported_at_ms,"database_bytes":fs::metadata(&state.database).map_err(error)?.len(),
        "pool_connections":pool.connections,"pool_idle_connections":pool.idle_connections,
        "processed_items":state.processed_items.load(Ordering::Relaxed),
        "item_processing_duration_ms":state.item_processing_duration_ms.load(Ordering::Relaxed),
        "endpoint_metrics":{"books":endpoint_metrics_snapshot(&state.books)}
    }))
}

fn endpoint_metrics_snapshot(metrics: &EndpointMetrics) -> EndpointMetricsSnapshot {
    let requests = metrics.requests.load(Ordering::Relaxed);
    let total_duration_ms = metrics.total_duration_ms.load(Ordering::Relaxed);
    EndpointMetricsSnapshot {
        requests,
        errors: metrics.errors.load(Ordering::Relaxed),
        in_flight: metrics.in_flight.load(Ordering::Relaxed),
        total_duration_ms,
        average_duration_ms: if requests == 0 { 0.0 } else { total_duration_ms as f64 / requests as f64 },
    }
}

async fn books(State(service): State<MetadataService>, axum::Json(request): axum::Json<metadata_contract::books::BookEnrichmentRequest>) -> Response {
    let item_count = u64::try_from(request.books.len()).unwrap_or(u64::MAX);
    let metrics = service.state.clone();
    let processing_started = Instant::now();
    match service.enrich_books(request).await {
        Ok(response) => {
            record_processed_items(&metrics, item_count, processing_started.elapsed());
            axum::Json(response).into_response()
        }
        Err(error) => api_error(StatusCode::BAD_REQUEST, &error.to_string()),
    }
}

pub(crate) fn validate_isbn_count(count: usize) -> Result<(), MetadataError> {
    if count == 0 {
        return Err(MetadataError("at least one ISBN is required".to_owned()));
    }
    if count > MAX_ISBNS_PER_REQUEST {
        return Err(MetadataError(format!("at most {MAX_ISBNS_PER_REQUEST} ISBNs may be requested")));
    }
    Ok(())
}

pub(crate) fn record_processed_items(state: &ServiceState, count: u64, duration: Duration) {
    state.processed_items.fetch_add(count, Ordering::Relaxed);
    state.item_processing_duration_ms.fetch_add(u64::try_from(duration.as_millis()).unwrap_or(u64::MAX), Ordering::Relaxed);
}

pub(crate) fn api_error(status: StatusCode, message: &str) -> Response {
    (status, [(header::CONTENT_TYPE, "text/plain; charset=utf-8")], message.to_owned()).into_response()
}

fn public_cors() -> tower_http::cors::CorsLayer {
    // Public bibliographic lookups carry no cookies or account credentials.
    tower_http::cors::CorsLayer::new().allow_origin(tower_http::cors::Any).allow_methods([axum::http::Method::POST, axum::http::Method::GET]).allow_headers([header::CONTENT_TYPE])
}

#[cfg(test)]
mod http_tests {
    use super::*;

    #[tokio::test]
    async fn browser_preflight_and_invalid_request_reach_the_books_route() {
        let temporary = tempfile::tempdir().unwrap();
        let database = crate::build::tests::snapshot_from(temporary.path(), &[], &[], &[]);
        let answers = temporary.path().join("answers.sqlite");
        build_answers(&database, None, None, None, &answers).unwrap();
        let service = MetadataService::open(&answers, &temporary.path().join("demand.sqlite")).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v3/books", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, app(service)).await.unwrap();
        });
        let client = reqwest::Client::new();
        let preflight =
            client.request(reqwest::Method::OPTIONS, &url).header("Origin", "https://app.bokheim.se").header("Access-Control-Request-Method", "POST").header("Access-Control-Request-Headers", "content-type").send().await.unwrap();
        assert!(preflight.status().is_success());
        assert_eq!(preflight.headers()["access-control-allow-origin"], "*");
        let invalid = client.post(&url).header("Origin", "https://app.bokheim.se").header("Content-Type", "application/json").body("{}").send().await.unwrap();
        assert_eq!(invalid.status().as_u16(), 422);
        assert_eq!(invalid.headers()["access-control-allow-origin"], "*");
        task.abort();
    }
}

async fn resolved_cover(State(service): State<MetadataService>, axum::extract::Path(id): axum::extract::Path<String>) -> Response {
    match run_database(service, move |service| service.cached_cover(&id)).await {
        Ok(Ok(Some(bytes))) => ([(header::CONTENT_TYPE, "application/octet-stream"), (header::CACHE_CONTROL, "public, max-age=2592000")], bytes).into_response(),
        Ok(Ok(None)) => StatusCode::NOT_FOUND.into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
