//! The EPUB half of marking up a book: the text selection a new highlight
//! would be made from, and the translation from a stored annotation to the
//! overlay the renderer paints.
//!
//! The marks themselves are shared with the other readers and live in
//! [`crate::shell::marks`]. What is here depends on EPUB CFIs, which no other
//! reader has.

use book_model::AnnotationStyle;
use library_backend::Annotation;
use skrift_column_paginator::{AnnotationStyle as RendererAnnotationStyle, RendererAnnotation};

/// Records a new selection, reporting whether it differs from the last one so
/// an unchanged re-report does not repaint.
pub(in crate::epub) fn set_selection(current: &mut Option<String>, selection: String) -> bool {
    if current.as_ref() == Some(&selection) {
        return false;
    }
    *current = Some(selection);
    true
}

/// Overlays for every annotation the renderer can place.
pub(crate) fn overlays(annotations: &[Annotation], book: &crate::book::NativeBook) -> Vec<RendererAnnotation> {
    annotations.iter().filter_map(|annotation| renderer_annotation(annotation, book)).collect()
}

/// Translates a stored annotation into a renderer overlay.
///
/// Returns `None` for annotations anchored to something other than an EPUB CFI
/// — PDF annotations share the same table and must be skipped here.
pub(crate) fn renderer_annotation(annotation: &Annotation, book: &crate::book::NativeBook) -> Option<RendererAnnotation> {
    let cfi_range = book.annotation_cfi(&annotation.anchor)?;
    Some(renderer_annotation_at(annotation, annotation.id.clone(), cfi_range, annotation.exact_text.clone()))
}

pub(crate) fn renderer_annotation_at(annotation: &Annotation, id: String, cfi_range: String, exact_text: String) -> RendererAnnotation {
    let (style, color) = renderer_appearance(annotation);
    RendererAnnotation { id, cfi_range, exact_text, prefix: None, suffix: None, style, color }
}

pub(crate) fn renderer_appearance(annotation: &Annotation) -> (RendererAnnotationStyle, [u8; 4]) {
    let style = match annotation.style {
        AnnotationStyle::Highlight => RendererAnnotationStyle::Highlight,
        AnnotationStyle::Underline => RendererAnnotationStyle::Underline,
        AnnotationStyle::Squiggly => RendererAnnotationStyle::Squiggly,
        AnnotationStyle::Strikethrough => RendererAnnotationStyle::Strikethrough,
    };
    let mut color = highlight_color(annotation.color);
    if annotation.style != AnnotationStyle::Highlight {
        color[3] = 255;
    }
    (style, color)
}

/// An annotation's `0xRRGGBB` as a translucent RGBA fill. Highlights are drawn
/// over text, so they are always translucent.
fn highlight_color(color: u32) -> [u8; 4] {
    let color = ui_components::reader_annotation_paint_color(color);
    [(color >> 16) as u8, (color >> 8) as u8, color as u8, ui_components::READER_ANNOTATION_HIGHLIGHT_ALPHA]
}
