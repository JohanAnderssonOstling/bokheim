use ui_text::text;

use crate::library::services::LibraryContext;
use crate::navigation::LibraryOperationDialog;
use book_model::{Annotation, AnnotationStyle};
use components::AnnotationEditorTarget;
use gpui::prelude::*;
use gpui::{AnyWindowHandle, App, AppContext, Context, DismissEvent, Entity, FocusHandle, Focusable, KeyDownEvent, Render, ScrollHandle, Window, div, px, rems};
use gpui_component::{
    WindowExt,
    input::{Input, InputState},
};
use ui_components as components;
use web_time::{SystemTime, UNIX_EPOCH};

pub(super) fn open_reader(library: &LibraryContext, title: &str, annotation: &Annotation, window: &mut Window, cx: &mut App) {
    if let Some(target) = annotation.anchor.open_target() {
        library.open_book_at(annotation.book_id, title.to_owned(), Some(target), window, cx);
    }
}

pub(super) fn show(library: LibraryContext, title: String, annotation: Annotation, window: &mut Window, cx: &mut App) {
    if annotation.is_external_pdf_annotation() {
        show_read_only(library, title, annotation, window, cx);
        return;
    }
    show_editor(library, title, annotation, window, cx);
}

fn show_editor(library: LibraryContext, title: String, annotation: Annotation, window: &mut Window, cx: &mut App) -> Entity<NotebookEditor> {
    let session = library.session();
    let editor = cx.new(|cx| {
        let input = cx.new(|cx| {
            let mut input = components::annotation_note_input(window, cx);
            input.set_value(annotation.note.clone(), window, cx);
            input
        });
        NotebookEditor { library, title, saved: annotation.clone(), draft: annotation, focus: cx.focus_handle(), input, scroll: ScrollHandle::default(), window: window.window_handle(), state: EditorState::Idle }
    });
    let _ = session.update(cx, |session, cx| {
        session.show_operation_dialog(editor.clone(), f32::from(window.rem_size()) * components::ANNOTATION_EDITOR_WIDTH_REM, true, window, cx);
    });
    editor
}

struct NotebookEditor {
    library: LibraryContext,
    title: String,
    saved: Annotation,
    draft: Annotation,
    focus: FocusHandle,
    input: Entity<InputState>,
    scroll: ScrollHandle,
    window: AnyWindowHandle,
    state: EditorState,
}

impl gpui::EventEmitter<DismissEvent> for NotebookEditor {}

impl Focusable for NotebookEditor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl LibraryOperationDialog for NotebookEditor {
    fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Keep the draft mounted until storage confirms the final edit.
        self.finish(Completion::Dismiss, window, cx);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Completion {
    Dismiss,
    OpenReader,
    Delete,
}

fn changed(a: &Annotation, b: &Annotation) -> bool {
    a.note != b.note || a.style != b.style || a.color != b.color
}

enum EditorState {
    Idle,
    Saving(Option<Completion>),
    Failed { error: String, completion: Option<Completion> },
    Closed,
}

impl NotebookEditor {
    fn finish(&mut self, completion: Completion, window: &mut Window, cx: &mut Context<Self>) {
        match &mut self.state {
            EditorState::Closed | EditorState::Saving(Some(Completion::Delete)) => return,
            EditorState::Saving(pending) => {
                *pending = Some(completion);
                cx.notify();
                return;
            }
            _ => {}
        }
        self.draft.note = self.input.read(cx).value().to_string();
        if completion == Completion::Delete || changed(&self.draft, &self.saved) {
            self.start_save(Some(completion), cx);
        } else {
            self.close(completion, window, cx);
        }
    }

    fn close(&mut self, completion: Completion, window: &mut Window, cx: &mut Context<Self>) {
        let active = self.focus.contains_focused(window, cx);
        self.state = EditorState::Closed;
        cx.emit(DismissEvent);
        // A completed save must not reopen a reader after switching libraries.
        if completion == Completion::OpenReader && active {
            open_reader(&self.library, &self.title, &self.saved, window, cx);
        } else {
            let session = self.library.session();
            cx.defer(move |cx| {
                let _ = session.update(cx, |session, cx| session.refresh_notebook(cx));
            });
        }
        cx.notify();
    }

    fn retry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let EditorState::Failed { completion, .. } = &self.state else { return };
        if let Some(completion) = *completion {
            self.finish(completion, window, cx);
        } else {
            self.persist(cx);
        }
    }

    fn persist(&mut self, cx: &mut Context<Self>) {
        if matches!(self.state, EditorState::Closed) {
            return;
        }
        self.draft.note = self.input.read(cx).value().to_string();
        if !matches!(self.state, EditorState::Saving(_)) {
            if changed(&self.draft, &self.saved) {
                self.start_save(None, cx);
                return;
            }
            self.state = EditorState::Idle;
        }
        cx.notify();
    }

    /// Keep one write in flight; later edits coalesce in the draft.
    fn start_save(&mut self, completion: Option<Completion>, cx: &mut Context<Self>) {
        let mut sent = self.draft.clone();
        sent.modified_at = now().max(self.saved.modified_at.saturating_add(1)).min(i64::MAX as u64);
        let backend = self.library.backend().clone();
        let handle = self.window;
        // Keep the draft alive until its write is acknowledged.
        let editor = cx.entity();
        self.state = EditorState::Saving(completion);
        cx.spawn(async move |_, cx| {
            let deleting = completion == Some(Completion::Delete);
            let result = if deleting { backend.delete_annotation(sent.id.clone(), sent.modified_at as i64).await } else { backend.upsert_annotation(sent.clone()).await }
                .map_err(|error| components::user_facing_error(&error, text!(browse::could_not_update_annotation_your_changes_are_still_here)).to_owned());
            let sent = (!deleting).then_some(sent);
            let _ = handle.update(cx, |_, window, cx| editor.update(cx, |editor, cx| editor.did_save(sent, result, window, cx)));
        })
        .detach();
        cx.notify();
    }

