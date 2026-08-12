use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

const HOME_ICON_PATH: &str = "icons/bokheim-home.svg";
const HOME_ICON: &[u8] = include_bytes!("../assets/icons/bokheim-home.svg");
const TRASH_ICON_PATH: &str = "icons/bokheim-trash.svg";
const TRASH_ICON: &[u8] = include_bytes!("../assets/icons/bokheim-trash.svg");
const APP_ICON_PATH: &str = "icons/bokheim.svg";
const APP_ICON: &[u8] = include_bytes!("../../../design-tokens/assets/icon/bokheim.svg");

/// Bokheim assets layered over the standard gpui-component bundle.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path == HOME_ICON_PATH {
            return Ok(Some(Cow::Borrowed(HOME_ICON)));
        }
        if path == TRASH_ICON_PATH {
            return Ok(Some(Cow::Borrowed(TRASH_ICON)));
        }
        if path == APP_ICON_PATH {
            return Ok(Some(Cow::Borrowed(APP_ICON)));
        }
        gpui_component_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut assets = gpui_component_assets::Assets.list(path)?;
        if HOME_ICON_PATH.starts_with(path) {
            assets.push(HOME_ICON_PATH.into());
        }
        if TRASH_ICON_PATH.starts_with(path) {
            assets.push(TRASH_ICON_PATH.into());
        }
        if APP_ICON_PATH.starts_with(path) {
            assets.push(APP_ICON_PATH.into());
        }
        Ok(assets)
    }
}
