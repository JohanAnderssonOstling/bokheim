use crate::state::AppState;
use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderValue};
use axum::Json;
use sync_common::api::opds::CatalogRegistry;

pub(super) async fn get_catalogs(State(state): State<AppState>) -> (HeaderMap, Json<CatalogRegistry>) {
    let mut headers = HeaderMap::new();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=3600, stale-if-error=604800"));
    (headers, Json((*state.opds_catalogs).clone()))
}
