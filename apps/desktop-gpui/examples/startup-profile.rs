//! Profile shared native startup against an isolated app-data directory.
//!
//! RUST_LOG=startup_profile=debug,browse_profile=debug cargo run --release \
//!   -p desktop-gpui --example startup-profile -- ISOLATED_APP_DATA_DIRECTORY
//!
//! Use BOKHEIM_CATALOG_FILE for a local catalog snapshot. Optional
//! BOKHEIM_STARTUP_DEFER_MAINTENANCE=1 holds background tasks until the first
//! frame callback, to measure contention. Never point this at live app data.
use app::{AppDataLocation, host::{BackendLaunch, startup_profile::StartupProfile}};
use std::{path::PathBuf, time::Duration};

fn main() {
    app::init_logging();
    let mut profile = StartupProfile::new("launch");
    let directory = PathBuf::from(std::env::args().nth(1).expect("usage: startup-profile ISOLATED_APP_DATA_DIRECTORY"));
    assert!(directory.join("app-state-v1.sqlite3").is_file(), "seed an isolated registry before profiling");
    let maintenance = std::env::var_os("BOKHEIM_STARTUP_DEFER_MAINTENANCE").map(|_| app::host::defer_background_work());
    let backend = BackendLaunch::initialize(AppDataLocation::native_path(directory)).expect("open profile directory");
    profile.mark("launch_config");
    let application = gpui_platform::application().with_assets(ui_components::Assets);
    profile.mark("platform_construct");
    application.run(move |cx| {
        profile.mark("event_loop_ready");
        desktop_gpui::open_ui(backend, desktop_gpui::UiLaunchOptions::DESKTOP, None, None, cx);
        profile.mark("ui_open");
        let window = cx.windows()[0];
        // This measures the next GPUI frame callback, not compositor presentation.
        window.update(cx, |_, window, _| window.on_next_frame(move |_, _| {
            profile.mark("frame_callback");
            if let Some(permit) = maintenance {
                permit.release();
            }
        })).expect("observe window");
        cx.spawn(async move |cx| {
            cx.background_executor().timer(Duration::from_secs(5)).await;
            let _ = cx.update(|cx| cx.quit());
        }).detach();
    });
}
