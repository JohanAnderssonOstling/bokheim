//! Per-window composition for the library and publication reader.

#[cfg(feature = "audiobooks")]
mod audiobook_footer;
#[cfg(feature = "kobo")]
mod kobo_bar;
#[cfg(feature = "audiobooks")]
mod playback;

use std::rc::Rc;

use bokheim_backend::AppClient;
use browser_ui::{AppServices, BrowserRoot, DismissSettings, OpenBookRequested};
use gpui::prelude::*;
use gpui::{AnyView, Context, Entity, IntoElement, Render, SharedString, Subscription, Window};
use library_model::BookLocator;
use sync_common::LibraryId;

struct ReaderSurface {
    locator: BookLocator,
    view: AnyView,
}

#[cfg(feature = "audiobooks")]
struct AudiobookSurface {
    locator: BookLocator,
    session: Entity<reader_ui::PlaybackSession>,
    dock: Entity<reader_ui::AudiobookDock>,
}

fn active_reader_library_is_missing(active_library: Option<&LibraryId>, contains_library: impl FnOnce(&LibraryId) -> bool) -> bool {
    active_library.is_some_and(|library_id| !contains_library(library_id))
}

/// Owns one application window and switches between browsing and reading.
pub struct ApplicationRoot {
    #[cfg(feature = "kobo")]
    device_bar: Entity<kobo_bar::KoboDeviceBar>,
    browser: Entity<BrowserRoot>,
    backend: AppClient,
    reader: Option<ReaderSurface>,
    #[cfg(feature = "audiobooks")]
    audiobook: Option<AudiobookSurface>,
    #[cfg(feature = "audiobooks")]
    audiobook_footer: Entity<audiobook_footer::AudiobookFooter>,
    #[cfg(feature = "audiobooks")]
    playback: Entity<playback::PlaybackController>,
    #[cfg(feature = "audiobooks")]
    pending_audiobook: Option<Entity<reader_ui::ReaderView>>,
    #[cfg(feature = "audiobooks")]
    open_task: Option<gpui::Task<()>>,
    application_title: SharedString,
    _browser_subscription: Subscription,
    notification_mode: Option<bool>,
    notification_task: Option<gpui::Task<()>>,
}

impl ApplicationRoot {
    pub fn open_window_target(&mut self, target: browser_ui::NewWindowTarget, window: &mut Window, cx: &mut Context<Self>) {
        match target {
            browser_ui::NewWindowTarget::Book { locator, title } => self.open_book(locator, title, None, window, cx),
            browser_ui::NewWindowTarget::Folder { library_id, directory_id, title } => {
                self.browser.update(cx, |browser, cx| browser.open_folder(library_id, directory_id, title, window, cx));
            }
        }
    }

