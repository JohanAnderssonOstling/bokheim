#[cfg(target_family = "wasm")]
use std::borrow::Cow;
#[cfg(target_family = "wasm")]
use std::cell::RefCell;
#[cfg(target_family = "wasm")]
use std::path::PathBuf;
#[cfg(target_family = "wasm")]
use std::rc::Rc;

#[cfg(target_family = "wasm")]
use bokheim_backend::{AppBackend, AppDataLocation};
#[cfg(target_family = "wasm")]
use browser_ui::{AppServices, ApplicationRoot, BrowserRoot, OpenReader};
#[cfg(target_family = "wasm")]
use gpui::{App, AppContext, Bounds, SharedString, WeakEntity, WindowBounds, WindowOptions, px, size};
#[cfg(target_family = "wasm")]
use gpui_component::Root;

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
        let backend = match bokheim_backend::initialize_web_storage().await.and_then(|()| AppBackend::initialize(AppDataLocation::PlatformLocal(PathBuf::new())).map_err(|error| error.to_string())) {
            Ok(backend) => backend,
            Err(error) => {
                log::error!("failed to start SQLite web backend: {error}");
                if let Some(window) = web_sys::window() {
                    let _ = window.alert_with_message(&format!("Bokheim could not open the browser database.\n\n{error}"));
                }
                return;
            }
        };
        let reader_preferences = backend
            .reader_preferences()
            .map(|saved| {
                let mut preferences = reader_ui::ReaderPreferences::default();
                preferences.apply_synchronized(&saved);
                preferences
            })
            .unwrap_or_else(|error| {
                log::warn!("failed to restore reader preferences: {error}");
                reader_ui::ReaderPreferences::default()
            });
        let application = gpui_platform::application().with_assets(ui_components::Assets::default());
        let handle = application.run_embedded(move |cx| {
            open_bokheim(backend, reader_preferences, cx);
        });
        APPLICATION.with(|application| *application.borrow_mut() = Some(handle));
    });
}

#[cfg(target_family = "wasm")]
fn open_bokheim(backend: AppBackend, reader_preferences: reader_ui::ReaderPreferences, cx: &mut App) {
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
    let saved_reader_preferences = Rc::new(RefCell::new(reader_preferences.clone()));
    let reader_preferences_backend = backend.clone();
    let save_reader_settings: reader_ui::SaveReaderSettings = Rc::new(move |preferences, _| {
        let patches = saved_reader_preferences.borrow().synchronized_patches(&preferences);
        *saved_reader_preferences.borrow_mut() = preferences;
        for patch in patches {
            if let Err(error) = reader_preferences_backend.update_reader_preference(patch) {
                log::warn!("failed to persist reader preference: {error}");
            }
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
                Ok(library) => std::rc::Rc::new(library),
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
