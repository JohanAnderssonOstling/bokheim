#![cfg(target_family = "wasm")]

use ui_text::text;

use std::cell::OnceCell;
use std::rc::Rc;

use app::{AppClient, BackendError};
use operation_failure::OperationError;
use application_shell::ApplicationRoot;
use browser_ui::{AppServices, AppStartup, NavigationSession};
use gpui::{App, AppContext, Bounds, SharedString, WindowBounds, WindowOptions, px, size};
use gpui_component::Root;
use wasm_bindgen::{JsCast, JsValue};

mod browser_back;
mod reader_preferences;

thread_local! {
    static APPLICATION: OnceCell<gpui::ApplicationHandle> = const { OnceCell::new() };
}

#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    // Parser workers initialize this module too, but only the window owns UI.
    if web_sys::window().is_none() {
        return;
    }
    console_error_panic_hook::set_once();
    gpui_platform::web_init();
    let application = gpui_platform::application().with_assets(ui_components::Assets::default());
    wasm_bindgen_futures::spawn_local(async move {
        let (backend, state) = match connect_backend().await {
            Ok(initialized) => initialized,
            Err(error) => {
                log::error!("failed to start SQLite web backend: {error}");
                notify_startup("__BOKHEIM_STARTUP_FAILED__", &OperationError::from_classified(&error).into_js());
                return;
            }
        };
        let startup = AppStartup {
            default_library_root: state.default_library_root,
            dictionary_storage_root: state.dictionary_storage_root,
            preference_storage_root: state.preference_storage_root,
            libraries: state.libraries,
            browsing_preferences: state.browsing_preferences,
            account_status: state.account_status,
        };
        let mut preferences = reader_preferences::load();
        preferences.apply_synchronized(&state.reader_preferences);
        let handle = application.run_embedded(move |cx| match open_bokheim(backend, startup, preferences, cx) {
            Ok(()) => notify_startup("__BOKHEIM_STARTUP_READY__", &JsValue::UNDEFINED),
            Err(error) => {
                log::error!("failed to open the Bokheim browser window: {error}");
                notify_startup("__BOKHEIM_STARTUP_FAILED__", &OperationError::from_classified(&error).into_js());
                cx.quit();
            }
        });
        APPLICATION.with(|application| assert!(application.set(handle).is_ok(), "web application already initialized"));
    });
}

fn notify_startup(name: &str, detail: &JsValue) {
    if let Ok(callback) = js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str(name)) {
        if let Some(callback) = callback.dyn_ref::<js_sys::Function>() {
            let _ = callback.call1(&JsValue::UNDEFINED, detail);
        }
    }
}

async fn connect_backend() -> Result<(AppClient, app::AppStartupState), BackendError> {
    let asset_base = js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("__BOKHEIM_ASSET_BASE__")).ok().and_then(|value| value.as_string()).unwrap_or_else(|| "./pkg".to_owned());
    AppClient::connect_web(&asset_base).await
}

fn open_bokheim(backend: AppClient, startup: AppStartup, reader_preferences: reader_ui::ReaderPreferences, cx: &mut App) -> Result<(), BackendError> {
    gpui_component::init(cx);
    ui_components::init_page_scrolling(cx);
    browser_ui::bind_keys(cx);
    reader_ui::bind_keys(cx);
    let reader_preferences_store = backend.preferences.clone();
    let save_reader_settings: reader_ui::SaveReaderSettings = Rc::new(move |preferences, patches, cx| {
        reader_preferences::save(&preferences);
        if !patches.is_empty() {
            let preferences_store = reader_preferences_store.clone();
            cx.spawn(async move |_cx| {
                if let Err(error) = preferences_store.update_reader(patches).await {
                    log::warn!("failed to persist reader preference: {error}");
                }
            })
            .detach();
        }
    });
    reader_ui::configure(reader_preferences, save_reader_settings, cx);

    let application_backend = backend.clone();
    let services = AppServices::new(backend, startup);
    let bounds = Bounds::centered(None, size(px(1240.0), px(820.0)), cx);

    cx.open_window(
        WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), titlebar: Some(gpui::TitlebarOptions { title: Some(SharedString::from(text!(common::bokheim))), ..Default::default() }), focus: true, ..Default::default() },
        move |window, cx| {
            let application = cx.new(|cx| ApplicationRoot::new(services, browser_ui::WindowSession { initial: load_navigation_session(), save: Rc::new(save_navigation_session) }, application_backend, "Bokheim", None, window, cx));
            browser_back::install(window, cx);
            cx.new(|cx| Root::new(application, window, cx))
        },
    )
    .map_err(BackendError::operation)?;
    cx.activate(true);
    Ok(())
}

#[cfg(target_arch = "wasm32")]

const NAVIGATION_STORAGE_KEY: &str = "bokheim.navigation.v1";

fn load_navigation_session() -> NavigationSession {
    let load = || -> Result<Option<NavigationSession>, OperationError> {
        let storage = web_sys::window().ok_or("browser window is unavailable")?.local_storage().map_err(OperationError::from)?.ok_or("local storage is unavailable")?;
        storage.get_item(NAVIGATION_STORAGE_KEY).map_err(OperationError::from)?.map(|value| serde_json::from_str(&value).map_err(OperationError::operation)).transpose()
    };
    match load() {
        Ok(session) => session.unwrap_or_default(),
        Err(error) => {
            log::warn!("could not restore navigation: {error}");
            NavigationSession::default()
        }
    }
}

fn save_navigation_session(session: NavigationSession, _: &mut App) {
    let save = || -> Result<(), OperationError> {
        let storage = web_sys::window().ok_or("browser window is unavailable")?.local_storage().map_err(OperationError::from)?.ok_or("local storage is unavailable")?;
        let value = serde_json::to_string(&session).map_err(OperationError::operation)?;
        storage.set_item(NAVIGATION_STORAGE_KEY, &value).map_err(OperationError::from)
    };
    if let Err(error) = save() {
        log::warn!("could not save navigation: {error}");
    }
}
