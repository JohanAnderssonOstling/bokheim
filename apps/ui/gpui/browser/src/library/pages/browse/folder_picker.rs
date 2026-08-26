//! Pure folder destination selection for folder-scoped browse actions.

use std::collections::HashSet;
use std::rc::Rc;

use app_contract::library::LibraryFolderDestination;
use gpui::prelude::*;
use gpui::{Context, EventEmitter, IntoElement, MouseButton, Render, Window, div, px};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::scroll::ScrollableElement;
use gpui_component::{Disableable, IconName, Sizable};
use sync_common::{DirId, ROOT_DIR_ID};
use ui_components as components;
use ui_components::ScrollableElement as BokheimScrollableElement;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FolderPickerEvent {
    Cancelled,
    Selected(DirId),
}

/// Browses the folder tree from a current directory and returns one directory
/// ID. It deliberately has no knowledge of the action using that destination.
pub(crate) struct FolderPicker {
    destinations: Rc<Vec<LibraryFolderDestination>>,
    selected_id: DirId,
    disabled_ids: HashSet<DirId>,
}

impl EventEmitter<FolderPickerEvent> for FolderPicker {}

impl FolderPicker {
    pub(crate) fn new(current_dir: DirId, destinations: Vec<LibraryFolderDestination>) -> Self {
        let selected_id = if destinations.iter().any(|destination| destination.id == current_dir) { current_dir } else { ROOT_DIR_ID };
        Self { destinations: Rc::new(destinations), selected_id, disabled_ids: HashSet::new() }
    }

    pub(crate) fn disabling(mut self, id: DirId) -> Self {
        self.disabled_ids.insert(id);
        if self.selected_id == id {
            self.selected_id = self.destinations.iter().find(|destination| !self.disabled_ids.contains(&destination.id)).map(|destination| destination.id).unwrap_or(ROOT_DIR_ID);
        }
        self
    }

    fn destination(&self, id: DirId) -> Option<&LibraryFolderDestination> {
        self.destinations.iter().find(|destination| destination.id == id)
    }

    /// IDs from the first folder below Library through the selected folder.
    fn selected_path(&self) -> Vec<DirId> {
        let mut reversed = Vec::new();
        let mut cursor = self.selected_id;
        let mut visited = HashSet::new();
        while cursor != ROOT_DIR_ID && visited.insert(cursor) {
            let Some(destination) = self.destination(cursor) else { break };
            reversed.push(cursor);
            let Some(parent_id) = destination.parent_id else { break };
            cursor = parent_id;
        }
        reversed.reverse();
        reversed
    }

    fn columns(&self) -> Vec<Vec<LibraryFolderDestination>> {
        let selected_path = self.selected_path();
        let mut columns = Vec::new();
        if let Some(root) = self.destination(ROOT_DIR_ID) {
            columns.push(vec![root.clone()]);
        }
        let mut parent_id = ROOT_DIR_ID;
        let mut depth = 0;
        loop {
            let children = self.destinations.iter().filter(|destination| destination.parent_id == Some(parent_id)).cloned().collect::<Vec<_>>();
            if children.is_empty() {
                break;
            }
            columns.push(children);
            let Some(selected_id) = selected_path.get(depth).copied() else { break };
            parent_id = selected_id;
            depth += 1;
        }
        columns
    }

    fn confirm(&self, cx: &mut Context<Self>) {
        let Some(destination) = self.destination(self.selected_id) else { return };
        if self.disabled_ids.contains(&destination.id) {
            return;
        }
        cx.emit(FolderPickerEvent::Selected(destination.id));
    }

    fn cancel(&self, cx: &mut Context<Self>) {
        cx.emit(FolderPickerEvent::Cancelled);
    }
}

