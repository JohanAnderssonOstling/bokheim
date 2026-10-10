//! The catalog's OPDS feed commands: adding a feed by its address, signing
//! in to one, and removing one. Feeds are library entries, so adding and
//! removing reach every device; what is typed to sign in stays on this one.

use gpui::prelude::*;
use gpui::{AnyElement, App, Context, Entity, EventEmitter, Focusable, MouseButton, SharedString, Subscription, Window, div, px, rems};
use gpui_component::Disableable;
use gpui_component::button::ButtonVariants;
use gpui_component::input::{Input, InputEvent, InputState};
use library_model::{OpdsCredentials, OpdsSourceId};
use ui_components as components;

use crate::library::services::LibraryContext;
use crate::services::{notify_error, notify_info};

/// A feed was added or removed, or credentials saved for one: what the page
/// lists from the feeds is stale.
pub(crate) struct FeedsChanged;

struct FeedDialog {
    /// The feed being signed in to, or `None` when adding one.
    sign_in: Option<(OpdsSourceId, String)>,
    url: Entity<InputState>,
    name: Entity<InputState>,
    username: Entity<InputState>,
    password: Entity<InputState>,
    error: Option<SharedString>,
    /// The feed is being fetched or the credentials saved, which the dialog
    /// has to stay up for.
    busy: bool,
    _subscriptions: Vec<Subscription>,
}

impl FeedDialog {
    /// Credentials as typed; none when the username is left empty.
    fn credentials(&self, cx: &App) -> Option<OpdsCredentials> {
        let username = self.username.read(cx).value().trim().to_owned();
        (!username.is_empty()).then(|| OpdsCredentials { username, password: self.password.read(cx).value().to_string() })
    }

    /// Whether there is enough typed to submit.
    fn ready(&self, cx: &App) -> bool {
        match self.sign_in {
            Some(_) => self.credentials(cx).is_some(),
            None => !self.url.read(cx).value().trim().is_empty(),
        }
    }
}

pub(crate) struct FeedActions {
    ui: LibraryContext,
    dialog: Option<FeedDialog>,
}

impl EventEmitter<FeedsChanged> for FeedActions {}

impl FeedActions {
    pub(crate) fn new(ui: LibraryContext) -> Self {
        Self { ui, dialog: None }
    }

    pub(crate) fn has_modal(&self) -> bool {
        self.dialog.is_some()
    }

    pub(crate) fn dismiss_modal(&mut self, cx: &mut Context<Self>) {
        if self.dialog.as_ref().is_some_and(|dialog| dialog.busy) {
            return;
        }
        self.dialog = None;
        cx.notify();
    }

