//! UI binding for the shared backend update state. The host supplies actions
//! only after its preparation and startup/installer lifecycle are available.
use ui_text::text;

use gpui::prelude::FluentBuilder;
use gpui::{App, AppContext, Context, Div, ElementId, Entity, ParentElement, Styled, div, px};
use gpui_component::IconName;
#[cfg(not(target_arch = "wasm32"))]
use std::panic::{AssertUnwindSafe, catch_unwind};
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
use std::rc::Rc;
use ui_components as components;
pub use update_client::coordinator::{UpdateAction, UpdateView};

pub type UpdateActionHandler = Rc<dyn Fn(UpdateAction, &mut App)>;

pub struct UpdateControls {
    pub(crate) view: UpdateView,
    pub(crate) action: UpdateActionHandler,
    worker_failed: bool,
    /// One availability prompt per shared launch, including across windows.
    prompt_shown: bool,
    message: Option<String>,
    error: Option<String>,
    update_label: &'static str,
    install_label: &'static str,
}

impl UpdateControls {
    pub fn new(view: UpdateView, action: UpdateActionHandler) -> Self {
        Self { view, action, worker_failed: false, prompt_shown: false, message: None, error: None, update_label: text!(browse::update), install_label: text!(browse::install) }
    }

    pub(crate) fn take_available_prompt(&mut self) -> Option<UpdateActionHandler> {
        if !matches!(self.view, UpdateView::Available) || self.prompt_shown {
            return None;
        }
        self.prompt_shown = true;
        Some(self.action.clone())
    }

    /// Backend notifications must enter through the platform's normal UI event
    /// bridge. In particular a browser worker must not re-enter a GPUI render.
    pub fn apply(&mut self, view: UpdateView, cx: &mut Context<Self>) {
        let changed = self.view != view;
        if changed {
            self.message = None;
        }
        if changed || self.worker_failed || self.error.is_some() {
            self.error = None;
            self.worker_failed = false;
            self.view = view;
            cx.notify();
        }
    }

    pub fn apply_event(&mut self, event: UpdateEvent, cx: &mut Context<Self>) {
        match event {
            UpdateEvent::View(view) => self.apply(view, cx),
            UpdateEvent::Status(view, message) => {
                self.apply(view, cx);
                self.message = Some(message);
                cx.notify();
            }
            UpdateEvent::Failed(error) => {
                self.error = error;
                self.worker_failed = true;
                cx.notify();
            }
        }
    }

    #[cfg(feature = "kobo")]
    pub(crate) fn action_labels(&mut self, update: &'static str, install: &'static str) {
        self.update_label = update;
        self.install_label = install;
    }
}

/// One transport between a native update worker and GPUI. Hosts retain their
/// startup ownership until the deferred task takes them after healthy().
pub enum UpdateEvent {
    View(UpdateView),
    Status(UpdateView, String),
    Failed(Option<String>),
}

pub type PublishUpdate<'a> = dyn Fn(UpdateEvent) -> Result<(), String> + 'a;

