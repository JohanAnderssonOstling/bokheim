#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use app::{AppDataLocation, host::BackendLaunch};
use desktop_gpui::{DesktopInstance, ReaderActivation};
use std::cell::RefCell;
use std::rc::Rc;

fn main() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    let activation_token = {
        let token = std::env::var("XDG_ACTIVATION_TOKEN").ok().filter(|token| !token.is_empty());
        // This is the first operation in main, before starting any threads.
        // The forwarded activation owns the token; GPUI must not also consume it.
        unsafe { std::env::remove_var("XDG_ACTIVATION_TOKEN") };
        token
    };
    #[cfg(not(target_os = "linux"))]
    let activation_token = None;
    let activation = ReaderActivation::from_launch_args(std::env::args().skip(1), activation_token)?;
    run(activation)
}

fn run(activation: ReaderActivation) -> Result<(), String> {
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    if desktop_gpui::update_entry::candidate_entry() {
        return Ok(());
    }
    let backend_context = BackendLaunch::initialize(AppDataLocation::SharedDesktop).map_err(|error| format!("failed to initialize backend app-data directory: {error}"))?;
    app::host::configure_opds_browser(backend_context.app_data_dir().join("opds-webview"));
    app::init_logging();
    // Carry the initial request explicitly through updater/helper relaunches.
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    let startup_arguments = activation.launch_args();
    let activate_existing = || -> Result<(), String> { DesktopInstance::send_activation(backend_context.app_data_dir(), &activation).map_err(|error| format!("failed to forward desktop activation: {error}")) };

    #[cfg(target_os = "windows")]
    if desktop_gpui::windows_updates::helper_entry(backend_context.app_data_dir()).map_err(|error| format!("failed to run update helper: {error}"))? {
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    let launch = match desktop_gpui::windows_updates::enter(backend_context.app_data_dir()).map_err(|error| format!("failed to establish startup ownership: {error}"))? {
        Some(launch) => launch,
        None => return activate_existing(),
    };

    let instance = DesktopInstance::acquire(backend_context.app_data_dir()).map_err(|error| format!("failed to establish desktop application ownership: {error}"))?;
    let DesktopInstance::Primary(instance) = instance else {
        // Release updater ownership before waiting for the running application.
        #[cfg(target_os = "windows")]
        drop(launch);
        return activate_existing();
    };
    #[cfg(target_os = "windows")]
    {
        let trial = launch.acquired().map_err(|error| format!("failed to acknowledge startup ownership: {error}"))?;
        if !desktop_gpui::windows_updates::initialize(backend_context.app_data_dir(), trial.as_deref(), &startup_arguments).map_err(|error| format!("failed to recover application update: {error}"))? {
            return Ok(());
        }
    }
    #[cfg(target_os = "linux")]
    match desktop_gpui::linux_updates::initialize(backend_context.app_data_dir()).map_err(|error| format!("failed to recover application update: {error}"))? {
        desktop_gpui::linux_updates::Startup::Run => {}
        desktop_gpui::linux_updates::Startup::Relaunch { executable, trial } => {
            drop(instance);
            if !desktop_gpui::linux_updates::launch(backend_context.app_data_dir(), &executable, trial.as_deref(), &startup_arguments).map_err(|error| format!("failed to supervise application startup: {error}"))? {
                // No backend has started in this process. Re-enter under desktop
                // ownership and restore the durable journal before opening any data.
                if trial.is_some() {
                    return run(activation);
                }
            }
            return Ok(());
        }
    }
    if !activation.urls.is_empty() || activation.activation_token.is_some() {
        instance.activation_sender.try_send(activation).map_err(|error| format!("failed to queue initial desktop activation: {error}"))?;
    }
    instance.start_listening();
    #[cfg(target_os = "windows")]
    if let Err(error) = desktop_gpui::windows_updates::register_reader_scheme() {
        log::warn!("could not register Bokheim reader links: {error}");
    }
    let activations = instance.activations.clone();
    let activation_sender = instance.activation_sender.clone();

    let _instance = instance;
    #[cfg(target_os = "linux")]
    let startup_permit = desktop_gpui::linux_updates::trial_pending().then(app::host::defer_background_work);
    #[cfg(target_os = "windows")]
    let startup_permit = desktop_gpui::windows_updates::trial_pending().then(app::host::defer_background_work);
    let startup_error = Rc::new(RefCell::new(None));
    let failure = startup_error.clone();
    let application = gpui_platform::application().with_assets(ui_components::Assets);
    let reopen_sender = activation_sender.clone();
    application.on_open_urls(move |urls| {
        if let Err(error) = activation_sender.try_send(urls.into()) {
            log::warn!("could not queue reader links: {error}");
        }
    });
    application.on_reopen(move |_| {
        if let Err(error) = reopen_sender.try_send(ReaderActivation::default()) {
            log::warn!("could not queue application reopen: {error}");
        }
    });
    application.run(move |cx| {
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
