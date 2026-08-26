//! Shared reader widgets used by EPUB and PDF readers.

use std::rc::Rc;

use app_contract::reader_state::{ReaderAnnotation, ReaderBookmark};
use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity, Window};
use gpui_component::Sizable;
use gpui_component::tab::TabBar;
use gpui_component::tree::{TreeState, tree};
use ui_components as components;

use crate::shell::marks::MarksState;

pub(crate) fn reader_sidebar_tabs<C, F>(id: &'static str, target: &Entity<C>, show_annotations: bool, outline_available: bool, _: &mut Context<C>, on_toggle: F) -> AnyElement
where
    C: 'static,
    F: Fn(bool, &mut C, &mut Context<C>) + 'static,
{
    let target = target.clone();
    let mut buttons = Vec::with_capacity(2);
    if outline_available {
        buttons.push(components::reader_choice_button("reader-sidebar-contents", "Contents", !show_annotations));
    }
    buttons.push(components::reader_choice_button("reader-sidebar-annotations", "Annotations", show_annotations || !outline_available));

    components::reader_choice_button_group(id, buttons)
        .w_full()
        .on_click(move |selected, _, cx| {
            let annotations = !outline_available || selected.contains(&1);
            target.update(cx, |this, cx| {
                on_toggle(annotations, this, cx);
            });
        })
        .into_any_element()
}

pub(crate) fn reader_state_tabs<C, F>(id: &'static str, target: &Entity<C>, showing_bookmarks: bool, marks: &MarksState, _: &mut Context<C>, on_toggle: F) -> AnyElement
where
    C: 'static,
    F: Fn(bool, &mut C, &mut Context<C>) + 'static,
{
    let target = target.clone();
    TabBar::new(id)
        .segmented()
        .small()
        .selected_index(usize::from(showing_bookmarks))
        .child(format!("Highlights ({})", marks.annotations.len()))
        .child(format!("Bookmarks ({})", marks.bookmarks.len()))
        .on_click(move |index, _, cx| {
            target.update(cx, |this, cx| {
                on_toggle(*index == 1, this, cx);
            });
        })
        .into_any_element()
}

/// The wording that differs between readers, which is only ever about what a
/// location is called: a place in the text, or a page.
pub(crate) struct StatePanelWording {
    pub(crate) bookmark_tooltip: &'static str,
    pub(crate) no_bookmarks: &'static str,
    pub(crate) no_annotations: &'static str,
}

/// What each control in the panel does, in terms of the reader that owns it.
///
/// Every list row needs its own copy of the callback it fires, so these are
/// reference counted rather than moved into the first card that asks.
pub(crate) struct StatePanelActions<C: 'static> {
    pub(crate) add_bookmark: Rc<dyn Fn(&mut C, &mut Context<C>)>,
    pub(crate) go_to_bookmark: Rc<dyn Fn(&ReaderBookmark, &mut C, &mut Context<C>)>,
    pub(crate) delete_bookmark: Rc<dyn Fn(String, &mut C, &mut Context<C>)>,
    pub(crate) select_annotation: Rc<dyn Fn(String, &mut Window, &mut C, &mut Context<C>)>,
    pub(crate) go_to_annotation: Rc<dyn Fn(&ReaderAnnotation, &mut C, &mut Context<C>)>,
    pub(crate) delete_annotation: Rc<dyn Fn(String, &mut C, &mut Context<C>)>,
}

/// The rows below the panel's tabs: either the bookmark list or the annotation
/// list, whichever tab is showing.
///
/// The caller appends these to its own panel container, because that is the
/// one part of the sidebar the readers build differently. Element ids are not
/// namespaced per reader: a window hosts one reader, so the two can never
/// collide.
pub(crate) fn reader_state_rows<C: 'static>(
    target: &Entity<C>, marks: &MarksState, showing_bookmarks: bool, can_bookmark: bool, wording: &StatePanelWording, actions: &StatePanelActions<C>, theme: components::BrowserTheme,
) -> Vec<AnyElement> {
    let mut rows = Vec::new();
    if showing_bookmarks {
        let add_bookmark = actions.add_bookmark.clone();
        let add_target = target.clone();
        rows.push(
            components::reader_sidebar_action_button("reader-state-bookmark-current", "Bookmark here", gpui_component::IconName::Star, can_bookmark)
                .tooltip(wording.bookmark_tooltip)
                .on_click(move |_, _, cx| {
                    add_target.update(cx, |this, cx| add_bookmark(this, cx));
                })
                .into_any_element(),
        );
        if marks.bookmarks.is_empty() {
            rows.push(components::reader_state_note(wording.no_bookmarks, theme).into_any_element());
        }
        for (index, bookmark) in marks.bookmarks.iter().cloned().enumerate() {
            let go = actions.go_to_bookmark.clone();
            let go_target = target.clone();
            let go_bookmark = bookmark.clone();
            let delete = actions.delete_bookmark.clone();
            let delete_target = target.clone();
            let delete_id = bookmark.id.clone();
            rows.push(
                components::reader_state_card(("reader-state-bookmark", index), false, theme)
                    .child(components::reader_state_quote(bookmark.label))
                    .child(
                        components::reader_state_actions()
                            .child(components::reader_sidebar_action_button(("reader-state-bookmark-go", index), "Go", gpui_component::IconName::ArrowRight, true).on_click(move |_, _, cx| {
                                go_target.update(cx, |this, cx| go(&go_bookmark, this, cx));
                            }))
                            .child(components::reader_sidebar_action_button(("reader-state-bookmark-delete", index), "Delete", gpui_component::IconName::Delete, true).on_click(move |_, _, cx| {
                                let id = delete_id.clone();
                                delete_target.update(cx, |this, cx| delete(id, this, cx));
                            })),
                    )
                    .into_any_element(),
            );
        }
        return rows;
    }

    if marks.annotations.is_empty() {
        rows.push(components::reader_state_note(wording.no_annotations, theme).into_any_element());
    }
    for (index, annotation) in marks.annotations.iter().cloned().enumerate() {
        let selected = marks.selected_id.as_deref() == Some(annotation.id.as_str());
        let note = annotation.note.clone();
        let select = actions.select_annotation.clone();
        let select_target = target.clone();
        let select_id = annotation.id.clone();
        let go = actions.go_to_annotation.clone();
        let go_target = target.clone();
        let go_annotation = annotation.clone();
        let delete = actions.delete_annotation.clone();
        let delete_target = target.clone();
        let delete_id = annotation.id.clone();
        rows.push(
            components::reader_state_card(("reader-state-annotation", index), selected, theme)
                .on_click(move |_, window, cx| {
                    let id = select_id.clone();
                    select_target.update(cx, |this, cx| select(id, window, this, cx));
                })
                .child(components::reader_state_quote(format!("“{}”", annotation.exact_text)))
                .when(!note.is_empty(), |card| card.child(components::reader_state_note(note, theme)))
                .child(
                    components::reader_state_actions()
                        .child(components::reader_sidebar_action_button(("reader-state-annotation-go", index), "Go", gpui_component::IconName::ArrowRight, true).on_click(move |_, _, cx| {
                            go_target.update(cx, |this, cx| go(&go_annotation, this, cx));
                        }))
                        .child(components::reader_sidebar_action_button(("reader-state-annotation-delete", index), "Delete", gpui_component::IconName::Delete, true).on_click(move |_, _, cx| {
                            let id = delete_id.clone();
                            delete_target.update(cx, |this, cx| delete(id, this, cx));
                        })),
                )
                .into_any_element(),
        );
    }
    rows
}