    fn did_save(&mut self, sent: Option<Annotation>, result: Result<(), String>, window: &mut Window, cx: &mut Context<Self>) {
        let EditorState::Saving(completion) = &self.state else { return };
        let completion = *completion;
        if let Err(error) = result {
            self.state = EditorState::Failed { error, completion };
            cx.notify();
            return;
        }
        self.state = EditorState::Idle;
        let Some(sent) = sent else {
            self.close(Completion::Delete, window, cx);
            return;
        };
        self.saved = sent;
        self.draft.note = self.input.read(cx).value().to_string();
        if changed(&self.draft, &self.saved) || completion == Some(Completion::Delete) {
            self.start_save(completion, cx);
        } else if let Some(completion) = completion {
            self.close(completion, window, cx);
        } else {
            cx.notify();
        }
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |time| time.as_millis().min(i64::MAX as u128) as u64)
}

impl AnnotationEditorTarget for NotebookEditor {
    fn save_annotation_changes(&mut self, style: Option<AnnotationStyle>, color: Option<u32>, cx: &mut Context<Self>) {
        if matches!(self.state, EditorState::Closed) {
            return;
        }
        if let Some(style) = style {
            self.draft.style = style;
        }
        if let Some(color) = color {
            self.draft.color = color;
        }
        self.persist(cx);
    }

    fn dismiss_annotation_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.finish(Completion::Dismiss, window, cx);
    }

    fn delete_edited_annotation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.finish(Completion::Delete, window, cx);
    }
}

impl Render for NotebookEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let passage = (!self.draft.exact_text.is_empty()).then(|| components::annotation_editor_passage(&self.draft, theme).debug_selector(|| "notebook-full-annotation".into()).into_any_element());
        let open = components::outlined_text_button("notebook-open-reader", text!(browse::open_in_reader)).on_click(cx.listener(|editor, _, window, cx| {
            editor.finish(Completion::OpenReader, window, cx);
        }));
        let close = components::outlined_icon_button("notebook-close-editor", text!(common::close), gpui_component::IconName::Close).on_click(cx.listener(|editor, _, window, cx| {
            editor.finish(Completion::Dismiss, window, cx);
        }));
        let heading = div().w_full().flex().items_center().justify_between().gap(rems(0.5)).child(open).child(close);
        let passage = Some(div().w_full().flex().flex_col().gap(rems(0.5)).child(heading).children(passage).into_any_element());
        let controls = components::annotation_editor_controls("notebook-editor", &self.draft, &cx.entity(), theme, window, cx);
        let controls = div().flex().flex_col().gap(rems(0.5)).child(controls);
        let error = match &self.state {
            EditorState::Failed { error, .. } => Some(error.clone()),
            _ => None,
        };
        let controls = controls.when_some(error, |controls, error| {
            controls.child(components::error_text(theme).child(error)).child(
                components::action_row().child(components::outlined_text_button("notebook-retry-save", text!(common::retry)).on_click(cx.listener(|editor, _, window, cx| editor.retry(window, cx)))).child(
                    components::outlined_text_button("notebook-discard-edit", text!(browse::discard_changes)).on_click(cx.listener(|editor, _, window, cx| {
                        editor.close(Completion::Dismiss, window, cx);
                    })),
                ),
            )
        });
        components::annotation_editor_panel(passage, Input::new(&self.input).appearance(false).w_full(), controls, &self.scroll, theme)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|editor, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    editor.finish(Completion::Dismiss, window, cx);
                    cx.stop_propagation();
                }
            }))
            .w_full()
            .max_h((window.viewport_size().height - rems(3.0).to_pixels(window.rem_size())).max(px(1.0)).min(px(components::ANNOTATION_EDITOR_MAX_HEIGHT)))
            .debug_selector(|| "notebook-annotation-editor".into())
    }
}

fn show_read_only(library: LibraryContext, title: String, annotation: Annotation, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, window, cx| {
        let theme = components::browser_theme(cx);
        let library = library.clone();
        let title = title.clone();
        let open_annotation = annotation.clone();
        let content = div()
            .id("notebook-annotation-content")
            .debug_selector(|| "notebook-full-annotation".into())
            .max_h(window.viewport_size().height * 0.6)
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(rems(0.75))
            .when(!annotation.exact_text.is_empty(), |body| body.child(components::annotation_editor_passage(&annotation, theme)))
            .when(!annotation.note.is_empty(), |body| body.child(components::reader_state_note(annotation.note.clone(), theme).text_color(theme.text)));
        dialog
            .title(text!(common::annotation))
            .w(rems(components::ANNOTATION_EDITOR_WIDTH_REM).to_pixels(window.rem_size()))
            .max_w(window.viewport_size().width - rems(1.5).to_pixels(window.rem_size()))
            .child(components::outlined_text_button("notebook-open-reader", text!(browse::open_in_reader)).on_click(move |_, window, cx| {
                window.close_dialog(cx);
                open_reader(&library, &title, &open_annotation, window, cx);
            }))
            .child(content)
    });
}

#[cfg(all(test, not(target_arch = "wasm32"), not(feature = "kobo")))]
mod tests;
