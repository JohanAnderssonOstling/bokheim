use serde::{Deserialize, Serialize};

/// The reading surface a supported book format requires.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BookKind {
    Text,
    Pdf,
    Audio,
    Comic,
}

struct MediaType {
    value: &'static str,
    acquisition_priority: Option<u8>,
}
struct FileType {
    format: BookFormat,
    extensions: &'static [&'static str],
    media_types: &'static [MediaType],
}

// Stored bits, extensions, MIME aliases and capabilities come from formats.json.
include!(concat!(env!("OUT_DIR"), "/book_formats.rs"));

impl BookFormat {
    pub fn from_extension(extension: &str) -> Option<Self> {
        FILE_TYPES.iter().find(|file| file.extensions.iter().any(|candidate| extension.eq_ignore_ascii_case(candidate))).map(|file| file.format)
    }

    pub fn from_import_extension(extension: &str) -> Option<Self> {
        Self::from_extension(extension)
    }

    /// Compound extensions are explicit entries; a generic ZIP is not a book.
    pub fn from_file_name(name: &str) -> Option<Self> {
        let lower = name.to_ascii_lowercase();
        FILE_TYPES.iter().find(|file| file.extensions.iter().any(|extension| lower.strip_suffix(extension).is_some_and(|prefix| prefix.ends_with('.')))).map(|file| file.format)
    }

    pub fn browser_file_name(name: &str) -> Option<Self> {
        Self::from_file_name(name).filter(|format| format.browser_download())
    }

    pub fn from_media_type(value: &str) -> Option<Self> {
        let bare = value.split(';').next()?.trim();
        FILE_TYPES.iter().find(|file| file.media_types.iter().any(|media| bare.eq_ignore_ascii_case(media.value))).map(|file| file.format)
    }

    pub fn extension_for_media_type(value: &str) -> Option<&'static str> {
        let bare = value.split(';').next()?.trim();
        FILE_TYPES.iter().find(|file| file.media_types.iter().any(|media| bare.eq_ignore_ascii_case(media.value))).map(|file| file.extensions[0])
    }

    /// An acquisition MIME type with the common preference order, if feeds
    /// offer this kind. Browser download support alone does not imply this.
    pub fn acquisition_media_type(value: &str) -> Option<(u8, &'static str)> {
        let bare = value.split(';').next()?.trim();
        FILE_TYPES.iter().flat_map(|file| file.media_types).find(|media| bare.eq_ignore_ascii_case(media.value)).and_then(|media| media.acquisition_priority.map(|priority| (priority, media.value)))
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
        for format in BookFormat::ALL {
            assert_eq!(BookFormat::from_code(format as i64), Some(format));
        }
    }
}
