use std::{error::Error, fmt};

use serde::{Deserialize, Serialize};

use crate::validation::bounded;

/// Exact subject evidence supplied by a book or authority. Taxonomy
/// projection never rewrites this value; it produces separate assignments.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct BookSubject {
    pub id: Option<String>,
    pub name: String,
    pub sort_name: Option<String>,
    pub source: String,
    pub authority: Option<String>,
    pub code: Option<String>,
}

impl<'de> Deserialize<'de> for BookSubject {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Encoded {
            id: Option<String>,
            name: String,
            sort_name: Option<String>,
            source: String,
            authority: Option<String>,
            code: Option<String>,
        }

        let value = Encoded::deserialize(deserializer)?;
        Self::with_details(value.id, value.name, value.sort_name, value.source, value.authority, value.code).map_err(serde::de::Error::custom)
    }
}

impl BookSubject {
    pub fn new(id: Option<String>, name: impl AsRef<str>, source: impl AsRef<str>, authority: Option<String>, code: Option<String>) -> Result<Self, SubjectError> {
        Self::with_details(id, name, None, source, authority, code)
    }

    pub fn with_details(id: Option<String>, name: impl AsRef<str>, sort_name: Option<String>, source: impl AsRef<str>, authority: Option<String>, code: Option<String>) -> Result<Self, SubjectError> {
        Ok(Self {
            id: id.map(|value| bounded(&value).map_err(|_| SubjectError)).transpose()?,
            name: bounded(name.as_ref()).map_err(|_| SubjectError)?,
            sort_name: sort_name.map(|value| bounded(&value).map_err(|_| SubjectError)).transpose()?,
            source: bounded(source.as_ref()).map_err(|_| SubjectError)?,
            authority: authority.map(|value| bounded(&value).map_err(|_| SubjectError)).transpose()?,
            code: code.map(|value| bounded(&value).map_err(|_| SubjectError)).transpose()?,
        })
    }

    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn authority(&self) -> Option<&str> {
        self.authority.as_deref()
    }

    pub fn code(&self) -> Option<&str> {
        self.code.as_deref()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubjectError;

impl fmt::Display for SubjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("subject metadata is invalid")
    }
}

impl Error for SubjectError {}
