use axum::extract::Request;
use axum::http::{header, Method};
use axum::middleware::Next;
use axum::response::Response;
use std::time::Instant;
use tracing::Instrument;

/// Correlates server work with the client's `sync_performance` logs through
/// the request's trace id. Paths are reduced to bounded, non-sensitive route
/// groups so library ids and content hashes never reach the log.
pub(crate) async fn trace_request(request: Request, next: Next) -> Response {
    let started = Instant::now();
    let route_group = route_group(request.uri().path());
    let method = method_group(request.method());
    let request_bytes = content_length(request.headers());
    let trace_id = request.headers().get("x-bokheim-trace-id").and_then(|value| value.to_str().ok()).and_then(|value| uuid::Uuid::parse_str(value).ok()).unwrap_or_else(uuid::Uuid::new_v4);
    let span = tracing::debug_span!(target: "sync_performance", "server_request", %trace_id, route_group, method, request_bytes);
    async {
        tracing::debug!(target: "sync_performance", event = "start", "request started");
        let response = next.run(request).await;
        tracing::debug!(target: "sync_performance", event = "complete", status = response.status().as_u16(), response_bytes = content_length(response.headers()), elapsed_ms = started.elapsed().as_secs_f64() * 1000.0, "request headers ready");
        response
    }
    .instrument(span)
    .await
}

fn content_length(headers: &axum::http::HeaderMap) -> i64 {
    headers.get(header::CONTENT_LENGTH).and_then(|value| value.to_str().ok()).and_then(|value| value.parse::<u64>().ok()).map(|value| i64::try_from(value).unwrap_or(i64::MAX)).unwrap_or(0)
}

fn method_group(method: &Method) -> &'static str {
    match *method {
        Method::GET => "GET",
        Method::HEAD => "HEAD",
        Method::POST => "POST",
        Method::PUT => "PUT",
        Method::DELETE => "DELETE",
        Method::OPTIONS => "OPTIONS",
        _ => "OTHER",
    }
}

fn route_group(path: &str) -> &'static str {
    if path == "/api/live" || path == "/api/ready" || path == "/api/health" {
        "/api/health"
    } else if path.starts_with("/api/auth/") {
        "/api/auth"
    } else if path == "/api/notifications" {
        "/api/notifications"
    } else if path.starts_with("/api/sync/") {
        "/api/sync"
    } else if path.contains("/thumbnail") {
        "/api/thumbnails"
    } else if path.contains("/blobs/") {
        "/api/blobs"
    } else if path.starts_with("/api/libraries") {
        "/api/libraries"
    } else if path.starts_with("/api/account/") {
        "/api/account"
    } else {
        "/other"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_are_reduced_to_bounded_non_sensitive_categories() {
        assert_eq!(route_group("/api/libraries/private-library/blobs/private-hash"), "/api/blobs");
        assert_eq!(route_group("/api/libraries/private-library/thumbnails/private-hash"), "/api/thumbnails");
        assert_eq!(route_group("/api/auth/login"), "/api/auth");
        assert_eq!(route_group("/unexpected/private-value"), "/other");
    }
}
