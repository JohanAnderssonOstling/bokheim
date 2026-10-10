//! EPUB document surface and its selection/table/image context menu.

use gpui::prelude::*;
use gpui::{AnyElement, App, ClickEvent, Context, Entity, IntoElement, Pixels, Point, Window, px};
use gpui_component::menu::{ContextMenuExt, OpenContextMenu};
use ui_components as components;

use crate::epub::{EpubContextCommand, LoadState, ReaderView, TableCopyFormat};
use crate::shell::context_sheet::{ContextAction, ContextActionTarget, ContextActions};

/// Keeps glyph ink and logical-to-framebuffer rounding inside the Kobo panel.
/// This is a renderer guard band, not an authored EPUB margin.
const KOBO_READER_EDGE_INSET: f32 = 2.0;

impl ReaderView {
    pub(in crate::epub) fn toggle_mobile_controls(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        if !components::uses_mobile_navigation(window) || self.sidebar.is_visible() && self.chrome.is_visible() {
            return;
        }
        if self.chrome.is_visible() {
            self.chrome.hide(cx);
            self.focus_document(window, cx);
        } else {
            self.sidebar.close();
            self.chrome.show_from_touch(cx);
        }
    }

    pub(super) fn render_document(&self, entity: Entity<Self>, horizontal_margin: f32, vertical_margin: f32, mobile: bool, cx: &mut Context<Self>) -> AnyElement {
        if let LoadState::Error(error) = &self.load_state {
            return components::reader_content().child(gpui::div().size_full().flex().flex_col().items_center().justify_center().gap(px(16.0)).p(px(24.0)).child("Could not display this book").child(error.clone())).into_any_element();
        }
        let LoadState::Ready(renderer) = &self.load_state else {
            return components::reader_content().child(gpui::div().size_full()).into_any_element();
        };

        let context_target = entity.clone();
        let keyboard_target = entity.clone();
        let document = components::reader_content()
            .id("epub-reader-document")
            .px(px(horizontal_margin + if cfg!(feature = "kobo") { KOBO_READER_EDGE_INSET } else { 0.0 }))
            .py(px(vertical_margin))
            .child(renderer.clone())
            .on_click(cx.listener(|reader, event, window, cx| {
                if crate::shell::chrome::is_page_tap(event) {
                    reader.toggle_mobile_controls(window, cx);
                }
            }))
            .on_aux_click(cx.listener(|reader, event: &ClickEvent, window, cx| {
                if event.is_secondary() && components::uses_mobile_navigation(window) {
                    reader.open_context_sheet(event.position(), cx);
                }
            }));
        if mobile {
            document.on_action(cx.listener(|reader, _: &OpenContextMenu, window, cx| reader.open_keyboard_context_sheet(window.mouse_position(), cx))).into_any_element()
        } else {
            document
                .context_menu(move |menu, window, cx| {
                    let position = window.mouse_position();
                    context_target.read(cx).context_actions_at(position, cx).popup(menu, &context_target)
                })
                .keyboard_menu(move |menu, window, cx| {
                    keyboard_target.read(cx).keyboard_context_actions(window.mouse_position(), cx).popup(menu, &keyboard_target)
                })
                .into_any_element()
        }
    }
}

impl ReaderView {
    fn context_actions_at(&self, position: Point<Pixels>, cx: &App) -> ContextActions<EpubContextCommand> {
        self.context_actions(position, false, cx)
    }

    fn keyboard_context_actions(&self, pointer: Point<Pixels>, cx: &App) -> ContextActions<EpubContextCommand> {
        let position = match &self.load_state {
            LoadState::Ready(renderer) => renderer.read(cx).selection_bounds().map(|bounds| bounds.center()).unwrap_or(pointer),
            _ => pointer,
        };
        self.context_actions(position, true, cx)
    }

    fn context_actions(&self, position: Point<Pixels>, keyboard: bool, cx: &App) -> ContextActions<EpubContextCommand> {
        let mut actions = ContextActions::default();
        let LoadState::Ready(renderer) = &self.load_state else { return actions };
        if self.selection.is_some() && (keyboard || renderer.read(cx).selection_contains(position)) {
            actions.add_section(vec![ContextAction { label: "Highlight", command: EpubContextCommand::Highlight }, ContextAction { label: "Cite", command: EpubContextCommand::Cite }]);
        }
        if renderer.read(cx).table_at(position) {
            actions.add_section(vec![
                ContextAction { label: "Copy Table as Markdown", command: EpubContextCommand::CopyTable { view: renderer.clone(), position, format: TableCopyFormat::Markdown } },
                ContextAction { label: "Copy Table as Unstyled HTML", command: EpubContextCommand::CopyTable { view: renderer.clone(), position, format: TableCopyFormat::UnstyledHtml } },
                ContextAction { label: "Copy Table as Styled HTML", command: EpubContextCommand::CopyTable { view: renderer.clone(), position, format: TableCopyFormat::StyledHtml } },
            ]);
        }
        if renderer.read(cx).image_at(position) {
            actions.add_section(vec![ContextAction { label: "Copy Image", command: EpubContextCommand::CopyImage { view: renderer.clone(), position } }]);
        }
        actions
    }

    fn open_keyboard_context_sheet(&mut self, pointer: Point<Pixels>, cx: &mut Context<Self>) {
        let actions = self.keyboard_context_actions(pointer, cx);
        self.show_context_sheet(actions, cx);
    }

    fn open_context_sheet(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let actions = self.context_actions_at(position, cx);
        self.show_context_sheet(actions, cx);
    }

    fn show_context_sheet(&mut self, actions: ContextActions<EpubContextCommand>, cx: &mut Context<Self>) {
        if !actions.is_empty() {
            self.context_sheet = Some(actions);
            self.chrome.hide(cx);
            cx.notify();
        }
    }
}

impl ContextActionTarget for ReaderView {
    type Command = EpubContextCommand;

    fn run_context_action(&mut self, command: Self::Command, window: &mut Window, cx: &mut Context<Self>) {
        match command {
            EpubContextCommand::Highlight => self.highlight_selection(window, cx),
            EpubContextCommand::Cite => self.copy_selection_citation(cx),
            EpubContextCommand::CopyTable { view, position, format } => {
                view.update(cx, |view, _| match format {
                    TableCopyFormat::Markdown => view.copy_table_at(position),
                    TableCopyFormat::UnstyledHtml => view.copy_table_unstyled_html_at(position),
                    TableCopyFormat::StyledHtml => view.copy_table_styled_html_at(position),
                });
            }
            EpubContextCommand::CopyImage { view, position } => {
                view.update(cx, |view, _| view.copy_image_at(position));
            }
        }
    }

    fn dismiss_context_actions(&mut self, cx: &mut Context<Self>) {
        self.context_sheet = None;
        cx.notify();
    }
}
