use epub_svelte_lib::APP_NAME;
use floem::window::WindowConfig;
use floem::Application;
use floem::View;

use crate::components::app_root;
use crate::page_navigation::page_navigation_view;
use crate::session::load_session;

mod auth_view;
mod components;
mod epub_reader;
mod home_page;
mod library_page;
mod library_tabs;
mod page_navigation;
mod session;
mod style;
mod toc_model;

fn app_view(core: epub_svelte_lib::app_core::AppCore) -> impl View {
    app_root(page_navigation_view(core))
}

fn main() {
    let app_core = epub_svelte_lib::app_core::AppCore::initialize(epub_svelte_lib::home::AppDataLocation::SharedDesktop).expect("failed to initialize application core");

    // Initialize logging
    epub_svelte_lib::init_logging();

    let mut window_config = WindowConfig::default().title(APP_NAME);
    if let Some(window) = load_session(&app_core).window.and_then(|window| window.sanitized()) {
        window_config = window_config.size(window.size()).position(window.position());
    }

    Application::new().window(move |_| app_view(app_core.clone()), Some(window_config)).run();
}
