use crate::{BookId, LibraryId, LibrarySyncStatus};
use serde::{Deserialize, Serialize};

use account_client::AccountStatus;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct ServerLibraryStorageUsage {
    pub library_id: LibraryId,
    pub library_name: String,
    pub used_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct ServerBookStorageUsage {
    #[serde(rename = "content_hash")]
    pub book_id: BookId,
    pub title: String,
    pub used_bytes: u64,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct AccountSnapshot {
    pub account: AccountStatus,
    pub storage_usage: Option<account_contract::StorageUsageResponse>,
    #[serde(default)]
    pub pending_cloud_bytes: Option<u64>,
    pub sync_statuses: Vec<LibrarySyncStatus>,
    pub server_storage_usage: Option<Vec<ServerLibraryStorageUsage>>,
    #[serde(default)]
    pub server_book_storage_usage: Option<Vec<ServerBookStorageUsage>>,
}
