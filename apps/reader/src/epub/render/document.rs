//! EPUB document surface, direct annotation creation, and table/image context menu.

use ui_text::text;

use dictionary::can_lookup;
use gpui::prelude::*;
use gpui::{AnyElement, App, Context, DispatchPhase, Entity, IntoElement, LongPressEvent, MouseButton, MouseDownEvent, MouseMoveEvent, Pixels, Point, Window, px};
use gpui_component::ElementExt;
use gpui_component::menu::{ContextMenuExt, OpenContextMenu};
use html::export::TableExportFormat;
use ui_components as components;

use crate::epub::{EpubContextCommand, LoadState, ReaderView};
use crate::shell::context_sheet::{ContextAction, ContextActionTarget, ContextActions, SelectionCommands};

impl ReaderView {
    pub(in crate::epub) fn toggle_mobile_controls(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        if !components::uses_mobile_reader_controls(window) || self.sidebar.is_visible() && self.chrome.is_visible() {
            return;
        }
        if self.chrome.is_visible() {
            self.chrome.hide(cx);
            self.focus_document(window, cx);
        } else {
            self.sidebar.close(cx);
            self.chrome.show_from_touch(cx);
        }
    }

    pub(super) fn render_document(&self, entity: Entity<Self>, horizontal_margin: f32, vertical_margin: f32, mobile: bool, cx: &mut Context<Self>) -> AnyElement {
        if let LoadState::Error(error) = &self.load_state {
            return components::reader_content()
                .child(gpui::div().size_full().flex().flex_col().items_center().justify_center().gap(px(16.0)).p(px(24.0)).child(text!(reader::could_not_display_this_book)).child(error.clone()))
                .into_any_element();
        }
        let LoadState::Ready(renderer) = &self.load_state else {
            return components::reader_content().child(gpui::div().size_full()).into_any_element();
        };

        let context_target = entity.clone();
        let movement_target = entity.downgrade();
        let keyboard_target = entity.clone();
        renderer.update(cx, |view, cx| view.set_reading_ruler_margins(horizontal_margin, vertical_margin, cx));
        let document = components::reader_content()
            .relative()
            .id("epub-reader-document")
            .px(px(horizontal_margin))
            .py(px(vertical_margin))
            .child(renderer.clone())
            .children(self.dictionary.as_ref().map(|popup| {
                let renderer = renderer.clone();
                crate::shell::dictionary::DictionaryPopup::track_anchor(popup.downgrade(), move |cx| renderer.read(cx).selection_bounds())
            }))
            .on_prepaint(move |_, window, _| {
                // The renderer consumes selection drags during bubbling.
                // Observe movement in capture, including outside the document.
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture {
                        let _ = movement_target.update(cx, |reader, _| reader.move_word_click(event.position));
                    }
                });
            })
            .on_click(cx.listener(|reader, event, window, cx| {
                if crate::shell::chrome::is_page_tap(event) {
                    reader.toggle_mobile_controls(window, cx);
                }
            }))
            // With no selection, resolve word lookup before the context-menu
            // wrapper. Existing selections and non-text content keep their menu.
            .capture_any_mouse_down(cx.listener(|reader, event: &MouseDownEvent, window, cx| {
                reader.begin_word_click(event);
                if event.button == MouseButton::Left {
                    reader.dismiss_selection(cx);
                    return;
                }
                if event.button != MouseButton::Right {
                    return;
                }
                if matches!(&reader.load_state, LoadState::Ready(renderer) if renderer.read(cx).content_at(event.position).equation) {
                    return;
                }
                if let LoadState::Ready(renderer) = &reader.load_state {
                    let selected = !renderer.read(cx).has_text_selection() && renderer.update(cx, |view, cx| view.select_word_at(event.position, cx));
                    if selected {
                        reader.lookup_selection(window, cx);
                        cx.stop_propagation();
                        return;
                    }
                }
                if reader.open_passage_context(event.position, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_any_mouse_up(cx.listener(Self::finish_word_click))
            .capture_long_press(cx.listener(|reader, event: &LongPressEvent, window, cx| {
                if reader.begin_touch_selection(event, window, cx) || reader.open_passage_context(event.position, window, cx) {
                    cx.stop_propagation();
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
                .keyboard_menu(move |menu, window, cx| keyboard_target.read(cx).keyboard_context_actions(window.mouse_position(), cx).popup(menu, &keyboard_target))
                .into_any_element()
        }
    }
}

impl ReaderView {
    fn open_passage_context(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if components::uses_mobile_reader_controls(window) {
            self.open_context_sheet(position, cx);
            true
        } else {
            false
        }
    }

    fn context_actions_at(&self, position: Point<Pixels>, cx: &App) -> ContextActions<EpubContextCommand> {
        self.context_actions(position, cx)
    }

    fn keyboard_context_actions(&self, pointer: Point<Pixels>, cx: &App) -> ContextActions<EpubContextCommand> {
        let position = match &self.load_state {
            LoadState::Ready(renderer) => renderer.read(cx).selection_bounds().map(|bounds| bounds.center()).unwrap_or(pointer),
            _ => pointer,
        };
        self.context_actions(position, cx)
    }

    fn context_actions(&self, position: Point<Pixels>, cx: &App) -> ContextActions<EpubContextCommand> {
        let mut actions = ContextActions::default();
        let LoadState::Ready(renderer) = &self.load_state else { return actions };
        if renderer.read(cx).has_text_selection() {
            actions = ContextActions::selection(SelectionCommands {
                annotate: EpubContextCommand::AnnotateSelection,
                lookup: renderer.read(cx).selected_passage().is_some_and(|(_, text)| can_lookup(&text)).then_some(EpubContextCommand::LookupSelection),
                translate: EpubContextCommand::TranslateSelection,
                copy: EpubContextCommand::CopySelectionMarkdown,
                copy_link: EpubContextCommand::CopySelectionWithSource,
            });
        }
        let hit = renderer.read(cx).content_at(position);
        if hit.table {
            actions.add_section(vec![
                ContextAction { label: text!(common::copy_table_as_markdown), icon: "icons/markdown-logo.svg", command: EpubContextCommand::CopyTable { view: renderer.clone(), position, format: TableExportFormat::Markdown } },
                ContextAction { label: text!(common::copy_table_as_unstyled_html), icon: "icons/file-html.svg", command: EpubContextCommand::CopyTable { view: renderer.clone(), position, format: TableExportFormat::Html } },
                ContextAction { label: text!(common::copy_table_as_styled_html), icon: "icons/file-html.svg", command: EpubContextCommand::CopyTable { view: renderer.clone(), position, format: TableExportFormat::AuthoredHtml } },
            ]);
        }
        if hit.image {
            actions.add_section(vec![ContextAction { label: text!(common::copy_image), icon: "icons/image.svg", command: EpubContextCommand::CopyImage { view: renderer.clone(), position } }]);
        }
        if hit.equation {
            actions.add_section(vec![ContextAction { label: text!(reader::copy_as_latex), icon: "icons/function.svg", command: EpubContextCommand::CopyEquationLatex { view: renderer.clone(), position } }]);
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
            EpubContextCommand::CopySelectionMarkdown => self.copy_selection(cx),
            EpubContextCommand::CopySelectionWithSource => self.copy_selection_with_source(cx),
            EpubContextCommand::AnnotateSelection => {
                self.annotation_popup_anchor = Some(window.mouse_position());
                let LoadState::Ready(renderer) = &self.load_state else { return };
                let Some((cfi_range, text)) = renderer.read(cx).selected_passage() else { return };
                self.create_annotation(crate::epub::Selection { cfi_range, text }, window, cx);
            }
            EpubContextCommand::LookupSelection => self.lookup_selection(window, cx),
            EpubContextCommand::TranslateSelection => self.translate_selection(window, cx),
            EpubContextCommand::CopyTable {
                view,
                position,
                format,
            } => {
                view.update(cx, |view, cx| view.copy_table_at(position, format, cx));
            }
            EpubContextCommand::CopyImage { view, position } => {
                view.update(cx, |view, cx| view.copy_image_at(position, cx));
            }
            EpubContextCommand::CopyEquationLatex { view, position } => {
                view.update(cx, |view, cx| view.copy_equation_latex_at(position, cx));
            }
        }
    }

    fn dismiss_context_actions(&mut self, cx: &mut Context<Self>) {
        self.context_sheet = None;
        cx.notify();
    }
}
