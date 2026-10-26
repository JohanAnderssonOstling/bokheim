//! UI binding for the shared backend update state. The host supplies actions
//! only after its preparation and startup/installer lifecycle are available.
use gpui::prelude::FluentBuilder;
use gpui::{App, AppContext, Context, Div, ElementId, Entity, ParentElement, Styled, div, px};
use gpui_component::{Disableable, IconName};
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
use std::rc::Rc;
use ui_components as components;
pub use update_client::coordinator::{UpdateAction, UpdateView};

pub type UpdateActionHandler = Rc<dyn Fn(UpdateAction, &mut App)>;

pub struct UpdateControls {
    pub(crate) view: UpdateView,
    pub(crate) action: UpdateActionHandler,
    pub(crate) worker_failed: bool,
    prompt: super::update_prompt::UpdatePromptState,
    message: Option<String>,
    error: Option<String>,
    update_label: &'static str,
    install_label: &'static str,
}

impl UpdateControls {
    pub fn new(view: UpdateView, action: UpdateActionHandler) -> Self {
        Self { view, action, worker_failed: false, prompt: Default::default(), message: None, error: None, update_label: "Update", install_label: "Install" }
    }

    pub(crate) fn take_available_prompt(&mut self) -> Option<UpdateActionHandler> {
        self.prompt.take(matches!(self.view, UpdateView::Available)).then(|| self.action.clone())
    }

    pub fn work_failed(&mut self, cx: &mut Context<Self>) {
        self.worker_failed = true;
        cx.notify();
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
                self.work_failed(cx);
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
    let (actions, requests) = std::sync::mpsc::channel();
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
                    let _ = actions.send(action);
                }
            }),
        )
    });
    cx.spawn(async move |_| {
        let Some(mut host) = take_host() else { return };
        let failure = events.clone();
        if let Err(error) = std::thread::Builder::new().name("application-updates".into()).spawn(move || {
            let publish = |event| events.try_send(event).map_err(|_| "update UI closed".to_owned());
            let mut action = if automatic_check {
                None
            } else {
                match requests.recv() {
                    Ok(action) => Some(action),
                    Err(_) => return,
                }
            };
            loop {
                match work(&mut host, action, &publish) {
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
                action = match requests.recv() {
                    Ok(action) => Some(action),
                    Err(_) => break,
                };
            }
        }) {
            log::warn!("could not start update worker: {error}");
            let _ = failure.try_send(UpdateEvent::Failed(None));
        }
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
        UpdateView::Available => ("Update available".to_owned(), Some((controls.update_label, UpdateAction::Update, false))),
        UpdateView::Preparing => ("Preparing update…".into(), Some(("Update", UpdateAction::Update, true))),
        UpdateView::WaitingForConnection => ("Check your internet connection, then retry the update.".into(), Some(("Retry", UpdateAction::Update, false))),
        UpdateView::Ready { action } => ("Update ready".into(), Some((if *action == UpdateAction::Install { controls.install_label } else { "Restart" }, action.clone(), false))),
        UpdateView::NeedsSpace { additional_bytes } => (format!("Free up {} to continue", needed_space(*additional_bytes)), Some(("Retry", UpdateAction::Update, false))),
        UpdateView::NeedsPermission => ("Allow Bokheim to install the update to continue.".into(), Some(("Continue", UpdateAction::GrantPermission, false))),
        UpdateView::Activating => ("Updating your library…".into(), None),
        UpdateView::Current { version, checked } => (if *checked { format!("Bokheim {version} is up to date") } else { format!("Bokheim {version}") }, Some(("Check for updates", UpdateAction::Check, false))),
        UpdateView::Checking => ("Checking for updates…".into(), Some(("Checking…", UpdateAction::Check, true))),
        UpdateView::Downloading { bytes, total } => (
            if *total == 0 {
                "Downloading update…".into()
            } else if bytes >= total {
                "Verifying and preparing update…".into()
            } else {
                format!("Downloading update — {}%", bytes.saturating_mul(100) / total)
            },
            Some(("Downloading…", UpdateAction::Update, true)),
        ),
    };
    let message = controls.message.clone().unwrap_or(message);
    let message = if controls.worker_failed && controls.error.is_none() { "Could not continue update. Try again.".to_owned() } else { message };
    let mut row = div().w_full().flex().flex_col().child({
        let mut content = div().w_full().px(px(components::SPACE_MD)).py(px(components::SPACE_SM)).min_h(px(56.)).flex().flex_wrap().items_center().justify_between().gap(px(components::SPACE_SM)).child(message);
        if let Some((label, action, busy)) = button {
            let handler = controls.action.clone();
            let loading = busy && !controls.worker_failed;
            let label = if controls.worker_failed { "Retry" } else { label };
            content = content.child(
                components::browser_settings_header_button(id, label, true, theme)
                    .when(loading, |button| button.icon(IconName::LoaderCircle))
                    .loading(loading)
                    .disabled(loading)
                    .on_click(move |_, _, cx| handler(action.clone(), cx)),
            );
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
    if bytes >= 1_000_000 { format!("{} MB", bytes.div_ceil(1_000_000)) } else { format!("{} KB", bytes.div_ceil(1_000)) }
}
