//! Device-local catalog feeds, synchronized OPDS libraries, and device credentials.
//! Also opens the embedded browser.

use library_backend::catalog_sources::CatalogClient;
use ui_text::text;

use ui_components::user_facing_error;

use gpui::prelude::*;
use gpui::{AnyElement, App, Context, Entity, EventEmitter, Focusable, SharedString, Subscription, Window, div, px, rems};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{Disableable, IconName};
use library_model::{AddOpdsFeedResult, BrowseLocation, OpdsCredentials, OpdsSourceId};
use ui_components as components;

use crate::library::services::LibraryContext;
use crate::library::widgets::SelectionAction;
use crate::services::{notify_error, notify_info};

/// A feed was added or removed, or credentials saved for a feed:
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

#[derive(Clone, Copy)]
enum FeedTarget {
    Catalog,
    Library,
}

pub(crate) struct FeedActions {
    app: app::AppClient,
    catalog: CatalogClient,
    target: FeedTarget,
    library_id: uuid::Uuid,
    dialog: Option<FeedDialog>,
    /// Whether this system can open the embedded browser.
    browser_available: bool,
}

impl EventEmitter<FeedsChanged> for FeedActions {}

impl FeedActions {
    pub(crate) fn new(ui: LibraryContext, cx: &mut Context<Self>) -> Self {
        let actions = Self { target: FeedTarget::Catalog, ..Self::for_app(ui.app().clone(), *ui.backend().id(), cx) };
        if !ui.is_opds() {
            cx.spawn(async move |actions, cx| {
                let available = crate::services::browser::available().await;
                let _ = actions.update(cx, |actions, cx| {
                    actions.browser_available = available;
                    cx.notify();
                });
            })
            .detach();
        }
        actions
    }