/// The table-of-contents tree.
///
/// `on_navigate` is handed the id of the entry that was clicked. It has to do
/// two things in every reader: go there, and hand the keyboard back to the
/// document. The tree binds the arrow keys for its own selection, so it holds
/// focus while it is being used, and paging is bound in the document's key
/// context — following an entry means "take me there to read", not "leave me
/// in the sidebar".
pub(crate) fn reader_toc_tree<C, F>(toc_tree: &Entity<TreeState>, target: &Entity<C>, theme: components::BrowserTheme, on_navigate: F) -> AnyElement
where
    C: 'static,
    F: Fn(&str, &mut Window, &mut C, &mut Context<C>) + 'static,
{
    let target = target.clone();
    let on_navigate = Rc::new(on_navigate);
    tree(toc_tree, move |index, entry, _selected, _window, _cx| {
        let item = entry.item();
        let id = item.id.to_string();
        let navigation_target = target.clone();
        let on_navigate = on_navigate.clone();
        let title = components::reader_toc_title(("reader-toc-title", index), item.label.clone(), theme).on_click(move |_, window, cx| {
            let id = id.clone();
            navigation_target.update(cx, |this, cx| on_navigate(&id, window, this, cx));
        });
        components::reader_toc_tree_item(("reader-toc-item", index), ("reader-toc-disclosure", index), entry.depth(), entry.is_folder(), entry.is_expanded(), title, theme)
    })
    .into_any_element()
}

pub(crate) fn now_millis() -> i64 {
    web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|duration| duration.as_millis() as i64).unwrap_or(0)
}
