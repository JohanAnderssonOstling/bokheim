use std::{error::Error, fmt};

use serde::{Deserialize, Serialize};

/// A validated, canonical BCP-47 language tag shared by authority and local
/// library metadata. Underscores are repaired at ingestion because they are a
/// common POSIX-style provider error; serialized values always use BCP-47
/// hyphens and canonical subtag casing.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct LanguageTag(String);

impl LanguageTag {
    pub fn parse(value: impl AsRef<str>) -> Result<Self, LanguageTagError> {
        let value = value.as_ref().trim().replace('_', "-");
        let tag = language_tags::LanguageTag::parse(&value).map_err(|_| LanguageTagError)?;
        tag.validate().map_err(|_| LanguageTagError)?;
        Ok(Self(tag.canonicalize().unwrap_or(tag).into_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl AsRef<str> for LanguageTag {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl PartialEq<&str> for LanguageTag {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl fmt::Display for LanguageTag {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for LanguageTag {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::parse(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LanguageTagError;

impl fmt::Display for LanguageTagError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("language must be a valid BCP-47 tag")
    }
}

impl Error for LanguageTagError {}
