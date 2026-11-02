//! Canonical annotation controls shared by reader popups and notebook overlays.
use crate as components;
use book_model::{Annotation, AnnotationStyle, BookId, RecurringScope};
use gpui::prelude::*;
use gpui::{AnyElement, Context, Div, Entity, FocusHandle, SharedString, Window, div, rems};
use gpui_component::input::InputState;
use std::rc::Rc;

/// Width shared by reader popups and notebook annotation overlays.
pub const ANNOTATION_EDITOR_WIDTH: f32 = 360.0;
/// Maximum editor height before its contents scroll.
pub const ANNOTATION_EDITOR_MAX_HEIGHT: f32 = 420.0;

/// Format-specific effects of the shared annotation editor.
pub trait AnnotationEditorTarget: Sized + 'static {
    fn save_annotation_changes(&mut self, style: Option<AnnotationStyle>, color: Option<u32>, cx: &mut Context<Self>);
    fn dismiss_annotation_editor(&mut self, window: &mut Window, cx: &mut Context<Self>);
    fn delete_edited_annotation(&mut self, window: &mut Window, cx: &mut Context<Self>);
    fn set_annotation_scope(&mut self, _scope: Option<RecurringScope>, _cx: &mut Context<Self>) {}
    fn select_editor_annotation(&mut self, _id: String, _window: &mut Window, _cx: &mut Context<Self>) {}
}

pub struct EditorBook {
    pub id: BookId,
    pub title: String,
    pub annotations: Vec<(String, String)>,
}

pub struct RecurringEditorContext {
    pub books: Vec<EditorBook>,
    pub chapter_available: bool,
    pub scope_editable: bool,
}

/// Explicit source books only; inherited matches never become source books.
pub fn recurring_annotation_editor_controls<C: AnnotationEditorTarget>(current: &Annotation, context: RecurringEditorContext, target: &Entity<C>, theme: components::BrowserTheme) -> AnyElement {
    let mut extra = components::column(components::SPACE_SM).w_full().min_w_0();
    if context.books.len() > 1 || !context.scope_editable {
        let books = context.books.iter().enumerate().filter_map(|(index, book)| {
            let id = book.annotations.first()?.0.clone();
            let target = target.clone();
            Some(
                components::settings_choice_button(("annotation-origin-book", index), book.title.clone(), book.id == current.content_hash, theme)
                    .debug_selector(move || format!("annotation-origin-book-{index}"))
                    .on_click(move |_, window, cx| target.update(cx, |this, cx| this.select_editor_annotation(id.clone(), window, cx))),
            )
        });
        extra = extra.child(components::reader_choice_button_group("annotation-origin-books", books, theme));
    }
    if let Some(book) = context.books.iter().find(|book| book.id == current.content_hash).filter(|book| book.annotations.len() > 1) {
        let notes = book.annotations.iter().enumerate().map(|(index, (id, note))| {
            let selected = current.id == *id;
            let id = id.clone();
            let target = target.clone();
            let label = if note.trim().is_empty() { format!("Note {}", index + 1) } else { note.chars().take(45).collect() };
            components::settings_choice_button(("annotation-origin-note", index), label, selected, theme).on_click(move |_, window, cx| target.update(cx, |this, cx| this.select_editor_annotation(id.clone(), window, cx)))
        });
        extra = extra.child(components::reader_choice_button_group("annotation-origin-notes", notes, theme));
    }
    if context.scope_editable {
        let current = current.anchor.recurring_scope();
        let scopes = [(None, "Selection"), (Some(RecurringScope::Chapter), "Chapter"), (Some(RecurringScope::Book), "Book"), (Some(RecurringScope::Library), "Library")];
        let buttons = scopes.into_iter().enumerate().filter(|(_, (scope, _))| *scope != Some(RecurringScope::Chapter) || context.chapter_available).map(|(index, (scope, label))| {
            let target = target.clone();
            components::settings_choice_button(("annotation-scope", index), label, current == scope, theme)
                .debug_selector(move || format!("annotation-scope-{index}"))
                .on_click(move |_, _, cx| target.update(cx, |this, cx| this.set_annotation_scope(scope, cx)))
        });
        extra = extra.child(components::reader_setting_row_smallcaps("Repeat in", theme).child(components::reader_choice_button_group("annotation-scopes", buttons, theme)));
    } else {
        extra = extra.child(components::reader_state_note("Library recurring note · edits apply everywhere", theme));
    }
    extra.into_any_element()
}

