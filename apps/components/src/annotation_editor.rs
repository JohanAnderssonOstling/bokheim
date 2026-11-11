//! Canonical annotation controls shared by reader popups and notebook overlays.
use ui_text::text;

use crate as components;
use book_model::{Annotation, AnnotationStyle, BookId, RecurringScope};
use gpui::prelude::*;
use gpui::{AnyElement, Context, Div, Entity, FocusHandle, SharedString, Window, div, px, rems};
use gpui_component::input::InputState;
use gpui_component::Selectable;
use std::rc::Rc;

/// Width shared by reader popups and notebook annotation overlays.
pub const ANNOTATION_EDITOR_WIDTH_REM: f32 = 22.5;
/// Maximum editor height before its contents scroll.
pub const ANNOTATION_EDITOR_MAX_HEIGHT: f32 = 420.0;

/// Format-specific effects of the shared annotation editor.
pub trait AnnotationEditorTarget: Sized + 'static {
    fn save_annotation_changes(&mut self, style: Option<AnnotationStyle>, color: Option<u32>, cx: &mut Context<Self>);
    fn dismiss_annotation_editor(&mut self, window: &mut Window, cx: &mut Context<Self>);
    fn delete_edited_annotation(&mut self, window: &mut Window, cx: &mut Context<Self>);
    fn select_annotation_scope(&mut self, _scope: Option<RecurringScope>, _window: &mut Window, _cx: &mut Context<Self>) {}
    fn select_editor_annotation(&mut self, _id: String, _window: &mut Window, _cx: &mut Context<Self>) {}
    fn add_annotation_alias(&mut self, _id: String, _alias: String, _cx: &mut Context<Self>) {}
    fn remove_annotation_alias(&mut self, _id: String, _alias: String, _cx: &mut Context<Self>) {}
}

pub struct EditorBook {
    pub id: BookId,
    pub title: String,
    pub annotations: Vec<Annotation>,
}

pub struct RecurringEditorContext {
    pub books: Vec<EditorBook>,
    pub chapter_available: bool,
    /// Whether the selected annotation belongs to the current book.
    pub scope_editable: bool,
}

