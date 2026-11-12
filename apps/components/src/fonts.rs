//! Interface and book fonts shared by every application platform.
mod bundled;
mod catalog;
pub use catalog::{FontCatalog, FontCatalogTarget, load_font_catalog, observe_font_catalog};

#[cfg(any(target_arch = "wasm32", target_os = "android"))]
use std::borrow::Cow;
use std::{cell::RefCell, rc::Rc};

use bundled::FAMILIES;
use operation_failure::OperationError;

use app_preferences::{BrowsingPreferences, UiFontFamily};
use gpui::{App, AsyncApp, Global, Pixels, Result, TextRun, TextSystem, Window, WindowTextSystem, px};
use gpui_component::Theme;

/// Font registration requested by a consumer of bundled application fonts.
#[derive(Clone, Copy, Debug)]
pub enum BundledFontRequest<'a> {
    Family(&'a str),
    TextFallbacks(&'a str),
}

pub type BundledFontLoader = Rc<dyn Fn(&WindowTextSystem, BundledFontRequest<'_>) -> bool>;

#[cfg(target_arch = "wasm32")]
/// A bundled face, also available to native browser DOM controls.
pub struct InterfaceFontFace {
    pub data: &'static [u8],
    pub weight: u16,
    pub style: &'static str,
}

#[cfg(target_arch = "wasm32")]
const GARAMOND: [InterfaceFontFace; 4] = [
    InterfaceFontFace { data: include_bytes!("../../desktop-gpui/assets/fonts/NV_Garamond-Regular.ttf"), weight: 400, style: "normal" },
    InterfaceFontFace { data: include_bytes!("../../desktop-gpui/assets/fonts/NV_Garamond-Bold.ttf"), weight: 700, style: "normal" },
    InterfaceFontFace { data: include_bytes!("../../desktop-gpui/assets/fonts/NV_Garamond-Italic.ttf"), weight: 400, style: "italic" },
    InterfaceFontFace { data: include_bytes!("../../desktop-gpui/assets/fonts/NV_Garamond-BoldItalic.ttf"), weight: 700, style: "italic" },
];
#[cfg(target_arch = "wasm32")]
const RALEWAY: [InterfaceFontFace; 4] = [
    InterfaceFontFace { data: include_bytes!("../../desktop-gpui/assets/fonts/Raleway-Regular.ttf"), weight: 400, style: "normal" },
    InterfaceFontFace { data: include_bytes!("../../desktop-gpui/assets/fonts/Raleway-Bold.ttf"), weight: 700, style: "normal" },
    InterfaceFontFace { data: include_bytes!("../../desktop-gpui/assets/fonts/Raleway-Italic.ttf"), weight: 400, style: "italic" },
    InterfaceFontFace { data: include_bytes!("../../desktop-gpui/assets/fonts/Raleway-BoldItalic.ttf"), weight: 700, style: "italic" },
];

#[cfg(target_arch = "wasm32")]
pub fn interface_font_faces(family: UiFontFamily) -> &'static [InterfaceFontFace] {
    match family {
        UiFontFamily::Serif => &GARAMOND,
        UiFontFamily::SansSerif => &RALEWAY,
    }
}

/// Avoid resolving a saved Android font before background registration finishes:
/// GPUI caches failed font lookups as well as successful ones.
pub fn configured_ui_font_name<'a>(preferences: &'a BrowsingPreferences, cx: &App) -> &'a str {
    #[cfg(target_os = "android")]
    if let Some(family) = preferences.custom_ui_font_family()
        && cx.try_global::<SystemFontFamilies>().is_some_and(|fonts| !fonts.0.iter().any(|available| available == family))
    {
        return preferences.ui_font_family().family_name();
    }
    #[cfg(not(target_os = "android"))]
    let _ = cx;
    preferences.ui_font_name()
}

