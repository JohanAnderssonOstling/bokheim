use epub_svelte_lib::app::{AppDataLocation, BackendContext};

fn main() {
    let backend_context = BackendContext::initialize(AppDataLocation::SharedDesktop).expect("failed to initialize backend app-data directory");
    epub_svelte_lib::init_logging();

    gpui_platform::application().with_assets(ui_components::Assets).run(move |cx| desktop_gpui::open_ui(backend_context, desktop_gpui::UiLaunchOptions::DESKTOP, cx));
}
