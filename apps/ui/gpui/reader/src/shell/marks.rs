//! What the reader has marked in a book: highlights and bookmarks.
//!
//! The library owns the durable copy; this is the in-memory mirror kept in
//! step with it. Every reader that can annotate keeps the same mirror, so it
//! lives here rather than beside one of them. How a mark is anchored, and how
//! it becomes something painted over the page, is the reader's own business.

use epub_svelte_lib::library::marks::{ReaderAnnotation, ReaderBookmark};

#[derive(Default)]
pub(crate) struct MarksState {
    pub(crate) annotations: Vec<ReaderAnnotation>,
    pub(crate) bookmarks: Vec<ReaderBookmark>,
    /// The annotation whose note editor is open, if any.
    pub(crate) selected_id: Option<String>,
}

impl MarksState {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use epub_svelte_lib::library::marks::{AnnotationAnchor, AnnotationStyle};

    fn annotation(id: &str) -> ReaderAnnotation {
        ReaderAnnotation {
            id: id.to_owned(),
            content_hash: epub_svelte_lib::ContentHash::new("1"),
            anchor: AnnotationAnchor::epub_cfi("epubcfi(/6/4!/4/2,/1:0,/1:8)".to_owned()),
            exact_text: "a furrow".to_owned(),
            style: AnnotationStyle::Highlight,
            color: "#f6c945".to_owned(),
            note: String::new(),
            created_at: 0,
            modified_at: 0,
        }
    }

    #[test]
    fn deleting_the_open_annotation_closes_its_editor() {
        let mut marks = MarksState { annotations: vec![annotation("a"), annotation("b")], selected_id: Some("a".to_owned()), ..MarksState::default() };

        marks.remove_annotation("a");

        assert_eq!(marks.annotations.len(), 1);
        assert_eq!(marks.selected_id, None, "the editor must not stay open on a deleted annotation");
    }

    #[test]
    fn deleting_another_annotation_leaves_the_editor_open() {
        let mut marks = MarksState { annotations: vec![annotation("a"), annotation("b")], selected_id: Some("a".to_owned()), ..MarksState::default() };

        marks.remove_annotation("b");

        assert_eq!(marks.selected_id.as_deref(), Some("a"));
    }

    #[test]
    fn replacing_a_missing_annotation_adds_nothing() {
        let mut marks = MarksState { annotations: vec![annotation("a")], ..MarksState::default() };

        marks.replace(annotation("gone"));

        assert_eq!(marks.annotations.len(), 1);
    }
}