/// The shared style, colour, keyboard and deletion controls.
pub fn annotation_editor_controls<C: AnnotationEditorTarget>(id_prefix: &'static str, current: &Annotation, target: &Entity<C>, theme: components::BrowserTheme, window: &mut Window, cx: &mut Context<C>) -> AnyElement {
    let mut styles = components::action_row().w_full().min_w_0().flex_none().flex_nowrap().debug_selector(|| "reader-annotation-style-row".into());
    for (style, label, suffix) in
        [(AnnotationStyle::Highlight, "Highlight", "highlight"), (AnnotationStyle::Underline, "Underline", "underline"), (AnnotationStyle::Squiggly, "Squiggly", "squiggly"), (AnnotationStyle::Strikethrough, "Strike", "strike")]
    {
        let target = target.clone();
        styles = styles.child(components::reader_annotation_action_button(SharedString::from(format!("{id_prefix}-{suffix}")), label, current.style == style, theme).flex_grow(1.0).flex_shrink(1.0).min_w_0().on_click(move |_, _, cx| {
            target.update(cx, |this, cx| this.save_annotation_changes(Some(style.clone()), None, cx));
        }));
    }
    // E-ink paints every annotation in one colour; see `reader_annotation_paint_color`.
    let palette: &'static [(&str, u32)] = if cfg!(feature = "kobo") { &[] } else { &components::READER_ANNOTATION_COLORS };
    let selected_color = palette.iter().position(|&(_, color)| current.color == color).unwrap_or(0);
    let swatch_id = |name: &str| SharedString::from(format!("{id_prefix}-{}", name.to_lowercase()));
    let focus_handles: Rc<Vec<FocusHandle>> =
        Rc::new(palette.iter().enumerate().map(|(index, &(name, _))| window.use_keyed_state(swatch_id(name), cx, |_, cx| cx.focus_handle()).read(cx).clone().tab_stop(index == selected_color)).collect());
    // With no swatches on e-ink, the row holds only Delete, kept at its end.
    let mut colors = components::reader_annotation_color_group().flex_none().when(cfg!(feature = "kobo"), |row| row.justify_end()).debug_selector(|| "reader-annotation-color-row".into());
    for (index, &(name, color)) in palette.iter().enumerate() {
        let target = target.clone();
        let keyboard_target = target.clone();
        let handles = focus_handles.clone();
        colors = colors.child(
            components::reader_annotation_color_swatch(swatch_id(name), color, name, current.color == color, theme)
                .track_focus(&focus_handles[index])
                .on_click(move |_, _, cx| {
                    target.update(cx, |this, cx| this.save_annotation_changes(None, Some(color), cx));
                })
                .on_key_down(move |event, window, cx| {
                    if event.keystroke.modifiers != gpui::Modifiers::default() {
                        return;
                    }
                    let next = match event.keystroke.key.as_str() {
                        "left" | "up" => (index + palette.len() - 1) % palette.len(),
                        "right" | "down" => (index + 1) % palette.len(),
                        "home" => 0,
                        "end" => palette.len() - 1,
                        "enter" | "space" => index,
                        _ => return,
                    };
                    keyboard_target.update(cx, |this, cx| this.save_annotation_changes(None, Some(palette[next].1), cx));
                    handles[next].focus(window, cx);
                    cx.stop_propagation();
                }),
        );
    }
    let colors = colors.when(!current.is_external_pdf_annotation(), |colors| {
        let target = target.clone();
        colors.child(components::delete_icon_button(SharedString::from(format!("{id_prefix}-delete")), "Delete annotation", theme).debug_selector(|| "reader-annotation-editor-delete".into()).on_click(move |_, window, cx| {
            cx.stop_propagation();
            target.update(cx, |this, cx| this.delete_edited_annotation(window, cx));
        }))
    });
    let controls = components::column(components::SPACE_SM).w_full().flex_none().child(styles).child(colors);
    controls.into_any_element()
}

/// The growing note field used in both reader and notebook editors.
pub fn annotation_note_input(window: &mut Window, cx: &mut Context<InputState>) -> InputState {
    InputState::new(window, cx).placeholder("Add a note").auto_grow(components::READER_NOTE_MIN_ROWS, components::READER_NOTE_MAX_ROWS)
}

/// The complete highlighted passage when its context is outside the reader.
pub fn annotation_editor_passage(annotation: &Annotation, theme: components::BrowserTheme) -> Div {
    div()
        .relative()
        .w_full()
        .min_w_0()
        .flex_none()
        .pl(rems(1.0))
        .child(components::reader_annotation_color_bar(annotation.color).left_0().top_0().bottom_0())
        .child(components::reader_state_full_quote(format!("“{}”", annotation.exact_text), theme))
}