impl Render for FolderPicker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let picker = cx.entity();
        let selected_path = self.selected_path();
        let mut column_strip = div().flex_1().min_h_0().min_w_0().flex().flex_row().overflow_x_scrollbar();
        for (depth, destinations) in self.columns().into_iter().enumerate() {
            let mut contents = div().w_full().flex().flex_col().p(px(components::SPACE_XS));
            for destination in destinations {
                let on_path = destination.id == self.selected_id || selected_path.contains(&destination.id);
                let disabled = self.disabled_ids.contains(&destination.id);
                let destination_id = destination.id;
                let select_picker = picker.clone();
                contents = contents.child(
                    Button::new(format!("folder-picker-{destination_id}"))
                        .ghost()
                        .small()
                        .disabled(disabled)
                        .w_full()
                        .h(px(36.0))
                        .justify_start()
                        .icon(IconName::Folder)
                        .when(on_path, |button| button.bg(theme.accent_soft))
                        .label(destination.label)
                        .on_click(move |_, _, cx| {
                            select_picker.update(cx, |picker, cx| {
                                picker.selected_id = destination_id;
                                cx.notify();
                            });
                        }),
                );
            }
            let column = div()
                .id(format!("folder-picker-column-{depth}"))
                .flex_1()
                .min_w(px(190.0))
                .h_full()
                .min_h_0()
                .border_r_1()
                .border_color(theme.border)
                .child(BokheimScrollableElement::overflow_y_scrollbar_with_id(contents.flex_1().min_h_0(), format!("folder-picker-column-scroll-{depth}")));
            column_strip = column_strip.child(column);
        }

        let can_select = self.destination(self.selected_id).is_some() && !self.disabled_ids.contains(&self.selected_id);
        let cancel_picker = picker.clone();
        let confirm_picker = picker.clone();
        let panel = components::modal_surface(theme)
            .size_full()
            .flex()
            .flex_col()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .h(px(58.0))
                    .flex_none()
                    .px(px(components::SPACE_MD))
                    .flex()
                    .items_center()
                    .gap(px(components::SPACE_SM))
                    .border_b_1()
                    .border_color(theme.border)
                    .child(div().flex_1())
                    .child(components::outlined_text_button("cancel-folder-picker", "Cancel").on_click(move |_, _, cx| cancel_picker.update(cx, |picker, cx| picker.cancel(cx))))
                    .child(Button::new("confirm-folder-picker").primary().disabled(!can_select).label("Choose").on_click(move |_, _, cx| confirm_picker.update(cx, |picker, cx| picker.confirm(cx)))),
            )
            .child(column_strip);
        let dismiss_picker = picker;
        gpui::deferred(
            components::modal_overlay()
                .occlude()
                .child(components::modal_scrim("folder-picker-scrim", theme).on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()).on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    dismiss_picker.update(cx, |picker, cx| picker.cancel(cx));
                }))
                .child(panel),
        )
        .with_priority(2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn destination(id: u128, parent_id: DirId, label: &str) -> LibraryFolderDestination {
        LibraryFolderDestination { id: DirId::from_u128(id), parent_id: Some(parent_id), label: label.to_owned(), path: label.to_owned() }
    }

    fn tree() -> Vec<LibraryFolderDestination> {
        let shelf = DirId::from_u128(1);
        vec![
            LibraryFolderDestination { id: ROOT_DIR_ID, parent_id: None, label: "Library".to_owned(), path: "Library".to_owned() },
            destination(1, ROOT_DIR_ID, "Shelf / literal separator"),
            destination(2, shelf, "Child"),
            destination(3, ROOT_DIR_ID, "Other"),
        ]
    }

    #[test]
    fn columns_follow_ids_instead_of_parsing_labels() {
        let child = DirId::from_u128(2);
        let picker = FolderPicker::new(child, tree());
        let columns = picker.columns();
        assert_eq!(columns.len(), 3);
        assert_eq!(columns[0].iter().map(|destination| destination.id).collect::<Vec<_>>(), [ROOT_DIR_ID]);
        assert_eq!(columns[1].iter().map(|destination| destination.id).collect::<Vec<_>>(), [DirId::from_u128(1), DirId::from_u128(3)]);
        assert_eq!(columns[2].iter().map(|destination| destination.id).collect::<Vec<_>>(), [child]);
    }
}
