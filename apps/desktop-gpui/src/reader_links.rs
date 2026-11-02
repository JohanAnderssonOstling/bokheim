//! Deliver external reader links to a live application window.

use application_shell::ApplicationRoot;
use gpui::{App, Context, Render, WeakEntity, Window};
use gpui_component::{Root, WindowExt, notification::Notification};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReaderActivation {
    pub urls: Vec<String>,
    #[serde(default)]
    pub activation_token: Option<String>,
}

impl From<Vec<String>> for ReaderActivation {
    fn from(urls: Vec<String>) -> Self {
        Self { urls, activation_token: None }
    }
}

#[cfg(feature = "desktop")]
impl ReaderActivation {
    pub fn from_launch_args(arguments: impl IntoIterator<Item = String>, activation_token: Option<String>) -> Result<Self, String> {
        let mut arguments = arguments.into_iter();
        let mut urls = Vec::new();
        let mut forwarded_token = None;
        while let Some(argument) = arguments.next() {
            if argument == "--bokheim-activation-token" {
                forwarded_token = Some(arguments.next().ok_or("missing forwarded activation token")?);
            } else if argument.starts_with("bokheim:") {
                urls.push(argument);
            }
        }
        Ok(Self { urls, activation_token: activation_token.or(forwarded_token) })
    }

    /// Carry the initial request through updater/helper relaunches in memory.
    pub fn launch_args(&self) -> Vec<String> {
        let mut arguments = self.urls.clone();
        if let Some(token) = &self.activation_token {
            arguments.extend(["--bokheim-activation-token".to_owned(), token.clone()]);
        }
        arguments
    }
}

#[cfg(all(feature = "desktop", not(feature = "mobile")))]
pub(crate) struct ReaderWindowFactory(pub std::rc::Rc<dyn Fn(&mut App) -> gpui::Task<Result<(), String>>>);

#[cfg(all(feature = "desktop", not(feature = "mobile")))]
impl gpui::Global for ReaderWindowFactory {}

#[cfg(all(feature = "desktop", not(feature = "mobile")))]
pub(crate) fn has_reader_window(cx: &mut App) -> bool {
    reader_window::<ApplicationRoot>(cx, |_, _| true).is_some()
}

pub(crate) fn forward_reader_links(links: async_channel::Receiver<ReaderActivation>, fallback: WeakEntity<ApplicationRoot>, cx: &mut App) {
    forward_activations(links, fallback, deliver_reader_links, cx);
}

fn deliver_reader_links(activation: ReaderActivation, fallback: &WeakEntity<ApplicationRoot>, cx: &mut App) -> Result<(), String> {
    if activation.urls.is_empty() {
        return with_reader_window(fallback, activation.activation_token.as_deref(), cx, |_, _| true, |_, _, _| {});
    }
    let mut token = activation.activation_token;
    for url in activation.urls {
        let link = library_model::ReaderLink::parse(&url);
        let locator = link.as_ref().ok().map(|link| link.locator.clone());
        with_reader_window(
            fallback,
            token.take().as_deref(),
            cx,
            |root, cx| locator.as_ref().is_some_and(|locator| root.has_open_reader(locator, cx)),
            |root, window, cx| match link {
                Ok(link) => root.open_reader_link(link, window, cx),
                Err(message) => window.push_notification(Notification::error(message), cx),
            },
        )?;
    }
    Ok(())
}

fn forward_activations<T: Render>(links: async_channel::Receiver<ReaderActivation>, fallback: WeakEntity<T>, deliver: impl Fn(ReaderActivation, &WeakEntity<T>, &mut App) -> Result<(), String> + 'static, cx: &mut App) {
    cx.spawn(async move |cx| {
        while let Ok(activation) = links.recv().await {
            #[cfg(all(feature = "desktop", not(feature = "mobile")))]
            {
                let prepare = cx.update(|cx| if reader_window::<T>(cx, |_, _| true).is_some() { None } else { cx.try_global::<ReaderWindowFactory>().map(|factory| factory.0.clone()).map(|factory| factory(cx)) });
                if let Some(prepare) = prepare {
                    if let Err(error) = prepare.await {
                        log::error!("could not reopen the reader window: {error}");
                        continue;
                    }
                }
            }
            let result = cx.update(|cx| deliver(activation, &fallback, cx));
            if let Err(error) = result {
                log::warn!("could not deliver reader link activation: {error}");
            }
        }
    })
    .detach();
}

