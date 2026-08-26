use serde::{Deserialize, Serialize};

/// Known persisted file formats, including legacy values retained for decoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum BookFormat {
    Epub,
    Pdf,
    Mobi,
    Fb2,
    Cbz,
    Html,
    WebPub,
    M4b,
}

impl BookFormat {
    /// Parses every format that may already exist in a library or arrive from
    /// an older synchronization peer. This is intentionally broader than the
    /// formats accepted for new imports.
    pub fn from_extension(extension: &str) -> Option<Self> {
        match extension.to_ascii_lowercase().as_str() {
            "epub" => Some(Self::Epub),
            "pdf" => Some(Self::Pdf),
            "mobi" | "azw" | "azw3" => Some(Self::Mobi),
            "fb2" => Some(Self::Fb2),
            "cbz" => Some(Self::Cbz),
            "html" | "htm" | "xhtml" => Some(Self::Html),
            "webpub" => Some(Self::WebPub),
            "m4b" => Some(Self::M4b),
            _ => None,
        }
    }

    /// Parses formats accepted for a new user-initiated import.
    pub fn from_import_extension(extension: &str) -> Option<Self> {
        let format = Self::from_extension(extension)?;
        match format {
            Self::Epub | Self::Pdf | Self::Mobi | Self::M4b => Some(format),
            Self::Fb2 | Self::Cbz | Self::Html | Self::WebPub => None,
        }
    }

    pub const fn canonical_extension(self) -> &'static str {
        match self {
            Self::Epub => "epub",
            Self::Pdf => "pdf",
            Self::Mobi => "mobi",
            Self::Fb2 => "fb2",
            Self::Cbz => "cbz",
            Self::Html => "html",
            Self::WebPub => "webpub",
            Self::M4b => "m4b",
        }
    }
}
