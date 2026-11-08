//! EPUB annotation, footnote, and image overlays.

use ui_text::text;

use crate::invalidation::notify;
use book_model::{AnnotationStyle, RecurringScope};
use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity, IntoElement, Window, px};
use std::collections::HashSet;
use ui_components as components;

use crate::epub::{LoadState, ReaderView, recurring};
use crate::invalidation::Component;
use crate::shell::annotation_editor::{AnnotationEditorTarget, BookmarkEditorTarget};
use crate::shell::{annotation_editor, chrome};

impl BookmarkEditorTarget for ReaderView {
    fn save_bookmark(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.annotation_note_input.read(cx).value().to_string();
        if self.marks.save_bookmark(name, "save reader bookmark").is_some() {
            self.close_annotation_popup(cx);
            self.focus_document(window, cx);
        }
    }
}

impl AnnotationEditorTarget for ReaderView {
    fn add_annotation_alias(&mut self, id: String, alias: String, cx: &mut Context<Self>) {
        if self.marks.selected_id() != Some(id.as_str()) {
            return;
        }
        self.save_selected_annotation(None, None, cx);
        self.marks.add_selected_alias(&alias, "add reader annotation alias");
        self.refresh_recurring_annotations(false, cx);
        notify(cx, Component::ReaderShell, "annotation_alias_added");
    }

    fn remove_annotation_alias(&mut self, id: String, alias: String, cx: &mut Context<Self>) {
        if self.marks.selected_id() != Some(id.as_str()) {
            return;
        }
        self.save_selected_annotation(None, None, cx);
        self.marks.remove_selected_alias(&alias, "remove reader annotation alias");
        self.refresh_recurring_annotations(false, cx);
        notify(cx, Component::ReaderShell, "annotation_alias_removed");
    }

    fn select_annotation_scope(&mut self, scope: Option<RecurringScope>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(passage) = self.recurring.editor_passage.clone().filter(|passage| !passage.text.trim().is_empty()) else { return };
        if self.marks.selected().is_none_or(|annotation| annotation.is_bookmark()) {
            return;
        }
        self.save_selected_annotation(None, None, cx);
        let Some(mut annotation) = self.marks.selected().cloned() else { return };
        if self.recurring.editor_overlay.is_none() {
            self.recurring.editor_overlay = Some(annotation.id.clone());
        }
        annotation.book_id = self.locator.book_id();
        annotation.anchor = self.book.annotation_anchor(passage.cfi_range.clone());
        annotation.exact_text = passage.text;
        annotation.toc_ordinal = self.toc.ordinal_for_cfi(&passage.cfi_range).map(|ordinal| ordinal as i64);
        annotation.progress = Some(book_model::layout_fraction_progress(self.progress.fraction));
        let note = self.marks.create_scoped_annotation(annotation, scope, "create reader annotation for scope");
        self.annotation_note_input.update(cx, |input, cx| input.set_value(note, window, cx));
        self.refresh_recurring_annotations(false, cx);
        notify(cx, Component::ReaderShell, "annotation_scope_selected");
    }

    fn select_editor_annotation(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.save_selected_annotation(None, None, cx);
        if self.recurring.editor_overlay.is_none() {
            self.recurring.editor_overlay = self.marks.selected_id().map(str::to_owned);
        }
        let note = self.marks.select(id);
        self.annotation_note_input.update(cx, |input, cx| input.set_value(note, window, cx));
        self.project_annotations(cx);
        notify(cx, Component::ReaderShell, "annotation_source_selected");
    }

    fn save_annotation_changes(&mut self, style: Option<AnnotationStyle>, color: Option<u32>, cx: &mut Context<Self>) {
        self.save_selected_annotation(style, color, cx);
    }

    fn dismiss_annotation_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_annotation_popup(cx);
        self.focus_document(window, cx);
    }

    fn delete_edited_annotation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.marks.selected_id().map(str::to_owned) {
            self.delete_annotation(id, cx);
            self.focus_document(window, cx);
        }
    }
}