fn reader_window<T: Render>(cx: &mut App, prefers: impl Fn(&T, &App) -> bool) -> Option<(gpui::AnyWindowHandle, gpui::Entity<T>)> {
    let mut windows = cx.window_stack().unwrap_or_default();
    // Minimized windows may be absent from the platform's visible stack.
    for window in cx.windows() {
        if !windows.contains(&window) {
            windows.push(window);
        }
    }
    if let Some(active) = cx.active_window() {
        if let Some(index) = windows.iter().position(|window| *window == active) {
            windows.remove(index);
            windows.insert(0, active);
        }
    }
    let mut fallback = None;
    for handle in windows {
        let target = handle
            .update(cx, |view, _, cx| {
                let root = view.downcast::<Root>().ok()?;
                root.read(cx).view().clone().downcast::<T>().ok()
            })
            .ok()
            .flatten();
        if let Some(target) = target {
            if prefers(target.read(cx), cx) {
                return Some((handle, target));
            }
            if fallback.is_none() {
                fallback = Some((handle, target));
            }
        }
    }
    fallback
}

fn with_reader_window<T: Render>(fallback: &WeakEntity<T>, activation_token: Option<&str>, cx: &mut App, prefers: impl Fn(&T, &App) -> bool, update: impl FnOnce(&mut T, &mut Window, &mut Context<T>)) -> Result<(), String> {
    if let Some((handle, target)) = reader_window::<T>(cx, prefers) {
        return handle
            .update(cx, |_, window, cx| {
                #[cfg(target_os = "macos")]
                cx.activate(true);
                activate_reader_window(window, activation_token);
                target.update(cx, |target, cx| update(target, window, cx));
            })
            .map_err(|error| error.to_string());
    }
    // Android and Kobo put an additional host around the application view.
    fallback
        .update_in(cx, |target, window, cx| {
            #[cfg(target_os = "macos")]
            cx.activate(true);
            activate_reader_window(window, activation_token);
            update(target, window, cx);
        })
        .map_err(|error| error.to_string())
}

