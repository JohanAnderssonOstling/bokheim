use bokheim_backend::app::{AppDataLocation, BackendContext};

fn main() {
    let backend_context = BackendContext::initialize(AppDataLocation::SharedDesktop).expect("failed to initialize backend app-data directory");
    bokheim_backend::init_logging();

    gpui_platform::application().with_assets(ui_components::Assets).run(move |cx| desktop_gpui::open_ui(backend_context, desktop_gpui::UiLaunchOptions::DESKTOP, None, cx));
}
