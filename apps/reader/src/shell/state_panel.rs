//! Shared reader widgets used by EPUB and PDF readers.

use std::{collections::HashMap, rc::Rc};

use gpui::prelude::*;
use gpui::{Anchor, AnyElement, App, Context, Entity, Window};
use gpui_component::IconName;
use gpui_component::list::ListItem;
use gpui_component::menu::DropdownMenu as _;
use gpui_component::tree::{TreeState, tree};
use library_backend::Annotation;
use ui_components as components;

use crate::settings::ReaderSettings;
pub(crate) use crate::settings::SidebarTab;
use crate::shell::marks::MarkSession;

/// Shared sidebar tab-selection state.
///
/// The sidebar itself is always visible on desktop while chrome is shown, so
/// `active` is normally `Some`. It only goes `None` on the mobile bottom-bar
/// layout, where tapping the already-open tab's icon closes its sheet while
/// the bar itself stays put. On Kobo the sidebar starts closed, matching the
/// device's minimal-chrome default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SidebarState {
    active: Option<SidebarTab>,
    last_tab: SidebarTab,
}

impl SidebarState {
    pub(crate) fn from_preferences(cx: &App) -> Self {
        let tab = ReaderSettings::preferences(cx).active_sidebar_tab;
        Self { active: (!cfg!(any(feature = "mobile", feature = "kobo"))).then_some(tab), last_tab: tab }
    }

    pub(crate) fn active(self) -> Option<SidebarTab> {
        self.active
    }

    /// The tab that should actually be drawn: falls back to Annotations when
    /// the stored/selected tab is Contents but no outline exists.
    pub(crate) fn effective(self, contents_available: bool) -> Option<SidebarTab> {
        match self.active {
            Some(SidebarTab::Contents) if !contents_available => Some(SidebarTab::Annotations),
            other => other,
        }
    }

    /// [`Self::effective`], defaulting to a sensible tab instead of `None`.
    ///
    /// A persistent desktop split always shows some tab — there is no closed
    /// state to fall back to once the toolbar's toggle button is gone — so
    /// this is what the split render path reaches for instead of `effective`.
    pub(crate) fn effective_or_default(self, contents_available: bool) -> SidebarTab {
        match self.active.unwrap_or(self.last_tab) {
            SidebarTab::Contents if !contents_available => SidebarTab::Annotations,
            tab => tab,
        }
    }

    pub(crate) fn shows(self, tab: SidebarTab) -> bool {
        self.active == Some(tab)
    }

    pub(crate) fn is_visible(self) -> bool {
        self.active.is_some()
    }

    pub(crate) fn show(&mut self, tab: SidebarTab) -> bool {
        if self.active == Some(tab) {
            return false;
        }
        self.active = Some(tab);
        self.last_tab = tab;
        true
    }

    /// Reopens the last selected tab without changing the saved preference.
    pub(crate) fn reopen(&mut self) -> bool {
        self.show(self.last_tab)
    }

    pub(crate) fn close(&mut self) -> bool {
        if self.active.is_none() {
            return false;
        }
        self.active = None;
        true
    }
}

/// Selects a sidebar tab and persists it as the tab to reopen next session.
pub(crate) fn select_tab<C: 'static>(sidebar: &mut SidebarState, tab: SidebarTab, cx: &mut Context<C>) -> bool {
    let state_changed = sidebar.show(tab);
    let preference_changed = ReaderSettings::update(cx, |preferences| preferences.active_sidebar_tab = tab);
    state_changed || preference_changed
}

/// The mobile bottom bar's self-toggling policy: tapping the tab that is
/// already open closes its sheet; tapping any other tab opens it. Contents
/// falls back to Annotations when there is no outline, the same fallback
/// `SidebarState::effective` applies on desktop.
pub(crate) fn toggle_tab<C: 'static>(sidebar: &mut SidebarState, tab: SidebarTab, contents_available: bool, cx: &mut Context<C>) -> bool {
    let tab = if tab == SidebarTab::Contents && !contents_available { SidebarTab::Annotations } else { tab };
    if sidebar.shows(tab) { sidebar.close() } else { select_tab(sidebar, tab, cx) }
}

/// Cycles only through tabs available in this book, wrapping at either end.
pub(crate) fn adjacent_sidebar_tab(active: SidebarTab, contents_available: bool, direction: i32) -> SidebarTab {
    let tabs: &[SidebarTab] = if contents_available { &[SidebarTab::Contents, SidebarTab::Annotations, SidebarTab::Settings] } else { &[SidebarTab::Annotations, SidebarTab::Settings] };
    let index = tabs.iter().position(|tab| *tab == active).unwrap_or(0) as i32;
    tabs[(index + direction).rem_euclid(tabs.len() as i32) as usize]
}

