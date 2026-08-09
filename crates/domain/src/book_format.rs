use serde::{Deserialize, Serialize};

/// File formats supported by the library's book workflows.
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
