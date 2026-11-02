//! Named reading locations, carried by the same durable registers as annotations.

use book_model::{Annotation, AnnotationAnchor, AnnotationStyle, BOOKMARK_ID_PREFIX};
use content_address::BookId;
use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity, Window};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::ContextMenuExt;
use gpui_component::{Disableable, IconName};
use ui_components as components;

use super::state_panel::AnnotationSidebarTarget;

mod model;
pub(crate) use model::{BookmarkList, BookmarkTree};

pub(crate) fn watch_name_validity<C: 'static>(input: &Entity<InputState>, cx: &mut Context<C>, editing_bookmark: fn(&C) -> bool) {
    let mut was_empty = input.read(cx).value().trim().is_empty();
    cx.subscribe(input, move |this, input, event: &InputEvent, cx| {
        if !matches!(event, InputEvent::Change) {
            return;
        }
        let empty = input.read(cx).value().trim().is_empty();
        let changed = was_empty != empty;
        was_empty = empty;
        if changed && editing_bookmark(this) {
            cx.notify();
        }
    })
    .detach();
}

pub(crate) trait BookmarkTarget: AnnotationSidebarTarget {
    fn create_bookmark(&mut self, window: &mut Window, cx: &mut Context<Self>);
    fn filter_bookmarks(&mut self, cx: &mut Context<Self>);
}

pub(crate) fn bookmark(book: BookId, anchor: AnnotationAnchor, toc_ordinal: usize, progress: f32) -> Annotation {
    let now = super::marks::now_millis();
    Annotation {
        id: format!("{BOOKMARK_ID_PREFIX}{}", uuid::Uuid::new_v4()),
        content_hash: book,
        anchor,
        exact_text: String::new(),
        style: AnnotationStyle::Highlight,
        color: 0,
        note: String::new(),
        created_at: now,
        toc_ordinal: Some(toc_ordinal as i64),
        progress: Some(progress),
        modified_at: now,
        deleted: false,
    }
}

/// The header's icon-only action saves the current reading location.
pub(crate) fn create_button<C: BookmarkTarget>(target: &Entity<C>, enabled: bool) -> AnyElement {
    let target = target.clone();
    components::outlined_icon_button("reader-create-bookmark", "Create bookmark at current location", IconName::Plus)
        .debug_selector(|| "reader-create-bookmark".into())
        .disabled(!enabled)
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            target.update(cx, |this, cx| this.create_bookmark(window, cx));
        })
        .into_any_element()
}

pub(crate) fn filter_input<C: BookmarkTarget>(window: &mut Window, cx: &mut Context<C>) -> Entity<InputState> {
    let input = cx.new(|cx| InputState::new(window, cx).placeholder("Filter bookmarks"));
    cx.subscribe(&input, |this, _, event: &InputEvent, cx| {
        if matches!(event, InputEvent::Change) {
            this.filter_bookmarks(cx);
            cx.notify();
        }
    })
    .detach();
    input
}

pub(crate) fn filter_bar(input: &Entity<InputState>, theme: components::BrowserTheme) -> AnyElement {
    components::reader_search_bar(theme).child(components::reader_search_box(theme).debug_selector(|| "reader-bookmark-filter".into()).child(Input::new(input).appearance(false).cleanable(true).size_full())).into_any_element()
}

/// Both readers use a trimmed, lowercase query to match bookmark names.
pub(crate) fn name_matches(name: &str, query: &str) -> bool {
    query.is_empty() || name.to_lowercase().contains(query)
}

/// Only the name is visible. Rename and deletion live in the context menu.
pub(crate) fn card<C: AnnotationSidebarTarget>(target: &Entity<C>, index: usize, bookmark: &Annotation, selected: bool, theme: components::BrowserTheme) -> AnyElement {
    let navigate = target.clone();
    let location = bookmark.clone();
    let keyboard = target.clone();
    let keyboard_bookmark = bookmark.clone();
    let menu_target = target.clone();
    let id = bookmark.id.clone();
    components::reader_state_card(("reader-bookmark", index), selected, theme)
        .debug_selector(move || format!("reader-bookmark-card-{index}"))
        .focusable()
        .tab_stop(true)
        .on_click(move |_, _, cx| navigate.update(cx, |this, cx| this.go_to_annotation(&location, cx)))
        .on_key_down(move |event, window, cx| {
            if event.keystroke.modifiers != gpui::Modifiers::default() {
                return;
            }
            match event.keystroke.key.as_str() {
                "enter" | "space" => keyboard.update(cx, |this, cx| this.go_to_annotation(&keyboard_bookmark, cx)),
                "f2" => keyboard.update(cx, |this, cx| this.select_sidebar_annotation(keyboard_bookmark.id.clone(), window, cx)),
                "delete" | "backspace" => keyboard.update(cx, |this, cx| this.delete_sidebar_annotation(keyboard_bookmark.id.clone(), cx)),
                _ => return,
            }
            cx.stop_propagation();
        })
        .context_menu(move |menu, _, _| {
            let rename = menu_target.clone();
            let rename_id = id.clone();
            let delete = menu_target.clone();
            let delete_id = id.clone();
            menu.item(components::menu_item("Rename bookmark", move |_, window, cx| {
                rename.update(cx, |this, cx| this.select_sidebar_annotation(rename_id.clone(), window, cx));
            }))
            .item(components::menu_item("Delete bookmark", move |_, _, cx| {
                delete.update(cx, |this, cx| this.delete_sidebar_annotation(delete_id.clone(), cx));
            }))
        })
        .child(components::reader_state_note(bookmark.note.clone(), theme).text_color(theme.text))
        .into_any_element()
}