    pub(crate) fn open_add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open(None, window, cx);
    }

    pub(crate) fn open_sign_in(&mut self, source: OpdsSourceId, name: String, window: &mut Window, cx: &mut Context<Self>) {
        self.open(Some((source, name)), window, cx);
    }

    fn open(&mut self, sign_in: Option<(OpdsSourceId, String)>, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialog.is_some() {
            return;
        }
        let url = cx.new(|cx| InputState::new(window, cx).placeholder("https://example.org/opds"));
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("Name (optional)"));
        let username_placeholder = if sign_in.is_some() { "Username" } else { "Username (if the feed asks)" };
        let username = cx.new(|cx| InputState::new(window, cx).placeholder(username_placeholder));
        let password = cx.new(|cx| InputState::new(window, cx).placeholder("Password").masked(true));
        let submit_on_enter = |actions: &mut Self, _: &Entity<InputState>, event: &InputEvent, _: &mut Window, cx: &mut Context<Self>| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                actions.submit(cx);
            }
        };
        let subscriptions = [&url, &name, &username, &password].into_iter().map(|input| cx.subscribe_in(input, window, submit_on_enter)).collect();
        let first = if sign_in.is_some() { &username } else { &url };
        first.read(cx).focus_handle(cx).focus(window, cx);
        self.dialog = Some(FeedDialog { sign_in, url, name, username, password, error: None, busy: false, _subscriptions: subscriptions });
        cx.notify();
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = self.dialog.as_ref() else { return };
        if dialog.busy || !dialog.ready(cx) {
            return;
        }
        let credentials = dialog.credentials(cx);
        let app = self.ui.app().clone();
        let request = match (&dialog.sign_in, credentials) {
            (Some((source, _)), Some(credentials)) => {
                let source = *source;
                futures_util::future::Either::Left(async move { app.set_opds_credentials(source, credentials).await })
            }
            (Some(_), None) => return,
            (None, credentials) => {
                let url = dialog.url.read(cx).value().trim().to_owned();
                let name = Some(dialog.name.read(cx).value().trim().to_owned()).filter(|name| !name.is_empty());
                futures_util::future::Either::Right(async move { app.add_opds_feed(url, name, credentials).await })
            }
        };
        if let Some(dialog) = self.dialog.as_mut() {
            dialog.busy = true;
            dialog.error = None;
        }
        cx.notify();
        cx.spawn(async move |actions, cx| {
            let result = request.await;
            let _ = actions.update(cx, |actions, cx| {
                match result {
                    Ok(()) => {
                        actions.dialog = None;
                        cx.emit(FeedsChanged);
                    }
                    Err(error) => {
                        if let Some(dialog) = actions.dialog.as_mut() {
                            dialog.busy = false;
                            dialog.error = Some(error.into());
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn remove(&mut self, id: OpdsSourceId, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let app = self.ui.app().clone();
        cx.spawn_in(window, async move |actions, cx| {
            let result = app.remove_opds_feed(id).await;
            let _ = actions.update_in(cx, |_, window, cx| match result {
                Ok(()) => {
                    notify_info("feed-removed", format!("Removed “{name}”"), window, cx);
                    cx.emit(FeedsChanged);
                }
                Err(error) => notify_error("feed-remove-error", format!("Could not remove “{name}”: {error}"), window, cx),
            });
        })
        .detach();
    }

    pub(crate) fn render_trigger(actions: Entity<Self>, cx: &App) -> AnyElement {
        components::outlined_icon_button("catalog-add-feed", "Add OPDS feed", gpui_component::IconName::Plus, components::browser_theme(cx))
            .on_click(move |_, window, cx| actions.update(cx, |actions, cx| actions.open_add(window, cx)))
            .into_any_element()
    }

    /// What a feed page that refused this device shows in place of its
    /// listing: a way to sign in.
    pub(crate) fn render_sign_in(actions: Entity<Self>, source: OpdsSourceId, name: String, cx: &App) -> AnyElement {
        let theme = components::browser_theme(cx);
        let button = components::base_button("feed-sign-in").primary().label("Sign in").on_click(move |_, window, cx| actions.update(cx, |actions, cx| actions.open_sign_in(source, name.clone(), window, cx)));
        components::absolute_full_size()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(components::SPACE_MD))
            .child(div().text_color(theme.text).child("This feed asks for a username and password."))
            .child(button)
            .into_any_element()
    }

    pub(crate) fn render_overlay(actions: Entity<Self>, cx: &mut App) -> Option<AnyElement> {
        let (title, adding, inputs, error, busy, ready) = {
            let dialog = actions.read(cx).dialog.as_ref()?;
            let title = match &dialog.sign_in {
                Some((_, name)) => format!("Sign in to {name}"),
                None => "Add OPDS feed".to_owned(),
            };
            let inputs = [dialog.url.clone(), dialog.name.clone(), dialog.username.clone(), dialog.password.clone()];
            (title, dialog.sign_in.is_none(), inputs, dialog.error.clone(), dialog.busy, dialog.ready(cx))
        };
        let [url, name, username, password] = inputs;
        let theme = components::browser_theme(cx);
        let (cancel_actions, dismiss_actions, submit_actions) = (actions.clone(), actions.clone(), actions);
        let cancel = components::dialog_cancel_button("cancel-feed-dialog", "Cancel").disabled(busy).on_click(move |_, _, cx| cancel_actions.update(cx, |actions, cx| actions.dismiss_modal(cx)));
        let (idle, working) = if adding { ("Add", "Adding…") } else { ("Sign in", "Signing in…") };
        let submit = components::base_button("confirm-feed-dialog")
            .primary()
            .label(if busy { working } else { idle })
            .disabled(busy || !ready)
            .on_click(move |_, _, cx| submit_actions.update(cx, |actions, cx| actions.submit(cx)));
        let input_height = rems(components::TOPBAR_ACTION_HEIGHT_REM);
        let panel = components::modal_surface(theme)
            .w(px(420.0))
            .max_w_full()
            .p(px(components::SPACE_MD))
            .flex()
            .flex_col()
            .gap(px(components::SPACE_MD))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(div().text_size(rems(1.125)).text_color(theme.text).child(title))
            .when(adding, |panel| panel.child(Input::new(&url).h(input_height)).child(Input::new(&name).h(input_height)))
            .child(Input::new(&username).h(input_height))
            .child(Input::new(&password).mask_toggle().h(input_height))
            .when(!adding, |panel| panel.child(div().text_color(theme.text_muted).text_size(rems(0.875)).child("Kept on this device only.")))
            .children(error.map(|error| components::error_text(theme).child(error)))
            .child(div().flex().items_center().justify_end().gap(px(components::SPACE_SM)).child(cancel).child(submit));
        Some(
            gpui::deferred(
                components::modal_overlay()
                    .occlude()
                    .child(components::modal_scrim("feed-dialog-scrim", theme).on_click(move |_, _, cx| dismiss_actions.update(cx, |actions, cx| actions.dismiss_modal(cx))))
                    .child(panel),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }
}
