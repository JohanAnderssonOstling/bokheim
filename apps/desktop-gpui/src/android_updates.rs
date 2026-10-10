//! Android startup and GPUI binding. Downloads never invoke the package installer.
use browser_ui::{UpdateAction, UpdateControls, UpdateView};
use gpui::{App, AppContext, Entity};
use linux_update_host::android::AndroidHost;
use std::{cell::RefCell, path::Path, sync::OnceLock};

type Bridge = dyn Fn(&str) -> Result<String, String> + Send + Sync;
static BRIDGE: OnceLock<Box<Bridge>> = OnceLock::new();
thread_local! {
    // Startup owns the host until healthy() commits; the worker then takes it.
    static HOST: RefCell<Option<AndroidHost>> = const { RefCell::new(None) };
}

fn command(value: serde_json::Value) -> Result<String, String> {
    BRIDGE.get().ok_or("Android update bridge is unavailable")?(&value.to_string())
}

pub fn initialize(data: &Path, bridge: impl Fn(&str) -> Result<String, String> + Send + Sync + 'static) -> Result<bool, String> {
    BRIDGE.set(Box::new(bridge)).map_err(|_| "Android update bridge already initialized")?;
    let config = match update_client::DiscoveryConfig::bundled() {
        Ok(config) => config,
        Err(error) => {
            if data.join("updates/activation.json").exists() || data.join("updates/active-taxonomy.json").exists() {
                return Err(error);
            }
            return Ok(false);
        }
    };
    let mut host = AndroidHost::open(data, env!("CARGO_PKG_VERSION"), config, &format!("android-{}-apk", std::env::consts::ARCH))?;
    let trial = host.startup(|path| command(serde_json::json!({ "action": "prepare", "path": path })).map(|_| ()))?;
    HOST.with_borrow_mut(|slot| {
        if slot.is_some() {
            return Err("Android update host already initialized");
        }
        *slot = Some(host);
        Ok(())
    })?;
    Ok(trial)
}

pub fn healthy() -> Result<(), String> {
    HOST.with_borrow_mut(|host| host.as_mut().map_or(Ok(()), AndroidHost::healthy))
}

pub fn bind(cx: &mut App) -> Option<Entity<UpdateControls>> {
    if !HOST.with_borrow(|host| host.is_some()) {
        return None;
    }
    let (actions, requests) = std::sync::mpsc::channel();
    let (views, received) = async_channel::unbounded();
    let controls = cx.new(|_| {
        UpdateControls::new(
            UpdateView::Hidden,
            std::rc::Rc::new(move |action, _| {
                let _ = actions.send(action);
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
                        match action {
                            Some(UpdateAction::Update) => {
                                host.approve()?;
                            }
                            Some(UpdateAction::Install) => {
                                let mut request = serde_json::to_value(host.install_request()?).map_err(|e| e.to_string())?;
                                request["action"] = "install".into();
                                if command(request)? == "invalidPackage" {
                                    host.reject_package()?;
                                }
                            }
                            Some(UpdateAction::GrantPermission) => {
                                if command(serde_json::json!({"action":"permission"}))? != "true" {
                                    command(serde_json::json!({"action":"grantPermission"}))?;
                                }
                            }
                            Some(UpdateAction::Restart) => {
                                command(serde_json::json!({"action":"restart"}))?;
                            }
                            None => host.check_for_updates()?,
                        }
                        // Show availability or approval before the blocking download.
                        views.try_send(Ok(host.view())).map_err(|_| "update UI closed".to_owned())?;
                        let mut view = host.prepare()?;
                        if view == (UpdateView::Ready { action: UpdateAction::Install }) && command(serde_json::json!({"action":"permission"}))? != "true" {
                            view = UpdateView::NeedsPermission;
                        }
                        Ok::<_, String>(view)
                    })();
                    match result {
                        Ok(view) => {
                            if views.try_send(Ok(view)).is_err() {
                                break;
                            }
                        }
                        Err(error) => {
                            log::warn!("Android update work failed: {error}");
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
