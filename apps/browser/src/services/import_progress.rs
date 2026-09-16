use std::{cell::RefCell, rc::Rc, time::Duration};

use bokheim_backend::{LibraryClient, ScanProgress};
use futures_util::{FutureExt, select_biased};
use gpui::{App, IntoElement, ParentElement, Styled, Window, div, px};
use gpui_component::{WindowExt, notification::Notification, progress::Progress};

/// The import owns this guard; completion, failure, and cancellation all close
/// its toast, even if the browse page that started the import has been closed.
pub(crate) struct ImportProgressToast(async_channel::Sender<()>);

impl Drop for ImportProgressToast {
    fn drop(&mut self) {
        let _ = self.0.try_send(());
    }
}

impl ImportProgressToast {
    pub(crate) fn start(library: LibraryClient, window: &mut Window, cx: &mut App) -> Self {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (library, window, cx);
            return Self(async_channel::bounded(1).0);
        }
        #[cfg(not(target_arch = "wasm32"))]
        Self::start_with_reader(
            move || {
                let library = library.clone();
                async move { library.directory_import_progress().await }
            },
            window,
            cx,
        )
    }

    fn start_with_reader<F: std::future::Future<Output = Result<Option<ScanProgress>, String>> + 'static>(mut read_progress: impl FnMut() -> F + 'static, window: &mut Window, cx: &mut App) -> Self {
        let id = gpui::SharedString::from(uuid::Uuid::new_v4().to_string());
        let progress = Rc::new(RefCell::new(presentation(None)));
        let content_progress = progress.clone();
        window.push_notification(
            Notification::new().id1::<Self>(id.clone()).autohide(false).title("Importing books").content(move |_, _, _| {
                let (label, percent) = content_progress.borrow().clone();
                div().w_full().flex().flex_col().gap(px(8.)).child(label).child(Progress::new("import-progress").loading(percent.is_none()).value(percent.unwrap_or_default())).into_any_element()
            }),
            cx,
        );
        let notification = window.notifications(cx).last().expect("import toast was just added").downgrade();
        let (done, finished) = async_channel::bounded::<()>(1);
        window
            .spawn(cx, async move |cx| {
                let completed = finished.recv().fuse();
                futures_util::pin_mut!(completed);
                loop {
                    // Completion wins even when a progress request is slow or queued
                    // behind the final database commit.
                    let update = read_progress().fuse();
                    futures_util::pin_mut!(update);
                    select_biased! {
                        _ = completed => break,
                        result = update => {
                            if let Ok(current) = result {
                                let current = presentation(current);
                                if *progress.borrow() != current {
                                    *progress.borrow_mut() = current;
                                    if notification.update(cx, |_, cx| cx.notify()).is_err() { break; }
                                }
                            }
                        }
                    }
                    let delay = cx.background_executor().timer(Duration::from_millis(250)).fuse();
                    futures_util::pin_mut!(delay);
                    select_biased! { _ = completed => break, _ = delay => {} }
                }
                let _ = cx.update(|window, cx| window.remove_notification1::<ImportProgressToast>(id, cx));
            })
            .detach();
        Self(done)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{AppContext, Context, Render};

    struct Host;
    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    #[gpui::test]
    fn completion_closes_only_its_own_toast_even_with_a_pending_progress_read(cx: &mut gpui::TestAppContext) {
        cx.update(gpui_component::init);
        let (_, cx) = cx.add_window_view(|window, cx| {
            let host = cx.new(|_| Host);
            gpui_component::Root::new(host, window, cx)
        });
        let first = cx.update(|window, cx| ImportProgressToast::start_with_reader(|| std::future::pending(), window, cx));
        let second = cx.update(|window, cx| ImportProgressToast::start_with_reader(|| std::future::ready(Ok(Some(ScanProgress { total: 10, succeeded: 3, failed: 1 }))), window, cx));
        cx.run_until_parked();
        cx.update(|window, cx| assert_eq!(window.notifications(cx).len(), 2));
        drop(first);
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(200));
        cx.run_until_parked();
        cx.update(|window, cx| assert_eq!(window.notifications(cx).len(), 1));
        drop(second);
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(200));
        cx.run_until_parked();
        cx.update(|window, cx| assert!(window.notifications(cx).is_empty()));
    }

    #[test]
    fn progress_counts_failed_books_as_processed_and_handles_unknown_totals() {
        assert_eq!(presentation(None).1, None);
        assert_eq!(presentation(Some(ScanProgress::default())).1, None);
        let (label, percent) = presentation(Some(ScanProgress { total: 10, succeeded: 3, failed: 1 }));
        assert_eq!(percent, Some(40.));
        assert!(label.contains("4 of 10"));
        assert!(label.contains("1 failed"));
    }
}

