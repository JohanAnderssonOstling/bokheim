use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

const APP_ICON_PATH: &str = "icons/bokheim.svg";
const APP_ICON: &[u8] = include_bytes!("../../../design-tokens/assets/icon/bokheim.svg");
const PHOSPHOR_LIGHT_ICONS: [(&str, &[u8]); 37] = [
    ("icons/arrow-left.svg", include_bytes!("../../../design-tokens/assets/phosphor-light/arrow-left.svg")),
    ("icons/arrow-right.svg", include_bytes!("../../../design-tokens/assets/phosphor-light/arrow-right.svg")),
    ("icons/book-open.svg", include_bytes!("../../../design-tokens/assets/phosphor-light/book-open.svg")),
    ("icons/check.svg", include_bytes!("../../../design-tokens/assets/phosphor-light/check.svg")),
    ("icons/chevron-down.svg", include_bytes!("../../../design-tokens/assets/phosphor-light/caret-down.svg")),
    ("icons/chevron-left.svg", include_bytes!("../../../design-tokens/assets/phosphor-light/caret-left.svg")),
    ("icons/chevron-right.svg", include_bytes!("../../../design-tokens/assets/phosphor-light/caret-right.svg")),
    ("icons/circle-x.svg", include_bytes!("../../../design-tokens/assets/phosphor-light/x-circle.svg")),
    ("icons/close.svg", include_bytes!("../../../design-tokens/assets/phosphor-light/x.svg")),
    ("icons/delete.svg", include_bytes!("../../../design-tokens/assets/phosphor-light/trash.svg")),
    ("icons/external-link.svg", include_bytes!("../../../design-tokens/assets/phosphor-light/arrow-square-out.svg")),
    ("icons/folder.svg", include_bytes!("../../../design-tokens/assets/phosphor-light/folder.svg")),
    ("icons/folder-open.svg", include_bytes!("../../../design-tokens/assets/phosphor-light/folder-open.svg")),
    ("icons/globe.svg", include_bytes!("../../../design-tokens/assets/phosphor-light/globe.svg")),
    ("icons/panel-left-open.svg", include_bytes!("../../../design-tokens/assets/phosphor-light/sidebar-simple.svg")),
    ("icons/panel-right-open.svg", include_bytes!("../../../design-tokens/assets/phosphor-light/sidebar-simple-right.svg")),
    ("icons/pause.svg", include_bytes!("../../../design-tokens/assets/phosphor-light/pause.svg")),
    ("icons/play.svg", include_bytes!("../../../design-tokens/assets/phosphor-light/play.svg")),
    ("icons/plus.svg", include_bytes!("../../../design-tokens/assets/phosphor-light/plus.svg")),
    ("icons/redo-2.svg", include_bytes!("../../../design-tokens/assets/phosphor-light/arrow-clockwise.svg")),
    ("icons/search.svg", include_bytes!("../../../design-tokens/assets/phosphor-light/magnifying-glass.svg")),
    ("icons/settings.svg", include_bytes!("../../../design-tokens/assets/phosphor-light/gear.svg")),
    ("icons/settings-2.svg", include_bytes!("../../../design-tokens/assets/phosphor-light/sliders-horizontal.svg")),
    ("icons/star.svg", include_bytes!("../../../design-tokens/assets/phosphor-light/star.svg")),
    ("icons/undo-2.svg", include_bytes!("../../../design-tokens/assets/phosphor-light/arrow-counter-clockwise.svg")),
    ("icons/user.svg", include_bytes!("../../../design-tokens/assets/phosphor-light/user.svg")),
    ("icons/window-close.svg", include_bytes!("../../../design-tokens/assets/phosphor-light/x.svg")),
    ("icons/navigation/house.svg", include_bytes!("../../../design-tokens/assets/phosphor-light/house.svg")),
    ("icons/navigation/book-open.svg", include_bytes!("../../../design-tokens/assets/phosphor-light/book-open.svg")),
    ("icons/navigation/folder.svg", include_bytes!("../../../design-tokens/assets/phosphor-light/folder.svg")),
    ("icons/navigation/asterisk.svg", include_bytes!("../../../design-tokens/assets/phosphor-light/asterisk.svg")),
    ("icons/navigation/tags.svg", include_bytes!("../../../design-tokens/assets/phosphor-light/tag.svg")),
    ("icons/navigation/user.svg", include_bytes!("../../../design-tokens/assets/phosphor-light/user.svg")),
    ("icons/navigation/trash-2.svg", include_bytes!("../../../design-tokens/assets/phosphor-light/trash.svg")),
    ("icons/navigation/building-2.svg", include_bytes!("../../../design-tokens/assets/phosphor-light/buildings.svg")),
    ("icons/navigation/circle-user.svg", include_bytes!("../../../design-tokens/assets/phosphor-light/user-circle.svg")),
    ("icons/navigation/settings.svg", include_bytes!("../../../design-tokens/assets/phosphor-light/gear.svg")),
];

/// Bokheim assets layered over the standard gpui-component bundle.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = PHOSPHOR_LIGHT_ICONS.iter().find(|(icon_path, _)| *icon_path == path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        if path == APP_ICON_PATH {
            return Ok(Some(Cow::Borrowed(APP_ICON)));
        }
        gpui_component_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut assets = gpui_component_assets::Assets.list(path)?;
        assets.extend(PHOSPHOR_LIGHT_ICONS.iter().filter(|(icon_path, _)| icon_path.starts_with(path)).map(|(icon_path, _)| SharedString::from(*icon_path)));
        if APP_ICON_PATH.starts_with(path) {
            assets.push(APP_ICON_PATH.into());
        }
        Ok(assets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn phosphor_overrides_are_unique_valid_light_svgs() {
        let mut paths = HashSet::new();
        for (path, bytes) in PHOSPHOR_LIGHT_ICONS {
            assert!(paths.insert(path), "duplicate icon override: {path}");
            let svg = std::str::from_utf8(bytes).expect("Phosphor SVG must be UTF-8");
            assert!(svg.starts_with("<svg"), "invalid SVG override: {path}");
            assert!(svg.contains("viewBox=\"0 0 256 256\""), "unexpected Phosphor viewBox: {path}");
            assert!(svg.contains("currentColor"), "icon must inherit the GPUI text color: {path}");
        }
    }
}
