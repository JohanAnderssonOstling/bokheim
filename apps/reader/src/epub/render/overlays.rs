//! EPUB annotation, footnote, and image overlays.

use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity, IntoElement, Window, px};
use ui_components as components;

use crate::epub::{LoadState, ReaderView};
use crate::invalidation::Component;
use crate::shell::{annotation_editor, chrome};

impl ReaderView {
    pub(super) fn render_annotation_popup(&self, entity: &Entity<Self>, sidebar_visible: bool, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
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
                view.annotation_bounds(&annotation.id).or_else(|| view.selection_bounds()).map(annotation_editor::Passage::from_bounds)
            }
            LoadState::Loading | LoadState::Error(_) => None,
        }
        .or_else(|| self.annotation_popup_anchor.map(annotation_editor::Passage::from_point));
        let geometry = annotation_editor::geometry(passage, f32::from(viewport.width), f32::from(viewport.height), sidebar_width, components::READER_TOOLBAR_HEIGHT, column_count.max(1));
        Some(annotation_editor::standard_editor(
            "reader-annotation-popup",
            entity,
            &annotation,
            geometry,
            &self.annotation_note_input,
            components::browser_theme(cx),
            |style, color, this, cx| this.save_selected_annotation(style, color, cx),
            |window, this, cx| {
                this.close_annotation_popup(cx);
                this.focus_document(window, cx);
            },
        ))
    }

    pub(super) fn render_footnote(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (_, prepared) = self.footnote.clone()?;
        let theme = components::browser_theme(cx);
        let mut panel = components::reader_footnote_panel(theme).child(components::section_title("Footnote"));
        if let LoadState::Ready(renderer) = &self.load_state {
            panel = panel.child(html_view_gpui::HtmlNoteElement::new(renderer.clone(), prepared));
        }
        Some(
            panel
                .child(components::reader_state_actions().child(components::reader_control_button("reader-footnote-open", "Go to note", theme).on_click(cx.listener(|this, _, _, cx| this.navigate_from_footnote(cx)))).child(
                    components::reader_control_button("reader-footnote-close", "Close", theme).on_click(cx.listener(|this, _, _, cx| {
                        this.footnote = None;
                        crate::invalidation::notify(cx, Component::ReaderShell, "footnote_closed");
                    })),
                ))
                .into_any_element(),
        )
    }

    pub(super) fn render_image_preview(&self, entity: &Entity<Self>, sidebar_width: f32, window: &Window, _cx: &mut Context<Self>) -> Option<AnyElement> {
        let (uri, image) = self.image_preview.clone()?;
        let copy_image = entity.clone();
        let close_image = entity.clone();
        let viewport = window.viewport_size();
        let reader_width = (f32::from(viewport.width) - sidebar_width).max(1.0);
        let reader_height = (f32::from(viewport.height) - components::READER_TOOLBAR_HEIGHT).max(1.0);
        Some(
            components::ReaderImagePopup::new(uri, image, gpui::size(px(reader_width), px(reader_height)))
                .on_copy(move |_, cx| copy_image.update(cx, |this, cx| this.copy_preview_image(cx)))
                .on_dismiss(move |_, cx| {
                    close_image.update(cx, |this, cx| {
                        this.image_preview = None;
                        crate::invalidation::notify(cx, Component::ReaderShell, "image_preview_closed");
                    })
                })
                .into_any_element(),
        )
    }
}
