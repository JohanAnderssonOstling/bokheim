//! Window-owned export alerts, including libraries without an open page session.
use ui_text::text;

use super::Libraries;
use app::AppClient;
use gpui::{App, Context, IntoElement, ParentElement, SharedString, Styled, Window, div, px};
use gpui_component::{Sizable, WindowExt, notification::Notification};
use library_model::{AnnotationExportDestination, AnnotationExportFailure, AnnotationExportFailureKind, AnnotationExportFormat, AutomaticAnnotationExportStatus, LibraryUpdate};
use std::rc::Rc;
use sync_common::LibraryId;
use ui_components::Button;

struct AnnotationExportToast;

#[derive(Clone, Debug, PartialEq, Eq)]
struct FailureKey {
    destination: AnnotationExportDestination,
    failure: AnnotationExportFailure,
}

#[derive(Default)]
struct Notices {
    outputs: Vec<FailureKey>,
    error: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
enum Change {
    Show,
    Clear,
}

impl Notices {
    fn update(&mut self, status: &AutomaticAnnotationExportStatus) -> Option<Change> {
        let enabled = status.options.is_some();
        let outputs: Vec<FailureKey> = if enabled { status.outputs.iter().map(|output| FailureKey { destination: output.destination.clone(), failure: output.failure }).collect() } else { Vec::new() };
        let error = enabled.then(|| status.error.clone()).flatten();
        let recovered = outputs.is_empty() && error.is_none() && (!self.outputs.is_empty() || self.error.is_some());
        // Attempt counts, retry deadlines, and retry state change on every
        // attempt. They must not bring back an alert the user dismissed.
        let new_failure = outputs.iter().any(|output| !self.outputs.contains(output)) || (error.is_some() && error != self.error);
        self.outputs = outputs;
        self.error = error;
        if new_failure {
            Some(Change::Show)
        } else if recovered {
            Some(Change::Clear)
        } else {
            None
        }
    }
}

fn clear(library: LibraryId, window: &mut Window, cx: &mut App) {
    window.remove_notification1::<AnnotationExportToast>(SharedString::from(library.to_string()), cx);
}

fn message(status: &AutomaticAnnotationExportStatus, name: &str) -> String {
    let blocked = |format| status.outputs.iter().any(|output| output.destination.format == format && output.failure.kind == AnnotationExportFailureKind::Busy && !output.failure.file_written);
    let readers = match (blocked(AnnotationExportFormat::Foliate), blocked(AnnotationExportFormat::Readest)) {
        (true, true) => text!(browse::foliate_and_readest),
        (true, false) => text!(browse::foliate),
        (false, true) => text!(browse::readest),
        _ => return format!(text!(browse::automatic_annotation_export_needs_attention_in), name = name),
    };
    format!(text!(browse::close_to_finish_exporting_annotations_in_exports_will_resume_automatically), name = name, readers = readers)
}

fn show(backend: Rc<AppClient>, library: LibraryId, name: String, status: &AutomaticAnnotationExportStatus, window: &mut Window, cx: &mut App) {
    let message = SharedString::from(message(status, &name));
    let toast = Notification::new().with_type(gpui_component::notification::NotificationType::Warning).id1::<AnnotationExportToast>(SharedString::from(library.to_string())).autohide(false).close_button_always_visible(true).content(
        move |_, _, cx| {
            let notification = cx.entity();
            let backend = backend.clone();
            let name = name.clone();
            div()
                .w_full()
                .flex()
                .items_center()
                .gap(px(ui_components::SPACE_SM))
                .child(div().flex_1().min_w_0().child(message.clone()))
                .child(Button::new("view-annotation-exports").label(text!(browse::view_exports)).ghost().small().on_click(move |_, window, cx| {
                    notification.update(cx, |notification, cx| notification.dismiss(window, cx));
                    crate::universal::open_annotation_export(backend.clone(), library, name.clone(), window, cx);
                }))
                .into_any_element()
        },
    );
    window.push_notification(toast, cx);
}

impl Libraries {
    pub(super) fn watch_annotation_exports(&mut self, cx: &mut Context<Self>) {
        let removed = self.annotation_export_notifications.keys().filter(|id| !self.entries.iter().any(|entry| entry.library_id() == *id)).copied().collect::<Vec<_>>();
        for id in removed {
            self.annotation_export_notifications.remove(&id);
            cx.spawn(async move |libraries, cx| {
                let _ = libraries.update_in(cx, |_, window, cx| clear(id, window, cx));
            })
            .detach();
        }
        for entry in &self.entries {
            let id = *entry.library_id();
            self.annotation_export_notifications.entry(id).or_insert_with(|| {
                let backend = self.backend.clone();
                let source = backend.management.library(id);
                cx.spawn(async move |libraries, cx| {
                    let Ok((updates, _)) = source.updates().await else { return };
                    let mut notices = Notices::default();
                    // Subscribe before taking the initial snapshot to include
                    // persisted failures and changes during startup.
                    loop {
                        match source.automatic_annotation_export().await {
                            Ok(status) => {
                                if let Some(change) = notices.update(&status) {
                                    if libraries
                                        .update_in(cx, |libraries, window, cx| {
                                            let Some(entry) = libraries.entries.iter().find(|entry| *entry.library_id() == id) else { return };
                                            match change {
                                                Change::Show => show(backend.clone(), id, entry.library_name().to_owned(), &status, window, cx),
                                                Change::Clear => clear(id, window, cx),
                                            }
                                        })
                                        .is_err()
                                    {
                                        return;
                                    }
                                }
                            }
                            Err(error) => log::warn!("could not read automatic annotation export status: {error}"),
                        }
                        loop {
                            let Ok(update) = updates.recv().await else { return };
                            if matches!(update, LibraryUpdate::AnnotationExportStatusChanged) {
                                break;
                            }
                        }
                        while updates.try_recv().is_ok() {}
                    }
                })
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::AppContext;
    use library_model::{AnnotationExportOptions, AutomaticAnnotationExportOutput, AutomaticAnnotationExportState};
    use std::time::Duration;
    use sync_common::BookId;

    fn status() -> AutomaticAnnotationExportStatus {
        AutomaticAnnotationExportStatus {
            options: Some(AnnotationExportOptions { destination: "/exports".into(), formats: vec![AnnotationExportFormat::Markdown], install_readers: false, conflict: Default::default(), overrides: vec![] }),
            error: None,
            outputs: vec![AutomaticAnnotationExportOutput {
                destination: AnnotationExportDestination { book_id: BookId::from_bytes([1; 32]), directory_id: LibraryId::nil(), format: AnnotationExportFormat::Markdown, path: "/exports/Book.md".into() },
                book_title: "Book".into(),
                state: AutomaticAnnotationExportState::Retrying,
                message: "Permission denied".into(),
                failure: AnnotationExportFailure { kind: AnnotationExportFailureKind::Io, file_written: false },
                attempts: 1,
                last_attempt_at: 1,
                next_retry_at: Some(30_001),
                last_success_at: None,
            }],
            last_success_at: None,
        }
    }

    #[test]
    fn busy_reader_alerts_name_only_readers_that_need_closing() {
        let mut status = status();
        status.outputs[0].destination.format = AnnotationExportFormat::Foliate;
        status.outputs[0].failure.kind = AnnotationExportFailureKind::Busy;
        assert!(message(&status, "Library").starts_with("Close Foliate to finish"));
        let mut second = status.outputs[0].clone();
        second.destination.format = AnnotationExportFormat::Readest;
        status.outputs.push(second);
        assert!(message(&status, "Library").starts_with("Close Foliate and Readest to finish"));
        status.outputs[0].failure.kind = AnnotationExportFailureKind::Io;
        assert!(message(&status, "Library").starts_with("Close Readest to finish"));
        status.outputs[1].failure.file_written = true;
        assert!(message(&status, "Library").contains("needs attention"));
    }

    #[test]
    fn retries_do_not_repeat_dismissed_alerts_but_recurrence_does() {
        let mut notices = Notices::default();
        let mut status = status();
        assert_eq!(notices.update(&status), Some(Change::Show));
        status.outputs[0].attempts += 1;
        status.outputs[0].last_attempt_at += 30_000;
        status.outputs[0].next_retry_at = Some(90_001);
        status.outputs[0].state = AutomaticAnnotationExportState::NeedsAttention;
        status.outputs[0].message = "The next attempt also failed".into();
        assert_eq!(notices.update(&status), None);
        let output = status.outputs.remove(0);
        assert_eq!(notices.update(&status), Some(Change::Clear));
        status.outputs.push(output);
        assert_eq!(notices.update(&status), Some(Change::Show));
        status.options = None;
        assert_eq!(notices.update(&status), Some(Change::Clear));
        assert_eq!(notices.update(&status), None);
    }

    #[test]
    fn new_files_and_new_failure_causes_can_alert_while_other_files_remain_blocked() {
        let mut notices = Notices::default();
        let mut status = status();
        assert_eq!(notices.update(&status), Some(Change::Show));
        let mut second = status.outputs[0].clone();
        second.destination.path = "/exports/Second.md".into();
        status.outputs.push(second);
        assert_eq!(notices.update(&status), Some(Change::Show));
        status.outputs.remove(1);
        assert_eq!(notices.update(&status), None);
        status.outputs[0].failure = AnnotationExportFailure { kind: AnnotationExportFailureKind::Tracking, file_written: true };
        assert_eq!(notices.update(&status), Some(Change::Show));
        status.error = Some("Could not save completion receipts".into());
        assert_eq!(notices.update(&status), Some(Change::Show));
        assert_eq!(notices.update(&status), None);
        status.outputs.clear();
        assert_eq!(notices.update(&status), None, "a remaining global error keeps the toast open");
        status.error = None;
        assert_eq!(notices.update(&status), Some(Change::Clear));
    }

    #[cfg(not(feature = "kobo"))]
    #[gpui::test]
    async fn toasts_are_replaced_per_library_and_cleared_independently(cx: &mut gpui::TestAppContext) {
        let _backend = crate::test_support::BACKEND.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let (_directory, _session, library) = crate::navigation::LibrarySession::test_context(cx).await;
        let backend = Rc::new(library.app().clone());
        let first = *library.backend().id();
        let second = LibraryId::new_v4();
        let window = cx.add_window(|window, cx| {
            let empty = cx.new(|_| gpui::Empty);
            gpui_component::Root::new(empty, window, cx)
        });
        let notifications = window.read_with(cx, |root, _| root.notification.clone()).unwrap();
        cx.update_window(window.into(), |_, window, cx| {
            show(backend.clone(), first, "First library".into(), &status(), window, cx);
            show(backend.clone(), first, "First library".into(), &status(), window, cx);
            show(backend.clone(), second, "Second library".into(), &status(), window, cx);
        })
        .unwrap();
        notifications.read_with(cx, |list, _| assert_eq!(list.notifications().len(), 2));
        cx.update_window(window.into(), |_, window, cx| clear(first, window, cx)).unwrap();
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(200));
        cx.run_until_parked();
        notifications.read_with(cx, |list, _| assert_eq!(list.notifications().len(), 1));
        cx.update_window(window.into(), |_, window, cx| clear(second, window, cx)).unwrap();
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(200));
        cx.run_until_parked();
        notifications.read_with(cx, |list, _| assert!(list.notifications().is_empty()));
    }
}
