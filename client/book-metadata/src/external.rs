//! Errors returned by optional external metadata helpers.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalMetadataError(String);

impl ExternalMetadataError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for ExternalMetadataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ExternalMetadataError {}
