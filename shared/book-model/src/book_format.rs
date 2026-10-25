use serde::{Deserialize, Serialize};

/// Supported book formats.
///
/// The discriminants are the stored database value. Each format is one bit so
/// a set of formats can be matched as a mask; the values must never change.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum BookFormat {
    Epub = 1,
    Pdf = 2,
    Mobi = 4,
    M4b = 8,
    Mp3Folder = 16,
    Fb2 = 32,
    Cbz = 64,
}

impl BookFormat {
    pub const fn from_code(code: i64) -> Option<Self> {
        match code {
            1 => Some(Self::Epub),
            2 => Some(Self::Pdf),
            4 => Some(Self::Mobi),
            8 => Some(Self::M4b),
            16 => Some(Self::Mp3Folder),
            32 => Some(Self::Fb2),
            64 => Some(Self::Cbz),
            _ => None,
        }
    }

    /// Parses supported book extensions.
    pub fn from_extension(extension: &str) -> Option<Self> {
        match extension.to_ascii_lowercase().as_str() {
            "epub" => Some(Self::Epub),
            "pdf" => Some(Self::Pdf),
            "mobi" | "azw" | "azw3" => Some(Self::Mobi),
            "m4b" => Some(Self::M4b),
            "mp3folder" => Some(Self::Mp3Folder),
            "fb2" | "fbz" => Some(Self::Fb2),
            "cbz" => Some(Self::Cbz),
            _ => None,
        }
    }

    /// Parses formats accepted for a new user-initiated import.
    pub fn from_import_extension(extension: &str) -> Option<Self> {
        Self::from_extension(extension)
    }

    /// Recognizes compound FB2 ZIP names without accepting arbitrary ZIP files.
    pub fn from_file_name(name: &str) -> Option<Self> {
        if name.to_ascii_lowercase().ends_with(".fb2.zip") {
            return Some(Self::Fb2);
        }
        name.rsplit_once('.').and_then(|(_, extension)| Self::from_extension(extension))
    }

    pub const fn canonical_extension(self) -> &'static str {
        match self {
            Self::Epub => "epub",
            Self::Pdf => "pdf",
            Self::Mobi => "mobi",
            Self::M4b => "m4b",
            Self::Mp3Folder => "mp3folder",
            Self::Fb2 => "fb2",
            Self::Cbz => "cbz",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compound_fb2_names_do_not_admit_arbitrary_zip_archives() {
        for name in ["novel.fb2", "NOVEL.FB2.ZIP", "novel.fbz"] {
            assert_eq!(BookFormat::from_file_name(name), Some(BookFormat::Fb2));
        }
        assert_eq!(BookFormat::from_file_name("comic.CBZ"), Some(BookFormat::Cbz));
        assert_eq!(BookFormat::from_file_name("unrelated.zip"), None);
        for format in [BookFormat::Epub, BookFormat::Pdf, BookFormat::Mobi, BookFormat::M4b, BookFormat::Mp3Folder, BookFormat::Fb2, BookFormat::Cbz] {
            assert_eq!(BookFormat::from_code(format as i64), Some(format));
        }
    }
}