impl ReaderView {
    pub(super) fn render_annotation_popup(&self, entity: &Entity<Self>, sidebar_visible: bool, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let annotation = self.marks.selected()?.clone();
        let sidebar_width = chrome::sidebar_width(sidebar_visible, window, cx);
        let viewport = window.viewport_size();
        let column_count = match &self.load_state {
            LoadState::Ready(renderer) => renderer.read(cx).layout_geometry().map_or(1, |geometry| usize::from(geometry.column_count)),
            LoadState::Loading | LoadState::Error(_) => 1,
        };
        // The passage itself, so the panel can be placed beside it rather than
        // over it. The annotation is the better source once the renderer has
        // resolved it onto the page; the live selection covers the moment
        // between marking a passage and that resolution; the pointer is the
        // last resort, and only clears its own line.
        let passage = match &self.load_state {
            LoadState::Ready(renderer) => {
                let view = renderer.read(cx);
                view.annotation_bounds(self.recurring.editor_overlay.as_deref().unwrap_or(&annotation.id)).or_else(|| view.selection_bounds()).map(annotation_editor::Passage::from_bounds)
            }
            LoadState::Loading | LoadState::Error(_) => None,
        }
        .or_else(|| self.annotation_popup_anchor.map(annotation_editor::Passage::from_point));
        let mobile = components::uses_mobile_reader_controls(window);
        #[cfg(all(any(feature = "native-tts", target_arch = "wasm32"), not(feature = "kobo")))]
        let dock_height = if mobile && self.tts.dock_open() { components::player_dock_height_px(window) } else { 0.0 };
        #[cfg(not(all(any(feature = "native-tts", target_arch = "wasm32"), not(feature = "kobo"))))]
        let dock_height = 0.0;
        let mut insets = window.insets().effective();
        insets.bottom = (insets.bottom - px(dock_height)).max(px(0.0));
        let toolbar_height = if mobile { 0.0 } else { components::reader_toolbar_size_px(window) };
        let geometry = annotation_editor::geometry(passage, f32::from(viewport.width), f32::from(viewport.height) - dock_height, sidebar_width, toolbar_height, column_count.max(1), insets);
        if annotation.is_bookmark() {
            return Some(annotation_editor::bookmark_editor(entity, geometry, &self.annotation_note_input, components::browser_theme(cx), cx));
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
                    || passage.is_some_and(|passage| self.book.annotation_cfi(&candidate.anchor).as_deref() == Some(passage.cfi_range.as_str()))
                    || (matching_ids.contains(candidate.id.as_str()) && passage.is_some_and(|passage| recurring::occurrence_applies(candidate, self.locator.book_id(), &self.book, &self.toc, &passage.cfi_range)))
            })
            .map(|candidate| candidate.id.clone())
            .collect::<Vec<_>>();
        local_ids.extend(overlapping_ids);
        let context = self.marks.editor_context(self.locator.book_id(), text, &local_ids, !text.trim().is_empty());
        Some(annotation_editor::recurring_editor("reader-annotation-popup", entity, &annotation, context, geometry, &self.annotation_note_input, components::browser_theme(cx), window, cx))
    }

    pub(super) fn render_footnote(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (_, prepared) = self.footnote.clone()?;
        let theme = components::browser_theme(cx);
        let mut panel = components::reader_footnote_panel(theme).child(components::section_title(text!(reader::footnote)));
        if let LoadState::Ready(renderer) = &self.load_state {
            panel = panel.child(html_view_gpui::HtmlNoteElement::new(renderer.clone(), prepared));
        }
        Some(
            panel
                .child(
                    components::action_row()
                        .child(components::topbar_action_button("reader-footnote-open", text!(reader::go_to_note), gpui_component::IconName::ArrowRight, theme).on_click(cx.listener(|this, _, _, cx| this.navigate_from_footnote(cx))))
                        .child(components::topbar_action_button("reader-footnote-close", text!(common::close), gpui_component::IconName::Close, theme).on_click(cx.listener(|this, _, _, cx| {
                            this.footnote = None;
                            notify(cx, Component::ReaderShell, "footnote_closed");
                        }))),
                )
                .into_any_element(),
        )
    }

    pub(super) fn render_image_preview(&self, entity: &Entity<Self>, window: &Window, _cx: &mut Context<Self>) -> Option<AnyElement> {
        let (uri, image) = self.image_preview.clone()?;
        let copy_image = entity.clone();
        let close_image = entity.clone();
        Some(
            components::ReaderImagePopup::new(uri, image, window.viewport_size())
                .on_copy(move |_, cx| copy_image.update(cx, |this, cx| this.copy_preview_image(cx)))
                .on_dismiss(move |_, cx| {
                    close_image.update(cx, |this, cx| {
                        this.image_preview = None;
                        notify(cx, Component::ReaderShell, "image_preview_closed");
                    })
                })
                .into_any_element(),
        )
    }
}