    pub(crate) fn for_app(app: app::AppClient, library_id: uuid::Uuid, cx: &mut Context<Self>) -> Self {
        Self { app, catalog: crate::services::catalog_client(cx), target: FeedTarget::Library, library_id, dialog: None, browser_available: false }
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
        let name = cx.new(|cx| InputState::new(window, cx).placeholder(text!(browse::name_optional)));
        let username = cx.new(|cx| InputState::new(window, cx).placeholder(text!(browse::username)));
        let password = cx.new(|cx| InputState::new(window, cx).placeholder(text!(common::password)).masked(true));
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
        let libraries = self.app.management.clone();
        let catalog = self.catalog.clone();
        let name = Some(dialog.name.read(cx).value().trim().to_owned()).filter(|name| !name.is_empty());
        let request: futures_util::future::LocalBoxFuture<'static, Result<AddOpdsFeedResult, app::BackendError>> = match (&dialog.sign_in, credentials) {
            (Some((source, _)), Some(credentials)) => {
                let source = *source;
                Box::pin(async move { catalog.set_credentials(source, credentials).await.map(|_| AddOpdsFeedResult::Added) })
            }
            (Some(_), None) => return,
            (None, credentials) => {
                let url = submitted_url.clone();
                let target = self.target;
                Box::pin(async move {
                    match target {
                        FeedTarget::Catalog => catalog.add(url, name, credentials).await,
                        FeedTarget::Library => libraries.add_opds_library(url, name, credentials).await,
                    }
                })
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
                            dialog.error = Some(user_facing_error(&error, text!(browse::could_not_add_or_sign_in_to_the_catalog_check_its_address)).into());
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Only a feed's root chip can remove its source.
    pub(super) fn removal_action(actions: Entity<Self>, location: BrowseLocation, name: String) -> Option<SelectionAction> {
        let BrowseLocation::Opds { source, page: None } = location else {
            return None;
        };
        Some(SelectionAction::new("remove-source", text!(browse::remove_feed), components::navigation_icon(components::NavigationIcon::Trash2), move |window, cx| {
            actions.update(cx, |actions, cx| actions.remove(source, name.clone(), window, cx))
        }))
    }

    fn remove(&mut self, source: OpdsSourceId, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let catalog = self.catalog.clone();
        cx.spawn_in(window, async move |actions, cx| {
            let result = catalog.remove(source).await;
            let _ = actions.update_in(cx, |_, window, cx| match result {
                Ok(()) => {
                    notify_info("feed-removed", format!(text!(browse::removed_name), name = name), window, cx);
                    cx.emit(FeedsChanged);
                }
                Err(error) => notify_error("feed-remove-error", format!(text!(browse::could_not_remove), user_facing_error(&error, text!(common::please_try_again)), name = name), window, cx),
            });
        })
        .detach();
    }

    fn open_browser(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let library = self.app.management.library(self.library_id);
        let preferences = crate::stores::preferences::current_browsing_preferences(cx);
        cx.spawn_in(window, async move |actions, cx| {
            if let Err(error) = crate::services::browser::open(library, preferences).await {
                let _ = actions.update_in(cx, |_, window, cx| notify_error("browser-open-error", format!(text!(browse::could_not_open_the_browser), user_facing_error(&error, text!(common::please_try_again))), window, cx));
            }
        })
        .detach();
    }

    pub(crate) fn render_trigger(actions: Entity<Self>) -> AnyElement {
        components::outlined_icon_button("catalog-add-feed", text!(browse::add_opds_feed), IconName::Plus).on_click(move |_, window, cx| actions.update(cx, |actions, cx| actions.open_add(window, cx))).into_any_element()
    }

    /// Opens the browser directly where a browser window is available.
    pub(crate) fn render_browser_trigger(actions: Entity<Self>, cx: &App) -> Option<AnyElement> {
        actions.read(cx).browser_available.then(|| {
            components::outlined_icon_button("catalog-open-browser", text!(browse::open_browser), IconName::Globe).on_click(move |_, window, cx| actions.update(cx, |actions, cx| actions.open_browser(window, cx))).into_any_element()
        })
    }

    /// What a feed page that refused this device shows in place of its
    /// listing: a way to sign in.
    pub(crate) fn render_sign_in(actions: Entity<Self>, source: OpdsSourceId, name: String, cx: &App) -> AnyElement {
        let theme = components::browser_theme(cx);
        let button = components::base_button("feed-sign-in").primary().label(text!(common::sign_in)).on_click(move |_, window, cx| actions.update(cx, |actions, cx| actions.open_sign_in(source, name.clone(), window, cx)));
        components::absolute_full_size()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(components::SPACE_MD))
            .child(div().text_color(theme.text).child(text!(browse::this_feed_asks_for_a_username_and_password)))
            .child(button)
            .into_any_element()
    }

    pub(crate) fn render_overlay(actions: Entity<Self>, window: &Window, cx: &App) -> Option<AnyElement> {
        let (title, adding, authenticating, inputs, error, busy, ready) = {
            let dialog = actions.read(cx).dialog.as_ref()?;
            let title = match &dialog.sign_in {
                Some((_, name)) => format!(text!(browse::sign_in_to), name = name),
                None => match actions.read(cx).target {
                    FeedTarget::Catalog => text!(browse::add_opds_feed).to_owned(),
                    FeedTarget::Library => text!(browse::add_opds_library).to_owned(),
                },
            };
            let inputs = [dialog.url.clone(), dialog.name.clone(), dialog.username.clone(), dialog.password.clone()];
            (title, dialog.sign_in.is_none(), dialog.requires_credentials(cx), inputs, dialog.error.clone(), dialog.busy, dialog.ready(cx))
        };
        let [url, name, username, password] = inputs;
        let theme = components::browser_theme(cx);
        let (cancel_actions, dismiss_actions, submit_actions) = (actions.clone(), actions.clone(), actions);
        let cancel = components::dialog_cancel_button("cancel-feed-dialog", text!(common::cancel)).disabled(busy).on_click(move |_, _, cx| cancel_actions.update(cx, |actions, cx| actions.dismiss_modal(cx)));
        let (idle, working) = if adding { (text!(browse::add), text!(browse::adding)) } else { (text!(common::sign_in), text!(account::signing_in)) };
        let submit = components::base_button("confirm-feed-dialog")
            .primary()
            .label(if busy { working } else { idle })
            .tooltip(components::shortcut_tooltip(idle, "enter"))
            .disabled(busy || !ready)
            .on_click(move |_, window, cx| submit_actions.update(cx, |actions, cx| actions.submit(window, cx)));
        let input_height = rems(components::topbar_action_height_rem());
        let panel = components::modal_form_surface(theme)
            .w(px(420.0))
            .child(div().text_size(rems(1.125)).text_color(theme.text).child(title))
            .when(adding, |panel| panel.child(Input::new(&url).h(input_height)).child(Input::new(&name).h(input_height)))
            .when(authenticating, |panel| {
                panel
                    .when(adding, |panel| panel.child(div().text_color(theme.text_muted).text_size(rems(0.875)).child(text!(browse::this_feed_requires_a_username_and_password))))
                    .child(Input::new(&username).h(input_height))
                    .child(Input::new(&password).mask_toggle().h(input_height))
                    .child(div().text_color(theme.text_muted).text_size(rems(0.875)).child(text!(browse::kept_on_this_device_only)))
            })
            .children(error.map(|error| components::error_text(theme).child(error)))
            .child(div().flex().items_center().justify_end().gap(px(components::SPACE_SM)).child(cancel).child(submit));
        let scrim = components::modal_scrim("feed-dialog-scrim", theme).on_click(move |_, _, cx| dismiss_actions.update(cx, |actions, cx| actions.dismiss_modal(cx)));
        Some(gpui::deferred(components::centered_modal(scrim, panel, window)).with_priority(2).into_any_element())
    }
}

#[cfg(all(test, not(target_arch = "wasm32"), not(feature = "kobo")))]
mod tests {
    use super::*;
    use crate::navigation::LibrarySession;
    use gpui::{Render, TestAppContext};
    use gpui_component::Root;
    use uuid::Uuid;

    struct FeedFixture(Entity<FeedActions>);

    impl Render for FeedFixture {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full().children(FeedActions::render_overlay(self.0.clone(), window, cx))
        }
    }

    #[gpui::test]
    async fn removal_actions_refresh_the_catalog_and_replace_feedback(cx: &mut TestAppContext) {
        let _backend = crate::test_support::BACKEND.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let (_directory, _session, library) = LibrarySession::test_context(cx).await;
        let actions = cx.new(|cx| FeedActions::new(library, cx));
        let (sent, received) = async_channel::unbounded();
        let _subscription = cx.update(|cx| cx.subscribe(&actions, move |_, _: &FeedsChanged, _| sent.try_send(()).unwrap()));
        let (root, visual) = cx.add_window_view(|window, cx| {
            let content = cx.new(|_| FeedFixture(actions.clone()));
            Root::new(content, window, cx)
        });
        let id = Uuid::new_v4();
        // Deletion is idempotent for already-removed feeds. Repeat the
        // command to check toast replacement.
        let feed = BrowseLocation::Opds { source: id, page: None };
        for (location, notifications) in [(feed, 1), (feed, 1)] {
            let action = FeedActions::removal_action(actions.clone(), location, "Source".into()).unwrap();
            visual.update(|window, cx| action.run(window, cx));
            received.recv().await.unwrap();
            root.read_with(visual, |root, cx| assert_eq!(root.notification.read(cx).notifications().len(), notifications));
            assert!(received.is_empty(), "one completed removal must request one catalog refresh");
        }
    }
}
