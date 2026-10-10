//! Windows desktop binding. Live files change only in the external startup helper.
use browser_ui::{UpdateAction, UpdateControls, UpdateView};
use gpui::{App, AppContext, Entity};
use linux_update_host::windows::{WindowsHost, WindowsStartup};
use std::{cell::RefCell, path::Path};
pub use linux_update_host::windows_process::{enter, Entry};
thread_local! {
    // Startup owns the host until healthy() commits; the worker then takes it.
    static HOST: RefCell<Option<WindowsHost>> = const { RefCell::new(None) };
}

fn config() -> Result<update_client::DiscoveryConfig, String> {
    let mut config = update_client::DiscoveryConfig::bundled()?;
    if let Ok(endpoint) = std::env::var("BOKHEIM_UPDATE_ENDPOINT") { config.endpoint = endpoint; }
    Ok(config)
}

pub fn helper_entry(data: &Path) -> Result<bool, String> {
    linux_update_host::windows_process::helper_entry(data, env!("CARGO_PKG_VERSION"), config)
}

/// False means the copied helper is ready and this process must exit.
pub fn initialize(data: &Path, trial: Option<&str>) -> Result<bool, String> {
    let config = match config() {
        Ok(config) => config,
        Err(error) => {
            if data.join("updates/activation.json").exists() || data.join("updates/active-taxonomy.json").exists() { return Err(error); }
            return Ok(true);
        }
    };
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let installation = executable.parent().ok_or("executable has no directory")?;
    let mut host = WindowsHost::open(data, installation, env!("CARGO_PKG_VERSION"), config)?;
    if matches!(host.startup(trial)?, WindowsStartup::Helper) {
        match linux_update_host::windows_process::spawn_helper(data, installation) {
            Ok(()) => return Ok(false),
            Err(error) => {
                let required = std::fs::metadata(&executable).map_err(|e| e.to_string())?.len();
                let available = fs2::available_space(data).map_err(|e| e.to_string())?;
                let reason = if required > available {
                    update_client::coordinator::WaitReason::Space { additional_bytes: required - available }
                } else { update_client::coordinator::WaitReason::Retry };
                host.helper_unavailable(reason, error)?;
                if !matches!(host.startup(None)?, WindowsStartup::Run) { return Err("update recovery requires its helper".into()); }
            }
        }
    }
    HOST.with_borrow_mut(|slot| {
        if slot.is_some() {
            return Err("Windows update host already initialized");
        }
        *slot = Some(host);
        Ok(())
    })?;
    Ok(true)
}

pub fn trial_pending() -> bool {
    HOST.with_borrow(|host| host.as_ref().is_some_and(WindowsHost::is_trial))
}
pub fn healthy() -> Result<(), String> {
    HOST.with_borrow_mut(|host| host.as_mut().map_or(Ok(()), WindowsHost::healthy))
}

pub fn bind(cx: &mut App) -> Option<Entity<UpdateControls>> {
    let restart = HOST.with_borrow(|host| host.as_ref().map(|host| host.installation.join("Bokheim.exe")))?;
    let (actions, requests) = std::sync::mpsc::channel();
    let (views, received) = async_channel::unbounded();
    let controls = cx.new(|_| {
        UpdateControls::new(
            UpdateView::Hidden,
            std::rc::Rc::new(move |action, cx| {
                if action == UpdateAction::Restart {
                    cx.set_restart_path(restart.clone());
                    cx.restart();
                } else {
                    let _ = actions.send(action);
                }
            }),
        )
    });
    let weak = controls.downgrade();
    let worker = crate::blocking_worker::BlockingWorker::new(cx.background_executor(), 1);
    // The startup callback commits healthy() before this task transfers ownership.
    cx.spawn(async move |_| {
        let Some(mut host) = HOST.with_borrow_mut(Option::take) else { return };
        let _ = worker
            .dispatch(move || {
                let mut action = None;
                loop {
                    let result = (|| {
                        if action.is_none() {
                            host.check_for_updates()?;
                        } else if action == Some(UpdateAction::Update) {
                            host.approve()?;
                        }
                        views.try_send(Ok(host.view())).map_err(|_| "update UI closed".to_owned())?;
                        host.prepare()
                    })();
                    match result {
                        Ok(view) => {
                            if views.try_send(Ok(view)).is_err() {
                                break;
                            }
                        }
                        Err(error) => {
                            log::warn!("update work failed: {error}");
                            if views.try_send(Ok(host.view())).is_err() || views.try_send(Err(())).is_err() {
                                break;
                            }
                        }
                    }
                    action = match requests.recv() {
                        Ok(action) => Some(action),
                        Err(_) => break,
                    };
                }
            })
            .recv()
            .await;
    })
    .detach();
    cx.spawn(async move |cx| {
        while let Ok(view) = received.recv().await {
            if weak
                .update(cx, |controls, cx| match view {
                    Ok(view) => controls.apply(view, cx),
                    Err(()) => controls.work_failed(cx),
                })
                .is_err()
            {
                break;
            }
        }
    })
    .detach();
    Some(controls)
}
