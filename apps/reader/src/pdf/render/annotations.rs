//! PDF annotation loading, editing, navigation, and sidebar presentation.

use ui_text::text;

use crate::invalidation::notify;
use crate::shell::bookmarks::{self, BookmarkTarget};
use crate::shell::marks::BookmarkChange;
use crate::shell::sidebar_list;
use crate::shell::state_panel::{AnnotationSidebarTarget, reader_state_rows, render_annotation_row};
use book_model::{AnnotationAnchor, AnnotationStyle, PdfAnnotationAnchor, PdfAnnotationRect, RecurringScope};
use gpui::prelude::*;
use gpui::{App, Context, Window, div};
use library_backend::Annotation;
use pdf_reader_core::NormalizedRect;
use pdf_view_gpui::PdfSelectionSnapshot;
use std::collections::HashSet;
use ui_components as components;

use super::PdfReaderView;
use crate::invalidation::Component;
use crate::pdf::marks::annotation_overlays;
use crate::shell::annotation_editor;
use crate::shell::annotation_editor::{AnnotationEditorTarget, BookmarkEditorTarget};

impl BookmarkEditorTarget for PdfReaderView {
    fn save_bookmark(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.annotation_note_input.read(cx).value().to_string();
        if self.marks.save_bookmark(name, "save PDF bookmark").is_some() {
            self.close_annotation_editor(cx);
            self.focus_document(window, cx);
        }
    }
}

impl AnnotationEditorTarget for PdfReaderView {
    fn add_annotation_alias(&mut self, id: String, alias: String, cx: &mut Context<Self>) {
        if self.marks.selected_id() != Some(id.as_str()) {
            return;
        }
        self.save_selected_annotation(None, None, cx);
        self.marks.add_selected_alias(&alias, "add PDF annotation alias");
        self.refresh_annotation_overlays(cx);
        notify(cx, Component::PdfShell, "annotation_alias_added");
    }

    fn remove_annotation_alias(&mut self, id: String, alias: String, cx: &mut Context<Self>) {
        if self.marks.selected_id() != Some(id.as_str()) {
            return;
        }
        self.save_selected_annotation(None, None, cx);
        self.marks.remove_selected_alias(&alias, "remove PDF annotation alias");
        self.refresh_annotation_overlays(cx);
        notify(cx, Component::PdfShell, "annotation_alias_removed");
    }

    fn select_annotation_scope(&mut self, scope: Option<RecurringScope>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(passage) = self.recurring.editor_passage.clone().filter(|passage| !passage.text.trim().is_empty()) else { return };
        if self.marks.selected().is_none_or(|annotation| annotation.is_external_pdf_annotation() || annotation.is_bookmark()) {
            return;
        }
        self.save_selected_annotation(None, None, cx);
        let Some(mut annotation) = self.marks.selected().cloned() else { return };
        annotation.book_id = self.locator.book_id();
        let rects = passage.rects.iter().map(|rect| PdfAnnotationRect::new(rect.left(), rect.top(), rect.width(), rect.height())).collect();
        annotation.anchor = AnnotationAnchor::Pdf(PdfAnnotationAnchor::new(passage.page_index as u32, rects, None));
        annotation.exact_text = passage.text;
        annotation.toc_ordinal = self.toc.ordinal_at(passage.page_index).map(|ordinal| ordinal as i64);
        annotation.progress = Some(book_model::pdf_page_progress(passage.page_index, self.page_count));
        let note = self.marks.create_scoped_annotation(annotation, scope, "create PDF annotation for scope");
        self.annotation_note_input.update(cx, |input, cx| input.set_value(note, window, cx));
        self.recurring.editor_overlay = self.marks.selected_id().map(str::to_owned);
        self.refresh_annotation_overlays(cx);
        notify(cx, Component::PdfShell, "annotation_scope_selected");
    }

    fn select_editor_annotation(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.save_selected_annotation(None, None, cx);
        let bookmark = self.marks.annotation(&id).is_some_and(Annotation::is_bookmark);
        if self.recurring.editor_overlay.is_none() {
            self.recurring.editor_overlay = self.marks.selected_id().map(str::to_owned);
        }
        let note = self.marks.select(id);
        self.annotation_note_input.update(cx, |input, cx| {
            input.set_placeholder(if bookmark { text!(reader::name_bookmark) } else { text!(common::add_a_note) }, window, cx);
            input.set_value(note, window, cx);
            if bookmark {
                input.focus(window, cx);
            }
        });
        self.sync_active_annotation(cx);
        notify(cx, Component::PdfShell, "annotation_source_selected");
    }

