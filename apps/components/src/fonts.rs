//! Interface and book fonts shared by every application platform.

use std::borrow::Cow;

use app_preferences::UiFontFamily;
use gpui::{App, Global, Result};

/// A bundled face, also available to native browser DOM controls.
pub struct InterfaceFontFace {
    pub data: &'static [u8],
    pub weight: u16,
    pub style: &'static str,
}

const GARAMOND: [InterfaceFontFace; 4] = [
    InterfaceFontFace { data: include_bytes!("../../desktop-gpui/assets/fonts/NV_Garamond-Regular.ttf"), weight: 400, style: "normal" },
    InterfaceFontFace { data: include_bytes!("../../desktop-gpui/assets/fonts/NV_Garamond-Bold.ttf"), weight: 700, style: "normal" },
    InterfaceFontFace { data: include_bytes!("../../desktop-gpui/assets/fonts/NV_Garamond-Italic.ttf"), weight: 400, style: "italic" },
    InterfaceFontFace { data: include_bytes!("../../desktop-gpui/assets/fonts/NV_Garamond-BoldItalic.ttf"), weight: 700, style: "italic" },
];
const RALEWAY: [InterfaceFontFace; 4] = [
    InterfaceFontFace { data: include_bytes!("../../desktop-gpui/assets/fonts/Raleway-Regular.ttf"), weight: 400, style: "normal" },
    InterfaceFontFace { data: include_bytes!("../../desktop-gpui/assets/fonts/Raleway-Bold.ttf"), weight: 700, style: "normal" },
    InterfaceFontFace { data: include_bytes!("../../desktop-gpui/assets/fonts/Raleway-Italic.ttf"), weight: 400, style: "italic" },
    InterfaceFontFace { data: include_bytes!("../../desktop-gpui/assets/fonts/Raleway-BoldItalic.ttf"), weight: 700, style: "italic" },
];

pub fn interface_font_faces(family: UiFontFamily) -> &'static [InterfaceFontFace] {
    match family {
        UiFontFamily::Serif => &GARAMOND,
        UiFontFamily::SansSerif => &RALEWAY,
    }
}

struct FontsRegistered;

impl Global for FontsRegistered {}

/// A host-provided catalog of installed families, without GPUI's synthetic
/// fallback aliases. Android supplies this after registering device fonts.
pub struct SystemFontFamilies(pub Vec<String>);

impl Global for SystemFontFamilies {}

/// Registers the interface, book and serif display faces once per application.
pub fn register_fonts(cx: &mut App) -> Result<()> {
    if cx.has_global::<FontsRegistered>() {
        return Ok(());
    }
    let mut fonts: Vec<Cow<'static, [u8]>> = GARAMOND.iter().chain(RALEWAY.iter()).map(|face| Cow::Borrowed(face.data)).collect();
    fonts.extend([Cow::Borrowed(include_bytes!("../../desktop-gpui/assets/fonts/BodoniModa-Regular.ttf").as_slice()), Cow::Borrowed(include_bytes!("../../desktop-gpui/assets/fonts/BodoniModa-SemiBold.ttf").as_slice())]);
    cx.text_system().add_fonts(fonts)?;
    cx.set_global(FontsRegistered);
    Ok(())
}