/// The three-tab strip shared by the desktop sidebar and the mobile sheet
/// header. Contents is omitted entirely when there is no outline, rather than
/// shown disabled — a PDF without bookmarks never had a Contents tab to begin
/// with.
pub(crate) fn reader_sidebar_tabs<C, F>(id: &'static str, target: &Entity<C>, active: SidebarTab, contents_available: bool, theme: components::BrowserTheme, on_select: F) -> AnyElement
where
    C: 'static,
    F: Fn(SidebarTab, &mut C, &mut Context<C>) + 'static,
{
    let on_select = Rc::new(on_select);
    let mut tabs = components::browser_tab_strip();
    for (index, tab, label) in [(0usize, SidebarTab::Contents, "Contents"), (1, SidebarTab::Annotations, "Annotations"), (2, SidebarTab::Settings, "Settings")] {
        if tab == SidebarTab::Contents && !contents_available {
            continue;
        }
        let keyboard_target = target.clone();
        let keyboard_select = on_select.clone();
        let target = target.clone();
        let on_select = on_select.clone();
        tabs = tabs.child(
            components::reader_choice_button((id, index), label, tab == active, theme)
                .on_key_down(move |event, _, cx| {
                    if active == SidebarTab::Settings || event.keystroke.modifiers.control || event.keystroke.modifiers.alt || event.keystroke.modifiers.platform {
                        return;
                    }
                    let direction = match event.keystroke.key.as_str() {
                        "left" => -1,
                        "right" => 1,
                        _ => return,
                    };
                    let tab = adjacent_sidebar_tab(active, contents_available, direction);
                    keyboard_target.update(cx, |this, cx| keyboard_select(tab, this, cx));
                    cx.stop_propagation();
                })
                .on_click(move |_, _, cx| target.update(cx, |this, cx| on_select(tab, this, cx))),
        );
    }
    components::browser_tab_bar().id(id).border_b_1().border_color(theme.rule).child(tabs).into_any_element()
}

pub(crate) trait AnnotationSidebarTarget: Sized + 'static {
    fn select_sidebar_annotation(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>);
    fn go_to_annotation(&mut self, annotation: &Annotation, cx: &mut Context<Self>);
    fn delete_sidebar_annotation(&mut self, id: String, cx: &mut Context<Self>);
}

