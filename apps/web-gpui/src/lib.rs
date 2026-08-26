#[cfg(target_family = "wasm")]
mod web_backend;

#[cfg(target_family = "wasm")]
use std::borrow::Cow;
#[cfg(target_family = "wasm")]
use std::cell::RefCell;
#[cfg(target_family = "wasm")]
use std::rc::Rc;

#[cfg(target_family = "wasm")]
use app_contract::backend::AppBackend as _;
#[cfg(target_family = "wasm")]
use browser_ui::{AppServices, ApplicationRoot, BrowserRoot, OpenReader};
#[cfg(target_family = "wasm")]
use gpui::{App, AppContext, Bounds, SharedString, WeakEntity, WindowBounds, WindowOptions, px, size};
#[cfg(target_family = "wasm")]
use gpui_component::Root;

#[cfg(target_family = "wasm")]
use web_backend::BrowserAppBackend;
#[cfg(target_family = "wasm")]
use bokheim_web_core::web_database as core;

#[cfg(target_family = "wasm")]
thread_local! {
    static APPLICATION: RefCell<Option<gpui::ApplicationHandle>> = const { RefCell::new(None) };
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    gpui_platform::web_init();
    wasm_bindgen_futures::spawn_local(async {
        if let Err(error) = core::bokheim_database_open().await {
            log::error!("failed to start SQLite web backend: {error:?}");
            return;
        }
        let reader_preferences = serde_wasm_bindgen::from_value(core::bokheim_preferences_reader_get().unwrap_or(wasm_bindgen::JsValue::NULL)).unwrap_or_default();
        let browser_session = serde_wasm_bindgen::from_value(core::bokheim_browser_session_get().unwrap_or(wasm_bindgen::JsValue::NULL)).unwrap_or_default();
        let backend = match BrowserAppBackend::load(browser_session).await {
            Ok(backend) => backend,
            Err(error) => {
                log::error!("failed to start SQLite web backend: {error}");
                if let Some(window) = web_sys::window() {
                    let _ = window.alert_with_message(&format!("Bokheim could not open the browser database.\n\n{error}"));
                }
                return;
            }
        };
        let application = gpui_platform::application().with_assets(ui_components::Assets::default());
        let handle = application.run_embedded(move |cx| {
            open_bokheim(backend, reader_preferences, cx);
        });
        APPLICATION.with(|application| *application.borrow_mut() = Some(handle));
    });
}

#[cfg(target_family = "wasm")]
fn open_bokheim(backend: BrowserAppBackend, reader_preferences: reader_ui::ReaderPreferences, cx: &mut App) {
    if let Err(error) = cx.text_system().add_fonts(vec![
        Cow::Borrowed(include_bytes!("../../desktop-gpui/assets/fonts/NotoSans-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../../desktop-gpui/assets/fonts/NotoSans-Bold.ttf")),
        Cow::Borrowed(include_bytes!("../../desktop-gpui/assets/fonts/LibertinusSerif-Regular.otf")),
        Cow::Borrowed(include_bytes!("../../desktop-gpui/assets/fonts/LibertinusSerif-Bold.otf")),
    ]) {
        log::warn!("failed to install Bokheim fonts: {error}");
    }

    gpui_component::init(cx);
    ui_components::init_page_scrolling(cx);
    browser_ui::bind_keys(cx);
    reader_ui::bind_keys(cx);
    let (reader_settings_saves, reader_settings_updates) = async_channel::unbounded();
    wasm_bindgen_futures::spawn_local(async move {
        while let Ok(mut preferences) = reader_settings_updates.recv().await {
            while let Ok(newer) = reader_settings_updates.try_recv() {
                preferences = newer;
            }
            let value = match serde_wasm_bindgen::to_value(&preferences) {
                Ok(value) => value,
                Err(error) => {
                    log::error!("failed to encode reader preferences: {error}");
                    continue;
                }
            };
            if let Err(error) = core::bokheim_preferences_reader_store(value) {
                log::error!("failed to persist reader preferences: {error:?}");
            }
        }
    });
    let save_reader_settings: reader_ui::SaveReaderSettings = Rc::new(move |preferences, _| {
        if reader_settings_saves.try_send(preferences).is_err() {
            log::error!("reader preference persistence task stopped");
        }
    });
    reader_ui::configure(reader_preferences, save_reader_settings, cx);

    let reader_backend = backend.clone();
    let application_root: Rc<RefCell<Option<WeakEntity<ApplicationRoot>>>> = Rc::new(RefCell::new(None));
    let application_root_for_open = application_root.clone();
    let open_reader: OpenReader = Rc::new(move |locator, title, initial_target, window, cx| {
        let Some(root) = application_root_for_open.borrow().clone() else {
            log::error!("cannot open fixture book {}: application window is unavailable", locator.content_hash());
            return;
        };
        let reader_backend = reader_backend.clone();
        window.defer(cx, move |window, cx| {
            let request = match root.update(cx, |root, _| root.begin_open(locator.clone())) {
                Ok(request) => request,
                Err(_) => {
                    log::error!("cannot open fixture book {}: application window was closed", locator.content_hash());
                    return;
                }
            };
            let library = match reader_backend.library(locator.library_id()) {
                Ok(library) => library,
                Err(error) => {
                    let message = format!("Could not open book: {error}");
                    log::error!("cannot open fixture book {}: {error}", locator.content_hash());
                    let _ = root.update_in(cx, move |root, window, cx| root.fail_open(request, message, window, cx));
                    return;
                }
            };
            if root
                .update(cx, move |root, cx| {
                    let return_to_library = root.return_to_library_callback(cx);
                    let reader = cx.new(|cx| reader_ui::ReaderView::new(locator, title, initial_target, library, return_to_library, window, cx));
                    root.show_reader(request, reader.into(), cx);
                })
                .is_err()
            {
                log::error!("cannot open fixture book: application window was closed");
            }
        });
    });
    let services = AppServices::new(backend, open_reader, None);
    let bounds = Bounds::centered(None, size(px(1240.0), px(820.0)), cx);

    cx.open_window(
        WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), titlebar: Some(gpui::TitlebarOptions { title: Some(SharedString::from("Bokheim")), ..Default::default() }), focus: true, ..Default::default() },
        move |window, cx| {
            let browser = cx.new(|cx| BrowserRoot::new(services, window, cx));
            let application = cx.new(|_| ApplicationRoot::new(browser, "Bokheim"));
            *application_root.borrow_mut() = Some(application.downgrade());
            cx.new(|cx| Root::new(application, window, cx))
        },
    )
    .expect("failed to open the Bokheim browser window");
    cx.activate(true);
}