    fn save_annotation_changes(&mut self, style: Option<AnnotationStyle>, color: Option<u32>, cx: &mut Context<Self>) {
        self.save_selected_annotation(style, color, cx);
    }

    fn dismiss_annotation_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_annotation_editor(cx);
        self.focus_document(window, cx);
    }

    fn delete_edited_annotation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.marks.selected_id().map(str::to_owned) {
            let anchor = self.recurring.editor_overlay.as_deref().unwrap_or(&id);
            let next = self.preferred_annotation_overlay(anchor, Some(&id), cx);
            let popup_anchor = self.annotation_popup_anchor;
            self.delete_annotation(id, cx);
            if let Some(next) = next {
                self.annotation_popup_anchor = popup_anchor;
                self.select_annotation(next, window, cx);
            } else {
                self.close_annotation_editor(cx);
                self.focus_document(window, cx);
            }
        }
    }
}

impl AnnotationSidebarTarget for PdfReaderView {
    fn copy_sidebar_annotation(&mut self, annotation: &Annotation, cx: &mut Context<Self>) {
        crate::shell::annotation_copy::copy_passage(&self.locator, &self.title, None, &annotation.exact_text, &annotation.note, Some(annotation.id.clone()), annotation.anchor.clone(), cx);
    }

    fn select_sidebar_annotation(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.annotation_popup_anchor = None;
        self.select_annotation(id, window, cx);
    }

    fn go_to_annotation(&mut self, annotation: &Annotation, cx: &mut Context<Self>) {
        self.navigate_to_annotation(annotation, cx);
    }

    fn delete_sidebar_annotation(&mut self, id: String, cx: &mut Context<Self>) {
        self.delete_annotation(id, cx);
    }
}

impl PdfReaderView {
    pub(super) fn copy_selection_with_source(&self, cx: &mut Context<Self>) {
        let Some(selection) = self.pdf.read(cx).selection_snapshot() else { return };
        let rects = selection.rects.iter().map(|rect| PdfAnnotationRect::new(rect.left(), rect.top(), rect.width(), rect.height())).collect();
        let anchor = AnnotationAnchor::Pdf(PdfAnnotationAnchor::new(selection.page_index as u32, rects, None));
        crate::shell::annotation_copy::copy_link(&self.locator, &self.title, self.toc.chapter_title_at(selection.page_index), anchor, cx);
    }

    pub(crate) fn follow_annotation_link(&mut self, anchor: PdfAnnotationAnchor, window: &mut Window, cx: &mut Context<Self>) {
        self.open_annotation_anchor(anchor, cx);
        self.focus_document(window, cx);
    }

    pub(crate) fn open_annotation_anchor(&mut self, anchor: PdfAnnotationAnchor, cx: &mut Context<Self>) {
        if self.pdf.read(cx).page_count() == 0 {
            self.pending_open_annotation = Some(anchor);
            return;
        }
        if let Some(rect) = anchor.rects.first() {
            self.pdf.update(cx, |pdf, cx| {
                if rect.width == 0.0 && rect.height == 0.0 {
                    // A bookmark records the viewport's position, rather than
                    // a passage to centre within it.
                    pdf.set_page(anchor.page_index as usize, cx);
                    pdf.set_full_page_position(rect.top, cx);
                } else {
                    pdf.reveal_annotation(anchor.page_index as usize, NormalizedRect::new(rect.left, rect.top, rect.width, rect.height), cx);
                }
            });
        } else {
            self.pdf.update(cx, |pdf, cx| pdf.set_page(anchor.page_index as usize, cx));
        }
    }
    pub(super) fn load_reader_state(&mut self, cx: &mut Context<Self>) {
        let book_id = self.locator.book_id();
        let revision = self.marks.revision();
        self.annotation_load_generation = self.annotation_load_generation.wrapping_add(1);
        let generation = self.annotation_load_generation;
        crate::shell::marks::load_annotations(cx, self.library.clone(), book_id, &self.marks, move |this, cx, (mut annotations, inherited)| {
            if generation != this.annotation_load_generation {
                return;
            }
            annotations.retain(|annotation| annotation.anchor.pdf_anchor().is_some());
            annotations.extend(this.marks.annotations().iter().filter(|annotation| annotation.is_external_pdf_annotation()).cloned());
            if this.marks.refresh_loaded(annotations, inherited, revision) {
                this.refresh_annotation_overlays(cx);
            }
            notify(cx, Component::PdfShell, "marks_loaded");
        });
    }

