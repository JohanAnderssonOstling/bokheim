//! The catalog's OPDS feed commands: adding a feed by its address, signing
//! in to one, and removing one. Feeds belong to the account, so adding and
//! removing reach every device; what is typed to sign in stays on this one.
//! Also opens the browser and the catalog's saved stores. A store is kept
//! on this device only, where its sign-in is.

use gpui::prelude::*;
use gpui::{AnyElement, App, Context, Entity, EventEmitter, Focusable, MouseButton, SharedString, Subscription, Window, div, px, rems};
use gpui_component::button::ButtonVariants;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{Disableable, IconName};
use library_model::{AddOpdsFeedResult, OpdsCredentials, OpdsSourceId, StoreId};
use ui_components as components;

use crate::library::services::LibraryContext;
use crate::services::{notify_error, notify_info};

/// A feed or store was added or removed, or credentials saved for a feed:
/// what the page lists from them is stale.
pub(crate) struct FeedsChanged;

struct FeedDialog {
    /// The feed being signed in to, or `None` when adding one.
    sign_in: Option<(OpdsSourceId, String)>,
    /// The address that requested credentials while adding a feed.
    authentication_url: Option<String>,
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
    fn requires_credentials(&self, cx: &App) -> bool {
        self.sign_in.is_some() || self.authentication_url.as_deref().is_some_and(|url| url == self.url.read(cx).value().trim())
    }

    /// Credentials as typed; none when the username is left empty.
    fn credentials(&self, cx: &App) -> Option<OpdsCredentials> {
        if !self.requires_credentials(cx) {
            return None;
        }
        let username = self.username.read(cx).value().trim().to_owned();
        (!username.is_empty()).then(|| OpdsCredentials { username, password: self.password.read(cx).value().to_string() })
    }

    /// Whether there is enough typed to submit.
    fn ready(&self, cx: &App) -> bool {
        (self.sign_in.is_some() || !self.url.read(cx).value().trim().is_empty()) && (!self.requires_credentials(cx) || self.credentials(cx).is_some())
    }
}

pub(crate) struct FeedActions {
    ui: LibraryContext,
    dialog: Option<FeedDialog>,
    /// Whether this system can open a store window; stores are not offered
    /// until it is known that it can.
    stores_available: bool,
}

impl EventEmitter<FeedsChanged> for FeedActions {}

