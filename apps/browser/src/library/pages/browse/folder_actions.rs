//! Folder commands and the shared placement picker for folder and flat browsing.
//!
//! "Folder-scoped" covers both the folders a listing holds and the books placed
//! in it: copying, moving and discarding are placement changes either way, and
//! keeping them together gives one picker, one confirmation and one error path
//! for a selection that mixes the two.

use ui_components::user_facing_error;

use library_backend::DownloadState;
use library_backend::{BackendError, LibraryClient};

use crate::services::notify_error;

use gpui::prelude::*;
use gpui::{Anchor, AnyElement, App, Context, Entity, EventEmitter, Focusable, IntoElement, MouseButton, SharedString, Subscription, Task, Window, div, px, rems};
use gpui_component::button::ButtonVariants;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::{DropdownMenu as _, PopupMenu};
use gpui_component::{Disableable, IconName};
use sync_common::{BookId, DirId, ROOT_DIR_ID};
use ui_components as components;

use super::folder_picker::{FolderPicker, FolderPickerEvent};
use crate::library::services::{LibraryContext, UndoEntry, UndoStep};

/// Names a set of targets the way a notification would: "3 books", "1 folder
/// and 2 books".
pub(super) fn describe(targets: &FolderTargets) -> String {
    [(targets.folders.len(), "folder"), (targets.books.len(), "book")]
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, noun)| if count == 1 { format!("1 {noun}") } else { format!("{count} {noun}s") })
        .collect::<Vec<_>>()
        .join(" and ")
}

/// The folder commands for a menu opened over a selection, worked out when the
/// menu opens and laid into it by [`FolderActions::item_menu_items`].
#[derive(Clone)]
pub(crate) struct FolderMenuOptions {
    targets: FolderTargets,
    /// A lone folder and the name it would be renamed from. Renaming is a
    /// single-folder command: there is one name to type.
    rename: Option<(DirId, String)>,
    /// Whether anything selected still holds downloaded bytes.
    can_evict: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FolderActionsEvent {
    Moved(Vec<DirId>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FolderNameMode {
    Create,
    Rename,
}

struct FolderNameDialog {
    mode: FolderNameMode,
    /// The folder being renamed, or the parent a new folder is created under.
    /// Held per dialog because a chip's
    /// context menu acts on a folder the page is not inside.
    target_id: DirId,
    /// The name a rename is replacing, which is what undoing it puts back.
    previous_name: Option<String>,
    input: Entity<InputState>,
    error: Option<SharedString>,
    busy: bool,
    _subscription: Subscription,
}

/// What a chosen destination does to whatever opened the picker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FolderPlacement {
    Copy,
    Move,
}

#[derive(Clone, Copy)]
enum TargetOperation {
    Place(FolderPlacement, DirId),
    Remove,
}

impl TargetOperation {
    async fn apply(self, library: &LibraryClient, targets: FolderTargets, entry: &mut UndoEntry) -> Result<(), BackendError> {
        let source = targets.source;
        entry
            .record(targets.folders, |folder| {
                let parent = targets.folder_parents.get(&folder).copied().or(source);
                async move {
                    match self {
                        Self::Place(FolderPlacement::Copy, destination) => library.copy_directory(folder, destination).await.map(|copy| UndoStep::TrashDirectory { directory: copy }),
                        Self::Place(FolderPlacement::Move, destination) => {
                            let parent = parent.ok_or("folder move requires a source folder")?;
                            library.move_directory(folder, destination).await.map(|_| UndoStep::MoveDirectory { directory: folder, parent })
                        }
                        Self::Remove => library.move_directory_to_trash(folder).await.map(|_| UndoStep::RestoreDirectory { directory: folder, parent }),
                    }
                }
            })
            .await?;
        for book in targets.books {
            let step = match self {
                Self::Place(FolderPlacement::Copy, destination) => library.copy_book_to_directory(book, source, destination).await?.then_some(UndoStep::RemoveBookFromDirectory { book, directory: destination }),
                Self::Place(FolderPlacement::Move, destination) => {
                    let source = source.ok_or("book move requires a source folder")?;
                    library.move_book_to_directory(book, source, destination).await?;
                    Some(UndoStep::MoveBook { book, from: destination, to: source })
                }
                Self::Remove => {
                    let source = source.ok_or("book removal requires a source folder")?;
                    library.remove_book_from_directory(book, source).await?;
                    Some(UndoStep::RestoreBookPlacement { book, directory: source })
                }
            };
            if let Some(step) = step {
                entry.push(step);
            }
        }
        Ok(())
    }
}

/// What a command acts on: one item from a context menu, or everything a
/// rubber band picked up.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct FolderTargets {
    pub(crate) folders: Vec<DirId>,
    pub(crate) folder_parents: std::collections::HashMap<DirId, DirId>,
    pub(crate) books: Vec<BookId>,
    /// The holder of listed books and fallback parent for ordinary folder chips.
    /// Promoted chips record their actual parents separately. Flat book copies
    /// resolve each source in the backend instead.
    pub(crate) source: Option<DirId>,
}

impl FolderTargets {
    pub(super) fn from_items(items: &[super::listing::BrowseItem], source: DirId, cx: &App) -> Option<Self> {
        let mut targets = Self { source: Some(source), ..Self::default() };
        for item in items {
            match item {
                super::listing::BrowseItem::Child { row, parent } => {
                    if let library_model::BrowseLocation::Folder(id) = row.id {
                        targets.folders.push(id);
                        if let Some(parent) = parent {
                            targets.folder_parents.insert(id, *parent);
                        }
                    }
                }
                super::listing::BrowseItem::Book(book) => targets.books.extend(book.read(cx).content_hash()),
            }
        }
        if targets.books.is_empty()
            && let Some(parent) = targets.folders.first().and_then(|id| targets.folder_parents.get(id)).copied()
            && targets.folders.iter().all(|id| targets.folder_parents.get(id).copied().unwrap_or(source) == parent)
        {
            targets.source = Some(parent);
            targets.folder_parents.clear();
        }
        (!targets.is_empty()).then_some(targets)
    }