/// Measure interface labels with the active face, including its real glyph advances.
pub fn interface_label_width(label: &str, size_rem: f32, window: &Window, cx: &App) -> Pixels {
    let mut font = window.text_style().font();
    font.family = Theme::global(cx).font_family.clone();
    let run = TextRun { len: label.len(), font, ..Default::default() };
    let line = window.text_system().shape_line(label.to_owned().into(), window.rem_size() * size_rem, &[run], None);
    px(f32::from(line.width).ceil())
}

/// Space for equal-width choices without clipping their labels. Insets are the
/// total horizontal padding and borders within each choice.
pub fn equal_choice_group_width(labels: &[&str], size_rem: f32, inset: f32, gap: f32, window: &Window, cx: &App) -> Pixels {
    let widest = labels.iter().map(|label| interface_label_width(label, size_rem, window, cx)).max().unwrap_or(px(0.0));
    (widest + px(inset)) * labels.len() as f32 + px(gap * labels.len().saturating_sub(1) as f32)
}

type FontSource = Rc<dyn Fn(&str, &TextSystem) -> Result<(), OperationError>>;

struct BundledFonts {
    loaded: [bool; FAMILIES.len()],
    source: FontSource,
    interface_family: Option<String>,
}

impl BundledFonts {
    fn register(&mut self, family: &str, text_system: &TextSystem) -> Result<bool> {
        let Some(index) = FAMILIES.iter().position(|entry| entry.name.eq_ignore_ascii_case(family)) else { return Ok(false) };
        if self.loaded[index] {
            return Ok(false);
        }
        for name in FAMILIES[index].files {
            (self.source)(name, text_system).map_err(std::io::Error::other)?;
        }
        if FAMILIES[index].name == CJK_FONT_FAMILY {
            text_system.set_fallback_font(CJK_FONT_FAMILY)?;
        }
        self.loaded[index] = true;
        Ok(true)
    }
}

struct RegisteredBundledFonts(Rc<RefCell<BundledFonts>>);
impl Global for RegisteredBundledFonts {}

fn bundled_fonts(cx: &mut App) -> Rc<RefCell<BundledFonts>> {
    if let Some(fonts) = cx.try_global::<RegisteredBundledFonts>() {
        return fonts.0.clone();
    }
    let fonts = Rc::new(RefCell::new(BundledFonts { loaded: [false; FAMILIES.len()], source: Rc::new(register_bundled_font), interface_family: None }));
    cx.set_global(RegisteredBundledFonts(fonts.clone()));
    fonts
}

/// Android reads a requested face directly from APK assets; no eager extraction.
#[cfg(target_os = "android")]
pub fn configure_bundled_font_source(source: impl Fn(&str) -> Result<Cow<'static, [u8]>, OperationError> + 'static, cx: &mut App) {
    bundled_fonts(cx).borrow_mut().source = Rc::new(move |name, text_system| text_system.add_fonts(vec![source(name)?]).map_err(OperationError::operation));
}

/// A host-provided catalog of installed families, without GPUI's synthetic
/// fallback aliases. Android supplies this after registering device fonts.
pub struct SystemFontFamilies(pub Vec<String>);
impl Global for SystemFontFamilies {}

/// Install only the default interface family and its display face. Hosts with
/// saved preferences use `register_interface_fonts` through `apply_appearance`.
pub fn register_fonts(cx: &mut App) -> Result<()> {
    register_interface_fonts(&BrowsingPreferences::default(), cx)
}

pub fn register_interface_fonts(preferences: &BrowsingPreferences, cx: &mut App) -> Result<()> {
    let fonts = bundled_fonts(cx);
    let mut fonts = fonts.borrow_mut();
    // Android has no synchronously loaded system faces at startup. Its native
    // generic font must exist even when widgets request .SystemUIFont.
    #[cfg(target_os = "android")]
    fonts.register("Noto Sans", cx.text_system())?;
    let family = configured_ui_font_name(preferences, cx);
    fonts.register(family, cx.text_system())?;
    if !preferences.custom_ui_font_active() && preferences.ui_font_family() == UiFontFamily::Serif {
        fonts.register(crate::DISPLAY_FONT_FAMILY, cx.text_system())?;
    }
    if fonts.interface_family.as_deref() != Some(family) {
        // Old previews and the previous UI face can be reloaded by their stable IDs.
        cx.text_system().release_font_data();
        fonts.interface_family = Some(family.to_owned());
    }
    Ok(())
}

