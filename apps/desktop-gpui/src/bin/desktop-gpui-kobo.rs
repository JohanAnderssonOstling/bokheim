use std::path::PathBuf;
use std::time::{Duration, Instant};

use epub_svelte_lib::app::{AppDataLocation, BackendContext, LIBRARY_MANIFEST_FILENAME};
use gpui::Application;
use gpui_kobo::{KoboExperienceMode, KoboPlatform, KoboPlatformOptions, PageButtonBehavior};

fn suspend_device() -> Result<(), String> {
    kobo_device::suspend().map_err(|error| error.to_string())
}

fn main() {
    let startup_started = Instant::now();
    println!("KOBO_APP_STARTUP phase=process_start elapsed_ms=0");
    let args: Vec<String> = std::env::args().collect();
    let no_display = args.iter().any(|argument| argument == "--no-display");
    let auto_rotate = !args.iter().any(|argument| argument == "--no-rotation");
    let data_dir = std::env::var_os("BOKHEIM_KOBO_DATA_DIR").map(PathBuf::from).unwrap_or_else(default_data_dir);
    let backend_context = BackendContext::initialize(AppDataLocation::PlatformLocal(data_dir)).expect("failed to initialize Kobo app-data directory");
    println!("KOBO_APP_STARTUP phase=backend_ready elapsed_ms={}", startup_started.elapsed().as_millis());
    let libraries_dir = std::env::var_os("BOKHEIM_KOBO_LIBRARIES_DIR").map(PathBuf::from).unwrap_or_else(default_libraries_dir);
    backend_context.discover_libraries(&libraries_dir).expect("failed to discover Kobo libraries");
    ensure_default_library(&backend_context, &libraries_dir).expect("failed to initialize Kobo library");
    println!("KOBO_APP_STARTUP phase=libraries_ready elapsed_ms={}", startup_started.elapsed().as_millis());
    epub_svelte_lib::init_logging();

    let platform = KoboPlatform::new(KoboPlatformOptions {
        display: !no_display,
        auto_rotate,
        interactive: !no_display,
        timeout: Duration::ZERO,
        page_buttons: PageButtonBehavior::ArrowKeys,
        power_button_handler: Some(suspend_device),
        show_exit_button: false,
        experience_mode: KoboExperienceMode::LibraryFast,
        monochrome_threshold: 160,
        ..Default::default()
    })
    .expect("failed to initialize Kobo GPUI platform");
    println!("KOBO_APP_STARTUP phase=platform_ready elapsed_ms={}", startup_started.elapsed().as_millis());
    gpui_kobo::begin_application_startup_profile(startup_started).expect("failed to start Kobo application startup profile");

    Application::new_inaccessible(platform.clone()).with_assets(ui_components::Assets).run(move |cx| {
        println!("KOBO_APP_STARTUP phase=ui_open_begin elapsed_ms={}", startup_started.elapsed().as_millis());
        desktop_gpui::open_ui(backend_context, desktop_gpui::UiLaunchOptions { kobo_auto_rotate: Some(auto_rotate), ..desktop_gpui::UiLaunchOptions::KOBO }, cx);
        println!("KOBO_APP_STARTUP phase=ui_open_complete elapsed_ms={}", startup_started.elapsed().as_millis());
    });

    if let Some(error) = platform.take_error() {
        panic!("Kobo GPUI platform failed: {error}");
    }
    if no_display {
        if let Some(frame) = platform.last_frame() {
            println!("rendered shared Bokheim UI {}x{}", frame.width(), frame.height());
        } else {
            println!("rendered shared Bokheim UI successfully before shutdown");
        }
    }
}

#[cfg(target_arch = "arm")]
fn default_data_dir() -> PathBuf {
    PathBuf::from("/mnt/onboard/.adds/bokheim")
}

#[cfg(not(target_arch = "arm"))]
fn default_data_dir() -> PathBuf {
    std::env::temp_dir().join("desktop-gpui-kobo-host")
}

fn ensure_default_library(context: &BackendContext, libraries_dir: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    if !context.try_get_library_entries()?.is_empty() {
        return Ok(());
    }
    let library_dir = std::env::var_os("BOKHEIM_KOBO_LIBRARY_DIR").map(PathBuf::from).unwrap_or_else(|| {
        let legacy = legacy_library_dir();
        if legacy.join(LIBRARY_MANIFEST_FILENAME).is_file() { legacy } else { libraries_dir.join("default") }
    });
    std::fs::create_dir_all(&library_dir)?;
    context.create_library("Kobo".to_owned(), Some(library_dir))?;
    Ok(())
}

#[cfg(target_arch = "arm")]
fn default_libraries_dir() -> PathBuf {
    PathBuf::from("/mnt/onboard/Bokheim/Libraries")
}

#[cfg(not(target_arch = "arm"))]
fn default_libraries_dir() -> PathBuf {
    std::env::temp_dir().join("desktop-gpui-kobo-libraries")
}

#[cfg(target_arch = "arm")]
fn legacy_library_dir() -> PathBuf {
    PathBuf::from("/mnt/onboard/Books")
}

#[cfg(not(target_arch = "arm"))]
fn legacy_library_dir() -> PathBuf {
    std::env::temp_dir().join("desktop-gpui-kobo-books")
}
