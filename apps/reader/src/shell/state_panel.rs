//! Shared reader widgets used by EPUB and PDF readers.

use std::rc::Rc;

use gpui::prelude::*;
use gpui::{AnyElement, App, Context, Entity, Window};
use gpui_component::list::ListItem;
use gpui_component::tree::{TreeState, tree};
use book_model::ReaderAnnotation;
use ui_components as components;

use crate::settings::ReaderSettings;
use crate::shell::marks::MarkSession;

/// The one sidebar a reader may display at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SidebarPanel {
    Contents,
    Annotations,
}

/// Shared sidebar visibility and selection state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SidebarState {
    active: Option<SidebarPanel>,
}

impl SidebarState {
    pub(crate) fn from_preferences(cx: &App) -> Self {
        Self::with_contents_visible(ReaderSettings::preferences(cx).toc_visible)
    }

    fn with_contents_visible(visible: bool) -> Self {
        Self { active: visible.then_some(SidebarPanel::Contents) }
    }

    pub(crate) fn active(self) -> Option<SidebarPanel> {
        self.active
    }

    pub(crate) fn shows(self, panel: SidebarPanel) -> bool {
        self.active == Some(panel)
    }

    pub(crate) fn is_visible(self) -> bool {
        self.active.is_some()
    }

    pub(crate) fn show(&mut self, panel: SidebarPanel) -> bool {
        if self.active == Some(panel) {
            return false;
        }
        self.active = Some(panel);
        true
    }

    pub(crate) fn close(&mut self) -> bool {
        if self.active.is_none() {
            return false;
        }
        self.active = None;
        true
    }

    fn set_contents_visible(&mut self, visible: bool) -> bool {
        if visible {
            self.show(SidebarPanel::Contents)
        } else if self.shows(SidebarPanel::Contents) {
            self.close()
        } else {
            false
        }
    }
}

/// Selects one sidebar panel and persists whether contents should reopen in a
/// future reader session.
pub(crate) fn select_panel<C: 'static>(sidebar: &mut SidebarState, panel: SidebarPanel, cx: &mut Context<C>) -> bool {
    let state_changed = sidebar.show(panel);
    let preference_changed = ReaderSettings::update(cx, |preferences| preferences.toc_visible = panel == SidebarPanel::Contents);
    state_changed || preference_changed
}

/// Shows or hides contents without disturbing an open annotations panel.
pub(crate) fn set_contents_visible<C: 'static>(sidebar: &mut SidebarState, visible: bool, cx: &mut Context<C>) -> bool {
    let state_changed = sidebar.set_contents_visible(visible);
    let preference_changed = ReaderSettings::update(cx, |preferences| preferences.toc_visible = visible);
    state_changed || preference_changed
}

/// Applies the common sidebar-button policy. Contents is preferred when the
/// document has it; annotations is the fallback for outline-free PDFs.
pub(crate) fn toggle_sidebar<C: 'static>(sidebar: &mut SidebarState, contents_available: bool, cx: &mut Context<C>) -> bool {
    let visible = sidebar.shows(SidebarPanel::Annotations) || (contents_available && sidebar.shows(SidebarPanel::Contents));
    if visible {
        let state_changed = sidebar.close();
        let preference_changed = ReaderSettings::update(cx, |preferences| preferences.toc_visible = false);
        state_changed || preference_changed
    } else if contents_available {
        set_contents_visible(sidebar, true, cx)
    } else {
        select_panel(sidebar, SidebarPanel::Annotations, cx)
    }
}

