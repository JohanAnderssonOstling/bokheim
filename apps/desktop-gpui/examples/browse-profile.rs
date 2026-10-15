//! Open an isolated, pre-seeded app-data directory for browse profiling.
//! Set RUST_LOG=browse_profile=debug and BOKHEIM_CATALOG_FILE to a snapshot copy.
//! BOKHEIM_BROWSE_PROFILE_INPUT=pages, arrows or mixed exercises normal GPUI input
//! handlers after startup, including native backend requests and UI merges.
use app::{AppDataLocation, host::BackendLaunch};
use gpui::{AppContext, Keystroke, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput, point, px};
use std::path::PathBuf;
use std::time::Duration;

fn main() {
    let directory = PathBuf::from(std::env::args().nth(1).expect("usage: browse-profile ISOLATED_APP_DATA_DIRECTORY"));
    assert!(directory.join("app-state-v1.sqlite3").is_file(), "seed the isolated registry before profiling");
    app::init_logging();
    log::debug!(target: "browse_profile", "profile navigation input {}", std::fs::read_to_string(directory.join("gpui-ui-session.json")).expect("read isolated session"));
    let backend = BackendLaunch::initialize(AppDataLocation::native_path(directory)).expect("open profile directory");
    let input = std::env::var("BOKHEIM_BROWSE_PROFILE_INPUT").ok();
    gpui_platform::application().with_assets(ui_components::Assets).run(move |cx| {
        desktop_gpui::open_ui(backend, desktop_gpui::UiLaunchOptions::DESKTOP, None, None, cx);
        if let Some(input) = input {
            let window = cx.windows()[0];
            cx.spawn(async move |cx| {
                cx.background_executor().timer(Duration::from_secs(10)).await;
                for index in 0..40 {
                    let _ = cx.update_window(window, |_, window, cx| {
                        if input == "pages" || input == "mixed" && index >= 8 {
                            let handled = window.dispatch_keystroke(Keystroke::parse("pagedown").unwrap(), cx);
                            log::debug!(target: "browse_profile", "profile page input handled={handled}");
                        } else if input == "arrows" || input == "mixed" {
                            let position = point(window.viewport_size().width - px(36.0), px(76.0));
                            window.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent { position, pressed_button: None, modifiers: Default::default() }), cx);
                            window.dispatch_event(PlatformInput::MouseDown(MouseDownEvent { position, button: MouseButton::Left, modifiers: Default::default(), click_count: 1, first_mouse: false }), cx);
                            window.dispatch_event(PlatformInput::MouseUp(MouseUpEvent { position, button: MouseButton::Left, modifiers: Default::default(), click_count: 1 }), cx);
                        }
                    });
                    cx.background_executor().timer(Duration::from_millis(400)).await;
                }
                log::debug!(target: "browse_profile", "profile inputs complete");
            })
            .detach();
        }
    });
}