    pub(super) fn refresh_annotation_overlays(&mut self, cx: &mut Context<Self>) {
        self.recurring.occurrences = if self.marks.recurring_annotations().next().is_none() {
            Vec::new()
        } else {
            self.pdf
                .read(cx)
                .annotation_text_layers()
                .flat_map(|(page, characters)| {
                    let characters = characters.collect::<Vec<_>>();
                    let text = characters.iter().map(|(character, _)| *character).collect::<String>();
                    crate::pdf::recurring::matched_page_occurrences(self.marks.recurring_matches(&text), self.locator.book_id(), page, &characters, |page| self.toc.chapter_at(page))
                })
                .collect()
        };
        let mut overlays = annotation_overlays(self.marks.annotations());
        overlays.extend(self.recurring.occurrences.iter().map(|occurrence| occurrence.overlay.clone()));
        self.pdf.update(cx, |pdf, cx| pdf.set_annotations(overlays, cx));
        self.sync_active_annotation(cx);
    }

    pub(super) fn preferred_annotation_overlay(&self, anchor: &str, exclude: Option<&str>, cx: &App) -> Option<String> {
        let annotation_id = |overlay: &str| self.recurring.occurrences.iter().find(|occurrence| occurrence.overlay.id == overlay).map(|occurrence| occurrence.annotation_id.clone()).unwrap_or_else(|| overlay.to_owned());
        let style = self.marks.annotation(exclude.unwrap_or(&annotation_id(anchor))).map(|annotation| &annotation.style);
        self.pdf.read(cx).overlapping_annotation_ids(anchor).into_iter().chain(std::iter::once(anchor.to_owned()))
            .filter_map(|overlay| {
                let id = annotation_id(&overlay);
                let annotation = self.marks.annotation(&id)?;
                if Some(id.as_str()) == exclude || annotation.is_bookmark() || annotation.is_external_pdf_annotation() { return None; }
                Some(((Some(&annotation.style) != style, crate::shell::recurring::annotation_specificity(annotation)), overlay))
            })
            .min_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1))).map(|(_, overlay)| overlay)
    }

    fn editor_overlay_ids(&self, cx: &App) -> Vec<String> {
        let Some(anchor) = self.recurring.editor_overlay.as_deref().or(self.marks.selected_id()) else { return Vec::new() };
        self.pdf.read(cx).overlapping_annotation_ids(anchor)
    }

    fn editor_annotation_ids(&self, cx: &App) -> Vec<String> {
        self.editor_overlay_ids(cx).into_iter().map(|id| self.recurring.occurrences.iter().find(|occurrence| occurrence.overlay.id == id).map_or(id.clone(), |occurrence| occurrence.annotation_id.clone())).collect()
    }

    fn sync_active_annotation(&self, cx: &mut Context<Self>) {
        let overlapping = self.editor_overlay_ids(cx);
        let active = self.marks.selected().filter(|annotation| !annotation.is_bookmark()).map(|annotation| {
            self.recurring.occurrences.iter().find(|occurrence| occurrence.annotation_id == annotation.id && overlapping.contains(&occurrence.overlay.id)).map_or_else(|| annotation.id.clone(), |occurrence| occurrence.overlay.id.clone())
        });
        self.pdf.update(cx, |pdf, cx| pdf.set_active_annotation(active, cx));
    }

    pub(super) fn watch_annotation_updates(&mut self, cx: &mut Context<Self>) {
        self._annotation_updates = Some(crate::shell::marks::watch_updates(self.library.clone(), Self::load_reader_state, cx));
    }

    /// Creates an annotation and selects it so
    /// its editor is immediately available for an optional note.
    pub(super) fn create_annotation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selection) = self.pdf.read(cx).selection_snapshot() else { return };
        self.create_annotation_from_selection(selection, window, cx);
    }

    pub(super) fn create_annotation_from_selection(&mut self, selection: PdfSelectionSnapshot, window: &mut Window, cx: &mut Context<Self>) {
        if self.marks.selected().is_some() {
            self.save_selected_annotation(None, None, cx);
        }
        self.recurring.editor_overlay = None;
        self.recurring.editor_passage = Some(selection.clone());
        let rects = selection.rects.iter().map(|rect| PdfAnnotationRect::new(rect.left(), rect.top(), rect.width(), rect.height())).collect::<Vec<_>>();
        let anchor = AnnotationAnchor::Pdf(PdfAnnotationAnchor::new(selection.page_index as u32, rects, None));
        let now = crate::shell::marks::now_millis();
        let annotation = Annotation {
            id: uuid::Uuid::new_v4().to_string(),
            book_id: self.locator.book_id(),
            anchor,
            exact_text: selection.text,
            style: AnnotationStyle::Highlight,
            color: ui_components::READER_ANNOTATION_DEFAULT_COLOR,
            note: String::new(),
            created_at: now,
            toc_ordinal: self.toc.ordinal_at(selection.page_index).map(|ordinal| ordinal as i64),
            progress: Some(book_model::pdf_page_progress(selection.page_index, self.page_count)),
            modified_at: now,
            deleted: false,
        };
        let annotation_id = annotation.id.clone();
        self.marks.prepend(annotation, "save PDF annotation");
        let note = self.marks.select(annotation_id.clone());
        self.pdf.update(cx, |pdf, cx| pdf.set_active_annotation(Some(annotation_id), cx));
        self.annotation_note_input.update(cx, |input, cx| {
            input.set_placeholder(text!(common::add_a_note), window, cx);
            input.set_value(note, window, cx);
        });
        self.refresh_annotation_overlays(cx);
        notify(cx, Component::PdfShell, "annotation_created");
    }

    pub(super) fn select_annotation(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.marks.selected().is_some() {
            self.save_selected_annotation(None, None, cx);
        }
        let id = if let Some(occurrence) = self.recurring.occurrences.iter().find(|occurrence| occurrence.overlay.id == id) {
            self.recurring.editor_overlay = Some(id.clone());
            self.recurring.editor_passage = Some(PdfSelectionSnapshot { page_index: occurrence.overlay.page_index, text: occurrence.exact_text.clone(), rects: occurrence.overlay.rects.clone() });
            occurrence.annotation_id.clone()
        } else {
            self.recurring.editor_overlay = None;
            self.recurring.editor_passage = self.marks.annotation(&id).filter(|annotation| annotation.book_id == self.locator.book_id()).and_then(|annotation| {
                let anchor = annotation.anchor.pdf_anchor()?;
                Some(PdfSelectionSnapshot { page_index: anchor.page_index as usize, text: annotation.exact_text.clone(), rects: anchor.rects.iter().map(|rect| NormalizedRect::new(rect.left, rect.top, rect.width, rect.height)).collect() })
            });
            id
        };
        if let Some(annotation) = self.marks.annotation(&id).filter(|annotation| annotation.is_external_pdf_annotation()).cloned() {
            self.marks.clear_selection();
            self.annotation_popup_anchor = None;
            self.pdf.update(cx, |pdf, cx| pdf.set_active_annotation(None, cx));
            self.navigate_to_annotation(&annotation, cx);
            return;
        }
        let bookmark = self.marks.annotation(&id).is_some_and(Annotation::is_bookmark);
        let note = self.marks.select(id.clone());
        self.sync_active_annotation(cx);
        self.annotation_note_input.update(cx, |input, cx| {
            input.set_placeholder(if bookmark { text!(reader::name_bookmark) } else { text!(common::add_a_note) }, window, cx);
            input.set_value(note, window, cx);
            if bookmark {
                input.focus(window, cx);
            }
        });
        notify(cx, Component::PdfShell, "annotation_selected");
    }

    pub(super) fn close_annotation_editor(&mut self, cx: &mut Context<Self>) {
        self.marks.clear_selection();
        self.annotation_popup_anchor = None;
        self.recurring.editor_overlay = None;
        self.recurring.editor_passage = None;
        self.pdf.update(cx, |pdf, cx| pdf.set_active_annotation(None, cx));
        notify(cx, Component::PdfShell, "annotation_editor_closed");
    }

    pub(super) fn save_selected_annotation(&mut self, style: Option<AnnotationStyle>, color: Option<u32>, cx: &mut Context<Self>) {
        if self.marks.selected().is_some_and(Annotation::is_external_pdf_annotation) {
            return;
        }
        let note = self.annotation_note_input.read(cx).value().to_string();
        let Some(annotation) = self.marks.update_selected(note, style, color, "update PDF annotation") else { return };
        if !annotation.is_bookmark() {
            self.refresh_annotation_overlays(cx);
        }
        notify(cx, Component::PdfShell, "annotation_updated");
    }

    pub(in crate::pdf) fn delete_annotation(&mut self, id: String, cx: &mut Context<Self>) {
        if self.marks.annotation(&id).is_some_and(Annotation::is_external_pdf_annotation) {
            return;
        }
        let bookmark = self.marks.annotation(&id).is_some_and(Annotation::is_bookmark);
        if self.marks.delete(id, "delete PDF annotation") {
            self.annotation_popup_anchor = None;
            self.pdf.update(cx, |pdf, cx| pdf.set_active_annotation(None, cx));
        }
        if !bookmark {
            self.refresh_annotation_overlays(cx);
        }
        notify(cx, Component::PdfShell, "annotation_deleted");
    }

    fn navigate_to_annotation(&mut self, annotation: &Annotation, cx: &mut Context<Self>) {
        let Some(anchor) = annotation.anchor.pdf_anchor() else { return };
        self.open_annotation_anchor(anchor.clone(), cx);
    }

    pub(super) fn render_annotations_body(&mut self, theme: components::BrowserTheme, cx: &mut Context<Self>) -> gpui::AnyElement {
        let rows = reader_state_rows(self.marks.annotations(), text!(reader::no_notes_yet_select_pdf_text_then_choose_annotation_to_add_a), |annotation: &Annotation| {
            let page = annotation.anchor.pdf_anchor().map(|anchor| anchor.page_index as usize);
            if let Some(page) = page
                && let Some(title) = self.toc.chapter_title_at(page)
            {
                return (format!("chapter:{}", self.toc.chapter_at(page)), title.to_owned());
            }
            if let Some(page) = page { (format!("page:{page}"), format!(text!(reader::page), page + 1)) } else { ("other".to_owned(), text!(reader::other_notes).to_owned()) }
        });
        let selected = self.marks.selected_id();
        let selected_row = selected.and_then(|id| rows.iter().position(|row| row.annotation_id() == Some(id)));
        self.annotation_list.set_rows(rows);
        self.annotation_list.reveal_selected(selected, selected_row);
        sidebar_list::panel("pdf-annotation-list", &cx.entity(), &self.annotation_list.state, theme, |this, row, target, theme, _| {
            let Some(row) = this.annotation_list.rows.get(row) else { return div().into_any_element() };
            render_annotation_row(target, row, this.marks.annotations(), this.marks.selected_id(), "", theme)
        })
    }

    pub(super) fn render_annotation_popup(&self, annotation: Annotation, geometry: annotation_editor::Geometry, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        if annotation.is_bookmark() {
            return annotation_editor::bookmark_editor(&cx.entity(), geometry, &self.annotation_note_input, components::browser_theme(cx), cx);
        }
        if annotation.is_external_pdf_annotation() {
            return annotation_editor::standard_editor("pdf-popup", &cx.entity(), &annotation, geometry, &self.annotation_note_input, components::browser_theme(cx), window, cx);
        }
        let passage = self.recurring.editor_passage.as_ref();
        let text = passage.map_or(annotation.exact_text.as_str(), |passage| passage.text.as_str());
        let matching_ids = self.marks.recurring_matches(text).map(|(annotation, _)| annotation.id.as_str()).collect::<HashSet<_>>();
        let overlapping_ids = self.editor_annotation_ids(cx);
        let mut local_ids = self
            .marks
            .annotations()
            .iter()
            .filter(|candidate| {
                candidate.id == annotation.id
                    || overlapping_ids.contains(&candidate.id)
                    || candidate.anchor.pdf_anchor().is_some_and(|anchor| {
                        passage.is_some_and(|passage| {
                            anchor.page_index as usize == passage.page_index
                                && anchor.rects.len() == passage.rects.len()
                                && anchor.rects.iter().zip(&passage.rects).all(|(origin, rect)| {
                                    (origin.left - rect.left()).abs() < 0.00001 && (origin.top - rect.top()).abs() < 0.00001 && (origin.width - rect.width()).abs() < 0.00001 && (origin.height - rect.height()).abs() < 0.00001
                                })
                        })
                    })
                    || (candidate.anchor.recurring_scope().is_some()
                        && matching_ids.contains(candidate.id.as_str())
                        && (candidate.anchor.recurring_scope() != Some(RecurringScope::Chapter)
                            || candidate.anchor.pdf_anchor().is_some_and(|anchor| passage.is_some_and(|passage| self.toc.chapter_at(anchor.page_index as usize) == self.toc.chapter_at(passage.page_index)))))
            })
            .map(|candidate| candidate.id.clone())
            .collect::<Vec<_>>();
        local_ids.extend(overlapping_ids);
        let context = self.marks.editor_context(self.locator.book_id(), text, &local_ids, self.toc.has_entries());
        annotation_editor::recurring_editor("pdf-popup", &cx.entity(), &annotation, context, geometry, &self.annotation_note_input, components::browser_theme(cx), window, cx)
    }
}