impl FeedActions {
    pub(crate) fn new(ui: LibraryContext, cx: &mut Context<Self>) -> Self {
        let app = ui.app().clone();
        cx.spawn(async move |actions, cx| {
            let available = app.stores_available().await.unwrap_or(false);
            let _ = actions.update(cx, |actions, cx| {
                actions.stores_available = available;
                cx.notify();
            });
        })
        .detach();
        Self { ui, dialog: None, stores_available: false }
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
        let username = cx.new(|cx| InputState::new(window, cx).placeholder("Username"));
        let password = cx.new(|cx| InputState::new(window, cx).placeholder("Password").masked(true));
        let submit_on_enter = |actions: &mut Self, _: &Entity<InputState>, event: &InputEvent, window: &mut Window, cx: &mut Context<Self>| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                actions.submit(window, cx);
            } else if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        };
        let subscriptions = [&url, &name, &username, &password].into_iter().map(|input| cx.subscribe_in(input, window, submit_on_enter)).collect();
        let first = if sign_in.is_some() { &username } else { &url };
        first.read(cx).focus_handle(cx).focus(window, cx);
        self.dialog = Some(FeedDialog { sign_in, authentication_url: None, url, name, username, password, error: None, busy: false, _subscriptions: subscriptions });
        cx.notify();
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self.dialog.as_ref() else { return };
        if dialog.busy || !dialog.ready(cx) {
            return;
        }
        let credentials = dialog.credentials(cx);
        let submitted_url = dialog.url.read(cx).value().trim().to_owned();
        let app = self.ui.app().clone();
        let name = Some(dialog.name.read(cx).value().trim().to_owned()).filter(|name| !name.is_empty());
        let request: futures_util::future::LocalBoxFuture<'static, Result<AddOpdsFeedResult, String>> = match (&dialog.sign_in, credentials) {
            (Some((source, _)), Some(credentials)) => {
                let source = *source;
                Box::pin(async move { app.set_opds_credentials(source, credentials).await.map(|_| AddOpdsFeedResult::Added) })
            }
            (Some(_), None) => return,
            (None, credentials) => {
                let url = submitted_url.clone();
                Box::pin(async move { app.add_opds_feed(url, name, credentials).await })
            }
        };
        if let Some(dialog) = self.dialog.as_mut() {
            dialog.busy = true;
            dialog.error = None;
        }
        cx.notify();
        cx.spawn_in(window, async move |actions, cx| {
            let result = request.await;
            let _ = actions.update_in(cx, |actions, window, cx| {
                match result {
                    Ok(AddOpdsFeedResult::Added) => {
                        actions.dialog = None;
                        cx.emit(FeedsChanged);
                    }
                    Ok(AddOpdsFeedResult::SignInRequired) => {
                        if let Some(dialog) = actions.dialog.as_mut() {
                            dialog.busy = false;
                            dialog.authentication_url = Some(submitted_url);
                            if dialog.requires_credentials(cx) {
                                dialog.username.read(cx).focus_handle(cx).focus(window, cx);
                            }
                        }
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

    /// Opens the store's window, where what is bought goes into this
    /// library's root.
    pub(crate) fn open_store(&mut self, id: StoreId, window: &mut Window, cx: &mut Context<Self>) {
        let (app, library_id) = (self.ui.app().clone(), *self.ui.backend().id());
        cx.spawn_in(window, async move |actions, cx| {
            if let Err(error) = app.open_store(id, library_id).await {
                let _ = actions.update_in(cx, |_, window, cx| notify_error("store-open-error", format!("Could not open the store: {error}"), window, cx));
            }
        })
        .detach();
    }

    fn open_browser(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (app, library_id) = (self.ui.app().clone(), *self.ui.backend().id());
        cx.spawn_in(window, async move |actions, cx| {
            if let Err(error) = app.open_browser(library_id).await {
                let _ = actions.update_in(cx, |_, window, cx| notify_error("browser-open-error", format!("Could not open the browser: {error}"), window, cx));
            }
        })
        .detach();
    }

    pub(crate) fn remove_store(&mut self, id: StoreId, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let app = self.ui.app().clone();
        cx.spawn_in(window, async move |actions, cx| {
            let result = app.remove_store(id).await;
            let _ = actions.update_in(cx, |_, window, cx| match result {
                Ok(()) => {
                    notify_info("store-removed", format!("Removed “{name}”"), window, cx);
                    cx.emit(FeedsChanged);
                }
                Err(error) => notify_error("store-remove-error", format!("Could not remove “{name}”: {error}"), window, cx),
            });
        })
        .detach();
    }

    pub(crate) fn render_trigger(actions: Entity<Self>, cx: &App) -> AnyElement {
        components::outlined_icon_button("catalog-add-feed", "Add OPDS feed", IconName::Plus, components::browser_theme(cx))
            .on_click(move |_, window, cx| actions.update(cx, |actions, cx| actions.open_add(window, cx)))
            .into_any_element()
    }

    /// Opens the browser directly where a browser window is available.
    pub(crate) fn render_store_trigger(actions: Entity<Self>, cx: &App) -> Option<AnyElement> {
        actions.read(cx).stores_available.then(|| {
            components::outlined_icon_button("catalog-open-browser", "Open browser", IconName::Globe, components::browser_theme(cx))
                .on_click(move |_, window, cx| actions.update(cx, |actions, cx| actions.open_browser(window, cx)))
                .into_any_element()
        })
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

    pub(crate) fn render_overlay(actions: Entity<Self>, window: &Window, cx: &mut App) -> Option<AnyElement> {
        let (title, adding, authenticating, inputs, error, busy, ready) = {
            let dialog = actions.read(cx).dialog.as_ref()?;
            let title = match &dialog.sign_in {
                Some((_, name)) => format!("Sign in to {name}"),
                None => "Add OPDS feed".to_owned(),
            };
            let inputs = [dialog.url.clone(), dialog.name.clone(), dialog.username.clone(), dialog.password.clone()];
            (title, dialog.sign_in.is_none(), dialog.requires_credentials(cx), inputs, dialog.error.clone(), dialog.busy, dialog.ready(cx))
        };
        let [url, name, username, password] = inputs;
        let theme = components::browser_theme(cx);
        let (cancel_actions, dismiss_actions, submit_actions) = (actions.clone(), actions.clone(), actions);
        let cancel = components::dialog_cancel_button("cancel-feed-dialog", "Cancel").disabled(busy).on_click(move |_, _, cx| cancel_actions.update(cx, |actions, cx| actions.dismiss_modal(cx)));
        let (idle, working) = if adding { ("Add", "Adding…") } else { ("Sign in", "Signing in…") };
        let submit =
            components::base_button("confirm-feed-dialog").primary().label(if busy { working } else { idle }).disabled(busy || !ready).on_click(move |_, window, cx| submit_actions.update(cx, |actions, cx| actions.submit(window, cx)));
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
            .when(authenticating, |panel| {
                panel
                    .when(adding, |panel| panel.child(div().text_color(theme.text_muted).text_size(rems(0.875)).child("This feed requires a username and password.")))
                    .child(Input::new(&username).h(input_height))
                    .child(Input::new(&password).mask_toggle().h(input_height))
                    .child(div().text_color(theme.text_muted).text_size(rems(0.875)).child("Kept on this device only."))
            })
            .children(error.map(|error| components::error_text(theme).child(error)))
            .child(div().flex().items_center().justify_end().gap(px(components::SPACE_SM)).child(cancel).child(submit));
        Some(
            gpui::deferred(components::anchor_to_window(
                components::modal_overlay().occlude().child(components::modal_scrim("feed-dialog-scrim", theme).on_click(move |_, _, cx| dismiss_actions.update(cx, |actions, cx| actions.dismiss_modal(cx)))).child(panel),
                window,
            ))
                .with_priority(2)
                .into_any_element(),
        )
    }
}
