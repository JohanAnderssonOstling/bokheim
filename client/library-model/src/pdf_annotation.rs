//! One projection of a stored foreign PDF mark into the annotation model.
use book_model::{parse_annotation_color, Annotation, AnnotationAnchor, AnnotationStyle, BookId, PdfAnnotationAnchor, PdfAnnotationRect};
use pdf_view_common::{PdfStoredAnnotation, PdfStoredAnnotationKind};

/// Invalid colors are rejected instead of acquiring a different fallback in
/// the reader, notebook, and export. Metadata is validated before projection.
pub fn stored_pdf_annotation(book: BookId, value: &PdfStoredAnnotation) -> Option<Annotation> {
    value.color.strip_prefix('#').filter(|hex| hex.len() == 6)?;
    let color = parse_annotation_color(&value.color).ok().flatten()?;
    Some(Annotation {
        id: format!("pdf-external:{book}:{}:{}", value.page_index, value.index),
        content_hash: book,
        anchor: AnnotationAnchor::Pdf(PdfAnnotationAnchor::new(value.page_index as u32, value.rects.iter().map(|r| PdfAnnotationRect::new(r[0], r[1], r[2], r[3])).collect(), None)),
        exact_text: value.text.clone(),
        style: match value.kind {
            PdfStoredAnnotationKind::Underline => AnnotationStyle::Underline,
            PdfStoredAnnotationKind::Squiggly => AnnotationStyle::Squiggly,
            PdfStoredAnnotationKind::Strikethrough => AnnotationStyle::Strikethrough,
            _ => AnnotationStyle::Highlight,
        },
        color,
        note: value.note.clone(),
        created_at: 0,
        modified_at: 0,
        toc_ordinal: None,
        progress: None,
        deleted: false,
    })
}
