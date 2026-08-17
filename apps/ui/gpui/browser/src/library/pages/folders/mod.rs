//! Physical directory browsing and folder mutations.

use epub_svelte_lib::library::LibraryFolderDestination;
use epub_svelte_lib::{ContentHash, DirId, FolderDestinationPolicy, ROOT_DIR_ID};
use gpui::prelude::*;
use gpui::{AnyElement, App, AppContext, Context, Entity, EventEmitter, ExternalPaths, Focusable, IntoElement, MouseButton, Render, Subscription, WeakEntity, Window, div, px};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::dialog::DialogButtonProps;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::{ContextMenuExt, DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_component::scroll::ScrollableElement;
use gpui_component::{Disableable, Icon, IconName, Sizable, WindowExt};
use std::rc::Rc;
use ui_components as components;
use ui_components::ScrollableElement as BokheimScrollableElement;

use super::hierarchy::{HierarchyCrumb, HierarchySplits, HierarchyState};
use crate::library::services::{LatestRequest, LibraryContext};
use crate::library::widgets::{BookManagement, BookManagementAction, LibraryDrag};
use crate::model::FolderCrumb;

#[derive(Clone)]
pub(crate) enum FoldersPageEvent {
    PathChanged(Vec<FolderCrumb>),
    ContentsChanged,
}

#[derive(Clone)]
enum FolderPickerOperation {
    CopyBook { content_hash: ContentHash, source_id: DirId },
    MoveBook { content_hash: ContentHash, source_id: DirId },
    MoveFolder { folder_id: DirId, source_parent_id: DirId, source_path: String },
}

impl FolderPickerOperation {
    fn destination_policy(&self) -> FolderDestinationPolicy {
        match self {
            Self::CopyBook { source_id, .. } | Self::MoveBook { source_id, .. } => FolderDestinationPolicy::Book { source_id: source_id.to_string() },
            Self::MoveFolder { source_parent_id, source_path, .. } => FolderDestinationPolicy::MoveFolder { source_parent_id: source_parent_id.to_string(), source_path: source_path.clone() },
        }
    }

    fn action_label(&self) -> &'static str {
        match self {
            Self::CopyBook { .. } => "Copy",
            Self::MoveBook { .. } | Self::MoveFolder { .. } => "Move",
        }
    }

    fn can_select(&self, destination: &LibraryFolderDestination) -> bool {
        self.destination_policy().can_select(&destination.id.to_string(), &destination.path)
    }

    fn visible(&self, destination: &LibraryFolderDestination) -> bool {
        self.destination_policy().is_visible(&destination.path)
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
    splits: HierarchySplits<FoldersPage>,
}

impl EventEmitter<FoldersPageEvent> for FoldersPage {}

impl FoldersPage {
    pub(crate) fn new(ui: LibraryContext, path: Vec<FolderCrumb>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::new_pane(ui, path, None, 0, window, cx)
    }

    fn new_pane(ui: LibraryContext, path: Vec<FolderCrumb>, split_root: Option<WeakEntity<FoldersPage>>, pane_id: u64, window: &mut Window, cx: &mut Context<Self>) -> Self {
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
            splits: HierarchySplits::new(split_root, pane_id),
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
        for split in self.splits.pages() {
            split.update(cx, |page, cx| page.set_library_view(library_view, cx));
        }
    }

    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        let folder_id = self.folder_id();
        let file_type = self.hierarchy.file_type();
        let generation = self.refresh_request.begin();
        let load = self.ui.load_folder(folder_id, file_type);
        let load_destinations = self.ui.folder_destinations();
        let task = cx.spawn(async move |page, cx| {
            let result = load.await;
            let destinations = load_destinations.await;
            let _ = page.update(cx, |page, cx| {
                if !page.refresh_request.is_current(generation) || page.folder_id() != folder_id || page.hierarchy.file_type() != file_type {
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
        for split in self.splits.pages() {
            split.update(cx, |page, cx| page.refresh(cx));
        }
    }

    fn request_split(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(root) = self.splits.root() {
            let _ = root.update(cx, |root, cx| root.add_split(window, cx));
            return;
        }
        self.add_split(window, cx);
    }

    fn add_split(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pane_id = self.splits.next_pane_id();
        let ui = self.ui.clone();
        let root = cx.entity().downgrade();
        let split_page = cx.new(|cx| Self::new_pane(ui, Vec::new(), Some(root), pane_id, window, cx));
        let subscription = cx.subscribe(&split_page, |page: &mut Self, _, event: &FoldersPageEvent, cx| {
            if matches!(event, FoldersPageEvent::ContentsChanged) {
                page.refresh(cx);
            }
        });
        split_page.update(cx, |page, cx| page.refresh(cx));
        self.splits.push(pane_id, split_page, vec![subscription]);
        cx.notify();
    }

    fn request_close_split(&self, cx: &mut Context<Self>) {
        let Some(root) = self.splits.root() else { return };
        let pane_id = self.splits.pane_id();
        let _ = root.update(cx, |root, cx| root.close_split(pane_id, cx));
    }

    fn close_split(&mut self, pane_id: u64, cx: &mut Context<Self>) {
        if !self.splits.close(pane_id) {
            return;
        }
        cx.notify();
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
        let file_type = self.hierarchy.file_type();
        let ui = self.ui.clone();
        let debounce = std::time::Duration::from_millis(u64::from(ui.contracts().browsing_content.search_debounce_ms));
        let timer = cx.background_executor().timer(debounce);
        let task = cx.spawn(async move |page, cx| {
            timer.await;
            let result = ui.search_folder(folder_id, query, file_type).await;
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

    fn set_file_type(&mut self, file_type: epub_svelte_lib::library::LibraryFileTypeFilter, cx: &mut Context<Self>) {
        if self.hierarchy.set_file_type(file_type, cx) {
            self.refresh(cx);
        }
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
        self.hierarchy.set_book_management(Some(BookManagement { on_action, source_id }), cx);
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
                Ok(()) => {
                    page.refresh(cx);
                    cx.emit(FoldersPageEvent::ContentsChanged);
                }
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
                Ok(_) => {
                    page.refresh(cx);
                    cx.emit(FoldersPageEvent::ContentsChanged);
                }
                Err(error) => page.hierarchy.set_error(format!("Could not move folder: {error}"), cx),
            });
        })
        .detach();
    }

    fn can_drop_into(&self, drag: &LibraryDrag, destination_id: DirId) -> bool {
        match drag {
            LibraryDrag::Book { source_id, .. } => *source_id != destination_id,
            LibraryDrag::Folder { source_parent_id, source_path, .. } => {
                let Some(destination) = self.destinations.iter().find(|destination| destination.id == destination_id) else {
                    return false;
                };
                FolderDestinationPolicy::MoveFolder { source_parent_id: source_parent_id.to_string(), source_path: source_path.clone() }.can_select(&destination.id.to_string(), &destination.path)
            }
        }
    }

    fn drop_into(&mut self, drag: LibraryDrag, destination_id: DirId, cx: &mut Context<Self>) {
        if !self.can_drop_into(&drag, destination_id) {
            return;
        }
        match drag {
            LibraryDrag::Book { content_hash, source_id, .. } => {
                let operation = self.ui.move_book_to_folder(content_hash, source_id, destination_id);
                self.run_book_operation(async move { operation.await.map(|_| ()) }, "move book", cx);
            }
            LibraryDrag::Folder { folder_id, .. } => self.move_folder(folder_id, destination_id, cx),
        }
    }

    fn can_import_external(&self, paths: &ExternalPaths) -> bool {
        let extensions = &self.ui.contracts().browsing_content.import_extensions;
        !paths.paths().is_empty()
            && paths.paths().iter().all(|path| path.is_file() && path.extension().and_then(|extension| extension.to_str()).is_some_and(|extension| extensions.iter().any(|supported| supported.eq_ignore_ascii_case(extension))))
    }

    fn import_external(&mut self, paths: ExternalPaths, destination_id: DirId, cx: &mut Context<Self>) {
        if !self.can_import_external(&paths) {
            self.hierarchy.set_error("Only supported book files can be dropped into a folder", cx);
            return;
        }
        let ui = self.ui.clone();
        let paths = paths.paths().to_vec();
        cx.spawn(async move |page, cx| {
            let mut imported = 0usize;
            let mut failures = Vec::new();
            for path in paths {
                let Some(file_name) = path.file_name().and_then(|name| name.to_str()).map(str::to_owned) else {
                    failures.push(format!("{} has no valid file name", path.display()));
                    continue;
                };
                match ui.import_book(destination_id, path, file_name.clone()).await {
                    Ok(()) => imported += 1,
                    Err(error) => failures.push(format!("{file_name}: {error}")),
                }
            }
            let _ = page.update(cx, |page, cx| {
                if imported > 0 {
                    page.refresh(cx);
                    cx.emit(FoldersPageEvent::ContentsChanged);
                }
                if !failures.is_empty() {
                    page.hierarchy.set_error(format!("Could not import {}", failures.join("; ")), cx);
                }
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

    fn trash_folder(&mut self, folder_id: DirId, cx: &mut Context<Self>) {
        log::info!("moving folder to trash: {folder_id}");
        let operation = self.ui.move_folder_to_trash(folder_id);
        cx.spawn(async move |page, cx| {
            let result = operation.await;
            let _ = page.update(cx, |page, cx| match result {
                Ok(()) => {
                    page.refresh(cx);
                    cx.emit(FoldersPageEvent::ContentsChanged);
                }
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

    fn render_download_action(folder: &FolderCrumb, page: Entity<Self>, download_label: String, show_completion: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = components::browser_theme(cx);
        let folder_id = folder.id;
        let downloaded = folder.hierarchy_downloaded_book_count();
        let action = components::hierarchy_download_action(format!("download-folder-{folder_id}"), downloaded, folder.book_count, download_label, show_completion, theme);
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
        let file_type = self.hierarchy.file_type();
        let (title, description) = if file_type != epub_svelte_lib::library::LibraryFileTypeFilter::All {
            let label = file_type.label().to_lowercase();
            (format!("No {label} here"), format!("This folder and its subfolders contain no {label}."))
        } else if self.hierarchy.path().is_empty() {
            (contracts.browsing_states.empty_library_title.clone(), contracts.browsing_states.empty_library_description.clone())
        } else {
            (contracts.browsing_states.empty_folder_title.clone(), contracts.browsing_states.empty_folder_description.clone())
        };
        components::browser_actionable_empty(title, description, components::browser_topbar_right(), theme).into_any_element()
    }

    fn header_actions(&self, cx: &mut Context<Self>) -> gpui::Div {
        let page = cx.entity();
        self.splits.actions("folder", page, |page, window, cx| page.update(cx, |page, cx| page.request_split(window, cx)), |page, cx| page.update(cx, |page, cx| page.request_close_split(cx)), cx)
    }
}

impl Render for FoldersPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let empty = self.render_empty(cx);
        let breadcrumb_actions = self.header_actions(cx);
        let selected_file_type = self.hierarchy.file_type();
        let filter_page = cx.entity();
        let file_type_control = components::catalog_dropdown_button("folder-file-type", selected_file_type.label(), false, false, theme).h(gpui::px(components::TOPBAR_ACTION_HEIGHT)).dropdown_caret(true).dropdown_menu(move |menu, _, _| {
            epub_svelte_lib::library::LibraryFileTypeFilter::ALL.into_iter().fold(menu, |menu, file_type| {
                let page = filter_page.clone();
                menu.item(PopupMenuItem::new(file_type.label()).checked(file_type == selected_file_type).on_click(move |_, _, cx| {
                    page.update(cx, |page, cx| page.set_file_type(file_type, cx));
                }))
            })
        });
        let control_actions = components::browser_topbar_right().child(file_type_control);
        let folder_actions = self.ui.contracts().library_actions.clone();
        let show_download_completion = self.ui.sync_server_connected();
        let child_action = Rc::new(move |folder: &FolderCrumb, page: Entity<Self>, cx: &mut Context<Self>| Self::render_download_action(folder, page, folder_actions.download_folder_label.clone(), show_download_completion, cx));
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
                .item(PopupMenuItem::new("Move to Trash").on_click(move |_, _, cx| trash_page.update(cx, |page, cx| page.trash_folder(trash_folder.id, cx))))
        });
        let rename_editor = self.rename_editor.as_ref().map(|editor| (editor.folder_id, editor.input.clone()));
        let child_override = Rc::new(move |folder: &FolderCrumb, page: Entity<Self>, width: f32, cx: &mut Context<Self>| {
            let (folder_id, input) = rename_editor.as_ref()?;
            (*folder_id == folder.id).then(|| Self::render_inline_rename(*folder_id, input.clone(), width, page, cx))
        });
        let source_parent_id = self.folder_id();
        let paths = self.destinations.iter().map(|destination| (destination.id, destination.path.clone())).collect::<std::collections::HashMap<_, _>>();
        let child_drag = Rc::new(move |folder: &FolderCrumb| LibraryDrag::Folder { folder_id: folder.id, source_parent_id, source_path: paths.get(&folder.id).cloned().unwrap_or_else(|| folder.name.clone()), name: folder.name.clone() });
        let child_can_drop = Rc::new(move |folder: &FolderCrumb, drag: &LibraryDrag, page: Entity<Self>, cx: &mut App| page.read(cx).can_drop_into(drag, folder.id));
        let child_drop = Rc::new(move |folder: &FolderCrumb, drag: &LibraryDrag, page: Entity<Self>, cx: &mut App| {
            let destination_id = folder.id;
            let drag = drag.clone();
            page.update(cx, |page, cx| page.drop_into(drag, destination_id, cx));
        });
        let hierarchy = self.hierarchy.render(
            "Library",
            app_contract::LibraryFeature::Folders.icon(),
            "folder-button",
            "folder-books",
            self.ui.contracts().browsing_states.retry_label.clone().into(),
            self.ui.contracts().browsing_states.searching_label.clone().into(),
            breadcrumb_actions,
            control_actions,
            empty,
            cx.entity(),
            |page, path, window, cx| page.set_path(path, window, cx),
            |page, cx| page.refresh(cx),
            Some(child_action),
            Some(child_menu),
            Some(child_override),
            true,
            None,
            None,
            Some(child_drag),
            Some(child_can_drop),
            Some(child_drop),
            window,
            cx,
        );
        let picker = self.render_folder_picker(cx);
        let menu_page = cx.entity();
        let can_drop_page = menu_page.clone();
        let drop_page = menu_page.clone();
        let external_drop_page = menu_page.clone();
        let destination_id = self.folder_id();
        let labels = self.ui.contracts().library_actions.clone();
        let is_secondary = self.splits.is_secondary();
        // Keep the page menu on a background hitbox rather than wrapping the
        // whole hierarchy. That lets book and folder context menus remain the
        // sole target when the pointer is over an item.
        let background = div()
            .absolute()
            .size_full()
            .can_drop(move |value, _, cx| {
                value.downcast_ref::<LibraryDrag>().is_some_and(|drag| can_drop_page.read(cx).can_drop_into(drag, destination_id))
                    || value.downcast_ref::<ExternalPaths>().is_some_and(|paths| can_drop_page.read(cx).can_import_external(paths))
            })
            .drag_over::<LibraryDrag>(move |style, _, _, cx| style.bg(components::browser_theme(cx).accent_soft))
            .drag_over::<ExternalPaths>(move |style, _, _, cx| style.bg(components::browser_theme(cx).accent_soft))
            .on_drop(move |drag: &LibraryDrag, _, cx| {
                let drag = drag.clone();
                drop_page.update(cx, |page, cx| page.drop_into(drag, destination_id, cx));
            })
            .on_drop(move |paths: &ExternalPaths, _, cx| {
                let paths = paths.clone();
                external_drop_page.update(cx, |page, cx| page.import_external(paths, destination_id, cx));
            });
        let background_menu = background.context_menu(move |menu, _, _| {
            let add_page = menu_page.clone();
            let folder_page = menu_page.clone();
            let mut menu = menu
                .item(PopupMenuItem::new(labels.add_book_label.clone()).on_click(move |_, _, cx| add_page.update(cx, |page, cx| page.choose_book_to_add(cx))))
                .item(PopupMenuItem::new(labels.new_folder_label.clone()).on_click(move |_, _, cx| folder_page.update(cx, |page, cx| page.show_new_folder_dialog(cx))));
            let split_page = menu_page.clone();
            menu = menu.separator().item(PopupMenuItem::new("Add split").on_click(move |_, window, cx| split_page.update(cx, |page, cx| page.request_split(window, cx))));
            if is_secondary {
                let close_page = menu_page.clone();
                menu = menu.item(PopupMenuItem::new("Close this split").on_click(move |_, _, cx| close_page.update(cx, |page, cx| page.request_close_split(cx))));
            }
            menu
        });
        let pane = components::hierarchy_page_host(background_menu).child(hierarchy).children(picker);
        self.splits.render(pane.into_any_element(), window, cx)
    }
}
