//! Physical directory browsing and folder mutations.

use epub_svelte_lib::library::LibraryFolderDestination;
use epub_svelte_lib::{ContentHash, DirId, ROOT_DIR_ID};
use gpui::prelude::*;
use gpui::{AnyElement, App, AppContext, Context, Entity, EventEmitter, Focusable, IntoElement, MouseButton, Render, Subscription, Window, div, px};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::dialog::DialogButtonProps;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::{PopupMenu, PopupMenuItem};
use gpui_component::scroll::ScrollableElement;
use gpui_component::{Disableable, Icon, IconName, Sizable, WindowExt};
use std::rc::Rc;
use ui_components as components;
use ui_components::ScrollableElement as BokheimScrollableElement;

use super::hierarchy::{HierarchyCrumb, HierarchyState};
use crate::library::services::{LatestRequest, LibraryContext};
use crate::library::widgets::{BookManagement, BookManagementAction};
use crate::model::FolderCrumb;

#[derive(Clone)]
pub(crate) enum FoldersPageEvent {
    PathChanged(Vec<FolderCrumb>),
}

#[derive(Clone)]
enum FolderPickerOperation {
    CopyBook { content_hash: ContentHash, source_id: DirId },
    MoveBook { content_hash: ContentHash, source_id: DirId },
    MoveFolder { folder_id: DirId, source_parent_id: DirId, source_path: String },
}

impl FolderPickerOperation {
    fn action_label(&self) -> &'static str {
        match self {
            Self::CopyBook { .. } => "Copy",
            Self::MoveBook { .. } | Self::MoveFolder { .. } => "Move",
        }
    }

    fn can_select(&self, destination: &LibraryFolderDestination) -> bool {
        match self {
            Self::CopyBook { source_id, .. } | Self::MoveBook { source_id, .. } => destination.id != *source_id,
            Self::MoveFolder { source_parent_id, source_path, .. } => destination.id != *source_parent_id && destination.path != *source_path && !destination.path.starts_with(&format!("{source_path} / ")),
        }
    }

    fn visible(&self, destination: &LibraryFolderDestination) -> bool {
        match self {
            Self::MoveFolder { source_path, .. } => destination.path != *source_path && !destination.path.starts_with(&format!("{source_path} / ")),
            _ => true,
        }
    }
}

#[derive(Clone)]
struct FolderPickerState {
    operation: FolderPickerOperation,
    selected_id: DirId,
}

struct FolderRenameEditor {
    folder_id: DirId,
    input: Entity<InputState>,
}

pub(crate) struct FoldersPage {
    ui: LibraryContext,
    hierarchy: HierarchyState<FolderCrumb>,
    refresh_request: LatestRequest,
    search_request: LatestRequest,
    _filter_subscription: Subscription,
    destinations: Rc<Vec<epub_svelte_lib::library::LibraryFolderDestination>>,
    folder_picker: Option<FolderPickerState>,
    rename_editor: Option<FolderRenameEditor>,
    rename_subscription: Option<Subscription>,
}

impl EventEmitter<FoldersPageEvent> for FoldersPage {}

impl FoldersPage {
    pub(crate) fn new(ui: LibraryContext, path: Vec<FolderCrumb>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (hierarchy, filter_subscription) = HierarchyState::new(ui.clone(), path, |page: &mut Self, cx| page.search(cx), window, cx);
        Self {
            hierarchy,
            ui,
            refresh_request: LatestRequest::default(),
            search_request: LatestRequest::default(),
            _filter_subscription: filter_subscription,
            destinations: Rc::new(Vec::new()),
            folder_picker: None,
            rename_editor: None,
            rename_subscription: None,
        }
    }

    fn folder_id(&self) -> DirId {
        self.hierarchy.last().map(|crumb| crumb.id).unwrap_or(ROOT_DIR_ID)
    }

