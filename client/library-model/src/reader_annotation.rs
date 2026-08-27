use serde::{Deserialize, Serialize};

use crate::{AnnotationAnchor, AnnotationState, AnnotationStyle, ContentHash, PdfAnnotationAnchor};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ReaderAnnotation {
    pub id: String,
    pub content_hash: ContentHash,
    pub anchor: AnnotationAnchor,
    pub exact_text: String,
    pub style: AnnotationStyle,
    pub color: String,
    pub note: String,
    pub created_at: i64,
    pub modified_at: i64,
}

pub const EXTERNAL_PDF_ANNOTATION_ID_PREFIX: &str = "pdf-external:";

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
            anchor: self.anchor.clone(),
            exact_text: self.exact_text.clone(),
            style: self.style.clone(),
            color: self.color.clone(),
            note: self.note.clone(),
            created_at: u64::try_from(self.created_at)?,
            modified_at: u64::try_from(self.modified_at)?,
            deleted,
        })
    }
}
