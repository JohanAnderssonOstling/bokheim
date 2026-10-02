use std::error::Error;
use crate::{AnnotationDetail, AnnotationState, PdfAnnotationAnchor};
use content_address::ContentHash;
use serde::{Deserialize, Serialize};

/// A reader annotation adds its identity to the canonical annotation state.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ReaderAnnotation {
    pub id: String,
    #[serde(flatten)]
    pub state: AnnotationState,
}
impl std::ops::Deref for ReaderAnnotation {
    type Target = AnnotationState;
    fn deref(&self) -> &Self::Target {
        &self.state
    }
}
impl std::ops::DerefMut for ReaderAnnotation {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.state
    }
}

pub const EXTERNAL_PDF_ANNOTATION_ID_PREFIX: &str = "pdf-external:";
impl ReaderAnnotation {
    pub fn is_external_pdf_annotation(&self) -> bool {
        self.id.starts_with(EXTERNAL_PDF_ANNOTATION_ID_PREFIX)
    }
    pub fn pdf_anchor(&self) -> Option<PdfAnnotationAnchor> {
        self.anchor.pdf_anchor()
    }
    pub fn validate(&self, _: bool) -> Result<(), Box<dyn Error + Send + Sync>> {
        self.state.validate()
    }
    pub fn detail_json(&self) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
        Ok(self.state.detail_json()?)
    }
    /// The row's cached positioning fields remain authoritative for ordering.
    pub fn from_row(id: String, content_hash: ContentHash, toc_ordinal: Option<i64>, progress: Option<f32>, modified_at: i64, detail: &[u8]) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let mut detail = AnnotationDetail::decode(detail)?;
        detail.toc_ordinal = toc_ordinal;
        detail.progress = progress;
        Ok(Self { id, state: AnnotationState { content_hash, detail, modified_at: modified_at.max(0) as u64, deleted: false } })
    }
}