/// Switch between independent notes at each scope, including their source books.
pub fn recurring_annotation_editor_controls<C: AnnotationEditorTarget>(current: &Annotation, context: RecurringEditorContext, target: &Entity<C>, theme: components::BrowserTheme) -> AnyElement {
    let mut extra = components::column(components::SPACE_SM).w_full().min_w_0();
    let scopes = [(None, text!(common::selection)), (Some(RecurringScope::Chapter), text!(audiobook::chapter)), (Some(RecurringScope::Book), text!(reader::book)), (Some(RecurringScope::Library), text!(common::library))];
    let mut buttons = Vec::new();
    for (index, (scope, label)) in scopes
        .into_iter()
        .enumerate()
        .filter(|(_, (scope, _))| *scope != Some(RecurringScope::Chapter) || context.chapter_available || context.books.iter().any(|book| book.annotations.iter().any(|note| note.anchor.recurring_scope() == *scope)))
    {
        let notes = context.books.iter().flat_map(|book| &book.annotations).filter(|note| note.anchor.recurring_scope() == scope).collect::<Vec<_>>();
        let selected = current.anchor.recurring_scope() == scope;
        let label = if notes.len() > 1 { format!("{label} · {}", notes.len()) } else { label.to_string() };
        let existing = notes.iter().find(|note| note.id == current.id).or_else(|| notes.first()).map(|note| note.id.clone());
        let target = target.clone();
        buttons.push(
            components::settings_choice_button(("annotation-scope", index), label, selected, theme)
                .whitespace_nowrap()
                .flex_basis(gpui::Length::Auto)
                .flex_shrink_0()
                .px(px(components::SPACE_XS))
                .when(!selected && !notes.is_empty(), |button| button.bg(theme.accent_wash).text_color(theme.text_accent).hover(|style| style.bg(theme.accent_wash_hover)))
                .on_click(move |_, window, cx| {
                    target.update(cx, |this, cx| {
                        if let Some(id) = &existing {
                            this.select_editor_annotation(id.clone(), window, cx);
                        } else {
                            this.select_annotation_scope(scope, window, cx);
                        }
                    })
                }),
        );
    }
    extra = extra.child(components::reader_choice_button_group("annotation-scopes", buttons, theme));
    let notes = context.books.iter().flat_map(|book| book.annotations.iter().map(move |note| (book, note))).filter(|(_, note)| note.anchor.recurring_scope() == current.anchor.recurring_scope()).collect::<Vec<_>>();
    if notes.len() > 1 || !context.scope_editable {
        let mut list = components::column(components::SPACE_XXS).w_full().min_w_0();
        for (book, note) in notes {
            let selected = current.id == note.id;
            let source = note.book_id != context.books.first().map_or(current.book_id, |book| book.id) || !context.scope_editable;
            let preview = note.note.split_whitespace().collect::<Vec<_>>().join(" ");
            let preview = if source && !preview.is_empty() {
                format!("{} · {preview}", book.title)
            } else if source {
                book.title.clone()
            } else {
                preview
            };
            let id = note.id.clone();
            let target = target.clone();
            let row = components::base_button(SharedString::from(format!("annotation-note-{id}")))
                .ghost()
                .selected(selected)
                .text_color(theme.text)
                .w_full()
                .h_auto()
                .min_h(rems(components::settings_control_size_rem()))
                .p(px(components::SPACE_SM))
                .border_1()
                .border_color(if selected { theme.rule } else { gpui::transparent_black() })
                .bg(if selected { theme.accent_wash } else { theme.page_bg })
                .hover(move |style| style.bg(if selected { theme.accent_wash_hover } else { theme.hover }))
                .aria_label(format!("{} {preview}", note.exact_text))
                .child(
                    div().flex().items_start().gap(px(components::SPACE_SM)).w_full().min_w_0().child(div().flex_none().mt(px(4.0)).size(px(8.0)).rounded_full().bg(components::reader_annotation_color(note.color))).child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_left()
                            .text_size(rems(components::TEXT_SM))
                            .child(div().text_ellipsis().child(note.exact_text.clone()))
                            .when(!preview.is_empty(), |content| content.child(div().text_size(rems(components::TEXT_XS)).text_color(theme.text_muted).text_ellipsis().child(preview))),
                    ),
                )
                .on_click(move |_, window, cx| target.update(cx, |this, cx| this.select_editor_annotation(id.clone(), window, cx)));
            list = list.child(row);
        }
        extra = extra.child(list);
    }
    if !context.scope_editable {
        extra = extra.child(components::reader_state_note(text!(common::library_recurring_note_edits_apply_everywhere), theme));
    }
    extra.into_any_element()
}

/// The shared style, colour, keyboard and deletion controls.
pub fn annotation_editor_controls<C: AnnotationEditorTarget>(id_prefix: &'static str, current: &Annotation, target: &Entity<C>, theme: components::BrowserTheme, window: &mut Window, cx: &mut Context<C>) -> AnyElement {
    let mut styles = components::action_row().w_full().min_w_0().flex_none().flex_nowrap().debug_selector(|| "reader-annotation-style-row".into());
    for (style, label, suffix) in [
        (AnnotationStyle::Highlight, text!(common::highlight), "highlight"),
        (AnnotationStyle::Underline, text!(common::underline), "underline"),
        (AnnotationStyle::Squiggly, text!(common::squiggly), "squiggly"),
        (AnnotationStyle::Strikethrough, text!(common::strike), "strike"),
    ] {
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
        colors.child(components::delete_icon_button(SharedString::from(format!("{id_prefix}-delete")), text!(common::delete_annotation), theme).debug_selector(|| "reader-annotation-editor-delete".into()).on_click(move |_, window, cx| {
            cx.stop_propagation();
            target.update(cx, |this, cx| this.delete_edited_annotation(window, cx));
        }))
    });
    let controls = components::column(components::SPACE_SM).w_full().flex_none().child(styles).child(colors);
    controls.into_any_element()
}

/// The growing note field used in both reader and notebook editors.
pub fn annotation_note_input(window: &mut Window, cx: &mut Context<InputState>) -> InputState {
    InputState::new(window, cx).placeholder(text!(common::add_a_note)).auto_grow(components::READER_NOTE_MIN_ROWS, components::READER_NOTE_MAX_ROWS)
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
