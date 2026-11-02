use crate::{BookId, LibraryId, LibrarySyncStatus};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct AccountStatus {
    signed_in: bool,
    server_url: Option<String>,
    email: Option<String>,
}

impl AccountStatus {
    pub fn signed_out() -> Self {
        Self::default()
    }

    pub fn authenticated(server_url: impl Into<String>, email: impl Into<String>) -> Self {
        Self { signed_in: true, server_url: Some(server_url.into()), email: Some(email.into()) }
    }

    pub const fn signed_in(&self) -> bool {
        self.signed_in
    }

    pub fn server_url(&self) -> Option<&str> {
        self.server_url.as_deref()
    }

    pub fn email(&self) -> Option<&str> {
        self.email.as_deref()
    }
}

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
