//! Shared library download actions for Settings and the library switcher.

use ui_text::text;

use app::AppClient;
use gpui::prelude::*;
use gpui::{App, Context, DismissEvent, Entity, FocusHandle, Focusable, KeyDownEvent, Render, SharedString, Task, Window, div, px};
use gpui_component::menu::PopupMenu;
use gpui_component::notification::Notification;
use gpui_component::progress::Progress;
use gpui_component::{Disableable, WindowExt};
use library_backend::{LibraryBookStorage, LibraryClient, ROOT_DIR_ID};
use library_model::{BookFileActivity, LibraryUpdate};
use sync_common::LibraryId;
use ui_components::{self as components, user_facing_error};

use crate::navigation::{Libraries, LibraryOperationDialog};
use crate::services::{format_storage_bytes, notify_error, notify_info};

#[derive(Clone, Copy)]
enum Action {
    Download,
    Remove,
}

impl Action {
    fn title(self) -> &'static str {
        match self {
            Self::Download => text!(browse::download_all_books),
            Self::Remove => text!(browse::remove_downloaded_books),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Download => text!(common::download),
            Self::Remove => text!(browse::remove_downloads),
        }
    }

    fn blocked_by(self, activity: BookFileActivity) -> bool {
        match self {
            Self::Download => activity.blocks_download_requests(),
            Self::Remove => activity.blocks_file_changes(),
        }
    }
}

pub(crate) fn library_book_menu(menu: PopupMenu, libraries: Entity<Libraries>, backend: AppClient, id: LibraryId, name: String, enabled: bool, busy: bool) -> PopupMenu {
    if !enabled {
        return menu;
    }
    let Ok(library) = backend.management.library(id) else { return menu };
    let remove_library = library.clone();
    let remove_name = name.clone();
    let remove_host = libraries.clone();
    menu.item(components::menu_item(Action::Download.title(), move |_, window, cx| open_dialog(libraries.clone(), library.clone(), name.clone(), Action::Download, window, cx)).disabled(busy))
        .item(components::menu_item(Action::Remove.title(), move |_, window, cx| open_dialog(remove_host.clone(), remove_library.clone(), remove_name.clone(), Action::Remove, window, cx)).disabled(busy))
}

fn open_dialog(libraries: Entity<Libraries>, library: LibraryClient, name: String, action: Action, window: &mut Window, cx: &mut App) {
    let selection_epoch = libraries.read(cx).selection_epoch();
    // Let the triggering popup finish restoring focus before the dialog opens.
    window.defer(cx, move |window, cx| {
        window.defer(cx, move |window, cx| {
            let id = *library.id();
            let view = cx.new(|cx| DownloadDialog::new(library, name, action, cx));
            libraries.update(cx, |libraries, cx| libraries.show_operation_dialog(id, view, 420.0, selection_epoch, window, cx));
        });
    });
}

struct DownloadDialog {
    focus: FocusHandle,
    library: LibraryClient,
    name: String,
    action: Action,
    review: Option<Result<LibraryBookStorage, SharedString>>,
    activity: Option<BookFileActivity>,
    activity_revision: u64,
    load: Option<Task<()>>,
    updates: Option<Task<()>>,
}

impl gpui::EventEmitter<DismissEvent> for DownloadDialog {}
impl LibraryOperationDialog for DownloadDialog {}
impl Focusable for DownloadDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl DownloadDialog {
    fn new(library: LibraryClient, name: String, action: Action, cx: &mut Context<Self>) -> Self {
        let mut view = Self { focus: cx.focus_handle(), library, name, action, review: None, activity: None, activity_revision: 0, load: None, updates: None };
        view.refresh(cx);
        view
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.updates.is_none() {
            self.observe_activity(cx);
            return;
        }
        self.review = None;
        let library = self.library.clone();
        let for_download = matches!(self.action, Action::Download);
        let activity_revision = self.activity_revision;
        self.load = Some(cx.spawn(async move |view, cx| {
            let result = library.book_storage(for_download).await;
            let _ = view.update(cx, |view, cx| {
                // A response started before a live update must not restore old restrictions.
                if view.activity_revision == activity_revision {
                    if let Ok(storage) = &result {
                        view.activity = Some(storage.activity);
                    }
                }
                view.review = Some(result.map_err(|error| user_facing_error(&error, text!(browse::could_not_check_book_storage)).into()));
                cx.notify();
            });
        }));
    }

