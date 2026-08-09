use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

const HOME_ICON_PATH: &str = "icons/bokheim-home.svg";
const HOME_ICON: &[u8] = include_bytes!("../assets/icons/bokheim-home.svg");

/// Bokheim assets layered over the standard gpui-component bundle.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path == HOME_ICON_PATH {
            return Ok(Some(Cow::Borrowed(HOME_ICON)));
        }
        gpui_component_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut assets = gpui_component_assets::Assets.list(path)?;
        if HOME_ICON_PATH.starts_with(path) {
            assets.push(HOME_ICON_PATH.into());
        }
        Ok(assets)
    }
}
