use serde::{Deserialize, Serialize};

/// Supported book formats.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum BookFormat {
    Epub,
    Pdf,
    Mobi,
    M4b,
}

impl BookFormat {
    /// Parses supported book extensions.
    pub fn from_extension(extension: &str) -> Option<Self> {
        match extension.to_ascii_lowercase().as_str() {
            "epub" => Some(Self::Epub),
            "pdf" => Some(Self::Pdf),
            "mobi" | "azw" | "azw3" => Some(Self::Mobi),
            "m4b" => Some(Self::M4b),
            _ => None,
        }
    }

    /// Parses formats accepted for a new user-initiated import.
    pub fn from_import_extension(extension: &str) -> Option<Self> {
        Self::from_extension(extension)
    }

    pub const fn canonical_extension(self) -> &'static str {
        match self {
            Self::Epub => "epub",
            Self::Pdf => "pdf",
            Self::Mobi => "mobi",
            Self::M4b => "m4b",
        }
    }
}
