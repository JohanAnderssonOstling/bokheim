//! EPUB annotation loading, editing, and renderer projection.

use ui_text::text;

use crate::invalidation::notify;
use book_model::{AnnotationAnchor, AnnotationStyle};
use gpui::{Context, Window};
use library_backend::Annotation;
use skrift_column_paginator::RendererCommand;

use crate::epub::{ReaderView, Selection, marks};
use crate::invalidation::Component;

impl ReaderView {
    pub(crate) fn open_annotation_anchor(&mut self, anchor: &AnnotationAnchor, window: &mut Window, cx: &mut Context<Self>) -> Result<(), String> {
        if !matches!(self.load_state, crate::epub::LoadState::Ready(_)) {
            return Err(text!(reader::this_reader_could_not_load_the_book_reopen_the_book_to_follow).to_owned());
        }
        let cfi = self.book.annotation_cfi(anchor).ok_or_else(|| text!(reader::the_linked_passage_could_not_be_located_in_this_book).to_owned())?;
        self.page_turn = None;
        self.swipe_preview.clear();
        self.navigate_to_cfi(cfi, cx);
        #[cfg(all(any(feature = "native-tts", target_arch = "wasm32"), not(feature = "kobo")))]
        self.tts_navigation_changed(cx);
        self.focus_document(window, cx);
        Ok(())
    }

    pub(super) fn load_reader_state(&mut self, cx: &mut Context<Self>) {
        let book_id = self.locator.book_id();
        let revision = self.marks.revision();
        self.annotation_load_generation = self.annotation_load_generation.wrapping_add(1);
        let generation = self.annotation_load_generation;
        crate::shell::marks::load_annotations(cx, self.library.clone(), book_id, &self.marks, move |this, cx, (mut annotations, inherited)| {
            if this.annotation_load_generation != generation {
                return;
            }
            for annotation in &mut annotations {
                if let Some(anchor) = this.book.migrate_annotation_anchor(&annotation.anchor)
                    && let (Some(legacy_cfi), Some(publication_cfi)) = (annotation.anchor.epub_cfi_value(), anchor.epub_cfi_value())
                {
                    this.marks.persist_migration(annotation.id.clone(), legacy_cfi.to_owned(), publication_cfi.to_owned());
                    annotation.anchor = anchor;
                }
            }
            // Preserve optimistic edits made while the backend snapshot was
            // in flight, including deletions that snapshot still contains.
            if this.marks.refresh_loaded(annotations, inherited, revision) {
                this.refresh_recurring_annotations(false, cx);
            }
            notify(cx, Component::ReaderShell, "marks_loaded");
        });
    }

    pub(super) fn watch_annotation_updates(&mut self, cx: &mut Context<Self>) {
        self._annotation_updates = Some(crate::shell::marks::watch_updates(self.library.clone(), Self::load_reader_state, cx));
    }

    pub(in crate::epub) fn create_annotation(&mut self, selection: Selection, window: &mut Window, cx: &mut Context<Self>) {
        let Selection { cfi_range, text: exact_text } = selection;
        self.selection = None;
        if self.marks.selected().is_some() {
            self.save_selected_annotation(None, None, cx);
        }
        let toc_ordinal = self.toc.ordinal_for_cfi(&cfi_range).or_else(|| self.toc.ordinal_of_active()).map(|ordinal| ordinal as i64);
        self.annotation_note_input.update(cx, |input, cx| {
            input.set_placeholder(text!(common::add_a_note), window, cx);
            input.set_value(String::new(), window, cx);
        });
        let now = crate::shell::marks::now_millis();
        let annotation = Annotation {
            id: uuid::Uuid::new_v4().to_string(),
            book_id: self.locator.book_id(),
            anchor: self.book.annotation_anchor(cfi_range),
            exact_text,
            style: AnnotationStyle::Highlight,
            color: ui_components::READER_ANNOTATION_DEFAULT_COLOR,
            note: String::new(),
            created_at: now,
            toc_ordinal,
            progress: Some(book_model::layout_fraction_progress(self.progress.fraction)),
            modified_at: now,
            deleted: false,
        };
        let Some(overlay) = marks::renderer_annotation(&annotation, &self.book) else {
            log::error!("Could not create EPUB annotation overlay");
            return;
        };
        let annotation_id = annotation.id.clone();
        self.recurring.editor_overlay = None;
        self.recurring.editor_passage = Some(Selection { cfi_range: overlay.cfi_range.clone(), text: overlay.exact_text.clone() });
        self.marks.append(annotation, "save reader annotation");
        self.marks.select(annotation_id);
        self.with_renderer(cx, |renderer, cx| renderer.apply(RendererCommand::UpsertAnnotation(overlay), cx));
        notify(cx, Component::ReaderShell, "annotation_created");
    }

