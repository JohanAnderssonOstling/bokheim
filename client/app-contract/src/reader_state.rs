use library_model::{AnnotationState, AnnotationStyle as SyncAnnotationStyle, BookmarkState};
use sync_common::ContentHash;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize, specta::Type)]
pub struct PdfAnnotationRect {
    #[specta(type = specta_typescript::Number)]
    pub left: f32,
    #[specta(type = specta_typescript::Number)]
    pub top: f32,
    #[specta(type = specta_typescript::Number)]
    pub width: f32,
    #[specta(type = specta_typescript::Number)]
    pub height: f32,
}

impl PdfAnnotationRect {
    pub const fn new(left: f32, top: f32, width: f32, height: f32) -> Self {
        Self { left, top, width, height }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, specta::Type)]
pub struct PdfAnnotationAnchor {
    #[specta(type = specta_typescript::Number)]
    pub page_index: u32,
    pub rects: Vec<PdfAnnotationRect>,
    pub fallback_cfi: Option<String>,
}

impl PdfAnnotationAnchor {
    pub fn new(page_index: u32, rects: Vec<PdfAnnotationRect>, fallback_cfi: Option<String>) -> Self {
        Self { page_index, rects, fallback_cfi }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AnnotationAnchor {
    EpubCfi {
        cfi: String,
    },
    Pdf {
        #[specta(type = specta_typescript::Number)]
        page_index: u32,
        rects: Vec<PdfAnnotationRect>,
        fallback_cfi: Option<String>,
    },
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

impl From<AnnotationAnchor> for library_model::AnnotationAnchor {
    fn from(anchor: AnnotationAnchor) -> Self {
        match anchor {
            AnnotationAnchor::EpubCfi { cfi } => Self::EpubCfi { cfi },
            AnnotationAnchor::Pdf { page_index, rects, fallback_cfi } => {
                Self::Pdf { page_index, rects: rects.into_iter().map(|rect| library_model::PdfAnnotationRect::new(rect.left, rect.top, rect.width, rect.height)).collect(), fallback_cfi }
            }
        }
    }
}

impl From<library_model::AnnotationAnchor> for AnnotationAnchor {
    fn from(anchor: library_model::AnnotationAnchor) -> Self {
        match anchor {
            library_model::AnnotationAnchor::EpubCfi { cfi } => Self::EpubCfi { cfi },
            library_model::AnnotationAnchor::Pdf { page_index, rects, fallback_cfi } => {
                Self::Pdf { page_index, rects: rects.into_iter().map(|rect| PdfAnnotationRect::new(rect.left, rect.top, rect.width, rect.height)).collect(), fallback_cfi }
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationStyle {
    Highlight,
    Underline,
    Squiggly,
    Strikethrough,
}

use serde::{Deserialize, Serialize};

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
    fn sync(&self) -> SyncAnnotationStyle {
        match self {
            Self::Highlight => SyncAnnotationStyle::Highlight,
            Self::Underline => SyncAnnotationStyle::Underline,
            Self::Squiggly => SyncAnnotationStyle::Squiggly,
            Self::Strikethrough => SyncAnnotationStyle::Strikethrough,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, specta::Type)]
pub struct ReaderAnnotation {
    pub id: String,
    #[specta(type = String)]
    pub content_hash: ContentHash,
    pub anchor: AnnotationAnchor,
    pub exact_text: String,
    pub style: AnnotationStyle,
    pub color: String,
    pub note: String,
    #[specta(type = specta_typescript::Number)]
    pub created_at: i64,
    #[specta(type = specta_typescript::Number)]
    pub modified_at: i64,
}

pub const EXTERNAL_PDF_ANNOTATION_ID_PREFIX: &str = "pdf-external:";

#[derive(Clone, Debug, Deserialize, Serialize, specta::Type)]
pub struct ReaderBookmark {
    pub id: String,
    #[specta(type = String)]
    pub content_hash: ContentHash,
    pub cfi: String,
    pub label: String,
    #[specta(type = specta_typescript::Number)]
    pub created_at: i64,
    #[specta(type = specta_typescript::Number)]
    pub modified_at: i64,
}

impl ReaderAnnotation {
    pub fn is_external_pdf_annotation(&self) -> bool {
        self.id.starts_with(EXTERNAL_PDF_ANNOTATION_ID_PREFIX)
    }

    pub fn pdf_anchor(&self) -> Option<PdfAnnotationAnchor> {
        self.anchor.pdf_anchor()
    }

    pub fn validate(&self, deleted: bool) -> Result<(), Box<dyn std::error::Error>> {
        self.to_sync_state(deleted).map(|_| ())
    }

    pub fn to_sync_state(&self, deleted: bool) -> Result<AnnotationState, Box<dyn std::error::Error>> {
        Ok(AnnotationState {
            content_hash: self.content_hash.clone(),
            anchor: self.anchor.clone().into(),
            exact_text: self.exact_text.clone(),
            style: self.style.sync(),
            color: self.color.clone(),
            note: self.note.clone(),
            created_at: u64::try_from(self.created_at)?,
            modified_at: u64::try_from(self.modified_at)?,
            deleted,
        })
    }
}

impl ReaderBookmark {
    pub fn validate(&self, deleted: bool) -> Result<(), Box<dyn std::error::Error>> {
        let _ = BookmarkState { content_hash: self.content_hash.clone(), cfi: self.cfi.clone(), label: self.label.clone(), created_at: u64::try_from(self.created_at)?, modified_at: u64::try_from(self.modified_at)?, deleted };
        Ok(())
    }
}
