use crate::{ContentHash, UnixMillis};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationStyle {
    Highlight,
    Underline,
    Squiggly,
    Strikethrough,
}

impl AnnotationStyle {
    pub const fn storage(&self) -> &'static str {
        match self {
            Self::Highlight => "highlight",
            Self::Underline => "underline",
            Self::Squiggly => "squiggly",
            Self::Strikethrough => "strikethrough",
        }
    }

    pub fn parse_storage(value: &str) -> Option<Self> {
        match value {
            "highlight" => Some(Self::Highlight),
            "underline" => Some(Self::Underline),
            "squiggly" => Some(Self::Squiggly),
            "strikethrough" => Some(Self::Strikethrough),
            _ => None,
        }
    }
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

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AnnotationAnchor {
    EpubCfi { cfi: String },
    Pdf { page_index: u32, rects: Vec<PdfAnnotationRect>, fallback_cfi: Option<String> },
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "AnnotationAnchor")]
enum AnnotationAnchorBinary {
    EpubCfi { cfi: String },
    Pdf { page_index: u32, rects: Vec<PdfAnnotationRect>, fallback_cfi: Option<String> },
}

mod annotation_anchor_binary {
    use super::{AnnotationAnchor, AnnotationAnchorBinary};
    pub fn serialize<S: serde::Serializer>(value: &AnnotationAnchor, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            serde::Serialize::serialize(value, serializer)
        } else {
            AnnotationAnchorBinary::serialize(value, serializer)
        }
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<AnnotationAnchor, D::Error> {
        if deserializer.is_human_readable() {
            serde::Deserialize::deserialize(deserializer)
        } else {
            AnnotationAnchorBinary::deserialize(deserializer)
        }
    }
}

impl AnnotationAnchor {
    pub fn epub_cfi(cfi: impl Into<String>) -> Self {
        Self::EpubCfi { cfi: cfi.into() }
    }
    pub fn pdf(anchor: PdfAnnotationAnchor) -> Self {
        Self::Pdf { page_index: anchor.page_index, rects: anchor.rects, fallback_cfi: anchor.fallback_cfi }
    }
    pub fn epub_cfi_value(&self) -> Option<&str> {
        match self {
            Self::EpubCfi { cfi } => Some(cfi),
            Self::Pdf { fallback_cfi, .. } => fallback_cfi.as_deref(),
        }
    }
    pub fn pdf_anchor(&self) -> Option<PdfAnnotationAnchor> {
        match self {
            Self::EpubCfi { .. } => None,
            Self::Pdf { page_index, rects, fallback_cfi } => Some(PdfAnnotationAnchor::new(*page_index, rects.clone(), fallback_cfi.clone())),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnnotationState {
    pub content_hash: ContentHash,
    #[serde(with = "annotation_anchor_binary")]
    pub anchor: AnnotationAnchor,
    pub exact_text: String,
    pub style: AnnotationStyle,
    pub color: String,
    pub note: String,
    pub created_at: UnixMillis,
    pub modified_at: UnixMillis,
    pub deleted: bool,
}
