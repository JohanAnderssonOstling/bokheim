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
            let rects = anchor.rects.iter().map(|rect| NormalizedRect::new(rect.left, rect.top, rect.width, rect.height)).collect();
            Some(annotation_overlay_at(annotation, annotation.id.clone(), anchor.page_index as usize, rects))
        })
        .collect()
}

pub(super) fn annotation_overlay_at(annotation: &Annotation, id: String, page_index: usize, rects: Vec<NormalizedRect>) -> PdfAnnotationOverlay {
    let style = match annotation.style {
        AnnotationStyle::Highlight => PdfAnnotationStyle::Highlight,
        AnnotationStyle::Underline => PdfAnnotationStyle::Underline,
        AnnotationStyle::Squiggly => PdfAnnotationStyle::Squiggly,
        AnnotationStyle::Strikethrough => PdfAnnotationStyle::Strikethrough,
    };
    PdfAnnotationOverlay { id, page_index, rects, style, color: overlay_color(annotation.color, style) }
}

fn overlay_color(rgb: u32, style: PdfAnnotationStyle) -> u32 {
    let alpha = if style == PdfAnnotationStyle::Highlight { ui_components::READER_ANNOTATION_HIGHLIGHT_ALPHA } else { 0xff };
    (ui_components::reader_annotation_paint_color(rgb) << 8) | u32::from(alpha)
}
