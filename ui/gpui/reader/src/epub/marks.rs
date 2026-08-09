//! What the reader has marked in a book: highlights, bookmarks, and the text
//! selection a new highlight would be made from.
//!
//! The library owns the durable copy; this is the in-memory mirror kept in
//! step with it, plus the translation from a stored annotation to the overlay
//! the renderer paints.

use epub_svelte_lib::library::marks::{ReaderAnnotation, ReaderBookmark};
use html_view_core::{AnnotationStyle as RendererAnnotationStyle, RendererAnnotation};

/// Highlight fill used when an annotation's colour cannot be parsed.
const DEFAULT_HIGHLIGHT: [u8; 4] = [246, 201, 69, HIGHLIGHT_ALPHA];

/// Highlights are drawn over text, so they are always translucent.
const HIGHLIGHT_ALPHA: u8 = 96;

/// The text the reader has selected, with enough context to anchor an
/// annotation to it if they choose to highlight.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Selection {
    pub(crate) cfi_range: String,
    pub(crate) text: String,
}

#[derive(Default)]
pub(crate) struct MarksState {
    pub(crate) annotations: Vec<ReaderAnnotation>,
    pub(crate) bookmarks: Vec<ReaderBookmark>,
    pub(crate) selection: Option<Selection>,
    /// The annotation whose note editor is open, if any.
    pub(crate) selected_id: Option<String>,
}

impl MarksState {
    /// Records a new selection, reporting whether it differs from the last one
    /// so an unchanged re-report does not repaint.
    pub(crate) fn set_selection(&mut self, selection: Selection) -> bool {
        if self.selection.as_ref() == Some(&selection) {
            return false;
        }
        self.selection = Some(selection);
        true
    }

    pub(crate) fn clear_selection(&mut self) {
        self.selection = None;
    }

    pub(crate) fn annotation(&self, id: &str) -> Option<&ReaderAnnotation> {
        self.annotations.iter().find(|annotation| annotation.id == id)
    }

    /// Replaces a stored annotation in place, or does nothing if it is gone.
    pub(crate) fn replace(&mut self, annotation: ReaderAnnotation) {
        if let Some(existing) = self.annotations.iter_mut().find(|item| item.id == annotation.id) {
            *existing = annotation;
        }
    }

    /// Forgets an annotation, also closing its editor if it was open.
    pub(crate) fn remove_annotation(&mut self, id: &str) {
        self.annotations.retain(|item| item.id != id);
        if self.selected_id.as_deref() == Some(id) {
            self.selected_id = None;
        }
    }

    pub(crate) fn remove_bookmark(&mut self, id: &str) {
        self.bookmarks.retain(|item| item.id != id);
    }

    /// Overlays for every annotation the renderer can place.
    pub(crate) fn overlays(&self) -> Vec<RendererAnnotation> {
        self.annotations.iter().filter_map(renderer_annotation).collect()
    }
}

/// Translates a stored annotation into a renderer overlay.
///
/// Returns `None` for annotations anchored to something other than an EPUB CFI
/// — PDF annotations share the same table and must be skipped here.
pub(crate) fn renderer_annotation(annotation: &ReaderAnnotation) -> Option<RendererAnnotation> {
    let cfi_range = annotation.anchor.epub_cfi_value()?.to_owned();
    let style = match annotation.style {
        epub_svelte_lib::library::marks::AnnotationStyle::Highlight => RendererAnnotationStyle::Highlight,
        epub_svelte_lib::library::marks::AnnotationStyle::Underline => RendererAnnotationStyle::Underline,
        epub_svelte_lib::library::marks::AnnotationStyle::Squiggly => RendererAnnotationStyle::Squiggly,
        epub_svelte_lib::library::marks::AnnotationStyle::Strikethrough => RendererAnnotationStyle::Strikethrough,
    };
    Some(RendererAnnotation { id: annotation.id.clone(), cfi_range, exact_text: annotation.exact_text.clone(), prefix: None, suffix: None, style, color: highlight_color(&annotation.color) })
}

/// Parses a `#rgb` or `#rrggbb` colour into a translucent RGBA fill, falling
/// back to the default highlight for anything unparseable.
fn highlight_color(color: &str) -> [u8; 4] {
    let Some(hex) = color.strip_prefix('#') else { return DEFAULT_HIGHLIGHT };
    let Ok(value) = u32::from_str_radix(hex, 16) else { return DEFAULT_HIGHLIGHT };
    let (red, green, blue) = match hex.len() {
        // CSS shorthand: each digit is doubled, so #fff is white, not #000fff.
        3 => (((value >> 8) & 0xf) * 0x11, ((value >> 4) & 0xf) * 0x11, (value & 0xf) * 0x11),
        6 => ((value >> 16) & 0xff, (value >> 8) & 0xff, value & 0xff),
        _ => return DEFAULT_HIGHLIGHT,
    };
    [red as u8, green as u8, blue as u8, HIGHLIGHT_ALPHA]
}

