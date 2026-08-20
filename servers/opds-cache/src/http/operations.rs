use super::super::{
    canonical_isbn, classify_catalog_failure, get, post, writer_metrics_snapshot, ApiError, AxumPath, CatalogCache, CatalogInventoryResponse, CatalogOperationsResponse, CatalogService, CatalogSourceId, DescriptionBackfillInventoryResponse,
    EpubExtractionInventoryResponse, EpubFailureDiagnosticResponse, EpubFailureExampleResponse, IdentityReviewAction, IdentityReviewQuery, IdentityReviewResponse, Json, MetadataBackfillInventoryResponse, Ordering, Query, Response, Router,
    State, StatusCode,
};
use super::public::json_response;
use crate::CatalogSourceInventoryResponse;

/// Loopback-only operational surface. The catalogue binary serves this on a
/// separate listener so public reverse proxies cannot accidentally expose it.
pub fn operations_app(service: CatalogService) -> Router {
    Router::new()
        .route("/internal/operations", get(catalog_operations))
        .route("/internal/subject-audit", get(catalog_subject_audit))
        .route("/internal/identity-reviews", get(catalog_identity_reviews))
        .route("/internal/identity-reviews/retry-ambiguous", post(catalog_retry_ambiguous_identity_reviews))
        .route("/internal/identity-reviews/:review_id", post(catalog_identity_review_action))
        .with_state(service)
}

pub(crate) async fn catalog_operations(State(service): State<CatalogService>) -> Result<Response, ApiError> {
    let inventory = catalog_inventory_response(&service).await?;
    let failures = service.cache_call(CatalogCache::epub_failure_diagnostics).await.map_err(ApiError::unavailable)?;
    let extraction_failures = failures
        .into_iter()
        .map(|failure| EpubFailureDiagnosticResponse {
            category: failure.category,
            retryable: failure.retryable,
            count: failure.count,
            first_failed_at: failure.first_failed_at,
            last_failed_at: failure.last_failed_at,
            next_retry_at: failure.next_retry_at,
            examples: failure
                .examples
                .into_iter()
                .map(|example| EpubFailureExampleResponse {
                    source_id: example.source_id.as_str().to_owned(),
                    acquisition_host: example.acquisition_host,
                    publication_id: example.publication_id,
                    attempts: example.attempts,
                    last_failed_at: example.last_failed_at,
                })
                .collect(),
        })
        .collect();
    json_response(&CatalogOperationsResponse { inventory, extraction_failures }, "no-store")
}

pub(crate) async fn catalog_retry_ambiguous_identity_reviews(State(service): State<CatalogService>) -> Result<Response, ApiError> {
    let count = service.cache_write_call_named("identity_review_requeue", |cache| cache.retry_ambiguous_identity_reviews()).await.map_err(ApiError::unavailable)?;
    service.state.extraction_notify.notify_one();
    json_response(&serde_json::json!({ "requeued": count }), "no-store")
}

pub(crate) async fn catalog_subject_audit(State(service): State<CatalogService>) -> Result<Response, ApiError> {
    let audit = service.cache_call(CatalogCache::subject_projection_audit).await.map_err(ApiError::unavailable)?;
    json_response(&audit, "no-store")
}

pub(crate) async fn catalog_identity_reviews(State(service): State<CatalogService>, Query(query): Query<IdentityReviewQuery>) -> Result<Response, ApiError> {
    let source_id = query.source_id.as_deref().map(CatalogSourceId::parse).transpose().map_err(ApiError::bad_request)?;
    let status = query.status.clone();
    let review_source = source_id.clone();
    let reviews = service.cache_call(move |cache| cache.identity_reviews(review_source.as_ref(), status.as_deref(), query.limit.unwrap_or(100))).await.map_err(ApiError::unavailable)?;
    let inventory = service
        .cache_call(move |cache| match source_id.as_ref() {
            Some(source_id) => cache.identity_review_inventory_for_source(source_id),
            None => cache.identity_review_inventory(),
        })
        .await
        .map_err(ApiError::unavailable)?;
    json_response(&IdentityReviewResponse { inventory, reviews }, "no-store")
}

