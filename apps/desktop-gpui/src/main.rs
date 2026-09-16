use bokheim_backend::{AppDataLocation, host::BackendLaunch};

fn main() {
    if bokheim_backend::host::cpu::run_pdf_cover_process() { return; }
    bokheim_backend::host::cpu::configure_pdf_cover_process(std::env::current_exe().expect("could not locate desktop executable"));
    let backend_context = BackendLaunch::initialize(AppDataLocation::SharedDesktop).expect("failed to initialize backend app-data directory");
    bokheim_backend::init_logging();

    let instance = desktop_gpui::DesktopInstance::acquire(backend_context.app_data_dir()).expect("failed to establish desktop application ownership");
    let desktop_gpui::DesktopInstance::Primary(instance) = instance else {
        return;
    };
    let activations = instance.activations.clone();

    gpui_platform::application().with_assets(ui_components::Assets).run(move |cx| {
        let _instance = instance;
        desktop_gpui::open_ui(backend_context, desktop_gpui::UiLaunchOptions::DESKTOP, None, Some(activations), cx);
    });
}
