//! Shared library download actions for Settings and the library switcher.

use ui_text::text;

use app::AppClient;
use gpui::prelude::*;
use gpui::{App, Context, Render, SharedString, Task, Window, div, px};
use gpui_component::menu::PopupMenu;
use gpui_component::notification::Notification;
use gpui_component::progress::Progress;
use gpui_component::{Disableable, WindowExt};
use library_backend::{LibraryBookStorage, LibraryClient, ROOT_DIR_ID};
use sync_common::LibraryId;
use ui_components::{self as components, user_facing_error};

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
}

pub(crate) fn library_book_menu(menu: PopupMenu, backend: AppClient, id: LibraryId, name: String, enabled: bool, busy: bool) -> PopupMenu {
    if !enabled {
        return menu;
    }
    let library = backend.management.library(id);
    let remove_library = library.clone();
    let remove_name = name.clone();
    menu.item(components::menu_item(Action::Download.title(), move |_, window, cx| open_dialog(library.clone(), name.clone(), Action::Download, window, cx)).disabled(busy))
        .item(components::menu_item(Action::Remove.title(), move |_, window, cx| open_dialog(remove_library.clone(), remove_name.clone(), Action::Remove, window, cx)).disabled(busy))
}

fn open_dialog(library: LibraryClient, name: String, action: Action, window: &mut Window, cx: &mut App) {
    // Let the triggering popup finish restoring focus before the dialog opens.
    window.defer(cx, move |window, cx| {
        window.defer(cx, move |window, cx| {
            let view = cx.new(|cx| DownloadDialog::new(library, name, action, cx));
            window.open_dialog(cx, move |dialog, window, _| {
                let confirm = view.clone();
                dialog
                    .w(px((f32::from(window.viewport_size().width) - 2.0 * components::SPACE_MD).clamp(1.0, 420.0)))
                    .p_0()
                    .close_button(false)
                    .overlay_closable(false)
                    .on_ok(move |_, window, cx| {
                        confirm.update(cx, |view, cx| view.start(window, cx));
                        false
                    })
                    .child(view.clone())
            });
        });
    });
}

struct DownloadDialog {
    library: LibraryClient,
    name: String,
    action: Action,
    review: Option<Result<LibraryBookStorage, SharedString>>,
    load: Option<Task<()>>,
}

impl DownloadDialog {
    fn new(library: LibraryClient, name: String, action: Action, cx: &mut Context<Self>) -> Self {
        let mut view = Self { library, name, action, review: None, load: None };
        view.refresh(cx);
        view
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.review = None;
        let library = self.library.clone();
        let for_download = matches!(self.action, Action::Download);
        self.load = Some(cx.spawn(async move |view, cx| {
            let result = library.book_storage(for_download).await;
            let _ = view.update(cx, |view, cx| {
                view.review = Some(result.map_err(|error| user_facing_error(&error, text!(browse::could_not_check_book_storage)).into()));
                cx.notify();
            });
        }));
    }

    fn can_start(&self) -> bool {
        let Some(Ok(storage)) = &self.review else { return false };
        !storage.activity.blocks_file_changes()
            && match self.action {
                Action::Download => !storage.download_book_ids.is_empty() && storage.download_blocker().is_none(),
                Action::Remove => storage.removable_books > 0,
            }
    }

    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_start() {
            return;
        }
        // Consuming the review also prevents a second confirmation.
        let Some(Ok(storage)) = self.review.take() else { return };
        let confirmed_bytes = storage.required_bytes;
        let confirmed_books = storage.download_book_ids;
        let library = self.library.clone();
        let action = self.action;
        let progress_id = SharedString::from(uuid::Uuid::new_v4().to_string());
        let title = match action {
            Action::Download => text!(browse::downloading_books),
            Action::Remove => text!(browse::removing_downloads),
        };
        window.push_notification(Notification::new().id1::<DownloadDialog>(progress_id.clone()).autohide(false).title(title).content(|_, _, _| Progress::new("library-download-progress").loading(true).into_any_element()), cx);
        window.close_dialog(cx);
        // Own the task independently of the dialog so navigation stays available
        // and dismissing the review cannot cancel an accepted download.
        window
            .spawn(cx, async move |cx| {
                let result = match action {
                    Action::Download => library.download_all_books(confirmed_books, confirmed_bytes).await.map(|_| text!(browse::books_downloaded).to_owned()),
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
        let recheck = error.is_some() || storage.is_some_and(|storage| storage.activity.blocks_file_changes() || matches!(self.action, Action::Download) && storage.download_blocker().is_some());
        let mut body = div()
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
                Action::Remove => (storage.removable_books, storage.removable_bytes),
            };
            body = body.child(div().child(format!(text!(browse::book_count_with_size), format_storage_bytes(bytes), count = count)));
            let stat = |label: &'static str, value: String| div().flex().justify_between().gap(px(components::SPACE_MD)).child(label).child(value);
            let mut stats = div().flex().flex_col().gap(px(components::SPACE_SM)).child(stat(text!(browse::on_disk), format_storage_bytes(storage.local_bytes)));
            if matches!(self.action, Action::Download) {
                stats = stats.child(stat(text!(browse::space_needed), format_storage_bytes(storage.required_bytes)));
                stats = stats.child(stat(text!(browse::available), storage.available_bytes.map(format_storage_bytes).unwrap_or_else(|| text!(common::unavailable).into())));
            }
            body = body.child(stats);
            if let Action::Download = self.action {
                if let Some(reason) = storage.download_blocker() {
                    body = body.child(components::error_text(theme).child(reason));
                }
            }
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
                .child(components::dialog_cancel_button("library-downloads-cancel", text!(common::cancel)).on_click(|_, window, cx| window.close_dialog(cx)))
                .child(components::base_button("library-downloads-confirm").primary().label(self.action.label()).disabled(!self.can_start()).on_click(cx.listener(|view, _, window, cx| view.start(window, cx)))),
        )
    }
}
