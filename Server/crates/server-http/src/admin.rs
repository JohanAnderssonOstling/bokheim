use crate::extract::AuthUser;
use crate::state::AppState;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde::{Deserialize, Serialize};
use server_core::SetStorageQuota;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Serialize)]
pub(super) struct GcReport {
    blobs_scanned: usize,
    blobs_deleted: usize,
    thumbnails_scanned: usize,
    thumbnails_deleted: usize,
    bytes_reclaimed: u64,
}

#[derive(Debug, Deserialize)]
pub(super) struct SetStorageQuotaRequest {
    change_id: String,
    quota_bytes: u64,
    reason: String,
}

#[derive(Debug, Serialize)]
pub(super) struct StorageQuotaResponse {
    quota_bytes: u64,
    used_bytes: u64,
    reserved_bytes: u64,
    over_quota: bool,
}

const GC_SAFETY_WINDOW: Duration = Duration::from_secs(3600);

fn require_admin(user: &AuthUser) -> Result<(), StatusCode> {
    if user.is_admin {
        Ok(())
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

pub(super) async fn post_admin_storage_quota(State(state): State<AppState>, admin: AuthUser, Path(user_id): Path<String>, headers: HeaderMap, body: Bytes) -> Result<Response, StatusCode> {
    require_admin(&admin)?;
    let request: SetStorageQuotaRequest = crate::api_wire::decode_request(&headers, &body)?;
    let status =
        state.storage_quotas.set_quota(SetStorageQuota { change_id: request.change_id, user_id, actor: admin.username, quota_bytes: request.quota_bytes, reason: request.reason }).await.map_err(crate::extract::storage_quota_error)?;
    crate::api_wire::response(&headers, StorageQuotaResponse { quota_bytes: status.quota_bytes, used_bytes: status.used_bytes, reserved_bytes: status.reserved_bytes, over_quota: status.over_quota() })
}

pub(super) async fn post_admin_gc(State(state): State<AppState>, admin: AuthUser, headers: HeaderMap) -> Result<Response, StatusCode> {
    require_admin(&admin)?;
    let cutoff = SystemTime::now().checked_sub(GC_SAFETY_WINDOW).unwrap_or(UNIX_EPOCH);
    let report = state.garbage_collector.collect(cutoff).await.map_err(crate::extract::sync_error)?;
    crate::api_wire::response(&headers, GcReport { blobs_scanned: report.blobs_scanned, blobs_deleted: report.blobs_deleted, thumbnails_scanned: report.thumbnails_scanned, thumbnails_deleted: report.thumbnails_deleted, bytes_reclaimed: report.bytes_reclaimed })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maintenance_requires_an_administrator_identity() {
        let ordinary = AuthUser { user_id: "user".to_string(), username: "reader".to_string(), is_admin: false, credential: std::sync::Arc::from("credential") };
        let admin = AuthUser { is_admin: true, ..ordinary };
        assert_eq!(require_admin(&admin), Ok(()));
        assert_eq!(require_admin(&AuthUser { is_admin: false, ..admin }), Err(StatusCode::FORBIDDEN));
    }
}
