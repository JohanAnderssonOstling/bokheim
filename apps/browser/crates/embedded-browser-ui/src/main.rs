#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

//! A browser window isolated from the application's GPUI event loop.
mod browser;
mod passwords;
mod profile;

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("probe") => std::process::exit(if wry::webview_version().is_ok() { 0 } else { 1 }),
        Some("browser") => {}
        _ => {
            eprintln!("Usage: bokheim-browser <browser|probe>");
            std::process::exit(2);
        }
    }
    #[cfg(target_os = "linux")]
    // Wry's child window uses X11. Configure it before creating threads.
    unsafe {
        std::env::remove_var("WAYLAND_DISPLAY");
        std::env::set_var("GDK_BACKEND", "x11");
    }
    if let Err(error) = browser::run() {
        eprintln!("Browser window failed: {error:#}");
        std::process::exit(1);
    }
}
