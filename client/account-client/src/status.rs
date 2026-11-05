//! Public account identity, without credentials.
use crate::Session;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
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

    pub(crate) fn from_session(session: Option<&Session>) -> Self {
        session.map(|session| Self::authenticated(session.server_url().as_str(), session.email())).unwrap_or_default()
    }
}
