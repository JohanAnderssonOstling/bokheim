pub use domain::{AnnotationAnchor, PdfAnnotationAnchor, PdfAnnotationRect};
use domain::{AnnotationState, AnnotationStyle as SyncAnnotationStyle, BookmarkState};
use sync_common::ContentHash;

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
    pub content_hash: String,
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

#[derive(Clone, Debug, Deserialize, Serialize, specta::Type)]
pub struct ReaderBookmark {
    pub id: String,
    pub content_hash: String,
    pub cfi: String,
    pub label: String,
    #[specta(type = specta_typescript::Number)]
    pub created_at: i64,
    #[specta(type = specta_typescript::Number)]
    pub modified_at: i64,
}

impl ReaderAnnotation {
    pub fn pdf_anchor(&self) -> Option<PdfAnnotationAnchor> {
        self.anchor.pdf_anchor()
    }

    pub fn validate(&self, deleted: bool) -> Result<(), Box<dyn std::error::Error>> {
        self.to_sync_state(deleted).map(|_| ())
    }

    pub fn to_sync_state(&self, deleted: bool) -> Result<AnnotationState, Box<dyn std::error::Error>> {
        Ok(AnnotationState {
            content_hash: ContentHash::new(&self.content_hash.clone()),
            anchor: self.anchor.clone(),
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
        let _ = BookmarkState {
            content_hash: ContentHash::new(&self.content_hash.clone()),
            cfi: self.cfi.clone(),
            label: self.label.clone(),
            created_at: u64::try_from(self.created_at)?,
            modified_at: u64::try_from(self.modified_at)?,
            deleted,
        };
        Ok(())
    }
}
