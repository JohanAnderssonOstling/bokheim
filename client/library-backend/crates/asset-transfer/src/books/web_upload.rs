//! Upload a verified OPFS File without copying its bytes into WASM memory.
use crate::TransferError;
use client_protocol::{BackendError, Failure};
use library_files::asset_store::VerifiedBook;
pub async fn upload(book: &VerifiedBook, url: &reqwest::Url, token: &str) -> Result<(), TransferError> {
    let mut trace = sync_transport::PerformanceTrace::new("book_http_upload", "open_opfs_file");
    let file = book.upload_file().await?;
    if file.size() != book.length as f64 {
        return Err(TransferError::retryable("verified book size changed"));
    }
    let bearer = format!("Bearer {token}");
    let checksum = book.book_id.to_hex();
    trace.phase("send_body_wait_response");
    let trace_id = trace.id();
    let status = client_platform_web::transport::upload::put_blob(
        &file,
        url.as_str(),
        &[("authorization", &bearer), ("x-bokheim-content-hash", &checksum), ("x-bokheim-trace-id", &trace_id), ("content-type", "application/octet-stream")],
    )
    .await
    .map_err(TransferError::retryable)?;
    log::debug!(target: "sync_performance", "trace_id={} http_status={status} bytes={}", trace.id(), book.length);
    let http_error = || {
        let message = format!("asset upload HTTP {status}");
        match Failure::from_http_status(status) {
            Some(failure) => BackendError::classified(failure, message),
            None => BackendError::message(message),
        }
    };
    let result = match sync_transport::classify_asset_status(status) {
        sync_transport::AssetStatus::Success => Ok(()),
        sync_transport::AssetStatus::AuthenticationRequired => Err(TransferError::AuthenticationRequired),
        sync_transport::AssetStatus::StorageQuotaExceeded => Err(TransferError::Retryable(BackendError::classified(Failure::CloudStorageFull, "book storage quota exceeded"))),
        sync_transport::AssetStatus::Retryable => Err(TransferError::Retryable(http_error())),
        sync_transport::AssetStatus::Rejected => Err(TransferError::Rejected(http_error())),
    };
    trace.finish(result.is_ok());
    result
}