fn register_picker_fonts(cx: &mut App) -> Result<()> {
    let fonts = bundled_fonts(cx);
    let mut fonts = fonts.borrow_mut();
    for family in &FAMILIES[..3] {
        fonts.register(family.name, cx.text_system())?;
    }
    Ok(())
}

/// Registration policy for consumers resolving a family or shaping text.
/// State contains registration flags, never an additional copy of font bytes.
pub fn bundled_font_loader(cx: &mut App) -> BundledFontLoader {
    loader_for_fonts(bundled_fonts(cx))
}

fn loader_for_fonts(fonts: Rc<RefCell<BundledFonts>>) -> BundledFontLoader {
    Rc::new(
        move |text_system: &WindowTextSystem, request: BundledFontRequest<'_>| {
            let mut fonts = fonts.borrow_mut();
            match request {
                BundledFontRequest::Family(family) => {
                    let known = FAMILIES
                        .iter()
                        .any(|entry| entry.name.eq_ignore_ascii_case(family));
                    if !known {
                        return false;
                    }
                    match fonts.register(family, text_system) {
                        Ok(_) => true,
                        Err(error) => {
                            log::error!("failed to install bundled font {family}: {error}");
                            false
                        }
                    }
                }
                BundledFontRequest::TextFallbacks(text) => {
                    #[cfg(not(target_arch = "wasm32"))]
                    if !fonts.loaded[3]
                        && !text.is_ascii()
                        && text.chars().any(html_style_model::is_cjk_character)
                    {
                        return match fonts.register(CJK_FONT_FAMILY, text_system) {
                            Ok(loaded) => loaded,
                            Err(error) => {
                                log::error!("failed to install CJK glyph fallback: {error}");
                                false
                            }
                        };
                    }
                    let _ = text;
                    false
                }
            }
        },
    )
}

/// Load a bundled family before an explicit native UI preview or cover uses it.
pub fn ensure_bundled_font(family: &str, cx: &App) {
    if let Some(fonts) = cx.try_global::<RegisteredBundledFonts>() {
        if let Err(error) = fonts.0.borrow_mut().register(family, cx.text_system()) {
            log::error!("failed to install bundled font {family}: {error}");
        }
    }
}

