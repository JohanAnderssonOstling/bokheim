use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

const APP_ICON_PATH: &str = "icons/bokheim.svg";
const APP_ICON: &[u8] = include_bytes!("../../../design-tokens/assets/icon/bokheim.svg");
const NAVIGATION_ICONS: [(&str, &[u8]); 10] = [
    ("icons/navigation/house.svg", include_bytes!("../../../design-tokens/assets/navigation/house.svg")),
    ("icons/navigation/book-open.svg", include_bytes!("../../../design-tokens/assets/navigation/book-open.svg")),
    ("icons/navigation/folder.svg", include_bytes!("../../../design-tokens/assets/navigation/folder.svg")),
    ("icons/navigation/asterisk.svg", include_bytes!("../../../design-tokens/assets/navigation/asterisk.svg")),
    ("icons/navigation/tags.svg", include_bytes!("../../../design-tokens/assets/navigation/tags.svg")),
    ("icons/navigation/user.svg", include_bytes!("../../../design-tokens/assets/navigation/user.svg")),
    ("icons/navigation/trash-2.svg", include_bytes!("../../../design-tokens/assets/navigation/trash-2.svg")),
    ("icons/navigation/building-2.svg", include_bytes!("../../../design-tokens/assets/navigation/building-2.svg")),
    ("icons/navigation/circle-user.svg", include_bytes!("../../../design-tokens/assets/navigation/circle-user.svg")),
    ("icons/navigation/settings.svg", include_bytes!("../../../design-tokens/assets/navigation/settings.svg")),
];

/// Bokheim assets layered over the standard gpui-component bundle.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = NAVIGATION_ICONS.iter().find(|(icon_path, _)| *icon_path == path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        if path == APP_ICON_PATH {
            return Ok(Some(Cow::Borrowed(APP_ICON)));
        }
        gpui_component_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut assets = gpui_component_assets::Assets.list(path)?;
        assets.extend(NAVIGATION_ICONS.iter().filter(|(icon_path, _)| icon_path.starts_with(path)).map(|(icon_path, _)| SharedString::from(*icon_path)));
        if APP_ICON_PATH.starts_with(path) {
            assets.push(APP_ICON_PATH.into());
        }
        Ok(assets)
    }
}