    pub(in crate::epub) fn select_annotation(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.marks.selected().is_some() {
            self.save_selected_annotation(None, None, cx);
        }
        let id = if let Some(occurrence) = self.recurring.occurrences.iter().find(|occurrence| occurrence.overlay.id == id) {
            self.recurring.editor_overlay = Some(id);
            self.recurring.editor_passage = Some(Selection { cfi_range: occurrence.overlay.cfi_range.clone(), text: occurrence.overlay.exact_text.clone() });
            occurrence.annotation_id.clone()
        } else {
            self.recurring.editor_overlay = None;
            self.recurring.editor_passage = self
                .marks
                .annotation(&id)
                .filter(|annotation| annotation.book_id == self.locator.book_id())
                .and_then(|annotation| Some(Selection { cfi_range: self.book.annotation_cfi(&annotation.anchor)?, text: annotation.exact_text.clone() }));
            id
        };
        let bookmark = self.marks.annotation(&id).is_some_and(Annotation::is_bookmark);
        let note = self.marks.select(id);
        self.annotation_note_input.update(cx, |input, cx| {
            input.set_placeholder(if bookmark { text!(reader::name_bookmark) } else { text!(common::add_a_note) }, window, cx);
            input.set_value(note, window, cx);
            if bookmark {
                input.focus(window, cx);
            }
        });
        notify(cx, Component::ReaderShell, "annotation_selected");
    }

    pub(in crate::epub) fn close_annotation_popup(&mut self, cx: &mut Context<Self>) {
        if self.marks.clear_selection() {
            self.annotation_popup_anchor = None;
            self.recurring.editor_overlay = None;
            self.recurring.editor_passage = None;
            notify(cx, Component::ReaderShell, "annotation_closed");
        }
    }

    pub(in crate::epub) fn navigate_to_cfi(&self, cfi: String, cx: &mut Context<Self>) {
        self.with_renderer(cx, |renderer, cx| renderer.apply(RendererCommand::NavigateToCfi(cfi), cx));
    }

    /// Writes the open annotation back while leaving the editor open.
    pub(in crate::epub) fn save_selected_annotation(&mut self, style: Option<AnnotationStyle>, color: Option<u32>, cx: &mut Context<Self>) {
        let note = self.annotation_note_input.read(cx).value().to_string();
        let Some(annotation) = self.marks.update_selected(note, style, color, "update reader annotation") else { return };
        if !annotation.is_bookmark() {
            self.project_annotations(cx);
        }
        notify(cx, Component::ReaderShell, "annotation_updated");
    }

    pub(in crate::epub) fn delete_annotation(&mut self, id: String, cx: &mut Context<Self>) {
        let bookmark = self.marks.annotation(&id).is_some_and(Annotation::is_bookmark);
        self.marks.delete(id, "delete reader annotation");
        self.annotation_popup_anchor = None;
        if !bookmark {
            self.refresh_recurring_annotations(false, cx);
        }
        notify(cx, Component::ReaderShell, "annotation_deleted");
    }
}
