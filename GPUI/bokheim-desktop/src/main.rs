mod registry;
mod session;

use std::cell::RefCell;
use std::rc::Rc;

use bokheim_ui_runtime::BlockingWorker;
use browser_ui::{BrowserView, BrowserWindowGeometry, OpenReader, SaveBrowserState};
use epub_svelte_lib::scanner::LibraryEvent;
use gpui::{App, AppContext, Bounds, Entity, SharedString, WeakEntity, WindowBounds, WindowOptions, px, size};
use gpui_component::Root;
use reader_ui::{ReaderPreferences, ReaderWindowGeometry, SaveReaderSettings};

use registry::LibraryRegistry;
use session::{SessionSaveScheduler, UiSession, session_path};

fn main() {
    let backend_context = epub_svelte_lib::home::BackendContext::initialize(epub_svelte_lib::home::AppDataLocation::SharedDesktop).expect("failed to initialize backend app-data directory");
    epub_svelte_lib::init_logging();

    let session_path = session_path(backend_context.app_data_dir());
    let saved_session = Rc::new(RefCell::new(UiSession::load(&session_path)));

    gpui_platform::application().with_assets(gpui_component_assets::Assets).run(move |cx: &mut App| {
        gpui_component::init(cx);
        browser_ui::bind_keys(cx);
        reader_ui::bind_keys(cx);
        let blocking_worker = BlockingWorker::new(&cx.background_executor(), 2);
        let persistence_worker = BlockingWorker::new(&cx.background_executor(), 1);
        let session_saves = SessionSaveScheduler::new(saved_session.clone(), session_path.clone(), persistence_worker.clone());

        let library_entries = match backend_context.try_get_library_entries() {
            Ok(entries) => entries,
            Err(error) => {
                log::error!("failed to load library entries: {error}");
                Vec::new()
            }
        };
        let registry = LibraryRegistry::open(&backend_context);
        let resolve_library = registry.resolver();
        let library_events = registry.events();
        let app_core = registry.core();
        cx.set_global(registry);

        let reader_settings = saved_session.clone();
        let reader_session_saves = session_saves.clone();
        let save_reader_settings: SaveReaderSettings = Rc::new(move |preferences: ReaderPreferences, geometry: Option<ReaderWindowGeometry>, cx: &mut App| {
            let mut session = reader_settings.borrow_mut();
            session.reader_preferences = preferences;
            if geometry.is_some() {
                session.reader_window = geometry;
            }
            drop(session);
            reader_session_saves.schedule(cx);
        });
        reader_ui::configure(saved_session.borrow().reader_preferences.clone(), saved_session.borrow().reader_window, save_reader_settings, blocking_worker.clone(), persistence_worker, cx);

        let reader_resolver = resolve_library.clone();
        let open_reader: OpenReader = Rc::new(move |locator, title, cx| {
            let Some(library) = reader_resolver(locator.library_id().as_str()) else {
                log::error!("cannot open book {}: library {} is unavailable", locator.book_id(), locator.library_id());
                return;
            };
            reader_ui::open_reader_window(locator, title, library, cx);
        });

        let browser_settings = saved_session.clone();
        let browser_session_saves = session_saves.clone();
        let save_browser_state: SaveBrowserState = Rc::new(move |browser, geometry: Option<BrowserWindowGeometry>, cx| {
            let mut session = browser_settings.borrow_mut();
            let browser_changed = session.browser != browser;
            let geometry_changed = geometry.is_some_and(|geometry| session.browser_window != Some(geometry));
            if !browser_changed && !geometry_changed {
                return;
            }
            session.browser = browser;
            if geometry.is_some() {
                session.browser_window = geometry;
            }
            drop(session);
            browser_session_saves.schedule(cx);
        });

        let browser_bounds = saved_session
            .borrow()
            .browser_window
            .map(|geometry| WindowBounds::Windowed(Bounds { origin: gpui::point(px(geometry.x), px(geometry.y)), size: size(px(geometry.width), px(geometry.height)) }))
            .unwrap_or_else(|| WindowBounds::Windowed(Bounds::centered(None, size(px(1240.0), px(820.0)), cx)));
        let browser_session = saved_session.borrow().browser.clone();
        let browser_out = Rc::new(RefCell::new(None));
        let browser_for_window = browser_out.clone();
        cx.open_window(
            WindowOptions { window_bounds: Some(browser_bounds), titlebar: Some(gpui::TitlebarOptions { title: Some(SharedString::from(epub_svelte_lib::APP_NAME)), ..Default::default() }), focus: true, ..Default::default() },
            move |window, cx| {
                let browser = cx.new(|cx| BrowserView::new(library_entries, browser_session, resolve_library, open_reader, save_browser_state, blocking_worker, app_core, window, cx));
                *browser_for_window.borrow_mut() = Some(browser.clone());
                cx.new(|cx| Root::new(browser, window, cx))
            },
        )
        .expect("failed to open browsing window");

        let browser: Entity<BrowserView> = browser_out.borrow_mut().take().expect("browser window always constructs its view");
        forward_library_events(browser.downgrade(), library_events, cx);

        let quit_session_saves = session_saves.clone();
        cx.on_app_quit(move |_| {
            quit_session_saves.flush();
            async {}
        })
        .detach();
        cx.activate(true);
    });
}

fn forward_library_events(browser: WeakEntity<BrowserView>, events: async_channel::Receiver<(String, LibraryEvent)>, cx: &mut App) {
    cx.spawn(async move |cx| {
        while let Ok((library_id, event)) = events.recv().await {
            cx.update(|cx| {
                if let LibraryEvent::BookRemoved { book_id, .. } = &event {
                    reader_ui::handle_book_removed(&library_id, *book_id, cx);
                }
                let _ = browser.update(cx, |browser, cx| {
                    browser.handle_library_event(&library_id, &event, cx);
                });
            });
        }
    })
    .detach();
}
