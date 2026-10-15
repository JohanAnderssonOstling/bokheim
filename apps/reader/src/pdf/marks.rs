//! PDF projection of shared reader annotations.

use book_model::AnnotationStyle;
use library_backend::Annotation;
use pdf_reader_core::NormalizedRect;
use pdf_view_gpui::{PdfAnnotationOverlay, PdfAnnotationStyle};

pub(super) fn annotation_overlays(annotations: &[Annotation]) -> Vec<PdfAnnotationOverlay> {
    annotations
        .iter()
        .filter(|annotation| !annotation.is_external_pdf_annotation())
        .filter_map(|annotation| {
            let anchor = annotation.anchor.pdf_anchor()?;
            let style = match annotation.style {
                AnnotationStyle::Highlight => PdfAnnotationStyle::Highlight,
                AnnotationStyle::Underline => PdfAnnotationStyle::Underline,
                AnnotationStyle::Squiggly => PdfAnnotationStyle::Squiggly,
                AnnotationStyle::Strikethrough => PdfAnnotationStyle::Strikethrough,
            };
            let rects = anchor.rects.iter().map(|rect| NormalizedRect::new(rect.left, rect.top, rect.width, rect.height)).collect();
            Some(PdfAnnotationOverlay { id: annotation.id.clone(), page_index: anchor.page_index as usize, rects, style, color: overlay_color(annotation.color, style) })
        })
        .collect()
}

fn overlay_color(rgb: u32, style: PdfAnnotationStyle) -> u32 {
    let alpha = if style == PdfAnnotationStyle::Highlight { ui_components::READER_ANNOTATION_HIGHLIGHT_ALPHA } else { 0xff };
    (ui_components::reader_annotation_paint_color(rgb) << 8) | u32::from(alpha)
}


pub(super) fn imported_annotations(content_hash: content_address::ContentHash, metadata: Option<&pdf_reader_core::PdfReaderMetadata>) -> Vec<Annotation> {
    use book_model::{AnnotationAnchor, PdfAnnotationAnchor, PdfAnnotationRect};
    use pdf_reader_core::PdfStoredAnnotationKind as Kind;
    let Some(metadata) = metadata.filter(|m| m.valid_for(&m.content_hash)) else { return Vec::new() };
    metadata
        .annotations
        .iter()
        .map(|a| Annotation {
            id: format!("pdf-external:{}:{}:{}", metadata.content_hash, a.page_index, a.index),
            content_hash,
            anchor: AnnotationAnchor::Pdf(PdfAnnotationAnchor::new(a.page_index as u32, a.rects.iter().map(|r| PdfAnnotationRect::new(r[0], r[1], r[2], r[3])).collect(), None)),
            exact_text: a.text.clone(),
            style: match a.kind {
                Kind::Highlight | Kind::Note => AnnotationStyle::Highlight,
                Kind::Underline => AnnotationStyle::Underline,
                Kind::Squiggly => AnnotationStyle::Squiggly,
                Kind::Strikethrough => AnnotationStyle::Strikethrough,
            },
            // The file's colour is `#rrggbb`, checked when the metadata was read.
            color: a.color.strip_prefix('#').and_then(|hex| u32::from_str_radix(hex, 16).ok()).unwrap_or(ui_components::READER_ANNOTATION_DEFAULT_COLOR),
            note: a.note.clone(),
            created_at: 0,
            toc_ordinal: None,
            progress: None,
            modified_at: 0,
            deleted: false,
        })
        .collect()
}
