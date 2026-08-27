//! Folder-only commands and modal state for folder browsing.

use gpui::prelude::*;
use gpui::{Anchor, AnyElement, App, Context, Entity, EventEmitter, FileDialogFilter, Focusable, IntoElement, MouseButton, PathPromptOptions, SharedString, Subscription, Window, div, px, rems};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_component::notification::Notification;
use gpui_component::{Disableable, IconName, WindowExt};
use sync_common::{DirId, ROOT_DIR_ID};
use ui_components as components;

use super::folder_picker::{FolderPicker, FolderPickerEvent};
use crate::library::services::{LatestRequest, LibraryContext};

fn book_import_extensions() -> Vec<String> {
    crate::BOOK_IMPORT_EXTENSIONS.into_iter().map(str::to_owned).collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FolderActionsEvent {
    Reload,
    Renamed(String),
    OpenRoot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FolderNameMode {
    Create,
    Rename,
}

struct FolderNameDialog {
    mode: FolderNameMode,
    input: Entity<InputState>,
    error: Option<SharedString>,
    busy: bool,
    _subscription: Subscription,
}

struct MoveFolderDialog {
    source_id: DirId,
    picker: Entity<FolderPicker>,
    _subscription: Subscription,
}

/// Owns commands that only exist for physical folders. The shared browse page
/// observes high-level outcomes instead of carrying backend handles and modal
/// implementation state for every adapter.
pub(crate) struct FolderActions {
    ui: LibraryContext,
    current_id: DirId,
    current_name: Option<String>,
    name_dialog: Option<FolderNameDialog>,
    move_dialog: Option<MoveFolderDialog>,
    move_request: LatestRequest,
}

impl EventEmitter<FolderActionsEvent> for FolderActions {}

impl FolderActions {
    pub(crate) fn new(ui: LibraryContext, current_id: DirId, current_name: Option<String>) -> Self {
        Self { ui, current_id, current_name, name_dialog: None, move_dialog: None, move_request: LatestRequest::default() }
    }

    pub(crate) fn set_location(&mut self, location: &str, name: Option<String>, cx: &mut Context<Self>) {
        let Ok(id) = DirId::parse_str(location) else {
            log::warn!("ignored invalid folder browse location {location}");
            return;
        };
        if self.current_id != id {
            self.cancel_move_selection(cx);
        }
        self.current_id = id;
        self.current_name = name;
        cx.notify();
    }

    pub(crate) fn has_modal(&self) -> bool {
        self.name_dialog.is_some() || self.move_dialog.is_some()
    }

    pub(crate) fn scanner_running(&self) -> bool {
        self.ui.backend().scanner_running()
    }

    pub(crate) fn dismiss_modal(&mut self, cx: &mut Context<Self>) {
        if self.name_dialog.as_ref().is_some_and(|dialog| dialog.busy) {
            return;
        }
        self.name_dialog = None;
        self.cancel_move_selection(cx);
        cx.notify();
    }

    pub(crate) fn cancel_move_selection(&mut self, cx: &mut Context<Self>) {
        self.move_request.begin();
        self.move_dialog = None;
        cx.notify();
    }

    fn choose_book(&mut self, cx: &mut Context<Self>) {
        let parent_id = self.current_id;
        let extensions = book_import_extensions();
        let selection = cx.prompt_for_files_with_filters(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("Add book".into()) }, vec![FileDialogFilter { name: "Books".to_owned(), extensions }]);
        let backend = self.ui.backend().id();
        let app_backend = self.ui.app_backend();
        let library = match app_backend.library(&backend) {
            Ok(library) => library,
            Err(error) => {
                log::error!("could not open library for import: {error}");
                return;
            }
        };
        cx.spawn(async move |actions, cx| {
            let result = match selection.await {
                Ok(Ok(Some(files))) => match files.into_iter().next() {
                    Some(file) => crate::services::backend::library_blocking(&library, move |library| library.import_book_bytes(&parent_id, file.name, file.bytes)).await,
                    None => return,
                },
                Ok(Ok(None)) => return,
                Ok(Err(error)) => Err(error.to_string()),
                Err(error) => Err(error.to_string()),
            };
            let _ = actions.update_in(cx, |_, window, cx| match result {
                Ok(_) => cx.emit(FolderActionsEvent::Reload),
                Err(error) => window.push_notification(Notification::error(format!("Could not add book: {error}")).id1::<Self>("folder-add-book-error").autohide(false), cx),
            });
        })
        .detach();
    }

    fn choose_folder(&mut self, cx: &mut Context<Self>) {
        let parent_id = self.current_id;
        let extensions = book_import_extensions();
        let selection = cx.prompt_for_directories_with_filters(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("Add folder".into()) }, vec![FileDialogFilter { name: "Books".to_owned(), extensions }]);
        let backend = self.ui.backend().id();
        let app_backend = self.ui.app_backend();
        let library = match app_backend.library(&backend) {
            Ok(library) => library,
            Err(error) => {
                log::error!("could not open library for import: {error}");
                return;
            }
        };
        cx.spawn(async move |actions, cx| {
            let result = match selection.await {
                Ok(Ok(Some(directories))) => match directories.into_iter().next() {
                    Some(directory) => {
                        let directory = crate::directory_import(directory);
                        crate::services::backend::library_blocking(&library, move |library| library.import_directory(&parent_id, directory)).await
                    }
                    None => return,
                },
                Ok(Ok(None)) => return,
                Ok(Err(error)) => Err(error.to_string()),
                Err(error) => Err(error.to_string()),
            };
            let _ = actions.update_in(cx, |_, window, cx| match result {
                Ok(failures) => {
                    cx.emit(FolderActionsEvent::Reload);
                    if !failures.is_empty() {
                        window.push_notification(Notification::warning(format!("Folder added, but {} item(s) could not be imported. {}", failures.len(), failures[0])).id1::<Self>("folder-add-folder-warning").autohide(false), cx);
                    }
                }
                Err(error) => window.push_notification(Notification::error(format!("Could not add folder: {error}")).id1::<Self>("folder-add-folder-error").autohide(false), cx),
            });
        })
        .detach();
    }

    fn open_name_dialog(&mut self, mode: FolderNameMode, window: &mut Window, cx: &mut Context<Self>) {
        if self.name_dialog.is_some() || (mode == FolderNameMode::Rename && self.current_name.is_none()) {
            return;
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Folder name"));
        if let Some(name) = self.current_name.as_ref().filter(|_| mode == FolderNameMode::Rename) {
            input.update(cx, |input, cx| input.set_value(name, window, cx));
        }
        let subscription = cx.subscribe_in(&input, window, |actions, _, event, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                actions.submit_name_dialog(window, cx);
            }
        });
        input.read(cx).focus_handle(cx).focus(window, cx);
        self.name_dialog = Some(FolderNameDialog { mode, input, error: None, busy: false, _subscription: subscription });
        cx.notify();
    }

    fn close_name_dialog(&mut self, cx: &mut Context<Self>) {
        if self.name_dialog.as_ref().is_some_and(|dialog| dialog.busy) {
            return;
        }
        self.name_dialog = None;
        cx.notify();
    }

    fn submit_name_dialog(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self.name_dialog.as_mut() else { return };
        if dialog.busy {
            return;
        }
        let name = dialog.input.read(cx).value().trim().to_owned();
        if name.is_empty() {
            dialog.error = Some("Enter a folder name".into());
            cx.notify();
            return;
        }
        dialog.busy = true;
        dialog.error = None;
        match dialog.mode {
            FolderNameMode::Create => {
                let parent_id = self.current_id;
                let operation = crate::services::backend::library_blocking(self.ui.backend(), move |library| library.create_directory(&parent_id, &name).map(|_| ()));
                cx.spawn(async move |actions, cx| {
                    let result = operation.await;
                    let _ = actions.update(cx, |actions, cx| match result {
                        Ok(()) => {
                            actions.name_dialog = None;
                            cx.emit(FolderActionsEvent::Reload);
                            cx.notify();
                        }
                        Err(error) => {
                            if let Some(dialog) = &mut actions.name_dialog {
                                dialog.busy = false;
                                dialog.error = Some(error.into());
                            }
                            cx.notify();
                        }
                    });
                })
                .detach();
            }
            FolderNameMode::Rename => {
                let directory_id = self.current_id;
                let operation = crate::services::backend::library_blocking(self.ui.backend(), move |library| library.rename_directory(&directory_id, &name));
                cx.spawn(async move |actions, cx| {
                    let result = operation.await;
                    let _ = actions.update(cx, |actions, cx| match result {
                        Ok(name) => {
                            actions.current_name = Some(name.clone());
                            actions.name_dialog = None;
                            cx.emit(FolderActionsEvent::Renamed(name));
                            cx.notify();
                        }
                        Err(error) => {
                            if let Some(dialog) = &mut actions.name_dialog {
                                dialog.busy = false;
                                dialog.error = Some(error.into());
                            }
                            cx.notify();
                        }
                    });
                })
                .detach();
            }
        }
    }

    fn open_move_folder(&mut self, cx: &mut Context<Self>) {
        if self.current_name.is_none() || self.name_dialog.is_some() || self.move_dialog.is_some() {
            return;
        }
        let source_id = self.current_id;
        let generation = self.move_request.begin();
        let load = crate::services::backend::library_blocking(self.ui.backend(), move |library| library.folder_destinations());
        let task = cx.spawn(async move |actions, cx| {
            let result = load.await;
            let _ = actions.update(cx, |actions, cx| {
                if !actions.move_request.is_current(generation) || actions.current_id != source_id {
                    return;
                }
                match result {
                    Ok(mut destinations) => {
                        let source_path = destinations.iter().find(|destination| destination.id == source_id).map(|destination| destination.path.clone()).unwrap_or_default();
                        let source_prefix = format!("{source_path} / ");
                        let current_parent = destinations.iter().find(|destination| destination.id == source_id).and_then(|destination| destination.parent_id).unwrap_or(ROOT_DIR_ID);
                        destinations.retain(|destination| destination.id != source_id && destination.path != source_path && !destination.path.starts_with(&source_prefix));
                        let picker = cx.new(|_| FolderPicker::new(ROOT_DIR_ID, destinations).disabling(current_parent));
                        let subscription = cx.subscribe(&picker, |actions, _, event, cx| actions.move_destination_selected(*event, cx));
                        actions.move_dialog = Some(MoveFolderDialog { source_id, picker, _subscription: subscription });
                        cx.notify();
                    }
                    Err(error) => log::error!("failed to load folder destinations: {error}"),
                }
            });
        });
        self.move_request.install(task);
    }

    fn move_destination_selected(&mut self, event: FolderPickerEvent, cx: &mut Context<Self>) {
        let Some(dialog) = self.move_dialog.take() else { return };
        let FolderPickerEvent::Selected(parent_id) = event else {
            cx.notify();
            return;
        };
        let source_id = dialog.source_id;
        let operation = crate::services::backend::library_blocking(self.ui.backend(), move |library| library.move_directory(&source_id, &parent_id));
        cx.spawn(async move |actions, cx| {
            let result = operation.await;
            let _ = actions.update_in(cx, |_, window, cx| match result {
                Ok(_) => cx.emit(FolderActionsEvent::OpenRoot),
                Err(error) => window.push_notification(Notification::error(format!("Could not move folder: {error}")).id1::<Self>("folder-move-error").autohide(false), cx),
            });
        })
        .detach();
    }

    fn render_name_dialog(actions: Entity<Self>, cx: &mut App) -> Option<AnyElement> {
        let (mode, input, error, busy) = {
            let actions = actions.read(cx);
            let dialog = actions.name_dialog.as_ref()?;
            (dialog.mode, dialog.input.clone(), dialog.error.clone(), dialog.busy)
        };
        let theme = components::browser_theme(cx);
        let cancel_actions = actions.clone();
        let dismiss_actions = actions.clone();
        let submit_actions = actions.clone();
        let (title, idle_label, busy_label, id_suffix) = match mode {
            FolderNameMode::Create => ("Create folder", "Create", "Creating…", "create-folder"),
            FolderNameMode::Rename => ("Rename folder", "Rename", "Renaming…", "rename-folder"),
        };
        let cancel = components::outlined_text_button(format!("cancel-{id_suffix}"), "Cancel").disabled(busy).on_click(move |_, _, cx| {
            cancel_actions.update(cx, |actions, cx| actions.close_name_dialog(cx));
        });
        let submit = Button::new(format!("confirm-{id_suffix}"))
            .primary()
            .label(if busy { busy_label } else { idle_label })
            .disabled(busy || input.read(cx).value().trim().is_empty())
            .on_click(move |_, window, cx| submit_actions.update(cx, |actions, cx| actions.submit_name_dialog(window, cx)));
        let panel = components::modal_surface(theme)
            .w(px(360.0))
            .max_w_full()
            .min_h(px(180.0))
            .p(px(components::SPACE_MD))
            .flex()
            .flex_col()
            .gap(px(components::SPACE_MD))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(div().text_size(rems(1.125)).font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme.text).child(title))
            .child(Input::new(&input).h(px(components::TOPBAR_ACTION_HEIGHT)))
            .children(error.map(|error| div().text_size(rems(0.75)).text_color(theme.danger).child(error)))
            .child(div().flex_1())
            .child(div().flex().items_center().justify_end().gap(px(components::SPACE_SM)).child(cancel).child(submit));
        Some(
            gpui::deferred(
                components::modal_overlay()
                    .occlude()
                    .child(components::modal_scrim(format!("{id_suffix}-scrim"), theme).on_click(move |_, _, cx| {
                        dismiss_actions.update(cx, |actions, cx| actions.close_name_dialog(cx));
                    }))
                    .child(panel),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    pub(crate) fn render_trigger(actions: Entity<Self>, cx: &mut App) -> AnyElement {
        let add_book_actions = actions.clone();
        let add_folder_actions = actions.clone();
        let create_actions = actions.clone();
        let rename_actions = actions.clone();
        let move_actions = actions.clone();
        let has_current_folder = actions.read(cx).current_name.is_some();
        let trigger = components::topbar_icon_action_button("folder-add", "Add", IconName::Plus, components::browser_theme(cx));
        let menu = trigger.dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
            let add_book_actions = add_book_actions.clone();
            let add_folder_actions = add_folder_actions.clone();
            let create_actions = create_actions.clone();
            let rename_actions = rename_actions.clone();
            let move_actions = move_actions.clone();
            let menu = menu
                .item(PopupMenuItem::new("Add book").icon(IconName::BookOpen).on_click(move |_, _, cx| add_book_actions.update(cx, |actions, cx| actions.choose_book(cx))))
                .item(PopupMenuItem::new("Add folder").icon(IconName::FolderOpen).on_click(move |_, _, cx| add_folder_actions.update(cx, |actions, cx| actions.choose_folder(cx))))
                .item(PopupMenuItem::new("Create folder").icon(IconName::Plus).on_click(move |_, window, cx| create_actions.update(cx, |actions, cx| actions.open_name_dialog(FolderNameMode::Create, window, cx))));
            if has_current_folder {
                menu.item(PopupMenuItem::new("Rename current folder…").on_click(move |_, window, cx| rename_actions.update(cx, |actions, cx| actions.open_name_dialog(FolderNameMode::Rename, window, cx))))
                    .item(PopupMenuItem::new("Move current folder…").on_click(move |_, _, cx| move_actions.update(cx, |actions, cx| actions.open_move_folder(cx))))
            } else {
                menu
            }
        });
        div().flex().items_center().child(menu).into_any_element()
    }

    pub(crate) fn render_overlay(actions: Entity<Self>, cx: &mut App) -> Option<AnyElement> {
        if let Some(dialog) = Self::render_name_dialog(actions.clone(), cx) {
            return Some(dialog);
        }
        actions.read(cx).move_dialog.as_ref().map(|dialog| dialog.picker.clone().into_any_element())
    }
}