    fn set_path(&mut self, path: Vec<FolderCrumb>, window: &mut Window, cx: &mut Context<Self>) {
        if self.hierarchy.set_path(path, window, cx) {
            cx.emit(FoldersPageEvent::PathChanged(self.hierarchy.path().to_vec()));
            self.refresh(cx);
        }
    }

    pub(crate) fn set_library_view(&mut self, library_view: app_contract::preferences::LibraryView, cx: &mut Context<Self>) {
        self.hierarchy.set_library_view(library_view, cx);
    }

    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        let folder_id = self.folder_id();
        let generation = self.refresh_request.begin();
        let load = self.ui.load_folder(folder_id);
        let load_destinations = self.ui.folder_destinations();
        let task = cx.spawn(async move |page, cx| {
            let result = load.await;
            let destinations = load_destinations.await;
            let _ = page.update(cx, |page, cx| {
                if !page.refresh_request.is_current(generation) || page.folder_id() != folder_id {
                    return;
                }
                match result {
                    Ok(data) => {
                        page.hierarchy.clear_error();
                        page.hierarchy.install(data.children, data.books, cx);
                        if let Ok(destinations) = destinations {
                            page.destinations = Rc::new(destinations);
                        }
                        page.configure_book_management(cx);
                        cx.notify();
                    }
                    Err(error) => {
                        log::error!("failed to load Library hierarchy: {error}");
                        page.hierarchy.set_error(error, cx);
                    }
                }
            });
        });
        self.refresh_request.install(task);
    }

    /// Searches every book beneath this folder, debounced, and drops answers a
    /// newer query has already superseded.
    fn search(&mut self, cx: &mut Context<Self>) {
        let query = self.hierarchy.query(cx);
        if query.is_empty() {
            self.hierarchy.clear_search();
            return;
        }
        let generation = self.hierarchy.begin_search();
        let folder_id = self.folder_id();
        let ui = self.ui.clone();
        let debounce = std::time::Duration::from_millis(u64::from(ui.contracts().browsing_content.search_debounce_ms));
        let timer = cx.background_executor().timer(debounce);
        let task = cx.spawn(async move |page, cx| {
            timer.await;
            let result = ui.search_folder(folder_id, query).await;
            let _ = page.update(cx, |page, cx| {
                if !page.hierarchy.is_current_search(generation) {
                    return;
                }
                match result {
                    Ok(found) => {
                        page.hierarchy.clear_error();
                        page.hierarchy.set_results(crate::library::pages::hierarchy::merge_matches(found), &page.ui.clone(), cx);
                    }
                    Err(error) => {
                        log::error!("failed to search this folder: {error}");
                        page.hierarchy.set_error(error, cx);
                    }
                }
            });
        });
        self.search_request.install(task);
    }

    pub(crate) fn focus_search(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.hierarchy.focus_search(window, cx);
    }

    fn choose_book_to_add(&mut self, cx: &mut Context<Self>) {
        let folder_id = self.folder_id();
        self.ui.clone().choose_and_import_book(folder_id, cx, |page, result, cx| match result {
            Ok(()) => page.refresh(cx),
            Err(error) => {
                log::error!("failed to add book: {error}");
                page.hierarchy.set_error(format!("Could not add book: {error}"), cx);
            }
        });
    }

    fn show_new_folder_dialog(&mut self, cx: &mut Context<Self>) {
        let page = cx.entity();
        self.ui.with_window(cx, move |window, cx| {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Folder name"));
            window.open_dialog(cx, move |dialog, _, _| {
                let input = input.clone();
                let page = page.clone();
                dialog.title("New folder").child(Input::new(&input)).button_props(DialogButtonProps::default().ok_text("Create").show_cancel(true)).on_ok(move |_, _, cx: &mut App| {
                    let name = input.read(cx).value().trim().to_owned();
                    if name.is_empty() {
                        return false;
                    }
                    page.update(cx, |page, cx| page.create_folder(name.clone(), cx));
                    true
                })
            });
        });
    }

    fn create_folder(&mut self, name: String, cx: &mut Context<Self>) {
        let operation = self.ui.create_folder(self.folder_id(), name);
        cx.spawn(async move |page, cx| {
            let result = operation.await;
            let _ = page.update(cx, |page, cx| {
                if let Err(error) = result {
                    page.hierarchy.set_error(format!("Could not create folder: {error}"), cx);
                } else {
                    page.refresh(cx);
                }
            });
        })
        .detach();
    }

    fn configure_book_management(&mut self, cx: &mut Context<Self>) {
        let source_id = self.folder_id();
        let page = cx.entity();
        let on_action = Rc::new(move |action, cx: &mut App| {
            page.update(cx, |page, cx| page.handle_book_action(source_id, action, cx));
        });
        self.hierarchy.set_book_management(Some(BookManagement { on_action }), cx);
    }

    fn handle_book_action(&mut self, source_id: DirId, action: BookManagementAction, cx: &mut Context<Self>) {
        match action {
            BookManagementAction::Download(content_hash) => {
                let operation = self.ui.download_book(content_hash);
                self.run_book_operation(operation, "download book", cx);
            }
            BookManagementAction::RemoveDownload(content_hash) => {
                let operation = self.ui.remove_book_download(content_hash);
                self.run_book_operation(operation, "remove download", cx);
            }
            BookManagementAction::ChooseCopyDestination(content_hash) => self.open_folder_picker(FolderPickerOperation::CopyBook { content_hash, source_id }, cx),
            BookManagementAction::ChooseMoveDestination(content_hash) => self.open_folder_picker(FolderPickerOperation::MoveBook { content_hash, source_id }, cx),
            BookManagementAction::RemoveFromFolder(content_hash) => {
                let operation = self.ui.remove_book_from_folder(content_hash, source_id);
                self.run_book_operation(async move { operation.await.map(|_| ()) }, "remove book from folder", cx);
            }
            BookManagementAction::MoveToTrash(content_hash) => {
                let operation = self.ui.move_book_to_trash(content_hash);
                self.run_book_operation(operation, "move book to Trash", cx);
            }
        }
    }

    fn run_book_operation(&mut self, operation: impl Future<Output = Result<(), String>> + 'static, action: &'static str, cx: &mut Context<Self>) {
        cx.spawn(async move |page, cx| {
            let result = operation.await;
            let _ = page.update(cx, |page, cx| match result {
                Ok(()) => page.refresh(cx),
                Err(error) => page.hierarchy.set_error(format!("Could not {action}: {error}"), cx),
            });
        })
        .detach();
    }

    fn open_folder_picker(&mut self, operation: FolderPickerOperation, cx: &mut Context<Self>) {
        self.folder_picker = Some(FolderPickerState { operation, selected_id: self.folder_id() });
        cx.notify();
    }

    fn close_folder_picker(&mut self, cx: &mut Context<Self>) {
        self.folder_picker = None;
        cx.notify();
    }

    fn confirm_folder_picker(&mut self, cx: &mut Context<Self>) {
        let Some(picker) = self.folder_picker.clone() else {
            return;
        };
        let Some(destination) = self.destinations.iter().find(|destination| destination.id == picker.selected_id) else {
            return;
        };
        if !picker.operation.can_select(destination) {
            return;
        }
        self.folder_picker = None;
        match picker.operation {
            FolderPickerOperation::CopyBook { content_hash, source_id } => {
                let operation = self.ui.copy_book_to_folder(content_hash, source_id, destination.id);
                self.run_book_operation(async move { operation.await.map(|_| ()) }, "copy book", cx);
            }
            FolderPickerOperation::MoveBook { content_hash, source_id } => {
                let operation = self.ui.move_book_to_folder(content_hash, source_id, destination.id);
                self.run_book_operation(async move { operation.await.map(|_| ()) }, "move book", cx);
            }
            FolderPickerOperation::MoveFolder { folder_id, .. } => self.move_folder(folder_id, destination.id, cx),
        }
    }

    fn destination_parts(path: &str) -> Vec<&str> {
        if path == "Library" { Vec::new() } else { path.split(" / ").collect() }
    }

    fn folder_picker_columns(&self, picker: &FolderPickerState) -> Vec<Vec<LibraryFolderDestination>> {
        let selected_parts = self.destinations.iter().find(|destination| destination.id == picker.selected_id).map(|destination| Self::destination_parts(&destination.path)).unwrap_or_default();
        let mut columns = (0..=selected_parts.len())
            .map(|depth| {
                self.destinations
                    .iter()
                    .filter(|destination| picker.operation.visible(destination))
                    .filter(|destination| {
                        let parts = Self::destination_parts(&destination.path);
                        parts.len() == depth + 1 && parts[..depth] == selected_parts[..depth]
                    })
                    .cloned()
                    .collect()
            })
            .collect::<Vec<_>>();
        if columns.last().is_some_and(Vec::is_empty) {
            columns.pop();
        }
        columns
    }

    fn render_folder_picker(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let picker = self.folder_picker.clone()?;
        let theme = components::browser_theme(cx);
        let page = cx.entity();
        let selected = self.destinations.iter().find(|destination| destination.id == picker.selected_id).cloned().unwrap_or(LibraryFolderDestination { id: ROOT_DIR_ID, path: "Library".to_owned() });
        let selected_parts = Self::destination_parts(&selected.path);
        let columns = self.folder_picker_columns(&picker);
        let mut column_strip = div().flex_1().min_h_0().min_w_0().flex().flex_row().overflow_x_scrollbar();
        for (depth, destinations) in columns.into_iter().enumerate() {
            let mut contents = div().w_full().flex().flex_col().p(px(components::SPACE_XS));
            for destination in destinations {
                let parts = Self::destination_parts(&destination.path);
                let name = parts.last().copied().unwrap_or("Library").to_owned();
                let on_path = selected_parts.get(depth).is_some_and(|part| *part == name);
                let destination_id = destination.id;
                let select_page = page.clone();
                contents = contents.child(
                    Button::new(format!("folder-picker-{destination_id}")).ghost().small().w_full().h(px(36.0)).justify_start().icon(IconName::Folder).when(on_path, |button| button.bg(theme.accent_soft)).label(name).on_click(
                        move |_, _, cx| {
                            select_page.update(cx, |page, cx| {
                                if let Some(picker) = &mut page.folder_picker {
                                    picker.selected_id = destination_id;
                                    cx.notify();
                                }
                            });
                        },
                    ),
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

        let can_select = picker.operation.can_select(&selected);
        let cancel_page = page.clone();
        let confirm_page = page.clone();
        let action_label = picker.operation.action_label();
        let panel = div()
            .relative()
            .size_full()
            .min_h_0()
            .min_w_0()
            .flex()
            .flex_col()
            .occlude()
            .rounded(px(components::RADIUS_MD))
            .border_1()
            .border_color(theme.border)
            .bg(theme.surface)
            .shadow(components::overlay_shadow(theme))
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
                    .child(Button::new("cancel-folder-picker").secondary().outline().label("Cancel").on_click(move |_, _, cx| cancel_page.update(cx, |page, cx| page.close_folder_picker(cx))))
                    .child(Button::new("confirm-folder-picker").primary().disabled(!can_select).label(action_label).on_click(move |_, _, cx| confirm_page.update(cx, |page, cx| page.confirm_folder_picker(cx)))),
            )
            .child(column_strip);

        let dismiss_page = page;
        Some(
            gpui::deferred(
                components::library_switcher_overlay()
                    .child(components::library_switcher_scrim(theme).on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()).on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        dismiss_page.update(cx, |page, cx| page.close_folder_picker(cx));
                    }))
                    .child(panel),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    fn start_inline_rename(&mut self, folder: FolderCrumb, window: &mut Window, cx: &mut Context<Self>) {
        let folder_id = folder.id;
        let input = cx.new(|cx| InputState::new(window, cx).default_value(folder.name));
        let subscription = cx.subscribe(&input, move |page, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                page.commit_inline_rename(folder_id, cx);
            }
        });
        input.read(cx).focus_handle(cx).focus(window, cx);
        self.rename_editor = Some(FolderRenameEditor { folder_id, input });
        self.rename_subscription = Some(subscription);
        cx.notify();
    }

    fn cancel_inline_rename(&mut self, cx: &mut Context<Self>) {
        self.rename_editor = None;
        self.rename_subscription = None;
        cx.notify();
    }

    fn commit_inline_rename(&mut self, folder_id: DirId, cx: &mut Context<Self>) {
        let Some(editor) = self.rename_editor.as_ref().filter(|editor| editor.folder_id == folder_id) else {
            return;
        };
        let name = editor.input.read(cx).value().trim().to_owned();
        if name.is_empty() {
            self.hierarchy.set_error("Folder name cannot be empty".to_owned(), cx);
            return;
        }
        self.rename_editor = None;
        self.rename_subscription = None;
        self.rename_folder(folder_id, name, cx);
    }

    fn render_inline_rename(folder_id: DirId, input: Entity<InputState>, width: f32, page: Entity<Self>, cx: &mut Context<Self>) -> AnyElement {
        let theme = components::browser_theme(cx);
        let save_page = page.clone();
        div()
            .id(format!("folder-inline-rename-{folder_id}"))
            .w(px(width))
            .h(px(components::HIERARCHY_CELL_HEIGHT))
            .flex_none()
            .m(px(components::HIERARCHY_CELL_MARGIN))
            .px(px(components::SPACE_SM))
            .flex()
            .items_center()
            .gap(px(components::SPACE_XS))
            .bg(theme.raised_bg)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(Icon::new(IconName::Folder).small().text_color(theme.accent))
            .child(div().flex_1().min_w_0().child(Input::new(&input)))
            .child(Button::new(format!("save-folder-rename-{folder_id}")).ghost().small().icon(IconName::Check).on_click(move |_, _, cx| save_page.update(cx, |page, cx| page.commit_inline_rename(folder_id, cx))))
            .child(Button::new(format!("cancel-folder-rename-{folder_id}")).ghost().small().icon(IconName::Close).on_click(move |_, _, cx| page.update(cx, |page, cx| page.cancel_inline_rename(cx))))
            .into_any_element()
    }

    fn rename_folder(&mut self, folder_id: DirId, name: String, cx: &mut Context<Self>) {
        let operation = self.ui.rename_folder(folder_id, name);
        cx.spawn(async move |page, cx| {
            let result = operation.await;
            let _ = page.update(cx, |page, cx| match result {
                Ok(_) => page.refresh(cx),
                Err(error) => page.hierarchy.set_error(format!("Could not rename folder: {error}"), cx),
            });
        })
        .detach();
    }

    fn move_folder(&mut self, folder_id: DirId, destination_id: DirId, cx: &mut Context<Self>) {
        let operation = self.ui.move_folder(folder_id, destination_id);
        cx.spawn(async move |page, cx| {
            let result = operation.await;
            let _ = page.update(cx, |page, cx| match result {
                Ok(_) => page.refresh(cx),
                Err(error) => page.hierarchy.set_error(format!("Could not move folder: {error}"), cx),
            });
        })
        .detach();
    }

    fn show_move_folder_picker(&mut self, folder_id: DirId, cx: &mut Context<Self>) {
        let Some(source_path) = self.destinations.iter().find(|destination| destination.id == folder_id).map(|destination| destination.path.clone()) else {
            self.hierarchy.set_error("Could not open folder picker: folder is no longer available".to_owned(), cx);
            return;
        };
        self.open_folder_picker(FolderPickerOperation::MoveFolder { folder_id, source_parent_id: self.folder_id(), source_path }, cx);
    }

    fn confirm_trash_folder(&self, folder: FolderCrumb, cx: &mut Context<Self>) {
        let page = cx.entity();
        self.ui.with_window(cx, move |window, cx| {
            window.open_dialog(cx, move |dialog, _, _| {
                let page = page.clone();
                dialog
                    .title(format!("Move “{}” to Trash?", folder.name))
                    .child("The folder and its placements will be removed. Books with no other folder placement will also move to Trash.")
                    .button_props(DialogButtonProps::default().ok_text("Move to Trash").show_cancel(true))
                    .on_ok(move |_, _, cx| {
                        page.update(cx, |page, cx| page.trash_folder(folder.id, cx));
                        true
                    })
            });
        });
    }

    fn trash_folder(&mut self, folder_id: DirId, cx: &mut Context<Self>) {
        let operation = self.ui.move_folder_to_trash(folder_id);
        cx.spawn(async move |page, cx| {
            let result = operation.await;
            let _ = page.update(cx, |page, cx| match result {
                Ok(()) => page.refresh(cx),
                Err(error) => page.hierarchy.set_error(format!("Could not move folder to Trash: {error}"), cx),
            });
        })
        .detach();
    }

    fn open_child(&mut self, folder: FolderCrumb, window: &mut Window, cx: &mut Context<Self>) {
        let mut path = self.hierarchy.path().to_vec();
        path.push(folder);
        self.set_path(path, window, cx);
    }

    fn download_folder(&mut self, folder_id: epub_svelte_lib::DirId, cx: &mut Context<Self>) {
        let operation = self.ui.download_folder(folder_id);
        cx.spawn(async move |page, cx| {
            let result = operation.await;
            let _ = page.update(cx, |page, cx| match result {
                Ok(()) => page.refresh(cx),
                Err(error) => {
                    log::error!("failed to download folder: {error}");
                    page.hierarchy.set_error(format!("Could not download folder: {error}"), cx);
                }
            });
        })
        .detach();
    }

    fn remove_folder_downloads(&mut self, folder_id: epub_svelte_lib::DirId, cx: &mut Context<Self>) {
        let operation = self.ui.remove_folder_downloads(folder_id);
        cx.spawn(async move |page, cx| {
            let result = operation.await;
            let _ = page.update(cx, |page, cx| match result {
                Ok(()) => page.refresh(cx),
                Err(error) => {
                    log::error!("failed to remove local folder copies: {error}");
                    page.hierarchy.set_error(format!("Could not remove local copies: {error}"), cx);
                }
            });
        })
        .detach();
    }

    fn render_download_action(folder: &FolderCrumb, page: Entity<Self>, download_label: String, cx: &mut Context<Self>) -> AnyElement {
        let theme = components::browser_theme(cx);
        let folder_id = folder.id;
        let downloaded = folder.hierarchy_downloaded_book_count();
        let action = components::hierarchy_download_action(format!("download-folder-{folder_id}"), downloaded, folder.book_count, download_label, theme);
        if folder.book_count > 0 && downloaded >= folder.book_count {
            return action.into_any_element();
        }
        action
            .disabled(folder.book_count == 0)
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                page.update(cx, |page, cx| page.download_folder(folder_id, cx));
            })
            .into_any_element()
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = components::browser_theme(cx);
        let contracts = self.ui.contracts();
        let page = cx.entity();
        let actions = components::browser_topbar_right().child(components::topbar_action_button("empty-add-book", contracts.library_actions.add_book_label.clone(), gpui_component::IconName::BookOpen, theme).on_click(move |_, _, cx| {
            page.update(cx, |page, cx| page.choose_book_to_add(cx));
        }));
        let (title, description) = if self.hierarchy.path().is_empty() {
            (&contracts.browsing_states.empty_library_title, &contracts.browsing_states.empty_library_description)
        } else {
            (&contracts.browsing_states.empty_folder_title, &contracts.browsing_states.empty_folder_description)
        };
        components::browser_actionable_empty(title.clone(), description.clone(), actions, theme).into_any_element()
    }

    fn header_actions(&self, cx: &mut Context<Self>) -> gpui::Div {
        let theme = components::browser_theme(cx);
        let actions = self.ui.contracts().library_actions.clone();
        let add_page = cx.entity();
        let folder_page = add_page.clone();
        components::browser_topbar_right()
            .child(components::topbar_icon_action_button("add-book", actions.add_book_label, gpui_component::IconName::BookOpen, theme).on_click(move |_, _, cx| {
                add_page.update(cx, |page, cx| page.choose_book_to_add(cx));
            }))
            .child(components::topbar_icon_action_button("new-folder", actions.new_folder_label, gpui_component::IconName::Plus, theme).on_click(move |_, _, cx| {
                folder_page.update(cx, |page, cx| page.show_new_folder_dialog(cx));
            }))
    }
}