impl BookmarkTarget for PdfReaderView {
    fn filter_bookmarks(&mut self, cx: &mut Context<Self>) {
        self.bookmark_list.filter(&self.bookmark_filter_input.read(cx).value(), self.marks.selected_id());
    }

    fn create_bookmark(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pdf.read(cx).page_count() == 0 || !self.toc.is_loaded() {
            return;
        }
        self.save_selected_annotation(None, None, cx);
        self.close_annotation_editor(cx);
        let position = self.pdf.read(cx).source_position();
        let page = position.page_index();
        let anchor = AnnotationAnchor::Pdf(PdfAnnotationAnchor::new(page as u32, vec![PdfAnnotationRect::new(0.0, position.page_y(), 0.0, 0.0)], None));
        let bookmark = bookmarks::bookmark(self.locator.book_id(), anchor, self.toc.bookmark_ordinal_at(page), book_model::pdf_page_progress(page, self.page_count));
        self.marks.start_bookmark(bookmark);
        self.annotation_note_input.update(cx, |input, cx| {
            input.set_placeholder(text!(reader::name_bookmark), window, cx);
            input.set_value("", window, cx);
            input.focus(window, cx);
        });
        if components::uses_mobile_reader_controls(window) {
            self.sidebar.close(cx);
        }
        cx.notify();
    }
}