pub(crate) async fn catalog_identity_review_action(State(service): State<CatalogService>, AxumPath(review_id): AxumPath<String>, Json(action): Json<IdentityReviewAction>) -> Result<Response, ApiError> {
    if review_id.len() != 64 || !review_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ApiError::bad_request("invalid identity review id"));
    }
    let requeue = !matches!(&action, IdentityReviewAction::Reject);
    let changed = match action {
        IdentityReviewAction::SelectCandidate { edition_id, isbn13 } => {
            let canonical = canonical_isbn(&isbn13).ok_or_else(|| ApiError::bad_request("invalid ISBN"))?;
            let cache_review = review_id.clone();
            let cache_edition = edition_id.clone();
            let candidate = service.cache_call(move |cache| cache.identity_review_candidate_isbn(&cache_review, &cache_edition, &canonical)).await.map_err(ApiError::unavailable)?;
            let Some(candidate) = candidate else { return Err(ApiError::bad_request("candidate does not belong to this review")) };
            let cache_review = review_id.clone();
            service.cache_write_call(move |cache| cache.resolve_identity_review_with_isbn(&cache_review, &candidate)).await.map_err(ApiError::unavailable)?
        }
        IdentityReviewAction::ManualIsbn { isbn } => {
            let canonical = canonical_isbn(&isbn).ok_or_else(|| ApiError::bad_request("invalid ISBN"))?;
            let cache_review = review_id.clone();
            service.cache_write_call(move |cache| cache.resolve_identity_review_with_isbn(&cache_review, &canonical)).await.map_err(ApiError::unavailable)?
        }
        IdentityReviewAction::Reject => {
            let cache_review = review_id.clone();
            service.cache_write_call(move |cache| cache.reject_identity_review(&cache_review)).await.map_err(ApiError::unavailable)?
        }
        IdentityReviewAction::Retry => {
            let cache_review = review_id.clone();
            service.cache_write_call(move |cache| cache.retry_identity_review(&cache_review)).await.map_err(ApiError::unavailable)?
        }
    };
    if !changed {
        return Err(ApiError { status: StatusCode::NOT_FOUND, message: "identity review is no longer pending".to_owned() });
    }
    if requeue {
        service.state.extraction_notify.notify_one();
    }
    json_response(&serde_json::json!({ "status": "accepted", "review_id": review_id }), "no-store")
}

pub(crate) async fn catalog_inventory_response(service: &CatalogService) -> Result<CatalogInventoryResponse, ApiError> {
    let registry = service.registry().await;
    let uptime_seconds = service.state.started_at.elapsed().as_secs();
    let metadata_enrichment_configured = service.state.metadata_endpoint.is_some();
    let inventory = service.inventory().await.map_err(ApiError::unavailable)?;
    let crawl_progress = service.state.crawl_progress.lock().await.clone();
    let response = CatalogInventoryResponse {
        status: "available",
        uptime_seconds,
        registry_revision: registry.revision,
        metadata_enrichment_configured,
        search_index_ready: inventory.search_index_ready,
        active_page_count: inventory.active_page_count,
        active_publication_count: inventory.active_publication_count,
        active_epub_count: inventory.active_epub_count,
        epub_extractions: epub_extraction_inventory_response(inventory.epub_extractions),
        metadata_backfill: MetadataBackfillInventoryResponse {
            pending: inventory.metadata_backfill.pending,
            running: inventory.metadata_backfill.running,
            succeeded: inventory.metadata_backfill.succeeded,
            last_error: inventory.metadata_backfill.last_error,
        },
        description_backfill: DescriptionBackfillInventoryResponse {
            pending: inventory.description_backfill.pending,
            running: inventory.description_backfill.running,
            succeeded: inventory.description_backfill.succeeded,
            no_description: inventory.description_backfill.no_description,
            last_error: inventory.description_backfill.last_error,
        },
        description_backfill_available: service.state.description_backfill_available.load(Ordering::Relaxed),
        metadata_backfill_metrics: service.state.metadata_backfill_metrics.snapshot(),
        description_backfill_metrics: service.state.description_backfill_metrics.snapshot(),
        writer_metrics: writer_metrics_snapshot(&service.state.writer_metrics),
        identity_reviews: inventory.identity_reviews,
        sources: inventory
            .sources
            .into_iter()
            .map(|source| {
                let progress = (source.crawl_status == "running").then(|| crawl_progress.get(&source.source_id)).flatten();
                CatalogSourceInventoryResponse {
                    source_id: source.source_id.as_str().to_owned(),
                    crawl_status: source.crawl_status,
                    last_crawl_started_at: source.last_crawl_started_at,
                    last_crawl_succeeded_at: source.last_crawl_succeeded_at,
                    last_crawl_error_category: source.last_crawl_error.as_deref().map(|error| classify_catalog_failure(error).0),
                    crawl_frontier_count: source.crawl_frontier_count,
                    crawl_pages_fetched: progress.map_or(0, |progress| progress.pages_fetched),
                    crawl_publications_seen: progress.map_or(0, |progress| progress.publications_seen),
                    crawl_elapsed_seconds: progress.map_or(0, |progress| progress.started_at.elapsed().as_secs()),
                    active_page_count: source.active_page_count,
                    active_publication_count: source.active_publication_count,
                    active_epub_count: source.active_epub_count,
                    epub_extractions: epub_extraction_inventory_response(source.epub_extractions),
                }
            })
            .collect(),
    };
    Ok(response)
}

pub(crate) fn epub_extraction_inventory_response(inventory: catalog_core::CatalogEpubExtractionInventory) -> EpubExtractionInventoryResponse {
    EpubExtractionInventoryResponse {
        pending: inventory.pending,
        running: inventory.running,
        downloading: inventory.downloading,
        extracting: inventory.extracting,
        enriching: inventory.enriching,
        succeeded: inventory.succeeded,
        failed: inventory.failed,
        attempts: inventory.attempts,
        downloaded_bytes: inventory.downloaded_bytes,
    }
}
