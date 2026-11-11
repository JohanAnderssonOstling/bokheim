//! Ingestion output for a book identity and its PDF byte revision, independent
//! of a renderer.
#[path = "metadata/transport.rs"]
mod transport;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PdfReaderMetadata {
    pub version: u32,
    #[serde(rename = "content_hash")]
    pub book_id: String,
    #[serde(with = "transport::pages")]
    pub pages: Vec<PdfStoredPage>,
    /// Legacy document-wide annotations; reader rendering discovers one page at a time.
    #[serde(default)]
    pub annotations: Vec<PdfStoredAnnotation>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PdfStoredPage {
    /// Display dimensions, with the page's intrinsic rotation applied.
    pub width: f32,
    pub height: f32,
    pub rotation: u16,
}

impl PdfReaderMetadata {
    pub fn valid_for(&self, book_id: &str) -> bool {
        self.version == 1
            && self.book_id == book_id
            && (book_id.len() == 64 || (book_id.len() == 66 && (book_id.starts_with("01") || book_id.starts_with("02") || book_id.starts_with("03"))))
            && book_id.bytes().all(|c| c.is_ascii_hexdigit())
            && !self.pages.is_empty()
            && self.pages.len() <= 32767
            && self.pages.iter().all(|p| p.width.is_finite() && p.height.is_finite() && p.width > 0.0 && p.height > 0.0 && matches!(p.rotation, 0 | 90 | 180 | 270))
            && self.annotations.len() <= 16384
            && self.annotations.iter().all(|a| a.valid(self.pages.len()))
            && serde_json::to_vec(self).is_ok_and(|bytes| bytes.len() <= 7 * 512 * 1024)
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PdfStoredAnnotation {
    pub page_index: usize,
    pub index: usize,
    pub kind: PdfStoredAnnotationKind,
    /// Rectangles in the same normalized display coordinates as text selection.
    pub rects: Vec<[f32; 4]>,
    pub color: String,
    pub text: String,
    pub note: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PdfStoredAnnotationKind {
    Highlight,
    Underline,
    Squiggly,
    Strikethrough,
    Note,
}

impl PdfStoredAnnotation {
    fn valid(&self, pages: usize) -> bool {
        self.page_index < pages
            && self.rects.len() <= 4096
            && !self.rects.is_empty()
            && self.rects.iter().all(|r| r.iter().all(|v| v.is_finite()) && r[0] >= 0.0 && r[1] >= 0.0 && r[2] > 0.0 && r[3] > 0.0 && r[0] + r[2] <= 1.0001 && r[1] + r[3] <= 1.0001)
            && self.text.len() <= 16384
            && self.note.len() <= 16384
            && self.color.len() == 7
            && self.color.starts_with('#')
            && self.color[1..].bytes().all(|c| c.is_ascii_hexdigit())
    }
}

impl PdfReaderMetadata {
    /// Keep each synchronized snapshot below the mutation envelope limit.
    pub fn bound_optional_data(&mut self) {
        let mut remaining = 512 * 1024usize;
        self.annotations.truncate(16384);
        self.annotations.retain(|a| {
            let size = serde_json::to_vec(a).map_or(usize::MAX, |bytes| bytes.len());
            if size > remaining {
                false
            } else {
                remaining -= size;
                true
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn large_notes_stay_syncable() {
        let book_id = "a".repeat(64);
        let mut metadata: PdfReaderMetadata = serde_json::from_value(serde_json::json!({
            "version":1,"content_hash":book_id,"pages":[{"width":600,"height":800,"rotation":0}],"annotations":[]
        }))
        .unwrap();
        assert!(metadata.valid_for(&book_id));
        let note = PdfStoredAnnotation { page_index: 0, index: 0, kind: PdfStoredAnnotationKind::Note, rects: vec![[0.1, 0.1, 0.2, 0.1]], color: "#abcdef".into(), text: String::new(), note: "\n".repeat(16384) };
        metadata.annotations = vec![note; 200];
        assert!(!metadata.valid_for(&book_id));
        metadata.bound_optional_data();
        assert!(!metadata.annotations.is_empty());
        assert!(metadata.valid_for(&book_id));
        assert!(serde_json::to_vec(&metadata).unwrap().len() < 4 * 1024 * 1024);
    }
}