#[cfg(test)]
mod tests {
    use super::*;
    use epub_svelte_lib::library::marks::{AnnotationAnchor, AnnotationStyle};

    fn annotation(id: &str, color: &str) -> ReaderAnnotation {
        ReaderAnnotation {
            id: id.to_owned(),
            content_hash: "1".to_owned(),
            anchor: AnnotationAnchor::epub_cfi("epubcfi(/6/4!/4/2,/1:0,/1:8)".to_owned()),
            exact_text: "a furrow".to_owned(),
            style: AnnotationStyle::Highlight,
            color: color.to_owned(),
            note: String::new(),
            created_at: 0,
            modified_at: 0,
        }
    }

    fn selection(text: &str) -> Selection {
        Selection { cfi_range: "epubcfi(/6/4!/4/2,/1:0,/1:8)".to_owned(), text: text.to_owned() }
    }

    #[test]
    fn re_reporting_the_same_selection_does_not_repaint() {
        let mut marks = MarksState::default();
        assert!(marks.set_selection(selection("a furrow")));
        assert!(!marks.set_selection(selection("a furrow")));
        assert!(marks.set_selection(selection("a deeper furrow")));
    }

    #[test]
    fn deleting_the_open_annotation_closes_its_editor() {
        let mut marks = MarksState { annotations: vec![annotation("a", "#f6c945"), annotation("b", "#f6c945")], selected_id: Some("a".to_owned()), ..MarksState::default() };

        marks.remove_annotation("a");

        assert_eq!(marks.annotations.len(), 1);
        assert_eq!(marks.selected_id, None, "the editor must not stay open on a deleted annotation");
    }

    #[test]
    fn deleting_another_annotation_leaves_the_editor_open() {
        let mut marks = MarksState { annotations: vec![annotation("a", "#f6c945"), annotation("b", "#f6c945")], selected_id: Some("a".to_owned()), ..MarksState::default() };

        marks.remove_annotation("b");

        assert_eq!(marks.selected_id.as_deref(), Some("a"));
    }

    #[test]
    fn replacing_a_missing_annotation_adds_nothing() {
        let mut marks = MarksState { annotations: vec![annotation("a", "#f6c945")], ..MarksState::default() };

        marks.replace(annotation("gone", "#f6c945"));

        assert_eq!(marks.annotations.len(), 1);
    }

    #[test]
    fn a_valid_colour_becomes_a_translucent_fill() {
        assert_eq!(highlight_color("#ff8000"), [0xff, 0x80, 0x00, HIGHLIGHT_ALPHA]);
    }

    #[test]
    fn css_shorthand_expands_rather_than_being_read_as_a_dark_colour() {
        assert_eq!(highlight_color("#fff"), [0xff, 0xff, 0xff, HIGHLIGHT_ALPHA]);
        assert_eq!(highlight_color("#f80"), [0xff, 0x88, 0x00, HIGHLIGHT_ALPHA]);
    }

    #[test]
    fn unparseable_colours_fall_back_to_the_default_highlight() {
        assert_eq!(highlight_color("f6c945"), DEFAULT_HIGHLIGHT, "a missing # must not be reinterpreted");
        assert_eq!(highlight_color("#zzz"), DEFAULT_HIGHLIGHT);
        assert_eq!(highlight_color("#ff"), DEFAULT_HIGHLIGHT, "a length that is neither shorthand nor full must not be reinterpreted");
    }

    #[test]
    fn overlays_skip_annotations_the_renderer_cannot_place() {
        let mut pdf_annotation = annotation("pdf", "#f6c945");
        let rects = vec![epub_svelte_lib::library::marks::PdfAnnotationRect { left: 0.1, top: 0.1, width: 0.2, height: 0.05 }];
        let anchor = epub_svelte_lib::library::marks::PdfAnnotationAnchor::new(0, rects, None);
        pdf_annotation.anchor = AnnotationAnchor::pdf(anchor);
        let marks = MarksState { annotations: vec![annotation("epub", "#f6c945"), pdf_annotation], ..MarksState::default() };

        let overlays = marks.overlays();

        assert_eq!(overlays.len(), 1, "a PDF annotation shares the table but has no EPUB CFI");
        assert_eq!(overlays[0].id, "epub");
    }
}
