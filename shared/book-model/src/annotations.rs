use crate::UnixMillis;
use content_address::ContentHash;
use serde::{Deserialize, Serialize};
use wire::WireError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationStyle {
    Highlight,
    Underline,
    Squiggly,
    Strikethrough,
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
    EpubCfi { cfi: String },
    Pdf(PdfAnnotationAnchor),
}

impl AnnotationAnchor {
    pub fn epub_cfi(cfi: impl Into<String>) -> Self {
        Self::EpubCfi { cfi: cfi.into() }
    }
    pub fn epub_cfi_value(&self) -> Option<&str> {
        match self {
            Self::EpubCfi { cfi } => Some(cfi),
            Self::Pdf(anchor) => anchor.fallback_cfi.as_deref(),
        }
    }
    pub fn pdf_anchor(&self) -> Option<&PdfAnnotationAnchor> {
        match self {
            Self::EpubCfi { .. } => None,
            Self::Pdf(anchor) => Some(anchor),
        }
    }
}

/// A reader annotation: what is marked, how it looks, and its sync state.
/// The same value is synchronized, stored as the database row's blob, and
/// handed to the readers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Annotation {
    pub id: String,
    pub content_hash: ContentHash,
    pub anchor: AnnotationAnchor,
    pub exact_text: String,
    pub style: AnnotationStyle,
    /// `0xRRGGBB`.
    pub color: u32,
    pub note: String,
    pub created_at: UnixMillis,
    pub toc_ordinal: Option<i64>,
    pub progress: Option<f32>,
    pub modified_at: UnixMillis,
    pub deleted: bool,
}

pub const EXTERNAL_PDF_ANNOTATION_ID_PREFIX: &str = "pdf-external:";

impl Annotation {
    pub fn encode(&self) -> Result<Vec<u8>, WireError> {
        wire::encode(self)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, WireError> {
        wire::decode(bytes, wire::MAX_DECODED_REQUEST_BYTES)
    }

    pub fn is_external_pdf_annotation(&self) -> bool {
        self.id.starts_with(EXTERNAL_PDF_ANNOTATION_ID_PREFIX)
    }

    pub fn validate(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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