/// Register the fallback before native UI text is measured, including book
/// titles displayed without opening the book. Does not infer a book language.
pub fn ensure_text_fonts(text: &str, cx: &App) {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(fonts) = cx.try_global::<RegisteredBundledFonts>() {
        let mut fonts = fonts.0.borrow_mut();
        if !fonts.loaded[3] && !text.is_ascii() && text.chars().any(html_style_model::is_cjk_character) {
            if let Err(error) = fonts.register(CJK_FONT_FAMILY, cx.text_system()) {
                log::error!("failed to install CJK glyph fallback: {error}");
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (text, cx);
}

/// Compact common CJK coverage; users can select an installed regional face.
pub const CJK_FONT_FAMILY: &str = "Droid Sans Fallback";

/// Explicit preload for hosts that cannot synchronously read a font at shaping
/// time (the web). Native readers use the content-triggered loader instead.
pub async fn ensure_cjk_fonts(cx: &mut AsyncApp) -> Result<(), OperationError> {
    cx.wait_for_update_slot().await;
    let loaded = cx.update(|cx| bundled_fonts(cx).borrow().loaded[3]);
    if loaded {
        return Ok(());
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        cx.update(|cx| {
            let fonts = bundled_fonts(cx);
            fonts.borrow_mut().register(CJK_FONT_FAMILY, cx.text_system()).map(|_| ()).map_err(OperationError::operation)
        })
    }
    #[cfg(target_arch = "wasm32")]
    {
        let bytes = load_cjk_font().await?;
        cx.wait_for_update_slot().await;
        cx.update(move |cx| {
            let fonts = bundled_fonts(cx);
            let mut fonts = fonts.borrow_mut();
            if !fonts.loaded[3] {
                cx.text_system().add_fonts(vec![bytes]).map_err(OperationError::operation)?;
                cx.text_system().set_fallback_font(CJK_FONT_FAMILY).map_err(OperationError::operation)?;
                fonts.loaded[3] = true;
            }
            Ok(())
        })
    }
}

#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
fn register_bundled_font(name: &str, text_system: &TextSystem) -> Result<(), OperationError> {
    let executable = std::env::current_exe().map_err(|error| OperationError::operation(error).context("locate bundled fonts"))?;
    let directory = executable.parent().ok_or("executable has no parent directory")?;
    let directory = if matches!(directory.file_name().and_then(|name| name.to_str()), Some("deps" | "examples")) { directory.parent().unwrap_or(directory) } else { directory };
    let path = directory.join("fonts").join(name);
    text_system.add_font_file(&path).map_err(|error| OperationError::operation(error).context(format!("register bundled font {}", crate::path_label(&path))))
}

#[cfg(any(target_arch = "wasm32", target_os = "android"))]
fn register_bundled_font(name: &str, text_system: &TextSystem) -> Result<(), OperationError> {
    text_system.add_fonts(vec![load_bundled_font(name)?]).map_err(OperationError::operation)
}

#[cfg(target_os = "android")]
fn load_bundled_font(_name: &str) -> Result<Cow<'static, [u8]>, OperationError> {
    Err("Android bundled font source is not configured".into())
}

#[cfg(target_arch = "wasm32")]
fn load_bundled_font(name: &str) -> Result<Cow<'static, [u8]>, OperationError> {
    for (family, files) in [(UiFontFamily::Serif, FAMILIES[0].files), (UiFontFamily::SansSerif, FAMILIES[1].files)] {
        if let Some(index) = files.iter().position(|file| *file == name) {
            return Ok(Cow::Borrowed(interface_font_faces(family)[index].data));
        }
    }
    match name {
        "BodoniModa-Regular.ttf" => Ok(Cow::Borrowed(include_bytes!("../../desktop-gpui/assets/fonts/BodoniModa-Regular.ttf"))),
        "BodoniModa-SemiBold.ttf" => Ok(Cow::Borrowed(include_bytes!("../../desktop-gpui/assets/fonts/BodoniModa-SemiBold.ttf"))),
        _ => Err(format!("unknown bundled font: {name}").into()),
    }
}

#[cfg(target_arch = "wasm32")]
async fn load_cjk_font() -> Result<Cow<'static, [u8]>, OperationError> {
    use js_sys::{Reflect, global};
    use wasm_bindgen::JsValue;

    let url = Reflect::get(&global(), &JsValue::from_str("__BOKHEIM_CJK_FONT_URL__")).ok().and_then(|value| value.as_string()).ok_or("bundled CJK font URL is unavailable")?;
    html_fonts::load_remote_font(&url).await.map(Cow::Owned).map_err(Into::into)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod cjk_tests {
    use super::*;

    #[gpui::test]
    async fn cjk_registration_is_available_to_ui_and_idempotent(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            cx.set_global(SystemFontFamilies(vec!["Latin font".into()]));
            register_fonts(cx).unwrap();
            assert!(!bundled_fonts(cx).borrow().loaded[3]);
        });
        let mut async_cx = cx.to_async();
        ensure_cjk_fonts(&mut async_cx).await.unwrap();
        ensure_cjk_fonts(&mut async_cx).await.unwrap();
        cx.update(|cx| {
            assert!(bundled_fonts(cx).borrow().loaded[3]);
            assert_eq!(cx.global::<SystemFontFamilies>().0, vec!["Latin font"]);
        });
    }
}