    fn observe_activity(&mut self, cx: &mut Context<Self>) {
        self.review = None;
        self.activity = None;
        self.load = None;
        let library = self.library.clone();
        self.updates = Some(cx.spawn(async move |view, cx| {
            let (updates, _) = match library.updates().await {
                Ok(updates) => updates,
                Err(error) => {
                    log::warn!("could not observe library download activity: {error}");
                    let _ = view.update(cx, |view, cx| view.activity_unavailable(user_facing_error(&error, text!(browse::could_not_check_book_storage)).into(), cx));
                    return;
                }
            };
            // Register first so activity changes cannot be missed while loading storage.
            if view.update(cx, |view, cx| view.refresh(cx)).is_err() {
                return;
            }
            while let Ok(update) = updates.recv().await {
                let mut activity = match update {
                    LibraryUpdate::BookFileActivity(activity) => Some(activity),
                    _ => None,
                };
                while let Ok(update) = updates.try_recv() {
                    if let LibraryUpdate::BookFileActivity(latest) = update {
                        activity = Some(latest);
                    }
                }
                let Some(activity) = activity else { continue };
                if view
                    .update(cx, |view, cx| {
                        let was_blocked = view.activity.is_some_and(|activity| view.action.blocked_by(activity));
                        view.activity_revision = view.activity_revision.wrapping_add(1);
                        view.activity = Some(activity);
                        if was_blocked && !view.action.blocked_by(activity) {
                            view.refresh(cx);
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    return;
                }
            }
            let _ = view.update(cx, |view, cx| view.activity_unavailable(text!(browse::could_not_check_book_storage).into(), cx));
        }));
    }

    fn activity_unavailable(&mut self, message: SharedString, cx: &mut Context<Self>) {
        self.updates = None;
        self.load = None;
        self.activity = None;
        self.review = Some(Err(message));
        cx.notify();
    }

    fn can_start(&self) -> bool {
        let (Some(Ok(storage)), Some(activity)) = (&self.review, self.activity) else { return false };
        if self.action.blocked_by(activity) {
            return false;
        }
        match self.action {
            Action::Download => !storage.download_book_ids.is_empty(),
            Action::Remove => storage.removable_books > 0,
        }
    }

    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_start() {
            return;
        }
        // Consuming the review also prevents a second confirmation.
        let Some(Ok(storage)) = self.review.take() else { return };
        let confirmed_books = storage.download_book_ids;
        let library = self.library.clone();
        let action = self.action;
        let progress_id = SharedString::from(uuid::Uuid::new_v4().to_string());
        let title = match action {
            Action::Download => text!(browse::downloading_books),
            Action::Remove => text!(browse::removing_downloads),
        };
        window.push_notification(Notification::new().id1::<DownloadDialog>(progress_id.clone()).autohide(false).title(title).content(|_, _, _| Progress::new("library-download-progress").loading(true).into_any_element()), cx);
        cx.emit(DismissEvent);
        // Own the task independently of the dialog so navigation stays available
        // and dismissing the review cannot cancel an accepted download.
        window
            .spawn(cx, async move |cx| {
                let result = match action {
                    Action::Download => library.download_all_books(confirmed_books).await.map(|_| text!(browse::books_downloaded).to_owned()),
                    Action::Remove => library.evict_folder_downloads(ROOT_DIR_ID).await.map(|count| format!(text!(browse::removed_downloads_for_count_books), count = count)),
                };
                let _ = cx.update(|window, cx| {
                    window.remove_notification1::<DownloadDialog>(progress_id, cx);
                    match result {
                        Ok(message) => notify_info("library-book-downloads", message, window, cx),
                        Err(error) => {
                            let fallback = match action {
                                Action::Download => text!(browse::could_not_download_the_books),
                                Action::Remove => text!(browse::could_not_remove_the_downloads),
                            };
                            notify_error("library-book-downloads", user_facing_error(&error, fallback), window, cx);
                        }
                    }
                });
            })
            .detach();
    }
}

impl Render for DownloadDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let loading = self.review.is_none();
        let storage = self.review.as_ref().and_then(|review| review.as_ref().ok());
        let error = self.review.as_ref().and_then(|review| review.as_ref().err());
        let recheck = error.is_some() || self.activity.is_some_and(|activity| self.action.blocked_by(activity));
        let mut body = div()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| match event.keystroke.key.as_str() {
                "enter" => {
                    view.start(window, cx);
                    cx.stop_propagation();
                }
                "escape" => {
                    cx.emit(DismissEvent);
                    cx.stop_propagation();
                }
                _ => {}
            }))
            .w_full()
            .p(px(components::SPACE_MD))
            .flex()
            .flex_col()
            .gap(px(components::SPACE_MD))
            .text_color(theme.text)
            .child(div().text_size(gpui::rems(components::TEXT_MD)).child(self.action.title()))
            .child(div().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(self.name.clone()));
        if let Some(storage) = storage {
            let (count, bytes) = match self.action {
                Action::Download => (storage.download_book_ids.len(), storage.download_bytes),
                Action::Remove => (storage.removable_books, Some(storage.removable_bytes)),
            };
            body = body.child(div().child(format!(text!(browse::book_count_with_size), bytes.map(format_storage_bytes).unwrap_or_else(|| text!(common::unavailable).into()), count = count)));
            body = body.child(div().flex().justify_between().gap(px(components::SPACE_MD)).child(text!(browse::on_disk)).child(format_storage_bytes(storage.local_bytes)));
        } else if loading {
            body = body.child(components::form_hint(text!(browse::checking_storage), theme));
        }
        body.children(error.cloned().map(|error| components::error_text(theme).child(error))).child(
            div()
                .flex()
                .justify_end()
                .gap(px(components::SPACE_SM))
                .children(recheck.then(|| {
                    components::link_button("library-downloads-recheck", text!(browse::check_again), loading).on_click(cx.listener(|view, _, _, cx| {
                        view.refresh(cx);
                        cx.notify();
                    }))
                }))
                .child(components::dialog_cancel_button("library-downloads-cancel", text!(common::cancel)).on_click(cx.listener(|_, _, _, cx| cx.emit(DismissEvent))))
                .child(components::base_button("library-downloads-confirm").primary().label(self.action.label()).disabled(!self.can_start()).on_click(cx.listener(|view, _, window, cx| view.start(window, cx)))),
        )
    }
}