    pub fn new(
        services: AppServices, backend: AppClient, application_title: impl Into<SharedString>, playback_restore_path: Option<std::path::PathBuf>, #[cfg(feature = "kobo")] kobo_auto_rotate: bool, window: &mut Window, cx: &mut Context<Self>,
    ) -> Self {
        cx.set_volume_button_capture(true);
        #[cfg(feature = "kobo")]
        let kobo_bar_services = services.clone();
        #[cfg(feature = "kobo")]
        let browser = cx.new(|cx| BrowserRoot::new(services, window, cx));
        #[cfg(not(feature = "kobo"))]
        let browser = cx.new(|cx| BrowserRoot::new(services, window, cx));
        let browser_subscription = cx.observe_in(&browser, window, |root, browser, window, cx| {
            let active_library = root.reader.as_ref().map(|reader| reader.locator.library_id());
            let active_removed = active_reader_library_is_missing(active_library, |library_id| browser.read(cx).contains_library(library_id, cx));
            if active_removed {
                log::warn!("an open book's library was deleted; returning to the library browser");
                root.return_to_library(window, cx);
            }
        });
        cx.subscribe_in(&browser, window, |root, _, request: &OpenBookRequested, window, cx| {
            root.open_book(request.locator.clone(), request.title.clone(), request.initial_target.clone(), window, cx);
        })
        .detach();
        cx.observe_window_activation(window, |root, window, cx| root.update_notification_interest(window, cx)).detach();
        #[cfg(feature = "audiobooks")]
        let playback = playback::PlaybackController::shared(backend.clone(), playback_restore_path, cx);
        #[cfg(feature = "audiobooks")]
        cx.observe_in(&playback, window, |root, _, window, cx| root.sync_playback(window, cx)).detach();
        #[cfg(feature = "audiobooks")]
        let audiobook_footer = cx.new(|_| audiobook_footer::AudiobookFooter::default());
        #[cfg(feature = "audiobooks")]
        cx.subscribe(&audiobook_footer, |root, _, _: &audiobook_footer::CancelPending, cx| {
            root.pending_audiobook = None;
            root.open_task = None;
            cx.notify();
        })
        .detach();
        let mut root = Self {
            #[cfg(feature = "kobo")]
            device_bar: cx.new(|cx| kobo_bar::KoboDeviceBar::new(backend.clone(), kobo_bar_services, kobo_auto_rotate, window, cx)),
            browser,
            backend,
            reader: None,
            application_title: application_title.into(),
            _browser_subscription: browser_subscription,
            notification_mode: None,
            notification_task: None,
            #[cfg(feature = "audiobooks")]
            audiobook: None,
            #[cfg(feature = "audiobooks")]
            audiobook_footer,
            #[cfg(feature = "audiobooks")]
            playback,
            #[cfg(feature = "audiobooks")]
            pending_audiobook: None,
            #[cfg(feature = "audiobooks")]
            open_task: None,
        };
        root.update_notification_interest(window, cx);
        #[cfg(feature = "audiobooks")]
        root.sync_playback(window, cx);
        root
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn import_and_open_path(&mut self, name: String, path: std::path::PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.browser.update(cx, |browser, cx| browser.import_and_open_path(name, path, window, cx));
    }

    pub fn incoming_import_failed(&mut self, error: String, window: &mut Window, cx: &mut Context<Self>) {
        self.browser.update(cx, |browser, cx| browser.incoming_import_failed(error, window, cx));
    }

    pub fn open_book(&mut self, locator: BookLocator, title: String, initial_target: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        if browser_ui::library_is_being_removed(*locator.library_id(), cx) {
            return;
        }
        #[cfg(feature = "audiobooks")]
        let audio_request = self.playback.update(cx, |controller, _| controller.reserve_open());
        #[cfg(feature = "audiobooks")]
        if self.playback.update(cx, |controller, cx| controller.activate_current(audio_request, &locator, initial_target.as_deref(), cx)) {
            self.open_task.take();
            self.pending_audiobook = None;
            self.sync_playback(window, cx);
            self.return_to_library(window, cx);
            return;
        }
        #[cfg(feature = "audiobooks")]
        {
            let library = self.backend.library(*locator.library_id());
            self.pending_audiobook = None;
            self.open_task = Some(cx.spawn(async move |root, cx| {
                let audio = matches!(library.book_format(locator.content_hash()).await, Ok(Some(library_model::BookFormat::M4b)));
                let _ = root.update_in(cx, |root, window, cx| {
                    if audio {
                        if !root.playback.update(cx, |controller, cx| controller.prepare_open(audio_request, &locator, cx)) {
                            return;
                        }
                        root.sync_playback(window, cx);
                    }
                    root.open_reader_surface(locator, title, initial_target, audio, audio_request, window, cx);
                });
            }));
        }
        #[cfg(not(feature = "audiobooks"))]
        self.open_reader_surface(locator, title, initial_target, false, 0, window, cx);
    }

    fn open_reader_surface(&mut self, locator: BookLocator, title: String, initial_target: Option<String>, audio: bool, audio_request: u64, window: &mut Window, cx: &mut Context<Self>) {
        #[cfg(feature = "audiobooks")]
        if !audio {
            self.playback.update(cx, |controller, cx| controller.close(cx));
            self.sync_playback(window, cx);
            self.browser.update(cx, |browser, cx| browser.clear_cached_book_details(cx));
        }
        let reader_locator = locator.clone();
        let library = Rc::new(self.backend.library(*locator.library_id()));
        #[cfg(feature = "audiobooks")]
        let audio_title = title.clone();
        let reader = cx.new(|cx| reader_ui::ReaderView::new(reader_locator, title, initial_target, library, window, cx));
        cx.subscribe_in(&reader, window, |root, owner, _: &reader_ui::CloseRequested, window, cx| {
            #[cfg(feature = "audiobooks")]
            if root.pending_audiobook.as_ref().is_some_and(|pending| pending == owner) {
                root.pending_audiobook = None;
            }
            root.return_to_library(window, cx);
        })
        .detach();
        cx.observe(&reader, |_, _, cx| cx.notify()).detach();
        #[cfg(feature = "audiobooks")]
        {
            let audio_locator = locator.clone();
            let generation = self.playback.read(cx).generation;
            cx.subscribe_in(&reader, window, move |root, owner, event: &reader_ui::AudiobookOpened, window, cx| {
                let session = event.0.read(cx).playback_session();
                let accepted = if audio { root.playback.read(cx).generation == generation } else { root.playback.update(cx, |controller, cx| controller.prepare_open(audio_request, &audio_locator, cx)) };
                if !accepted {
                    session.update(cx, |session, _| session.stop_playback());
                    if root.reader.as_ref().is_some_and(|reader| reader.locator == audio_locator) {
                        root.return_to_library(window, cx);
                    }
                    if root.pending_audiobook.as_ref().is_some_and(|pending| pending == owner) {
                        root.pending_audiobook = None;
                        cx.notify();
                    }
                    return;
                }
                root.playback.update(cx, |controller, cx| controller.replace(audio_locator.clone(), audio_title.clone(), session, cx));
                root.sync_playback(window, cx);
                root.pending_audiobook = None;
                root.return_to_library(window, cx);
            })
            .detach();
        }
        #[cfg(feature = "audiobooks")]
        if audio {
            self.pending_audiobook = Some(reader);
            window.set_window_title(&self.application_title);
            cx.notify();
            return;
        }
        let view = reader.into();
        self.reader = Some(ReaderSurface { locator, view });
        self.update_notification_interest(window, cx);
        cx.set_volume_button_capture(true);
        cx.set_keep_screen_awake(true);
        cx.notify();
    }

    pub fn return_to_library(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reader = None;
        self.update_notification_interest(window, cx);
        cx.set_volume_button_capture(true);
        cx.set_keep_screen_awake(false);
        cx.set_system_bars_visible(false);
        #[cfg(feature = "kobo")]
        if let Err(error) = gpui_kobo::request_experience_mode(gpui_kobo::KoboExperienceMode::LibraryFast) {
            log::error!("failed to return to Kobo library fast mode: {error}");
        }
        window.set_window_title(&self.application_title);
        self.browser.update(cx, |browser, cx| browser.focus_active_page(window, cx));
        cx.notify();
    }

    #[cfg(feature = "audiobooks")]
    pub fn audiobook_session(&self) -> Option<Entity<reader_ui::PlaybackSession>> {
        self.audiobook.as_ref().map(|audio| audio.session.clone())
    }

    #[cfg(feature = "audiobooks")]
    pub fn audiobook_is_expanded(&self) -> bool {
        self.audiobook.as_ref().is_some_and(|audio| self.reader.as_ref().is_some_and(|reader| reader.locator == audio.locator))
    }

    #[cfg(feature = "audiobooks")]
    pub fn expand_audiobook(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.audiobook_is_expanded() {
            return;
        }
        if let Some(audio) = &self.audiobook {
            let locator = audio.locator.clone();
            self.browser.update(cx, |browser, cx| browser.open_book_detail(*locator.library_id(), locator.content_hash(), window, cx));
            cx.notify();
        }
    }

    #[cfg(feature = "audiobooks")]
    pub fn close_audiobook(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.playback.update(cx, |controller, cx| controller.close(cx));
        self.sync_playback(window, cx);
    }

    #[cfg(feature = "audiobooks")]
    fn sync_playback(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let active = self.playback.read(cx).current.as_ref().map(|active| (active.record.locator.clone(), active.session.clone()));
        if active.as_ref().is_some_and(|(_, session)| self.audiobook.as_ref().is_some_and(|audio| &audio.session == session)) {
            return;
        }
        if let Some(old) = self.audiobook.take() {
            if self.reader.as_ref().is_some_and(|reader| reader.locator == old.locator) {
                self.return_to_library(window, cx);
            }
        }
        let Some((locator, session)) = active else {
            cx.notify();
            return;
        };
        let dock = cx.new(|cx| reader_ui::AudiobookDock::new(session.clone(), cx));
        cx.subscribe_in(&dock, window, |root, _, action: &reader_ui::DockAction, window, cx| match action {
            reader_ui::DockAction::Expand => root.expand_audiobook(window, cx),
            reader_ui::DockAction::Close => root.close_audiobook(window, cx),
        })
        .detach();
        self.audiobook = Some(AudiobookSurface { locator, session, dock });
        cx.notify();
    }

    fn update_notification_interest(&mut self, window: &Window, cx: &mut Context<Self>) {
        let mode = window.is_window_active().then_some(self.reader.is_none());
        if self.notification_mode == mode {
            return;
        }
        self.notification_mode = mode;
        self.notification_task.take();
        if let Some(browsing) = mode {
            let backend = self.backend.clone();
            self.notification_task = Some(cx.spawn(async move |_, _| match backend.notification_interest(browsing).await {
                Ok(interest) => {
                    let _interest = interest;
                    std::future::pending::<()>().await;
                }
                Err(error) => log::warn!("could not register notification interest: {error}"),
            }));
        }
    }

    fn on_back(&mut self, _: &DismissSettings, window: &mut Window, cx: &mut Context<Self>) {
        if self.reader.is_some() {
            self.return_to_library(window, cx);
        } else {
            cx.propagate();
        }
    }
}

impl Render for ApplicationRoot {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(feature = "audiobooks")]
        let footer = {
            let dock = self.audiobook.as_ref().filter(|audio| self.reader.as_ref().is_none_or(|reader| reader.locator != audio.locator)).map(|audio| audio.dock.clone());
            let pending = self.pending_audiobook.as_ref().map(|pending| pending.read(cx).loading_error().unwrap_or_else(|| "Opening audiobook…".into()));
            let visible = dock.is_some() || pending.is_some();
            self.audiobook_footer.update(cx, |footer, cx| footer.set(dock, pending, cx));
            visible.then(|| AnyView::from(self.audiobook_footer.clone()))
        };
        #[cfg(feature = "audiobooks")]
        self.browser.update(cx, |browser, cx| browser.set_content_footer(footer.clone().filter(|_| self.reader.is_none()), cx));
        let content = self.reader.as_ref().map(|reader| reader.view.clone()).unwrap_or_else(|| AnyView::from(self.browser.clone()));
        let root = gpui::div().size_full().flex().flex_col().key_context("Application").on_action(cx.listener(Self::on_back));
        #[cfg(feature = "kobo")]
        let root = root.when(self.reader.is_none(), |root| root.child(self.device_bar.clone()));
        let root = root.child(gpui::div().flex_1().min_h_0().w_full().child(content));
        #[cfg(feature = "audiobooks")]
        let root = root.children(footer.filter(|_| self.reader.is_some()));
        root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removed_reader_library_requires_returning_to_the_browser() {
        let library_id = LibraryId::from_u128(1);
        assert!(active_reader_library_is_missing(Some(&library_id), |_| false));
        assert!(!active_reader_library_is_missing(Some(&library_id), |_| true));
        assert!(!active_reader_library_is_missing(None, |_| false));
    }
}