fn activate_reader_window(window: &Window, activation_token: Option<&str>) {
    if let Some(token) = activation_token {
        window.activate_window_with_token(token);
    } else {
        window.activate_window();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "desktop")]
    #[test]
    fn launch_arguments_preserve_links_and_tokens_across_relaunches() {
        for activation in [ReaderActivation::default(), ReaderActivation { urls: vec!["bokheim://open?book=one&location=here".into(), "bokheim://open?book=two".into()], activation_token: Some("token with spaces and \"quotes\"".into()) }] {
            let arguments = ["--bokheim-supervised-launch".to_owned(), "helper directory".to_owned()].into_iter().chain(activation.launch_args());
            assert_eq!(ReaderActivation::from_launch_args(arguments, None).unwrap(), activation);
        }
    }

    #[cfg(feature = "desktop")]
    #[test]
    fn launch_arguments_require_a_forwarded_token_value() {
        assert!(ReaderActivation::from_launch_args(["--bokheim-activation-token".to_owned()], None).is_err());
    }
    use gpui::{AppContext, Entity, IntoElement, TestAppContext, WindowHandle, div};

    #[derive(Default)]
    struct Recipient {
        deliveries: usize,
        received: Vec<String>,
        book: Option<u8>,
    }

    impl Render for Recipient {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    fn add_recipient(cx: &mut TestAppContext) -> (WindowHandle<Root>, Entity<Recipient>) {
        let window = cx.add_window(|window, cx| {
            let recipient = cx.new(|_| Recipient::default());
            Root::new(recipient, window, cx)
        });
        let recipient = window.update(cx, |root, _, _| root.view().clone().downcast::<Recipient>().ok().unwrap()).unwrap();
        (window, recipient)
    }

    #[gpui::test]
    fn links_use_a_surviving_window_after_the_original_closes(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let (original, recipient) = add_recipient(cx);
        let fallback = recipient.downgrade();
        drop(recipient);
        let (_, survivor) = add_recipient(cx);
        original.update(cx, |_, window, _| window.remove_window()).unwrap();
        cx.run_until_parked();
        assert!(fallback.upgrade().is_none());
        cx.update(|cx| with_reader_window(&fallback, None, cx, |_, _| true, |recipient, _, _| recipient.deliveries += 1)).unwrap();
        survivor.read_with(cx, |recipient, _| assert_eq!(recipient.deliveries, 1));
    }

    #[gpui::test]
    fn links_prefer_the_active_application_window(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let (_, original) = add_recipient(cx);
        let (active, recipient) = add_recipient(cx);
        active.update(cx, |_, window, _| window.activate_window()).unwrap();
        cx.update(|cx| with_reader_window(&original.downgrade(), None, cx, |_, _| true, |recipient, _, _| recipient.deliveries += 1)).unwrap();
        original.read_with(cx, |recipient, _| assert_eq!(recipient.deliveries, 0));
        recipient.read_with(cx, |recipient, _| assert_eq!(recipient.deliveries, 1));
    }

    #[gpui::test]
    fn delivery_activates_the_destination_window_at_the_platform_level(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let (destination, recipient) = add_recipient(cx);
        let other = cx.add_window(|_, _| gpui::Empty);
        other.update(cx, |_, window, _| window.activate_window()).unwrap();
        assert_eq!(cx.update(|cx| cx.active_window()), Some(other.into()));
        cx.update(|cx| with_reader_window(&recipient.downgrade(), None, cx, |_, _| true, |recipient, _, _| recipient.deliveries += 1)).unwrap();
        assert_eq!(cx.update(|cx| cx.active_window()), Some(destination.into()));
        recipient.read_with(cx, |recipient, _| assert_eq!(recipient.deliveries, 1));
    }
    #[cfg(all(feature = "desktop", not(feature = "mobile")))]
    #[gpui::test]
    fn queued_links_and_reopen_create_only_one_window_after_the_last_closes(cx: &mut TestAppContext) {
        use std::{cell::Cell, rc::Rc};
        cx.update(|cx| {
            gpui_component::init(cx);
            cx.set_quit_mode(gpui::QuitMode::Explicit);
        });
        let (original, recipient) = add_recipient(cx);
        let fallback = recipient.downgrade();
        drop(recipient);
        original.update(cx, |_, window, _| window.remove_window()).unwrap();
        cx.run_until_parked();
        assert!(fallback.upgrade().is_none());
        let creations = Rc::new(Cell::new(0));
        let factory_creations = creations.clone();
        cx.update(|cx| {
            cx.set_global(ReaderWindowFactory(Rc::new(move |cx| {
                let creations = factory_creations.clone();
                cx.spawn(async move |cx| {
                    cx.background_executor().timer(std::time::Duration::from_millis(10)).await;
                    cx.update(|cx| {
                        creations.set(creations.get() + 1);
                        cx.open_window(Default::default(), |window, cx| {
                            let recipient = cx.new(|_| Recipient::default());
                            cx.new(|cx| Root::new(recipient, window, cx))
                        })
                        .map(|_| ())
                        .map_err(|error| error.to_string())
                    })
                })
            })))
        });
        let (sender, receiver) = async_channel::unbounded();
        cx.update(|cx| {
            forward_activations(
                receiver,
                fallback,
                |activation, fallback, cx| {
                    with_reader_window(
                        fallback,
                        activation.activation_token.as_deref(),
                        cx,
                        |_, _| true,
                        |recipient, _, _| {
                            recipient.deliveries += 1;
                            recipient.received.extend(activation.urls);
                        },
                    )
                },
                cx,
            )
        });
        sender.try_send(ReaderActivation::default()).unwrap();
        sender.try_send(vec!["first".to_owned()].into()).unwrap();
        sender.try_send(vec!["second".to_owned()].into()).unwrap();
        cx.run_until_parked();
        cx.executor().advance_clock(std::time::Duration::from_millis(20));
        cx.run_until_parked();
        assert_eq!(creations.get(), 1);
        cx.update(|cx| {
            let (_, recipient) = reader_window::<Recipient>(cx, |_, _| true).expect("reader window was not recreated");
            let recipient = recipient.read(cx);
            assert_eq!(recipient.deliveries, 3);
            assert_eq!(recipient.received, ["first", "second"]);
        });
    }

    #[gpui::test]
    fn token_activation_uses_the_destination_window_on_platforms_without_tokens(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let (destination, recipient) = add_recipient(cx);
        let other = cx.add_window(|_, _| gpui::Empty);
        other.update(cx, |_, window, _| window.activate_window()).unwrap();
        cx.update(|cx| with_reader_window(&recipient.downgrade(), Some("launcher-token"), cx, |_, _| true, |recipient, _, _| recipient.deliveries += 1)).unwrap();
        assert_eq!(cx.update(|cx| cx.active_window()), Some(destination.into()));
    }
    #[gpui::test]
    fn reader_links_prefer_the_window_with_the_book_over_the_active_window(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let (destination, matching) = add_recipient(cx);
        matching.update(cx, |recipient, _| recipient.book = Some(7));
        let (active_window, active) = add_recipient(cx);
        active.update(cx, |recipient, _| recipient.book = Some(9));
        active_window.update(cx, |_, window, _| window.activate_window()).unwrap();
        cx.update(|cx| with_reader_window(&active.downgrade(), Some("launcher-token"), cx, |recipient, _| recipient.book == Some(7), |recipient, _, _| recipient.deliveries += 1)).unwrap();
        assert_eq!(cx.update(|cx| cx.active_window()), Some(destination.into()));
        matching.read_with(cx, |recipient, _| assert_eq!(recipient.deliveries, 1));
        active.read_with(cx, |recipient, _| assert_eq!(recipient.deliveries, 0));
    }

    #[gpui::test]
    fn reader_links_use_the_active_window_when_no_window_has_the_book(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let (_, original) = add_recipient(cx);
        let (active_window, active) = add_recipient(cx);
        active_window.update(cx, |_, window, _| window.activate_window()).unwrap();
        cx.update(|cx| with_reader_window(&original.downgrade(), None, cx, |recipient, _| recipient.book == Some(7), |recipient, _, _| recipient.deliveries += 1)).unwrap();
        original.read_with(cx, |recipient, _| assert_eq!(recipient.deliveries, 0));
        active.read_with(cx, |recipient, _| assert_eq!(recipient.deliveries, 1));
    }

    #[gpui::test]
    fn reader_links_prefer_the_active_window_when_both_have_the_book(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let (_, original) = add_recipient(cx);
        let (active_window, active) = add_recipient(cx);
        original.update(cx, |recipient, _| recipient.book = Some(7));
        active.update(cx, |recipient, _| recipient.book = Some(7));
        active_window.update(cx, |_, window, _| window.activate_window()).unwrap();
        cx.update(|cx| with_reader_window(&original.downgrade(), None, cx, |recipient, _| recipient.book == Some(7), |recipient, _, _| recipient.deliveries += 1)).unwrap();
        original.read_with(cx, |recipient, _| assert_eq!(recipient.deliveries, 0));
        active.read_with(cx, |recipient, _| assert_eq!(recipient.deliveries, 1));
    }
}
