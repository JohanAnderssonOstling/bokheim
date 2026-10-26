//! Android startup and GPUI binding. Downloads never invoke the package installer.
pub use crate::update_binding::healthy;
use browser_ui::{UpdateAction, UpdateControls, UpdateView};
use gpui::{App, Entity};
use linux_update_host::android::AndroidHost;
use std::{path::Path, sync::OnceLock};

type Bridge = dyn Fn(&str) -> Result<String, String> + Send + Sync;
static BRIDGE: OnceLock<Box<Bridge>> = OnceLock::new();

fn command(value: serde_json::Value) -> Result<String, String> {
    BRIDGE.get().ok_or("Android update bridge is unavailable")?(&value.to_string())
}

pub fn initialize(data: &Path, bridge: impl Fn(&str) -> Result<String, String> + Send + Sync + 'static) -> Result<bool, String> {
    BRIDGE.set(Box::new(bridge)).map_err(|_| "Android update bridge already initialized")?;
    let Some(config) = crate::update_binding::startup_config(data)? else {
        return Ok(false);
    };
    let (host, trial) = AndroidHost::open_and_startup(data, env!("CARGO_PKG_VERSION"), config, &format!("android-{}-apk", std::env::consts::ARCH), |path| command(serde_json::json!({ "action": "prepare", "path": path })).map(|_| ()))?;
    crate::update_binding::store(host)?;
    Ok(trial)
}

pub fn bind(cx: &mut App) -> Option<Entity<UpdateControls>> {
    crate::update_binding::bind(
        None,
        |host: &mut AndroidHost, action| {
            match action {
                UpdateAction::Install => {
                    let mut request = serde_json::to_value(host.install_request()?).map_err(|error| error.to_string())?;
                    request["action"] = "install".into();
                    if command(request)? == "invalidPackage" {
                        host.reject_package()?;
                    }
                }
                UpdateAction::GrantPermission => {
                    if command(serde_json::json!({"action":"permission"}))? != "true" {
                        command(serde_json::json!({"action":"grantPermission"}))?;
                    }
                }
                UpdateAction::Restart => {
                    command(serde_json::json!({"action":"restart"}))?;
                }
                _ => {}
            }
            Ok(())
        },
        |mut view| {
            if view == (UpdateView::Ready { action: UpdateAction::Install }) && command(serde_json::json!({"action":"permission"}))? != "true" {
                view = UpdateView::NeedsPermission;
            }
            Ok(view)
        },
        cx,
    )
}