    /// Whether dropping these into `destination` would do anything: a folder
    /// cannot go into itself, and a drop that leaves everything where it is
    /// has nothing to ask about.
    pub(crate) fn accepts(&self, destination: DirId) -> bool {
        !self.folders.contains(&destination) && !self.all_in_folder(destination)
    }

    pub(crate) fn all_in_folder(&self, directory: DirId) -> bool {
        (self.books.is_empty() || self.source == Some(directory)) && self.folders.iter().all(|folder| self.folder_parents.get(folder).copied().or(self.source) == Some(directory))
    }

    pub(crate) fn len(&self) -> usize {
        self.folders.len() + self.books.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

struct MoveFolderDialog {
    placement: FolderPlacement,
    targets: FolderTargets,
    picker: Entity<FolderPicker>,
    _subscription: Subscription,
}

/// The one modal the folder commands can have up at a time.
enum FolderDialog {
    Name(FolderNameDialog),
    Move(MoveFolderDialog),
}

/// Owns folder commands and book-copy destination selection. The shared browse page
/// observes high-level outcomes instead of carrying backend handles and modal
/// implementation state for every adapter.
pub(crate) struct FolderActions {
    ui: LibraryContext,
    dialog: Option<FolderDialog>,
    /// The destinations being loaded for the move picker. Replacing or
    /// clearing it drops the load, which cancels it.
    move_request: Option<Task<()>>,
}

impl EventEmitter<FolderActionsEvent> for FolderActions {}

impl FolderActions {
    pub(crate) fn new(ui: LibraryContext) -> Self {
        Self { ui, dialog: None, move_request: None }
    }

    pub(crate) fn has_modal(&self) -> bool {
        self.dialog.is_some()
    }

    fn name_dialog(&mut self) -> Option<&mut FolderNameDialog> {
        match &mut self.dialog {
            Some(FolderDialog::Name(dialog)) => Some(dialog),
            _ => None,
        }
    }

    /// Whether a name is being saved, which the dialog has to stay up for.
    fn saving_name(&self) -> bool {
        matches!(&self.dialog, Some(FolderDialog::Name(dialog)) if dialog.busy)
    }

    pub(crate) fn dismiss_modal(&mut self, cx: &mut Context<Self>) {
        if self.saving_name() {
            return;
        }
        self.dialog = None;
        self.cancel_move_selection(cx);
    }

    pub(crate) fn cancel_move_selection(&mut self, cx: &mut Context<Self>) {
        self.move_request = None;
        if matches!(self.dialog, Some(FolderDialog::Move(_))) {
            self.dialog = None;
        }
        cx.notify();
    }

    fn choose_book(&mut self, parent_id: DirId, cx: &mut Context<Self>) {
        self.ui.choose_and_import_book(parent_id, cx);
    }

    fn choose_folder(&mut self, parent_id: DirId, cx: &mut Context<Self>) {
        self.ui.choose_and_import_folder(parent_id, cx);
    }

    /// Renames one listed folder, which need not be the folder the page is
    /// inside.
    pub(crate) fn open_rename(&mut self, directory_id: DirId, name: String, window: &mut Window, cx: &mut Context<Self>) {
        if directory_id == ROOT_DIR_ID {
            return;
        }
        self.open_name_dialog_for(FolderNameMode::Rename, directory_id, Some(name), window, cx);
    }

    /// Renames a named folder, or creates a folder under `target_id`. The chip
    /// context menu passes the chip's folder; the toolbar passes the folder the
    /// page is currently inside.
    fn open_name_dialog_for(&mut self, mode: FolderNameMode, target_id: DirId, target_name: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialog.is_some() {
            return;
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Folder name"));
        if let Some(name) = target_name.as_ref().filter(|_| mode == FolderNameMode::Rename) {
            input.update(cx, |input, cx| input.set_value(name, window, cx));
        }
        let subscription = cx.subscribe_in(&input, window, |actions, _, event, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                actions.submit_name_dialog(window, cx);
            }
        });
        input.read(cx).focus_handle(cx).focus(window, cx);
        let previous_name = target_name.filter(|_| mode == FolderNameMode::Rename);
        self.dialog = Some(FolderDialog::Name(FolderNameDialog { mode, target_id, previous_name, input, error: None, busy: false, _subscription: subscription }));
        cx.notify();
    }

    fn close_name_dialog(&mut self, cx: &mut Context<Self>) {
        if self.saving_name() {
            return;
        }
        self.dialog = None;
        cx.notify();
    }

    fn submit_name_dialog(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self.name_dialog() else { return };
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
        let (mode, target_id, previous_name) = (dialog.mode, dialog.target_id, dialog.previous_name.clone());
        let session = self.ui.session();
        let library = self.ui.backend().clone();
        cx.spawn(async move |actions, cx| {
            let result = match mode {
                FolderNameMode::Create => library.create_directory(target_id, name).await,
                FolderNameMode::Rename => library.rename_directory(target_id, name).await.map(|_| ()),
            };
            if result.is_ok()
                && mode == FolderNameMode::Rename
                && let Some(previous) = previous_name
            {
                let _ = session.update(cx, |session, _| session.push_undo(UndoEntry::new(format!("Renamed “{previous}”")).with(UndoStep::RenameDirectory { directory: target_id, name: previous })));
            }
            let _ = actions.update(cx, |actions, cx| {
                match result {
                    Ok(()) => actions.dialog = None,
                    Err(error) => {
                        if let Some(dialog) = actions.name_dialog() {
                            dialog.busy = false;
                            dialog.error = Some(user_facing_error(&error, "Could not save the folder name. Please try again.").into());
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Chooses a new home for everything in `targets`.
    pub(crate) fn open_move_targets(&mut self, targets: FolderTargets, cx: &mut Context<Self>) {
        self.open_placement_picker(FolderPlacement::Move, targets, cx);
    }

    /// Uses the same destination picker for folder copies and books from flat
    /// listings. Existing book assignments are retained and excluded from undo.
    pub(crate) fn open_copy_targets(&mut self, targets: FolderTargets, cx: &mut Context<Self>) {
        self.open_placement_picker(FolderPlacement::Copy, targets, cx);
    }

    fn open_placement_picker(&mut self, placement: FolderPlacement, targets: FolderTargets, cx: &mut Context<Self>) {
        if targets.is_empty() || targets.folders.contains(&ROOT_DIR_ID) || self.dialog.is_some() {
            return;
        }
        let library = self.ui.backend().clone();
        let load = async move { library.folder_destinations().await };
        let task = cx.spawn(async move |actions, cx| {
            let result = load.await;
            let _ = actions.update(cx, |actions, cx| {
                // A dialog opened while the destinations loaded keeps the screen.
                if actions.dialog.is_some() {
                    return;
                }
                match result {
                    Ok(mut destinations) => {
                        // No folder can land inside itself, either way, so every
                        // selected subtree is dropped. A folder copy may land
                        // beside its original; an existing book assignment is
                        // skipped when the copy runs.
                        for folder in &targets.folders {
                            let Some(path) = destinations.iter().find(|destination| destination.id == *folder).map(|destination| destination.path.clone()) else { continue };
                            let prefix = format!("{path} / ");
                            destinations.retain(|destination| destination.id != *folder && destination.path != path && !destination.path.starts_with(&prefix));
                        }
                        let picker = cx.new(|cx| {
                            let picker = FolderPicker::new(ROOT_DIR_ID, destinations, cx);
                            match targets.source {
                                Some(source) if placement == FolderPlacement::Move && targets.all_in_folder(source) => picker.disabling(source),
                                _ => picker,
                            }
                        });
                        let subscription = cx.subscribe(&picker, |actions, _, event, cx| actions.move_destination_selected(*event, cx));
                        actions.dialog = Some(FolderDialog::Move(MoveFolderDialog { placement, targets, picker, _subscription: subscription }));
                        cx.notify();
                    }
                    Err(error) => log::error!("failed to load folder destinations: {error}"),
                }
            });
        });
        self.move_request = Some(task);
    }

    fn move_destination_selected(&mut self, event: FolderPickerEvent, cx: &mut Context<Self>) {
        let Some(FolderDialog::Move(dialog)) = self.dialog.take() else { return };
        let FolderPickerEvent::Selected(destination_id) = event else {
            cx.notify();
            return;
        };
        self.place_targets(dialog.placement, dialog.targets, destination_id, None, cx);
    }

    /// Transfer resource bytes first; source removal and undo use their owning library.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn transfer_targets(&mut self, source_id: uuid::Uuid, placement: FolderPlacement, targets: FolderTargets, destination: DirId, destination_name: Option<SharedString>, cx: &mut Context<Self>) {
        if targets.is_empty() {
            return;
        }
        let source = self.ui.app().library(source_id);
        let library = self.ui.backend().clone();
        let session = self.ui.session();
        let label = format!("{} {}", if placement == FolderPlacement::Copy { "Copied" } else { "Moved" }, describe(&targets));
        let confirmation = destination_name.map(|name| format!("{label} to {name}")).unwrap_or_else(|| label.clone());
        cx.spawn(async move |actions, cx| {
            let progress = actions.update_in(cx, |_, window, cx| crate::services::ImportProgressToast::start(window, cx)).ok();
            let selection = library_backend::client::TransferSelection { folders: targets.folders, books: targets.books, source: targets.source };
            let outcome = library.transfer_from(&source, selection, destination, placement == FolderPlacement::Move).await;
            let mut entry = UndoEntry::new(if outcome.error.is_some() { "Copied items between libraries".to_owned() } else { label });
            for directory in outcome.created_folders {
                entry.push(UndoStep::TrashDirectory { directory });
            }
            for (book, directory) in outcome.created_placements {
                entry.push(UndoStep::RemoveBookFromDirectory { book, directory });
            }
            // Undo restores the source before it removes destination placements.
            for (directory, parent) in outcome.removed_folders {
                entry.push_in(&source, UndoStep::RestoreDirectory { directory, parent });
            }
            for (book, directory) in outcome.removed_placements {
                entry.push_in(&source, UndoStep::RestoreBookPlacement { book, directory });
            }
            let changed = !entry.is_empty();
            let _ = session.update(cx, |session, _| session.push_undo(entry));
            drop(progress);
            let _ = actions.update_in(cx, |_, window, cx| {
                if let Some(error) = outcome.error {
                    if changed {
                        crate::services::notify_undoable("library-transfer-partial", "Copied items; transfer did not finish", window, cx);
                    }
                    notify_error("library-transfer-error", format!("Could not finish the transfer. Some items may already have been copied. {}", user_facing_error(&error, "Please try again.")), window, cx);
                } else if changed {
                    crate::services::notify_undoable("library-transfer", confirmation, window, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn library_id(&self) -> &uuid::Uuid {
        self.ui.backend().id()
    }

    /// Runs a placement whose destination is already known — a drop onto a
    /// folder, where the folder under the pointer is the answer the picker
    /// would otherwise have asked for.
    ///
    /// `destination_name` names the folder in the confirmation, where it is
    /// known.
    pub(crate) fn place_targets(&mut self, placement: FolderPlacement, targets: FolderTargets, destination_id: DirId, destination_name: Option<SharedString>, cx: &mut Context<Self>) {
        if targets.is_empty() || targets.folders.contains(&destination_id) {
            return;
        }
        let moved = if placement == FolderPlacement::Move { targets.folders.clone() } else { Vec::new() };
        let label = format!("{} {}", if placement == FolderPlacement::Copy { "Copied" } else { "Moved" }, describe(&targets));
        let confirmation = match destination_name {
            Some(name) => format!("{label} to {name}"),
            None => label.clone(),
        };
        self.run_targets(
            targets,
            TargetOperation::Place(placement, destination_id),
            label,
            move |result, window, cx| match result {
                Ok(()) => {
                    cx.emit(FolderActionsEvent::Moved(moved));
                    crate::services::notify_undoable("folder-placement", confirmation, window, cx);
                }
                Err(error) => {
                    // Items are placed one at a time, so a failure part-way
                    // leaves the earlier ones placed; each placement already
                    // reported its own contents change.
                    notify_error(
                        "folder-placement-error",
                        format!(
                            "Could not {} everything selected. Some items may already have been transferred. {}",
                            match placement {
                                FolderPlacement::Copy => "copy",
                                FolderPlacement::Move => "move",
                            },
                            user_facing_error(&error, "Please try again for the remaining items.")
                        ),
                        window,
                        cx,
                    );
                }
            },
            cx,
        );
    }

    /// Discards everything in `targets` at once. Folders go to Trash whole; a
    /// book leaves the folder it was listed in, which sends it to Trash only if
    /// that was the last place holding it.
    ///
    /// No confirmation: the removal is on the undo stack, and the toast that
    /// reports it offers the undo.
    pub(crate) fn trash_targets(&mut self, targets: FolderTargets, cx: &mut Context<Self>) {
        if targets.is_empty() || targets.folders.contains(&ROOT_DIR_ID) {
            return;
        }
        let label = format!("Trashed {}", describe(&targets));
        let report = label.clone();
        self.run_targets(targets, TargetOperation::Remove, label, move |result, window, cx| Self::finish_removal(result, report, "folder-trash", "move everything selected to Trash", window, cx), cx);
    }

    /// Takes the selected books out of the folder they are listed in without
    /// moving anything to Trash directly: a book stays in the library while
    /// another folder holds it, and only reaches Trash when this was its last
    /// folder. Folders in the selection are left alone.
    pub(crate) fn remove_books_from_folder(&mut self, mut targets: FolderTargets, cx: &mut Context<Self>) {
        if targets.books.is_empty() {
            return;
        }
        targets.folders.clear();
        let label = format!("Removed {}", describe(&targets));
        let report = label.clone();
        self.run_targets(targets, TargetOperation::Remove, label, move |result, window, cx| Self::finish_removal(result, report, "folder-remove", "remove everything selected from the folder", window, cx), cx);
    }

    fn finish_removal(result: Result<(), BackendError>, label: String, id: &'static str, action: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        match result {
            Ok(()) => crate::services::notify_undoable(id, label, window, cx),
            Err(error) => notify_error(id, format!("Could not {action}. {}", user_facing_error(&error, "Please try again.")), window, cx),
        }
    }

    /// Commit undo before reporting completion, including any successful prefix.
    fn run_targets(&mut self, targets: FolderTargets, operation: TargetOperation, label: String, finished: impl FnOnce(Result<(), BackendError>, &mut Window, &mut Context<Self>) + 'static, cx: &mut Context<Self>) {
        let library = self.ui.backend().clone();
        let session = self.ui.session();
        cx.spawn(async move |actions, cx| {
            let mut entry = UndoEntry::new(label);
            let result = operation.apply(&library, targets, &mut entry).await;
            let unchanged = result.is_ok() && entry.is_empty();
            let _ = session.update(cx, |session, _| session.push_undo(entry));
            let _ = actions.update_in(cx, |_, window, cx| {
                // A fully skipped copy must not offer undo for an earlier action.
                if unchanged {
                    cx.notify();
                } else {
                    finished(result, window, cx);
                }
            });
        })
        .detach();
    }

    /// Drops the downloaded copies for everything selected while keeping every
    /// entry. Books the server does not hold keep their bytes, and surface as
    /// an error naming the first one that could not be evicted.
    pub(crate) fn evict_downloads(&mut self, targets: FolderTargets, _window: &mut Window, cx: &mut Context<Self>) {
        if targets.is_empty() {
            return;
        }
        let library = self.ui.backend().clone();
        cx.spawn(async move |actions, cx| {
            let mut result = Ok(());
            for book in targets.books {
                if let Err(error) = library.remove_local_book_copy(book).await {
                    result = Err(error);
                    break;
                }
            }
            if result.is_ok() {
                for folder in targets.folders {
                    if let Err(error) = library.evict_folder_downloads(folder).await {
                        result = Err(error);
                        break;
                    }
                }
            }
            let _ = actions.update_in(cx, |_, window, cx| {
                if let Err(error) = result {
                    notify_error("folder-evict-error", format!("Could not remove every download. {}", user_facing_error(&error, "Please try again.")), window, cx);
                }
            });
        })
        .detach();
    }

    fn render_name_dialog(actions: Entity<Self>, window: &Window, cx: &mut App) -> Option<AnyElement> {
        let (mode, input, error, busy) = {
            let actions = actions.read(cx);
            let Some(FolderDialog::Name(dialog)) = &actions.dialog else { return None };
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
        let cancel = components::dialog_cancel_button(format!("cancel-{id_suffix}"), "Cancel").disabled(busy).on_click(move |_, _, cx| {
            cancel_actions.update(cx, |actions, cx| actions.close_name_dialog(cx));
        });
        let submit = components::base_button(format!("confirm-{id_suffix}"))
            .primary()
            .label(if busy { busy_label } else { idle_label })
            .tooltip(components::shortcut_tooltip(idle_label, "enter"))
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
            .child(div().text_size(rems(1.125)).text_color(theme.text).child(title))
            .child(Input::new(&input).h(gpui::rems(components::topbar_action_height_rem())))
            .children(error.map(|error| components::error_text(theme).child(error)))
            .child(div().flex_1())
            .child(div().flex().items_center().justify_end().gap(px(components::SPACE_SM)).child(cancel).child(submit));
        Some(
            gpui::deferred(components::anchor_to_window(
                components::modal_overlay()
                    .occlude()
                    .child(components::modal_scrim(format!("{id_suffix}-scrim"), theme).on_click(move |_, _, cx| {
                        dismiss_actions.update(cx, |actions, cx| actions.close_name_dialog(cx));
                    }))
                    .child(panel),
                window,
            ))
            .with_priority(2)
            .into_any_element(),
        )
    }

    /// Works out the commands over `targets`. The listing supplies what only it
    /// knows about its folders: their names, and whether they hold downloaded
    /// books.
    pub(super) fn item_options(&self, targets: FolderTargets, items: &[super::listing::BrowseItem], cx: &App) -> FolderMenuOptions {
        let rename = match (targets.folders.as_slice(), targets.books.is_empty()) {
            ([id], true) => Some((*id, items.iter().filter_map(super::listing::BrowseItem::child).find(|row| row.id == library_model::BrowseLocation::Folder(*id)).map(|row| row.name.clone()).unwrap_or_default())),
            _ => None,
        };
        let bridge = self.ui.updates().read(cx);
        let can_evict = targets.books.iter().any(|hash| matches!(bridge.download_state(*hash), Some(DownloadState::Downloaded))) || items.iter().filter_map(super::listing::BrowseItem::child).any(|row| row.downloaded_book_count > 0);
        FolderMenuOptions { targets, rename, can_evict }
    }

    /// Lays the commands over a selection into a menu: rename, copy and move,
    /// taking books out of this folder, dropping downloads, and Trash.
    pub(crate) fn item_menu_items(actions: Entity<Self>, commands: FolderMenuOptions, mut menu: PopupMenu) -> PopupMenu {
        let FolderMenuOptions { targets, rename, can_evict } = commands;
        let folder_icon = || components::navigation_icon(components::NavigationIcon::Folder);
        if let Some((id, name)) = rename {
            let actions = actions.clone();
            menu = menu.item(components::menu_item_with_icon("Rename…", IconName::Replace, move |_, window, cx| actions.update(cx, |actions, cx| actions.open_rename(id, name.clone(), window, cx))));
        }
        let (copy_actions, copy_targets) = (actions.clone(), targets.clone());
        let (move_actions, move_targets) = (actions.clone(), targets.clone());
        menu = menu.item(components::menu_item_with_icon("Copy to folder…", IconName::Copy, move |_, _, cx| copy_actions.update(cx, |actions, cx| actions.open_copy_targets(copy_targets.clone(), cx)))).item(components::menu_item_with_icon(
            "Move to folder…",
            folder_icon(),
            move |_, _, cx| move_actions.update(cx, |actions, cx| actions.open_move_targets(move_targets.clone(), cx)),
        ));
        // Leaving a folder and leaving the library are separate commands: the
        // first keeps the books in their other folders, the second moves the
        // whole selection to Trash.
        if !targets.books.is_empty() {
            let (remove_actions, remove_targets) = (actions.clone(), targets.clone());
            menu = menu.item(components::menu_item_with_icon("Remove from folder", folder_icon(), move |_, _, cx| remove_actions.update(cx, |actions, cx| actions.remove_books_from_folder(remove_targets.clone(), cx))));
        }
        menu = menu.separator();
        if can_evict {
            let (evict_actions, evict_targets) = (actions.clone(), targets.clone());
            menu = menu.item(components::menu_item_with_icon(if cfg!(target_arch = "wasm32") { "Remove local copy" } else { "Remove download" }, IconName::HardDrive, move |_, window, cx| {
                evict_actions.update(cx, |actions, cx| actions.evict_downloads(evict_targets.clone(), window, cx))
            }));
        }
        menu.item(components::menu_item_with_icon("Move to Trash", components::navigation_icon(components::NavigationIcon::Trash2), move |_, _, cx| actions.update(cx, |actions, cx| actions.trash_targets(targets.clone(), cx))))
    }

    /// The same move/copy choices for internal items and external files.
    pub(super) fn drop_menu_items(name: &str, menu: PopupMenu, place: impl Fn(FolderPlacement, &mut App) + 'static) -> PopupMenu {
        let place = std::rc::Rc::new(place);
        let move_items = place.clone();
        menu.item(components::menu_item_with_icon(format!("Move to {name}"), components::navigation_icon(components::NavigationIcon::Folder), move |_, _, cx| move_items(FolderPlacement::Move, cx))).item(components::menu_item_with_icon(
            format!("Copy to {name}"),
            IconName::Copy,
            move |_, _, cx| place(FolderPlacement::Copy, cx),
        ))
    }

    /// The commands for adding content to the folder being browsed, as menu
    /// items. Shared by the toolbar's Add button and by a secondary click on
    /// the listing's empty space.
    pub(crate) fn add_menu_items(actions: Entity<Self>, parent_id: DirId, menu: PopupMenu) -> PopupMenu {
        let add_book_actions = actions.clone();
        let add_folder_actions = actions.clone();
        let create_actions = actions;
        menu.item(components::menu_item_with_icon("Add book", IconName::BookOpen, move |_, _, cx| add_book_actions.update(cx, |actions, cx| actions.choose_book(parent_id, cx))))
            .item(components::menu_item_with_icon("Add folder", IconName::FolderOpen, move |_, _, cx| add_folder_actions.update(cx, |actions, cx| actions.choose_folder(parent_id, cx))))
            .item(components::menu_item_with_icon("Create folder", IconName::Plus, move |_, window, cx| create_actions.update(cx, |actions, cx| actions.open_name_dialog_for(FolderNameMode::Create, parent_id, None, window, cx))))
    }

    pub(crate) fn render_trigger(actions: Entity<Self>, parent_id: DirId, menu_anchor: Anchor, cx: &mut App) -> AnyElement {
        let trigger = components::outlined_icon_button("folder-add", "Add", IconName::Plus, components::browser_theme(cx));
        let menu = trigger.dropdown_menu_with_anchor(menu_anchor, move |menu, _, _| Self::add_menu_items(actions.clone(), parent_id, menu));
        div().flex().items_center().child(menu).into_any_element()
    }

    pub(crate) fn render_overlay(actions: Entity<Self>, window: &Window, cx: &mut App) -> Option<AnyElement> {
        match &actions.read(cx).dialog {
            Some(FolderDialog::Move(dialog)) => Some(dialog.picker.clone().into_any_element()),
            _ => Self::render_name_dialog(actions, window, cx),
        }
    }
}