impl Render for FoldersPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let empty = self.render_empty(cx);
        let actions = self.header_actions(cx);
        let folder_actions = self.ui.contracts().library_actions.clone();
        let child_action = Rc::new(move |folder: &FolderCrumb, page: Entity<Self>, cx: &mut Context<Self>| Self::render_download_action(folder, page, folder_actions.download_folder_label.clone(), cx));
        let child_menu = Rc::new(move |menu: PopupMenu, folder: FolderCrumb, page: Entity<Self>, _window: &mut Window, _cx: &mut Context<PopupMenu>| {
            let folder_id = folder.id;
            let downloaded = folder.hierarchy_downloaded_book_count();
            let total = folder.book_count;
            let open_page = page.clone();
            let open_folder = folder.clone();
            let rename_page = page.clone();
            let rename_folder = folder.clone();
            let download_page = page.clone();
            let remove_downloads_page = page.clone();
            let move_page = page.clone();
            let trash_page = page.clone();
            let trash_folder = folder.clone();
            let mut menu = menu.item(PopupMenuItem::new("Open").on_click(move |_, window, cx| open_page.update(cx, |page, cx| page.open_child(open_folder.clone(), window, cx))));
            if total > downloaded {
                menu = menu.item(PopupMenuItem::new("Download folder").on_click(move |_, _, cx| download_page.update(cx, |page, cx| page.download_folder(folder_id, cx))));
            }
            if downloaded > 0 {
                menu = menu.item(PopupMenuItem::new("Remove local copies").on_click(move |_, _, cx| remove_downloads_page.update(cx, |page, cx| page.remove_folder_downloads(folder_id, cx))));
            }
            menu.item(PopupMenuItem::new("Rename").on_click(move |_, window, cx| rename_page.update(cx, |page, cx| page.start_inline_rename(rename_folder.clone(), window, cx))))
                .item(PopupMenuItem::new("Move to folder…").on_click(move |_, _, cx| move_page.update(cx, |page, cx| page.show_move_folder_picker(folder.id, cx))))
                .separator()
                .item(PopupMenuItem::new("Move to Trash").on_click(move |_, _, cx| trash_page.update(cx, |page, cx| page.confirm_trash_folder(trash_folder.clone(), cx))))
        });
        let rename_editor = self.rename_editor.as_ref().map(|editor| (editor.folder_id, editor.input.clone()));
        let child_override = Rc::new(move |folder: &FolderCrumb, page: Entity<Self>, width: f32, cx: &mut Context<Self>| {
            let (folder_id, input) = rename_editor.as_ref()?;
            (*folder_id == folder.id).then(|| Self::render_inline_rename(*folder_id, input.clone(), width, page, cx))
        });
        let hierarchy = self.hierarchy.render(
            "Library",
            "folder-button",
            "folder-books",
            self.ui.contracts().browsing_states.retry_label.clone().into(),
            self.ui.contracts().browsing_states.searching_label.clone().into(),
            actions,
            empty,
            cx.entity(),
            |page, path, window, cx| page.set_path(path, window, cx),
            |page, cx| page.refresh(cx),
            Some(child_action),
            Some(child_menu),
            Some(child_override),
            window,
            cx,
        );
        let picker = self.render_folder_picker(cx);
        components::hierarchy_page_host(hierarchy).children(picker)
    }
}