impl PdfReaderView {
    pub(super) fn update_bookmarks(&mut self, change: BookmarkChange, cx: &mut Context<Self>) {
        match change {
            BookmarkChange::Contents => self.refresh_bookmark_rows(cx),
            BookmarkChange::Selection => self.bookmark_list.select(self.marks.selected_id()),
        }
    }

    pub(super) fn refresh_bookmark_rows(&mut self, cx: &mut Context<Self>) {
        let bookmarks = self.marks.bookmarks();
        let groups = reader_state_rows(bookmarks, text!(reader::no_bookmarks_yet), |bookmark| {
            let page = bookmark.anchor.pdf_anchor().map_or(0, |anchor| anchor.page_index as usize);
            let chapter = bookmark.toc_ordinal.and_then(|ordinal| usize::try_from(ordinal).ok()).and_then(|ordinal| self.toc.chapter_for_ordinal(ordinal));
            if let Some((start, title)) = chapter { (format!("chapter:{start}"), title.to_owned()) } else { (format!("page:{page}"), format!(text!(reader::page), page + 1)) }
        });
        self.bookmark_list.replace(groups, bookmarks, &self.bookmark_filter_input.read(cx).value(), self.marks.selected_id());
    }

    pub(super) fn render_bookmarks_body(&self, theme: components::BrowserTheme, cx: &mut Context<Self>) -> gpui::AnyElement {
        let rows = sidebar_list::panel("pdf-bookmark-list", &cx.entity(), &self.bookmark_list.list.state, theme, |this, row, target, theme, _| {
            let Some(row) = this.bookmark_list.list.rows.get(row) else { return div().into_any_element() };
            render_annotation_row(target, row, this.marks.bookmarks(), this.marks.selected_id(), "", theme)
        });
        div().size_full().min_h_0().min_w_0().flex().flex_col().child(bookmarks::filter_bar(&self.bookmark_filter_input, theme)).child(rows).into_any_element()
    }
}