fn presentation(progress: Option<ScanProgress>) -> (String, Option<f32>) {
    match progress.filter(|progress| progress.total > 0) {
        Some(progress) => {
            let completed = progress.succeeded.saturating_add(progress.failed).min(progress.total);
            let failures = if progress.failed > 0 { format!(" · {} failed", progress.failed) } else { String::new() };
            (format!("{completed} of {} books{failures}", progress.total), Some(100. * completed as f32 / progress.total as f32))
        }
        None => ("Preparing import…".into(), None),
    }
}

/// One styled notification per coordinator job, including recovered imports.
#[cfg(target_arch = "wasm32")]
pub(crate) fn watch_browser_imports(backend: Rc<bokheim_backend::AppClient>, window: &mut Window, cx: &mut App) -> gpui::Task<()> {
    use super::import_presentation::browser_presentation;
    use bokheim_backend::ImportProgress;
    use std::collections::BTreeMap;
    struct BrowserImport;
    let mut updates = backend.import_updates();
    window.spawn(cx, async move |cx| {
        let mut shown: BTreeMap<String, (Rc<RefCell<ImportProgress>>, gpui::WeakEntity<Notification>)> = BTreeMap::new();
        loop {
            let snapshot = updates.borrow_and_update().clone();
            for (id, current) in snapshot {
                if let Some((value, notification)) = shown.get(&id) {
                    if std::mem::discriminant(&value.borrow().state) == std::mem::discriminant(&current.state) {
                        if *value.borrow() != current {
                            *value.borrow_mut() = current;
                            let _ = notification.update(cx, |_, cx| cx.notify());
                        }
                        continue;
                    }
                }
                let active = matches!(&current.state, bokheim_backend::ImportState::Queued | bokheim_backend::ImportState::Copying { .. } | bokheim_backend::ImportState::Adding { .. });
                let retry = matches!(&current.state, bokheim_backend::ImportState::Paused { .. });
                let title = current.name.clone();
                let value = Rc::new(RefCell::new(current));
                let content = value.clone();
                let backend = backend.clone();
                let retry_id = id.clone();
                let result = cx.update(|window, cx| {
                    let mut toast = Notification::new().id1::<BrowserImport>(gpui::SharedString::from(id.clone())).title(title).autohide(false).content(move |_, _, _| {
                        let state = content.borrow();
                        let (text, percent) = browser_presentation(&state.state);
                        let label = div().w_full().flex().flex_col().gap(px(8.)).child(text);
                        if active { label.child(Progress::new("browser-import-progress").loading(percent.is_none()).value(percent.unwrap_or_default())).into_any_element() } else { label.into_any_element() }
                    });
                    if retry {
                        toast = toast.action(move |_, _, _| {
                            let backend = backend.clone();
                            let id = retry_id.clone();
                            ui_components::base_button("retry-import").label("Retry").on_click(move |_, _, _| backend.retry_import(&id))
                        });
                    }
                    window.push_notification(toast, cx);
                    window.notifications(cx).last().unwrap().downgrade()
                });
                match result {
                    Ok(notification) => {
                        shown.insert(id, (value, notification));
                    }
                    Err(_) => return,
                }
            }
            if updates.changed().await.is_err() {
                break;
            }
        }
    })
}
