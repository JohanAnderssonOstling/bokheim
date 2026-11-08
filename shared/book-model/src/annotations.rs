use ui_text::text;

use crate::{PUBLICATION_CFI_PREFIX, UnixMillis};
use content_address::BookId;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use wire::WireError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationStyle {
    Highlight,
    Underline,
    Squiggly,
    Strikethrough,
}

/// Parse the opaque CSS color subset used by reader annotation files. Unknown
/// spellings return None; malformed numeric components return their parse error.
pub fn parse_annotation_color(value: &str) -> Result<Option<u32>, std::num::ParseIntError> {
    let named = match value {
        "yellow" => Some(0xffff00),
        "orange" => Some(0xffa500),
        "red" => Some(0xff0000),
        "magenta" | "fuchsia" => Some(0xff00ff),
        "aqua" | "cyan" => Some(0x00ffff),
        "lime" => Some(0x00ff00),
        "blue" => Some(0x0000ff),
        "green" => Some(0x008000),
        "black" => Some(0),
        "white" => Some(0xffffff),
        _ => None,
    };
    if named.is_some() {
        return Ok(named);
    }
    if let Some(hex) = value.strip_prefix('#') {
        let hex = if hex.len() == 3 { hex.chars().flat_map(|c| [c, c]).collect::<String>() } else { hex.into() };
        if hex.len() == 6 {
            return u32::from_str_radix(&hex, 16).map(Some);
        }
    }
    if let Some(rgb) = value.strip_prefix("rgb(").or_else(|| value.strip_prefix("rgba(")).and_then(|value| value.strip_suffix(')')) {
        let components = rgb.split(',').map(str::trim).collect::<Vec<_>>();
        if components.len() == 3 || (components.len() == 4 && components[3] == "1") {
            let mut color = 0;
            for component in &components[..3] {
                color = (color << 8) | component.parse::<u8>()? as u32;
            }
            return Ok(Some(color));
        }
    }
    Ok(None)
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct PdfAnnotationRect {
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}

impl PdfAnnotationRect {
    pub const fn new(left: f32, top: f32, width: f32, height: f32) -> Self {
        Self { left, top, width, height }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PdfAnnotationAnchor {
    pub page_index: u32,
    pub rects: Vec<PdfAnnotationRect>,
    pub fallback_cfi: Option<String>,
}

impl PdfAnnotationAnchor {
    pub fn new(page_index: u32, rects: Vec<PdfAnnotationRect>, fallback_cfi: Option<String>) -> Self {
        Self { page_index, rects, fallback_cfi }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum AnnotationAnchor {
    EpubCfi {
        cfi: String,
    },
    Pdf(PdfAnnotationAnchor),
    /// An annotation that repeats on matching text, retaining its creation passage.
    /// Appending a variant keeps existing persisted annotation layouts intact.
    Recurring {
        origin: Box<AnnotationAnchor>,
        scope: RecurringScope,
    },
    /// Appended to preserve the binary layout of annotations without aliases.
    RecurringAliases {
        origin: Box<AnnotationAnchor>,
        scope: RecurringScope,
        aliases: Vec<String>,
    },
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum RecurringScope {
    Chapter,
    Book,
    Library,
}

impl RecurringScope {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Chapter => text!(audiobook::chapter),
            Self::Book => text!(reader::book),
            Self::Library => text!(common::library),
        }
    }
}

impl AnnotationAnchor {
    pub fn origin(&self) -> &Self {
        match self {
            Self::Recurring { origin, .. } | Self::RecurringAliases { origin, .. } => origin.origin(),
            _ => self,
        }
    }

    pub fn recurring_scope(&self) -> Option<RecurringScope> {
        match self {
            Self::Recurring { scope, .. } | Self::RecurringAliases { scope, .. } => Some(*scope),
            _ => None,
        }
    }

    pub fn recurring_aliases(&self) -> &[String] {
        match self {
            Self::RecurringAliases { aliases, .. } => aliases,
            _ => &[],
        }
    }

    pub fn with_recurring_aliases(&self, aliases: Vec<String>) -> Self {
        match self.recurring_scope() {
            Some(scope) if !aliases.is_empty() => Self::RecurringAliases { origin: Box::new(self.origin().clone()), scope, aliases },
            Some(scope) => Self::Recurring { origin: Box::new(self.origin().clone()), scope },
            None => self.clone(),
        }
    }

    /// Changing scope never changes the creation passage.
    pub fn with_recurring_scope(&self, scope: Option<RecurringScope>) -> Self {
        let origin = self.origin().clone();
        match scope {
            Some(scope) if !self.recurring_aliases().is_empty() => Self::RecurringAliases { origin: Box::new(origin), scope, aliases: self.recurring_aliases().to_vec() },
            Some(scope) => Self::Recurring { origin: Box::new(origin), scope },
            None => origin,
        }
    }

    pub fn with_origin(&self, origin: Self) -> Self {
        origin.with_recurring_scope(self.recurring_scope()).with_recurring_aliases(self.recurring_aliases().to_vec())
    }

    /// An explicit reader-opening destination. Keeps publication-CFI versioning
    /// and PDF rectangles intact across the browser/reader boundary.
    pub fn open_target(&self) -> Option<String> {
        serde_json::to_string(self.origin()).ok().map(|body| format!("annotation-location:{body}"))
    }

    pub fn from_open_target(target: &str) -> Option<Self> {
        serde_json::from_str(target.strip_prefix("annotation-location:")?).ok()
    }

    pub fn epub_publication_cfi(cfi: impl Into<String>) -> Self {
        // Distinguish publication addresses from document-relative locations.
        Self::EpubCfi { cfi: format!("{PUBLICATION_CFI_PREFIX}{}", cfi.into()) }
    }
    pub fn is_epub_publication_cfi(&self) -> bool {
        matches!(self.origin(), Self::EpubCfi { cfi } if cfi.starts_with(PUBLICATION_CFI_PREFIX))
    }
    pub fn epub_cfi(cfi: impl Into<String>) -> Self {
        Self::EpubCfi { cfi: cfi.into() }
    }
    pub fn epub_cfi_value(&self) -> Option<&str> {
        match self {
            Self::EpubCfi { cfi } => Some(cfi.strip_prefix(PUBLICATION_CFI_PREFIX).unwrap_or(cfi)),
            Self::Pdf(anchor) => anchor.fallback_cfi.as_deref(),
            Self::Recurring { origin, .. } | Self::RecurringAliases { origin, .. } => origin.epub_cfi_value(),
        }
    }
    pub fn pdf_anchor(&self) -> Option<&PdfAnnotationAnchor> {
        match self {
            Self::EpubCfi { .. } => None,
            Self::Pdf(anchor) => Some(anchor),
            Self::Recurring { origin, .. } | Self::RecurringAliases { origin, .. } => origin.pdf_anchor(),
        }
    }
}

/// A reader annotation: what is marked, how it looks, and its sync state.
/// The same value is synchronized, stored as the database row's blob, and
/// handed to the readers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Annotation {
    pub id: String,
    #[serde(rename = "content_hash")]
    pub book_id: BookId,
    pub anchor: AnnotationAnchor,
    pub exact_text: String,
    pub style: AnnotationStyle,
    /// `0xRRGGBB`.
    pub color: u32,
    pub note: String,
    pub created_at: UnixMillis,
    /// Required for bookmarks; optional for annotations.
    /// For bookmarks in a book without a TOC, zero denotes the whole book.
    pub toc_ordinal: Option<i64>,
    pub progress: Option<f32>,
    pub modified_at: UnixMillis,
    pub deleted: bool,
}

/// Bookmarks share the versioned mark register so existing databases and sync
/// relays preserve them without a wire-format or schema migration. The name is
/// stored in `note`; the anchor is a point and `exact_text` is empty.
pub const BOOKMARK_ID_PREFIX: &str = "bookmark:";

pub const EXTERNAL_PDF_ANNOTATION_ID_PREFIX: &str = "pdf-external:";

impl Annotation {
    pub fn recurring_phrases(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.exact_text.as_str()).chain(self.anchor.recurring_aliases().iter().map(String::as_str))
    }

    pub fn encode(&self) -> Result<Vec<u8>, WireError> {
        wire::encode(self)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, WireError> {
        wire::decode(bytes, wire::MAX_DECODED_REQUEST_BYTES)
    }

    pub fn is_bookmark(&self) -> bool {
        self.id.starts_with(BOOKMARK_ID_PREFIX)
    }

    pub fn is_external_pdf_annotation(&self) -> bool {
        self.id.starts_with(EXTERNAL_PDF_ANNOTATION_ID_PREFIX)
    }

    pub fn validate(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let AnnotationAnchor::Recurring { origin, .. } | AnnotationAnchor::RecurringAliases { origin, .. } = &self.anchor {
            if origin.recurring_scope().is_some() || self.exact_text.trim().is_empty() {
                return Err("recurring annotations require text and a single creation anchor".into());
            }
            let mut phrases = HashSet::from([self.exact_text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()]);
            for alias in self.anchor.recurring_aliases() {
                if alias.is_empty() || alias.contains('\0') || alias.split_whitespace().collect::<Vec<_>>().join(" ") != *alias || !phrases.insert(alias.to_lowercase()) {
                    return Err("recurring aliases must be distinct, non-empty names or phrases".into());
                }
            }
        }
        if self.created_at > i64::MAX as u64 || self.modified_at > i64::MAX as u64 {
            return Err("annotation timestamps exceed the database range".into());
        }
        if self.toc_ordinal.is_some_and(|ordinal| ordinal < 0) {
            return Err("annotation TOC ordinal must be non-negative".into());
        }
        if self.progress.is_some_and(|progress| !progress.is_finite() || !(0.0..=100.0).contains(&progress)) {
            return Err("annotation progress must be finite and between 0 and 100".into());
        }
        if self.color > 0xff_ffff {
            return Err("annotation colour must be 0xRRGGBB".into());
        }
        Ok(())
    }
}
