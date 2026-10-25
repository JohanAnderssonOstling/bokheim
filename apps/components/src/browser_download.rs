//! Download feedback shared by the desktop and Android book browsers.
use gpui::prelude::*;
use gpui::{App, AppContext, Context, Entity, SharedString, Window, div, px};
use gpui_component::notification::Notification;
use gpui_component::progress::Progress;
use gpui_component::{ActiveTheme, Root, WindowExt};
pub use library_model::{BrowserDownloadPhase, BrowserDownloadUpdate};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

pub struct BrowserDownload {
    pub name: String,
    pub progress: Entity<BrowserDownloadProgress>,
    pub phase: BrowserDownloadPhase,
    message: Option<String>,
}

#[derive(Default)]
pub struct BrowserDownloads {
    entries: HashMap<String, BrowserDownload>,
    cancelled: HashSet<String>,
    cancel: Option<Rc<dyn Fn(&str, &mut App)>>,
}

impl BrowserDownloads {
    pub fn on_cancel(mut self, cancel: impl Fn(&str, &mut App) + 'static) -> Self {
        self.cancel = Some(Rc::new(cancel));
        self
    }

    pub fn start(&mut self, id: &str) {
        self.cancelled.remove(id);
    }

    /// Records cancellation in the owning view before invoking its transport.
    /// Keep the ID until a new start so late updates cannot restore its toast.
    pub fn cancel(&mut self, id: &str) -> bool {
        self.cancelled.insert(id.to_owned())
    }
    pub fn get(&self, id: &str) -> Option<&BrowserDownload> {
        self.entries.get(id)
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Closing a browser also removes its terminal and status toasts, which
    /// are no longer represented by active download entries.
    pub fn clear(&mut self, window: &mut Window, cx: &mut App) {
        self.entries.clear();
        self.cancelled.clear();
        let notifications = Root::read(window, cx).notification.clone();
        notifications.update(cx, |list, cx| list.clear_by_type::<BrowserDownloadToast>(cx));
    }

    pub fn progress(&mut self, id: &str, percent: Option<u8>, window: &mut Window, cx: &mut App) {
        let Some(entry) = self.entries.get(id) else { return };
        self.apply(BrowserDownloadUpdate { download: id.into(), replaces: None, name: entry.name.clone(), phase: BrowserDownloadPhase::Downloading, percent, message: None }, window, cx);
    }

    /// Byte counts live inside the existing progress view so frequent updates
    /// cannot move the notification or restore one the reader dismissed.
    pub fn progress_detail(&mut self, id: &str, percent: Option<u8>, detail: SharedString, window: &mut Window, cx: &mut App) {
        self.progress(id, percent, window, cx);
        if let Some(entry) = self.entries.get(id) {
            entry.progress.update(cx, |progress, cx| {
                progress.detail = Some(detail);
                cx.notify();
            });
        }
    }

    pub fn waiting(&mut self, id: &str, window: &mut Window, cx: &mut App) {
        let Some(entry) = self.entries.get(id) else { return };
        self.apply(BrowserDownloadUpdate { download: id.into(), replaces: None, name: entry.name.clone(), phase: BrowserDownloadPhase::Waiting, percent: None, message: Some("Waiting for an unmetered connection".into()) }, window, cx);
    }

    pub fn apply(&mut self, update: BrowserDownloadUpdate, window: &mut Window, cx: &mut App) {
        let BrowserDownloadUpdate { download, replaces, name, phase, percent, message } = update;
        if replaces.as_ref().is_some_and(|previous| self.cancelled.contains(previous)) {
            self.cancelled.insert(download);
            return;
        }
        if phase != BrowserDownloadPhase::Dismissed && self.cancelled.contains(&download) {
            if replaces.is_some() {
                self.cancelled.remove(&download);
            } else {
                return;
            }
        }
        let mut renamed = false;
        if let Some(previous) = replaces.filter(|previous| previous != &download) {
            if let Some(entry) = self.entries.remove(&previous) {
                self.entries.entry(download.clone()).or_insert(entry);
                window.remove_notification1::<BrowserDownloadToast>(SharedString::from(previous), cx);
                renamed = true;
            }
        }
        if phase == BrowserDownloadPhase::Dismissed {
            if self.entries.remove(&download).is_some() {
                window.remove_notification1::<BrowserDownloadToast>(SharedString::from(download), cx);
            }
            return;
        }
        let message = if let BrowserDownloadPhase::Failed(failure) = phase { Some(crate::book_transfer_error_message(failure).to_owned()) } else { message };
        let text = message.as_ref().map_or_else(|| name.clone(), |message| format!("{name}: {message}"));
        let mut notification = match phase {
            BrowserDownloadPhase::Completed => {
                self.entries.remove(&download);
                Notification::success(text).title("Download complete")
            }
            BrowserDownloadPhase::Failed(_) => {
                self.entries.remove(&download);
                Notification::error(text).title("Download failed").autohide(false)
            }
            _ => {
                let entry = self.entries.entry(download.clone()).or_insert_with(|| BrowserDownload { name: String::new(), progress: cx.new(|_| BrowserDownloadProgress { percent: None, detail: None }), phase, message: None });
                entry.progress.update(cx, |progress, cx| {
                    progress.percent = percent.map(|value| value.min(100));
                    if phase != BrowserDownloadPhase::Downloading {
                        progress.detail = None;
                    }
                    cx.notify();
                });
                // Progress changes update the existing bar without moving the
                // toast or bringing back one the reader already dismissed.
                if !renamed && entry.name == name && entry.phase == phase && entry.message == message {
                    return;
                }
                entry.name = name;
                entry.phase = phase;
                entry.message = message;
                if phase == BrowserDownloadPhase::Waiting {
                    Notification::new().title("Transfer waiting").message(text).autohide(false)
                } else {
                    let progress = entry.progress.clone();
                    let title = match phase {
                        BrowserDownloadPhase::Preparing => "Preparing download",
                        BrowserDownloadPhase::Importing => "Adding to library",
                        _ => "Downloading",
                    };
                    Notification::new().title(title).message(text).autohide(false).content(move |_, _, _| progress.clone().into_any_element())
                }
            }
        };
        if !matches!(phase, BrowserDownloadPhase::Completed | BrowserDownloadPhase::Failed(_)) {
            if let Some(cancel) = self.cancel.clone() {
                let id = download.clone();
                notification = notification.on_close_button_click(move |_, cx| cancel(&id, cx));
            }
        }
        browser_download_notification(download, notification, window, cx);
    }
}

pub struct BrowserDownloadProgress {
    pub percent: Option<u8>,
    pub detail: Option<SharedString>,
}

impl Render for BrowserDownloadProgress {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = crate::browser_theme(cx).accent;
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .h_4()
                    .child(div().flex_1().child(Progress::new("download-progress").color(accent).loading(self.percent.is_none()).value(f32::from(self.percent.unwrap_or(0)))))
                    .when_some(self.percent, |row, percent| row.child(div().text_xs().child(format!("{percent}%")))),
            )
            .when_some(self.detail.clone(), |view, detail| view.child(div().text_xs().child(detail)))
    }
}

struct BrowserDownloadToast;

/// A transfer keeps one notification as it moves from progress to completion.
pub fn browser_download_notification(id: impl Into<SharedString>, notification: Notification, window: &mut Window, cx: &mut App) {
    let insets = window.insets();
    let margins = &cx.theme().notification.margins;
    let width = (window.viewport_size().width - insets.safe_area.left - insets.safe_area.right - margins.left - margins.right).max(px(0.0));
    window.push_notification(notification.id1::<BrowserDownloadToast>(id.into()).close_button_always_visible(true).max_w(width), cx);
}