/// The annotation rows below the panel header.
///
/// The caller appends these to its own panel container, because that is the
/// one part of the sidebar the readers build differently. Element ids are not
/// namespaced per reader: a window hosts one reader, so the two can never
/// collide.
pub(crate) fn reader_state_rows<C, H>(target: &Entity<C>, marks: &MarkSession, no_annotations: &'static str, theme: components::BrowserTheme, group_for_annotation: H) -> Vec<AnyElement>
where
    C: AnnotationSidebarTarget,
    H: Fn(&Annotation) -> (String, String),
{
    let mut rows = Vec::new();
    if marks.annotations().is_empty() {
        rows.push(components::reader_state_note(no_annotations, theme).p(gpui::px(12.0)).into_any_element());
    }
    let mut groups: Vec<(String, Vec<(usize, Annotation)>)> = Vec::new();
    let mut group_indices = HashMap::<String, usize>::new();
    for (index, annotation) in marks.annotations().iter().cloned().enumerate() {
        let (key, title) = group_for_annotation(&annotation);
        let group_index = *group_indices.entry(key).or_insert_with(|| {
            groups.push((title, Vec::new()));
            groups.len() - 1
        });
        groups[group_index].1.push((index, annotation));
    }
    for (title, annotations) in groups {
        rows.push(components::reader_annotation_group_heading(title, theme).into_any_element());
        for (index, annotation) in annotations {
            let selected = marks.selected_id() == Some(annotation.id.as_str());
            let note = annotation.note.clone();
            let editable = !annotation.is_external_pdf_annotation();
            let select_target = target.clone();
            let select_id = annotation.id.clone();
            let go_target = target.clone();
            let go_annotation = annotation.clone();
            let actions = if editable {
                let delete_target = target.clone();
                let delete_id = annotation.id.clone();
                Some(components::outlined_icon_button(("reader-state-annotation-more", index), "Annotation actions", IconName::Ellipsis, theme).dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                    let delete_target = delete_target.clone();
                    let delete_id = delete_id.clone();
                    menu.item(components::menu_item_with_icon("Delete annotation", IconName::Delete, move |_, _, cx| {
                        let id = delete_id.clone();
                        delete_target.update(cx, |this, cx| this.delete_sidebar_annotation(id, cx));
                    }))
                }))
            } else {
                None
            };
            rows.push(
                components::reader_state_card(("reader-state-annotation", index), selected, theme)
                    .on_click(move |_, window, cx| {
                        let id = select_id.clone();
                        select_target.update(cx, |this, cx| this.select_sidebar_annotation(id, window, cx));
                        // Clicking the annotation goes to its passage; there is no Go button.
                        let annotation = go_annotation.clone();
                        go_target.update(cx, |this, cx| this.go_to_annotation(&annotation, cx));
                    })
                    .child(components::reader_annotation_color_bar(annotation.color))
                    .when(!annotation.exact_text.is_empty(), |card| card.child(components::reader_state_quote(format!("“{}”", annotation.exact_text))))
                    .when(!note.is_empty(), |card| card.child(components::reader_state_note(note, theme)))
                    .children(actions.map(|actions| {
                        components::action_row().id(("reader-state-annotation-actions", index)).justify_end().on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation()).on_click(|_, _, cx| cx.stop_propagation()).child(actions)
                    }))
                    .into_any_element(),
            );
        }
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
pub(crate) fn reader_toc_tree<C, F>(toc_tree: &Entity<TreeState>, target: &Entity<C>, active_id: Option<&str>, theme: components::BrowserTheme, cx: &gpui::App, on_navigate: F) -> AnyElement
where
    C: 'static,
    F: Fn(&str, &mut Window, &mut C, &mut Context<C>) + 'static,
{
    let target = target.clone();
    let on_navigate = Rc::new(on_navigate);
    let current_index = active_id.and_then(|id| toc_tree.read(cx).index_of_id(id));
    let active_id = active_id.map(str::to_owned);
    // `selected` is the entry the reader is inside: the renderer resolves the
    // position to a link and the sidebar selects it. It used to be discarded,
    // so the list gave no sign of where you were and the only indication was
    // the chapter name in the toolbar.
    let keyboard_tree = toc_tree.clone();
    let keyboard_target = target.clone();
    let keyboard_navigate = on_navigate.clone();
    gpui::div()
        .size_full()
        .on_key_down(move |event, window, cx| {
            if event.keystroke.key == "enter" && !event.keystroke.modifiers.control && !event.keystroke.modifiers.alt && !event.keystroke.modifiers.platform {
                let id = keyboard_tree.read(cx).selected_item().filter(|item| !item.is_disabled()).map(|item| item.id.to_string());
                if let Some(id) = id {
                    keyboard_target.update(cx, |this, cx| keyboard_navigate(&id, window, this, cx));
                    cx.stop_propagation();
                }
            }
        })
        .child(tree(toc_tree, move |index, entry, selected, _window, _cx| {
            let item = entry.item();
            let id = item.id.to_string();
            let navigation_target = target.clone();
            let on_navigate = on_navigate.clone();
            let marked = active_id.as_deref() == Some(item.id.as_ref());
            let row = components::ContentsRow {
                depth: entry.depth(),
                has_children: entry.is_folder(),
                expanded: entry.is_expanded(),
                marked,
                read: !entry.is_folder() && current_index.is_some_and(|current| index < current),
                focused: selected && !marked,
            };
            // The tree toggles an entry when its row is clicked, so navigation
            // hangs on the title, which stops the click reaching the row.
            let title = components::contents_title(("reader-toc-title", index), item.label.clone(), theme).on_click(move |_, window, cx| {
                let id = id.clone();
                navigation_target.update(cx, |this, cx| on_navigate(&id, window, this, cx));
            });
            // The tree insists on a `ListItem`, which is where its own click and
            // keyboard handling attach; the row inside it is the shared one, so a
            // sidebar entry and a chapter in the player are the same drawing.
            // The tree uses a uniform list. Two title lines fit in 2.25rem when
            // their line height is 1.1 and the row has no vertical padding.
            ListItem::new(("reader-toc-item", index)).w_full().min_w_0().p_0().child(
                components::contents_row(("reader-toc-row", index), row, components::contents_disclosure(("reader-toc-disclosure", index), row, theme), title.line_height(gpui::relative(1.1)), None, None, theme).h(gpui::rems(2.25)).py_0(),
            )
        }))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{SidebarState, SidebarTab, adjacent_sidebar_tab};

    #[test]
    fn tab_navigation_wraps_and_skips_unavailable_contents() {
        assert_eq!(adjacent_sidebar_tab(SidebarTab::Settings, true, 1), SidebarTab::Contents);
        assert_eq!(adjacent_sidebar_tab(SidebarTab::Contents, true, -1), SidebarTab::Settings);
        assert_eq!(adjacent_sidebar_tab(SidebarTab::Settings, false, 1), SidebarTab::Annotations);
        assert_eq!(adjacent_sidebar_tab(SidebarTab::Annotations, false, -1), SidebarTab::Settings);
    }

    #[test]
    fn closing_and_reopening_preserves_selected_tab() {
        for tab in [SidebarTab::Contents, SidebarTab::Annotations, SidebarTab::Settings] {
            let mut sidebar = SidebarState::default();
            sidebar.show(tab);
            assert!(sidebar.close());
            assert_eq!(sidebar.active(), None);
            assert_eq!(sidebar.effective_or_default(true), tab);
            assert!(sidebar.reopen());
            assert_eq!(sidebar.active(), Some(tab));
        }
    }

    #[test]
    fn missing_contents_falls_back_without_forgetting_selection() {
        let mut sidebar = SidebarState::default();
        sidebar.show(SidebarTab::Contents);
        sidebar.close();
        assert_eq!(sidebar.effective_or_default(false), SidebarTab::Annotations);
        sidebar.reopen();
        assert_eq!(sidebar.effective(false), Some(SidebarTab::Annotations));
        assert_eq!(sidebar.effective(true), Some(SidebarTab::Contents));
    }
}