pub(crate) fn reader_sidebar_tabs<C, F>(id: &'static str, target: &Entity<C>, show_annotations: bool, outline_available: bool, cx: &mut Context<C>, on_toggle: F) -> AnyElement
where
    C: 'static,
    F: Fn(bool, &mut C, &mut Context<C>) + 'static,
{
    let theme = components::browser_theme(cx);
    let on_toggle = Rc::new(on_toggle);
    let mut tabs = components::browser_tab_strip();
    for (annotations, label) in [(false, "Contents"), (true, "Annotations")] {
        if !annotations && !outline_available {
            continue;
        }
        let target = target.clone();
        let on_toggle = on_toggle.clone();
        let selected = annotations == (show_annotations || !outline_available);
        tabs = tabs.child(components::browser_tab((id, usize::from(annotations)), label, None::<usize>, selected, theme).on_click(move |_, _, cx| target.update(cx, |this, cx| on_toggle(annotations, this, cx))));
    }
    components::browser_tab_bar().id(id).border_b_1().border_color(theme.rule).child(tabs).into_any_element()
}

/// The annotation rows below the panel header.
///
/// The caller appends these to its own panel container, because that is the
/// one part of the sidebar the readers build differently. Element ids are not
/// namespaced per reader: a window hosts one reader, so the two can never
/// collide.
pub(crate) fn reader_state_rows<C, S, G, D>(target: &Entity<C>, marks: &MarkSession, no_annotations: &'static str, theme: components::BrowserTheme, select_annotation: S, go_to_annotation: G, delete_annotation: D) -> Vec<AnyElement>
where
    C: 'static,
    S: Fn(String, &mut Window, &mut C, &mut Context<C>) + 'static,
    G: Fn(&ReaderAnnotation, &mut C, &mut Context<C>) + 'static,
    D: Fn(String, &mut C, &mut Context<C>) + 'static,
{
    let select_annotation = Rc::new(select_annotation);
    let go_to_annotation = Rc::new(go_to_annotation);
    let delete_annotation = Rc::new(delete_annotation);
    let mut rows = Vec::new();
    if marks.annotations().is_empty() {
        rows.push(components::reader_state_note(no_annotations, theme).into_any_element());
    }
    for (index, annotation) in marks.annotations().iter().cloned().enumerate() {
        let selected = marks.selected_id() == Some(annotation.id.as_str());
        let note = annotation.note.clone();
        let editable = !annotation.is_external_pdf_annotation();
        let select = select_annotation.clone();
        let select_target = target.clone();
        let select_id = annotation.id.clone();
        let go = go_to_annotation.clone();
        let go_target = target.clone();
        let go_annotation = annotation.clone();
        let delete = delete_annotation.clone();
        let delete_target = target.clone();
        let delete_id = annotation.id.clone();
        rows.push(
            components::reader_state_card(("reader-state-annotation", index), selected, theme)
                .on_click(move |_, window, cx| {
                    let id = select_id.clone();
                    select_target.update(cx, |this, cx| select(id, window, this, cx));
                })
                .when(!annotation.exact_text.is_empty(), |card| card.child(components::reader_state_quote(format!("“{}”", annotation.exact_text))))
                .when(!note.is_empty(), |card| card.child(components::reader_state_note(note, theme)))
                .child(
                    components::reader_state_actions()
                        .child(components::reader_sidebar_action_button(("reader-state-annotation-go", index), "Go", gpui_component::IconName::ArrowRight, true).on_click(move |_, _, cx| {
                            go_target.update(cx, |this, cx| go(&go_annotation, this, cx));
                        }))
                        // Deleting lives here now that the note editor has no
                        // buttons of its own: an icon in `danger`, apart from
                        // the actions that keep the annotation.
                        .when(editable, |actions| {
                            actions.child(components::reader_annotation_delete_button(("reader-state-annotation-delete", index), gpui::px(components::READER_ANNOTATION_ACTION_HEIGHT), theme).on_click(move |_, _, cx| {
                                let id = delete_id.clone();
                                delete_target.update(cx, |this, cx| delete(id, this, cx));
                            }))
                        }),
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
    // `selected` is the entry the reader is inside: the renderer resolves the
    // position to a link and the sidebar selects it. It used to be discarded,
    // so the list gave no sign of where you were and the only indication was
    // the chapter name in the toolbar.
    tree(toc_tree, move |index, entry, selected, _window, _cx| {
        let item = entry.item();
        let id = item.id.to_string();
        let navigation_target = target.clone();
        let on_navigate = on_navigate.clone();
        let row = components::ContentsRow { depth: entry.depth(), has_children: entry.is_folder(), expanded: entry.is_expanded(), marked: selected, focused: false };
        // The tree toggles an entry when its row is clicked, so navigation
        // hangs on the title, which stops the click reaching the row.
        let title = components::contents_title(("reader-toc-title", index), item.label.clone(), theme).on_click(move |_, window, cx| {
            let id = id.clone();
            navigation_target.update(cx, |this, cx| on_navigate(&id, window, this, cx));
        });
        // The tree insists on a `ListItem`, which is where its own click and
        // keyboard handling attach; the row inside it is the shared one, so a
        // sidebar entry and a chapter in the player are the same drawing.
        ListItem::new(("reader-toc-item", index)).w_full().min_w_0().p_0().child(components::contents_row(
            ("reader-toc-row", index),
            row,
            components::contents_disclosure(("reader-toc-disclosure", index), row, theme),
            title,
            None,
            None,
            theme,
        ))
    })
    .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{SidebarPanel, SidebarState};

    #[test]
    fn a_sidebar_has_exactly_one_active_panel() {
        let mut sidebar = SidebarState::with_contents_visible(true);
        assert_eq!(sidebar.active(), Some(SidebarPanel::Contents));

        sidebar.show(SidebarPanel::Annotations);
        assert_eq!(sidebar.active(), Some(SidebarPanel::Annotations));
    }

    #[test]
    fn hiding_contents_does_not_close_annotations() {
        let mut sidebar = SidebarState::default();
        sidebar.show(SidebarPanel::Annotations);
        sidebar.set_contents_visible(false);
        assert_eq!(sidebar.active(), Some(SidebarPanel::Annotations));
    }
}
