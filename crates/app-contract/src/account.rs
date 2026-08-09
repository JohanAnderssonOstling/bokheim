use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Clone, Debug, Default, Deserialize, Serialize, Type)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum LibrarySyncState {
    LocalOnly,
    Synced,
    SignInRequired,
    DifferentAccount,
    RemoteMissing,
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
pub struct LibrarySyncStatus {
    pub library_id: String,
    pub library_name: String,
    pub state: LibrarySyncState,
    pub server_url: Option<String>,
    pub account_id: Option<String>,
}