#[cfg(not(target_arch = "wasm32"))]
pub fn bind_worker<H: Send + 'static>(
    initial: UpdateView, restart: Option<PathBuf>, automatic_check: bool, take_host: impl FnOnce() -> Option<H> + 'static,
    mut work: impl FnMut(&mut H, Option<UpdateAction>, &PublishUpdate<'_>) -> Result<UpdateView, String> + Send + 'static, view: impl Fn(&H) -> UpdateView + Send + 'static, user_error: impl Fn(&str) -> Option<String> + Send + 'static,
    cx: &mut App,
) -> Entity<UpdateControls> {
    let (actions, requests) = async_channel::unbounded();
    let (events, received) = async_channel::unbounded();
    let controls = cx.new(|_| {
        UpdateControls::new(
            initial,
            Rc::new(move |action, cx| {
                if action == UpdateAction::Restart
                    && let Some(path) = &restart
                {
                    cx.set_restart_path(path.clone());
                    cx.restart();
                } else {
                    let _ = actions.try_send(action);
                }
            }),
        )
    });
    let executor = cx.background_executor().clone();
    cx.spawn(async move |_| {
        let Some(mut host) = take_host() else { return };
        executor
            .spawn(async move {
                let publish = |event| events.try_send(event).map_err(|_| "update UI closed".to_owned());
                let mut automatic = automatic_check;
                loop {
                    let action = if std::mem::take(&mut automatic) {
                        None
                    } else {
                        let Ok(action) = requests.recv().await else { break };
                        Some(action)
                    };
                    // Waiting for actions holds no OS thread. Blocking update work
                    // runs serially on the shared background pool, away from the UI.
                    let result = match catch_unwind(AssertUnwindSafe(|| work(&mut host, action, &publish))) {
                        Ok(result) => result,
                        Err(_) => {
                            log::error!("update worker panicked");
                            let _ = publish(UpdateEvent::Failed(Some(text!(browse::update_failed_restart_the_application_to_retry).into())));
                            break;
                        }
                    };
                    match result {
                        Ok(view) => {
                            if publish(UpdateEvent::View(view)).is_err() {
                                break;
                            }
                        }
                        Err(error) => {
                            log::warn!("update work failed: {error}");
                            if publish(UpdateEvent::View(view(&host))).is_err() || publish(UpdateEvent::Failed(user_error(&error))).is_err() {
                                break;
                            }
                        }
                    }
                }
            })
            .detach();
    })
    .detach();
    let weak = controls.downgrade();
    cx.spawn(async move |cx| {
        while let Ok(event) = received.recv().await {
            if weak.update(cx, |controls, cx| controls.apply_event(event, cx)).is_err() {
                break;
            }
        }
    })
    .detach();
    controls
}

/// Shared action/status presentation for native settings and Kobo settings.
pub(crate) fn render_row(controls: &Entity<UpdateControls>, id: impl Into<ElementId>, theme: components::BrowserTheme, cx: &App) -> Option<Div> {
    let controls = controls.read(cx);
    let (message, button) = match &controls.view {
        UpdateView::Hidden => return None,
        UpdateView::Available => (text!(browse::update_available).to_owned(), Some((controls.update_label, UpdateAction::Update, false))),
        UpdateView::Preparing => (text!(browse::preparing_update).into(), Some((text!(browse::update), UpdateAction::Update, true))),
        UpdateView::WaitingForConnection => (text!(browse::check_your_internet_connection_then_retry_the_update).into(), Some((text!(common::retry), UpdateAction::Update, false))),
        UpdateView::Ready { action } => (text!(browse::update_ready).into(), Some((if *action == UpdateAction::Install { controls.install_label } else { text!(common::restart) }, action.clone(), false))),
        UpdateView::NeedsSpace { additional_bytes } => (format!(text!(browse::free_up_to_continue), needed_space(*additional_bytes)), Some((text!(common::retry), UpdateAction::Update, false))),
        UpdateView::NeedsPermission => (text!(browse::allow_the_app_to_install_the_update).into(), Some((text!(browse::continue), UpdateAction::GrantPermission, false))),
        UpdateView::Activating => (text!(browse::updating_your_library).into(), None),
        UpdateView::Current { version, checked } => {
            (if *checked { format!(text!(browse::bokheim_is_up_to_date), version = version) } else { format!(text!(browse::bokheim), version = version) }, Some((text!(browse::check_for_updates), UpdateAction::Check, false)))
        }
        UpdateView::Checking => (text!(browse::checking_for_updates).into(), Some((text!(common::checking), UpdateAction::Check, true))),
        UpdateView::Downloading { bytes, total } => (
            if *total == 0 {
                text!(browse::downloading_update).into()
            } else if bytes >= total {
                text!(browse::verifying_and_preparing_update).into()
            } else {
                format!(text!(browse::update_download_percent), bytes.saturating_mul(100) / total)
            },
            Some((text!(browse::downloading), UpdateAction::Update, true)),
        ),
    };
    let message = controls.message.clone().unwrap_or(message);
    let message = if controls.worker_failed && controls.error.is_none() { text!(browse::could_not_continue_update_try_again).to_owned() } else { message };
    let mut row = div().w_full().flex().flex_col().child({
        let mut content = div().w_full().px(px(components::SPACE_MD)).py(px(components::SPACE_SM)).min_h(px(56.)).flex().flex_wrap().items_center().justify_between().gap(px(components::SPACE_SM)).child(message);
        if let Some((label, action, busy)) = button {
            let handler = controls.action.clone();
            let loading = busy && !controls.worker_failed;
            let label = if controls.worker_failed { text!(common::retry) } else { label };
            content = content.child(components::browser_settings_header_button(id, label, true, theme).when(loading, |button| button.icon(IconName::LoaderCircle)).loading(loading).on_click(move |_, _, cx| handler(action.clone(), cx)));
        }
        content
    });
    if let Some(error) = &controls.error {
        row = row.child(components::browser_settings_group_error(error.clone(), theme));
    }
    Some(row)
}

