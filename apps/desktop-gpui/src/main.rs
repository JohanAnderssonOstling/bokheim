#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use app::{AppDataLocation, host::BackendLaunch};
use std::cell::RefCell;
use std::rc::Rc;

fn main() -> Result<(), String> {
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    if desktop_gpui::update_entry::candidate_entry() {
        return Ok(());
    }
    let backend_context = BackendLaunch::initialize(AppDataLocation::SharedDesktop).map_err(|error| format!("failed to initialize backend app-data directory: {error}"))?;
    app::host::configure_opds_browser(backend_context.app_data_dir().join("opds-webview"));
    app::init_logging();

    #[cfg(target_os = "windows")]
    if desktop_gpui::windows_updates::helper_entry(backend_context.app_data_dir()).map_err(|error| format!("failed to run update helper: {error}"))? {
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    let launch = match desktop_gpui::windows_updates::enter(backend_context.app_data_dir()).map_err(|error| format!("failed to establish startup ownership: {error}"))? {
        Some(launch) => launch,
        None => return Ok(()),
    };

    let instance = desktop_gpui::DesktopInstance::acquire(backend_context.app_data_dir()).map_err(|error| format!("failed to establish desktop application ownership: {error}"))?;
    let desktop_gpui::DesktopInstance::Primary(instance) = instance else {
        return Ok(());
    };
    #[cfg(target_os = "windows")]
    {
        let trial = launch.acquired().map_err(|error| format!("failed to acknowledge startup ownership: {error}"))?;
        if !desktop_gpui::windows_updates::initialize(backend_context.app_data_dir(), trial.as_deref()).map_err(|error| format!("failed to recover application update: {error}"))? {
            return Ok(());
        }
    }
    #[cfg(target_os = "linux")]
    match desktop_gpui::linux_updates::initialize(backend_context.app_data_dir()).map_err(|error| format!("failed to recover application update: {error}"))? {
        desktop_gpui::linux_updates::Startup::Run => {}
        desktop_gpui::linux_updates::Startup::Relaunch { executable, trial } => {
            drop(instance);
            if !desktop_gpui::linux_updates::launch(backend_context.app_data_dir(), &executable, trial.as_deref()).map_err(|error| format!("failed to supervise application startup: {error}"))? {
                // No backend has started in this process. Re-enter under desktop
                // ownership and restore the durable journal before opening any data.
                if trial.is_some() {
                    return main();
                }
            }
            return Ok(());
        }
    }
    let activations = instance.activations.clone();

    let _instance = instance;
    #[cfg(target_os = "linux")]
    let startup_permit = desktop_gpui::linux_updates::trial_pending().then(app::host::defer_background_work);
    #[cfg(target_os = "windows")]
    let startup_permit = desktop_gpui::windows_updates::trial_pending().then(app::host::defer_background_work);
    let startup_error = Rc::new(RefCell::new(None));
    let failure = startup_error.clone();
    gpui_platform::application().with_assets(ui_components::Assets).run(move |cx| {
        cx.set_app_identity("se.bokheim.Bokheim", "Bokheim");
        let opened = desktop_gpui::open_ui(backend_context, desktop_gpui::UiLaunchOptions::DESKTOP, None, Some(activations), cx);
        if opened.is_none() {
            *failure.borrow_mut() = Some("failed to open the application UI".to_owned());
            cx.quit();
            return;
        }
        #[cfg(target_os = "windows")]
        if opened.is_some() {
            if let Err(error) = desktop_gpui::windows_updates::healthy() {
                *failure.borrow_mut() = Some(format!("failed to commit healthy update startup: {error}"));
                cx.quit();
                return;
            }
            if let Some(permit) = startup_permit {
                permit.release();
            }
        }
        #[cfg(target_os = "linux")]
        if opened.is_some() {
            if let Err(error) = desktop_gpui::linux_updates::healthy() {
                *failure.borrow_mut() = Some(format!("failed to commit healthy update startup: {error}"));
                cx.quit();
                return;
            }
            if let Some(permit) = startup_permit {
                permit.release();
            }
        }
    });
    let error = startup_error.borrow_mut().take();
    error.map_or(Ok(()), Err)
}
