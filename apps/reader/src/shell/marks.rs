//! What the reader has annotated in a book.
//!
//! The library owns the durable copy; this is the in-memory mirror kept in
//! step with it. Every reader that can annotate keeps the same mirror, so it
//! lives here rather than beside one of them. How a mark is anchored, and how
//! it becomes something painted over the page, is the reader's own business.

use std::collections::HashSet;
use std::rc::Rc;

use book_model::AnnotationStyle;
use gpui::App;
use library_backend::Annotation;
use library_backend::LibraryClient;

use crate::shell::persistence::MutationWriter;

#[derive(Default)]
struct MarksState {
    annotations: Vec<Annotation>,
    /// The annotation whose note editor is open, if any.
    selected_id: Option<String>,
    deleted_annotation_ids: HashSet<String>,
}

impl MarksState {
    fn annotation(&self, id: &str) -> Option<&Annotation> {
        self.annotations.iter().find(|annotation| annotation.id == id)
    }

    /// Replaces a stored annotation in place, or does nothing if it is gone.
    fn replace(&mut self, annotation: Annotation) {
        if let Some(existing) = self.annotations.iter_mut().find(|item| item.id == annotation.id) {
            *existing = annotation;
        }
    }

    /// Merges an asynchronous load into marks already changed optimistically.
    /// Local entries win duplicate ids, while local deletions must not be
    /// resurrected by a load that started before the deletion.
    fn merge_loaded(&mut self, annotations: Vec<Annotation>) {
        let local_annotation_ids = self.annotations.iter().map(|annotation| annotation.id.clone()).collect::<HashSet<_>>();
        self.annotations.extend(annotations.into_iter().filter(|annotation| !local_annotation_ids.contains(annotation.id.as_str()) && !self.deleted_annotation_ids.contains(&annotation.id)));
    }

    /// Forgets an annotation, also closing its editor if it was open.
    fn remove_annotation(&mut self, id: &str) -> bool {
        let closed_editor = self.selected_id.as_deref() == Some(id);
        self.deleted_annotation_ids.insert(id.to_owned());
        self.annotations.retain(|item| item.id != id);
        if closed_editor {
            self.selected_id = None;
        }
        closed_editor
    }
}

/// Annotation state plus its serialized durable mutation queue.
///
/// Readers still decide how annotations are anchored and painted, but the
/// optimistic in-memory mutation and matching backend write happen together.
pub(crate) struct MarkSession {
    state: MarksState,
    writes: MutationWriter,
}

impl MarkSession {
    pub(crate) fn new(cx: &App, library: Rc<LibraryClient>) -> Self {
        Self { state: MarksState::default(), writes: MutationWriter::new(cx, library) }
    }

    pub(crate) fn merge_loaded(&mut self, annotations: Vec<Annotation>) {
        self.state.merge_loaded(annotations);
    }

    pub(crate) fn annotations(&self) -> &[Annotation] {
        &self.state.annotations
    }

    pub(crate) fn annotation(&self, id: &str) -> Option<&Annotation> {
        self.state.annotation(id)
    }

    pub(crate) fn selected(&self) -> Option<&Annotation> {
        self.state.selected_id.as_deref().and_then(|id| self.state.annotation(id))
    }

    pub(crate) fn selected_id(&self) -> Option<&str> {
        self.state.selected_id.as_deref()
    }

    /// Selects an annotation and returns the note the UI should edit.
    pub(crate) fn select(&mut self, id: String) -> String {
        let note = self.state.annotation(&id).map(|annotation| annotation.note.clone()).unwrap_or_default();
        self.state.selected_id = Some(id);
        note
    }

    pub(crate) fn clear_selection(&mut self) -> bool {
        self.state.selected_id.take().is_some()
    }

    pub(crate) fn append(&mut self, annotation: Annotation, what: &'static str) {
        self.state.annotations.push(annotation.clone());
        self.persist(annotation, what);
    }

    pub(crate) fn prepend(&mut self, annotation: Annotation, what: &'static str) {
        self.state.annotations.insert(0, annotation.clone());
        self.persist(annotation, what);
    }

    fn update(&mut self, annotation: Annotation, what: &'static str) {
        self.state.replace(annotation.clone());
        self.persist(annotation, what);
    }

    /// Updates and persists the annotation currently selected by the UI.
    pub(crate) fn update_selected(&mut self, note: String, style: Option<AnnotationStyle>, color: Option<u32>, what: &'static str) -> Option<Annotation> {
        let id = self.state.selected_id.clone()?;
        let mut annotation = self.state.annotation(&id)?.clone();
        annotation.note = note;
        if let Some(style) = style {
            annotation.style = style;
        }
        if let Some(color) = color {
            annotation.color = color;
        }
        annotation.modified_at = now_millis();
        self.update(annotation.clone(), what);
        Some(annotation)
    }

    /// Deletes an annotation and reports whether its editor was closed.
    pub(crate) fn delete(&mut self, id: String, what: &'static str) -> bool {
        let closed_editor = self.state.remove_annotation(&id);
        let modified_at = now_millis() as i64;
        self.writes.record(what, move |library| {
            let library = library.clone();
            Box::pin(async move { library.delete_annotation(id, modified_at).await })
        });
        closed_editor
    }

    fn persist(&self, annotation: Annotation, what: &'static str) {
        self.writes.record(what, move |library| {
            let library = library.clone();
            Box::pin(async move { library.upsert_annotation(annotation).await })
        });
    }
}

pub(crate) fn now_millis() -> u64 {
    // Deletions use signed database timestamps; keep both representations in
    // range, including on devices whose clocks have not been initialized.
    web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|duration| duration.as_millis().min(i64::MAX as u128) as u64).unwrap_or(0)
}