pub(crate) fn needed_space(bytes: u64) -> String {
    // Round up by at most one decimal MB; never exaggerate a 1.1 GB deficit to 2 GB.
    if bytes >= 1_000_000 { format!(text!(browse::mb_2), bytes.div_ceil(1_000_000)) } else { format!(text!(browse::kb_2), bytes.div_ceil(1_000)) }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    #[gpui::test]
    fn availability_prompt_is_shared_and_survives_rechecks(cx: &mut TestAppContext) {
        let controls = cx.new(|_| UpdateControls::new(UpdateView::Hidden, Rc::new(|_, _| {})));
        let other_window = controls.clone();
        controls.update(cx, |controls, cx| {
            assert!(controls.take_available_prompt().is_none());
            controls.apply(UpdateView::Available, cx);
            assert!(controls.take_available_prompt().is_some());
        });
        other_window.update(cx, |controls, cx| {
            assert!(controls.take_available_prompt().is_none());
            controls.apply(UpdateView::Checking, cx);
            controls.apply(UpdateView::Available, cx);
            controls.apply_event(UpdateEvent::Failed(None), cx);
            controls.apply(UpdateView::Available, cx);
            assert!(controls.take_available_prompt().is_none());
        });
        let next_launch = cx.new(|_| UpdateControls::new(UpdateView::Available, Rc::new(|_, _| {})));
        assert!(next_launch.update(cx, |controls, _| controls.take_available_prompt().is_some()));
    }

    struct WorkerHost {
        view: UpdateView,
        calls: async_channel::Sender<Option<UpdateAction>>,
        released: async_channel::Sender<()>,
    }

    impl Drop for WorkerHost {
        fn drop(&mut self) {
            let _ = self.released.try_send(());
        }
    }

    #[gpui::test]
    async fn worker_preserves_initial_check_action_order_failure_recovery_and_shutdown(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        for automatic in [false, true] {
            let (calls, called) = async_channel::unbounded();
            let (released, finished) = async_channel::bounded(1);
            let host = WorkerHost { view: UpdateView::Hidden, calls, released };
            let controls = cx.update(|cx| {
                bind_worker(
                    UpdateView::Hidden,
                    None,
                    automatic,
                    move || Some(host),
                    |host, action, _| {
                        host.calls.try_send(action.clone()).unwrap();
                        if action == Some(UpdateAction::Update) {
                            return Err("fixture failure".into());
                        }
                        host.view = UpdateView::Available;
                        Ok(host.view.clone())
                    },
                    |host| host.view.clone(),
                    |error| Some(format!("Update failed: {error}")),
                    cx,
                )
            });
            let action = controls.read_with(cx, |controls, _| controls.action.clone());
            if automatic {
                assert_eq!(called.recv().await.unwrap(), None);
            }
            cx.update(|cx| {
                action(UpdateAction::Check, cx);
                action(UpdateAction::Update, cx);
            });
            assert_eq!(called.recv().await.unwrap(), Some(UpdateAction::Check));
            assert_eq!(called.recv().await.unwrap(), Some(UpdateAction::Update));
            cx.condition(&controls, |controls, _| controls.worker_failed).await;
            assert_eq!(controls.read_with(cx, |controls, _| controls.error.clone()), Some("Update failed: fixture failure".into()));
            cx.update(|cx| action(UpdateAction::Check, cx));
            assert_eq!(called.recv().await.unwrap(), Some(UpdateAction::Check));
            cx.condition(&controls, |controls, _| !controls.worker_failed && controls.error.is_none()).await;
            drop(action);
            drop(controls);
            cx.update(|_| {});
            cx.run_until_parked();
            finished.recv().await.unwrap();
        }
    }
}
