//! Native startup ownership, configuration and binding to the shared UI worker.
use browser_ui::{UpdateAction, UpdateControls, UpdateEvent, UpdateView, bind_update_worker};
use gpui::{App, Entity};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
};
use update_client::DiscoveryConfig;

#[cfg(all(feature = "desktop", target_os = "linux"))]
type CurrentHost = linux_update_host::Host;
#[cfg(all(feature = "desktop", target_os = "windows"))]
type CurrentHost = linux_update_host::windows::WindowsHost;
#[cfg(all(feature = "android", target_os = "android"))]
type CurrentHost = linux_update_host::android::AndroidHost;

thread_local! {
    // Startup owns the host until healthy() commits; only then may the deferred
    // update worker take ownership and start network work.
    static HOST: RefCell<Option<CurrentHost>> = const { RefCell::new(None) };
}

pub(crate) fn discovery_config() -> Result<DiscoveryConfig, String> {
    let mut config = DiscoveryConfig::bundled()?;
    #[cfg(all(feature = "desktop", any(target_os = "linux", target_os = "windows")))]
    if let Ok(endpoint) = std::env::var("BOKHEIM_UPDATE_ENDPOINT") {
        config.endpoint = endpoint;
    }
    Ok(config)
}

pub(crate) fn startup_config(data: &Path) -> Result<Option<DiscoveryConfig>, String> {
    match discovery_config() {
        Ok(config) => Ok(Some(config)),
        Err(error) => {
            for file in ["activation.json", "active-taxonomy.json"] {
                if data.join("updates").join(file).try_exists().map_err(|e| e.to_string())? {
                    return Err(error);
                }
            }
            Ok(None)
        }
    }
}

pub(crate) fn store(host: CurrentHost) -> Result<(), String> {
    HOST.with_borrow_mut(|slot| {
        if slot.is_some() {
            return Err("update host already initialized".into());
        }
        *slot = Some(host);
        Ok(())
    })
}

pub(crate) fn take_host() -> Option<CurrentHost> {
    HOST.with_borrow_mut(Option::take)
}

pub fn healthy() -> Result<(), String> {
    HOST.with_borrow_mut(|host| host.as_mut().map_or(Ok(()), CurrentHost::healthy))
}

#[cfg(all(feature = "desktop", any(target_os = "linux", target_os = "windows")))]
pub fn trial_pending() -> bool {
    HOST.with_borrow(|host| host.as_ref().is_some_and(CurrentHost::is_trial))
}

pub(crate) fn bind(
    restart: Option<PathBuf>, mut platform_action: impl FnMut(&mut CurrentHost, UpdateAction) -> Result<(), String> + Send + 'static, mut present: impl FnMut(UpdateView) -> Result<UpdateView, String> + Send + 'static, cx: &mut App,
) -> Option<Entity<UpdateControls>> {
    if !HOST.with_borrow(|host| host.is_some()) {
        return None;
    }
    Some(bind_update_worker(
        UpdateView::Hidden,
        restart,
        true,
        take_host,
        move |host: &mut CurrentHost, action, publish| {
            match action {
                None | Some(UpdateAction::Check) => host.check_for_updates()?,
                Some(UpdateAction::Update) => host.approve()?,
                Some(action) => platform_action(host, action)?,
            }
            publish(UpdateEvent::View(host.view()))?;
            present(host.prepare()?)
        },
        CurrentHost::view,
        |_| None,
        cx,
    ))
}

#[cfg(all(feature = "desktop", any(target_os = "linux", target_os = "windows")))]
pub fn bind_desktop(cx: &mut App) -> Option<Entity<UpdateControls>> {
    let restart = HOST.with_borrow(|host| {
        host.as_ref().map(|host| {
            #[cfg(target_os = "linux")]
            {
                std::env::var_os("APPIMAGE").map(PathBuf::from).unwrap_or_else(|| host.executable.clone())
            }
            #[cfg(target_os = "windows")]
            {
                host.installation.join("Bokheim.exe")
            }
        })
    })?;
    bind(Some(restart), |_, _| Ok(()), Ok, cx)
}
