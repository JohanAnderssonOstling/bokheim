//! The design-token file the web frontend builds its CSS variables from.
//!
//! Generated rather than hand-maintained. The palettes and layout constants
//! live in Rust; this renders them into the JSON shape `apps/mobile/vite.config.js`
//! already reads, so the two frontends cannot drift apart on colour the way
//! they had before — desktop borders at 3.2:1 against mobile's at 1.3:1.
//!
//! Only [`ApplicationPalette`]'s fields are authored. Shadow and scrim are
//! derived here the same way the GPUI bridge derives them, so they cannot
//! disagree, and the file no longer carries tokens nothing renders.

use std::fmt::Write as _;

use crate::frontend::{LARGE_BOOK_COVER_HEIGHT_PX, LARGE_BOOK_COVER_WIDTH_PX, LARGE_SIDEBAR_WIDTH_PX, LARGE_TOPBAR_ACTION_HEIGHT_PX, LARGE_TOPBAR_HEIGHT_PX};
use crate::preferences::{ApplicationPalette, ApplicationTheme, ResolvedAppearance};

/// Matches `ui-components`' scale. Kept here so one file describes the whole
/// token set to both frontends.
const SPACING: [(&str, u16); 4] = [("xxs", 2), ("xs", 4), ("sm", 8), ("md", 16)];
const RADIUS: [(&str, u16); 3] = [("sm", 8), ("md", 11), ("lg", 16)];
const TYPOGRAPHY: [(&str, u16); 6] = [("text_xs", 12), ("text_sm", 13), ("text_md", 15), ("text_lg", 17), ("title_md", 21), ("title_lg", 28)];

const CONTROL_SM_PX: u16 = 34;

fn hex(value: u32) -> String {
    format!("#{value:06x}")
}

/// `0xRRGGBBAA` as CSS `#rrggbbaa`, which browsers accept directly.
fn hex_with_alpha(value: u32) -> String {
    format!("#{value:08x}")
}

/// Same derivation as the GPUI theme bridge: the border colour at low alpha.
fn shadow(palette: ApplicationPalette) -> String {
    format!("{}40", hex(palette.border))
}

/// Same derivation as the GPUI theme bridge: the ink colour at half alpha.
fn scrim(palette: ApplicationPalette) -> String {
    format!("{}80", hex(palette.ink_text))
}

fn palette_json(palette: ApplicationPalette) -> String {
    let entries = [
        ("surface_bg", hex(palette.surface_bg)),
        ("raised_bg", hex(palette.raised_bg)),
        ("ink_text", hex(palette.ink_text)),
        ("subtle_text", hex(palette.subtle_text)),
        ("accent", hex(palette.accent)),
        ("border", hex(palette.border)),
        ("hover_overlay", hex_with_alpha(palette.hover_overlay)),
        ("active_highlight", hex(palette.active_highlight)),
        ("danger_text", hex(palette.danger_text)),
        ("danger_bg", hex(palette.danger_bg)),
        ("warning_text", hex(palette.warning_text)),
        ("warning_bg", hex(palette.warning_bg)),
        ("box_shadow", shadow(palette)),
        ("overlay_scrim", scrim(palette)),
    ];
    let body = entries.iter().map(|(name, value)| format!("        \"{name}\": \"{value}\"")).collect::<Vec<_>>().join(",\n");
    format!("{{\n{body}\n      }}")
}

fn theme_json(theme: ApplicationTheme) -> String {
    format!("{{\n      \"light\": {},\n      \"dark\": {}\n    }}", palette_json(theme.palette(ResolvedAppearance::Light)), palette_json(theme.palette(ResolvedAppearance::Dark)))
}

fn group(entries: &[(&str, u16)]) -> String {
    entries.iter().map(|(name, value)| format!("\"{name}\": {value}")).collect::<Vec<_>>().join(", ")
}

/// Renders the whole file, including its trailing newline.
pub fn render() -> String {
    let mut out = String::new();
    out.push_str("{\n");
    let _ = writeln!(out, "  \"generated\": \"by `cargo run -p app-contract --bin export_design_tokens`; edit client/app-contract instead\",");
    let _ = writeln!(out, "  \"spacing\": {{ {} }},", group(&SPACING));
    let _ = writeln!(out, "  \"radius\": {{ {} }},", group(&RADIUS));
    let _ = writeln!(
        out,
        "  \"size\": {{ \"control_sm\": {CONTROL_SM_PX}, \"control_md\": {LARGE_TOPBAR_ACTION_HEIGHT_PX}, \"nav_rail_width\": {LARGE_SIDEBAR_WIDTH_PX}, \"nav_item_height\": {LARGE_TOPBAR_HEIGHT_PX}, \"book_cover_width\": {LARGE_BOOK_COVER_WIDTH_PX}, \"book_cover_height\": {LARGE_BOOK_COVER_HEIGHT_PX} }},"
    );
    let _ = writeln!(out, "  \"typography\": {{ {} }},", group(&TYPOGRAPHY));
    let _ = writeln!(out, "  \"themes\": {{");
    let _ = writeln!(out, "    \"library_serif\": {},", theme_json(ApplicationTheme::LibrarySerif));
    let _ = writeln!(out, "    \"greco_futuristic\": {},", theme_json(ApplicationTheme::GrecoFuturistic));
    let _ = writeln!(out, "    \"classic_white\": {}", theme_json(ApplicationTheme::ClassicWhite));
    let _ = writeln!(out, "  }},");
    let _ = writeln!(out, "  \"font_families\": {{");
    let _ = writeln!(out, "    \"publisher\": \"\",");
    let _ = writeln!(out, "    \"serif\": \"Georgia, \\\"Times New Roman\\\", serif\",");
    let _ = writeln!(out, "    \"sans_serif\": \"Inter, system-ui, sans-serif\"");
    let _ = writeln!(out, "  }}");
    out.push_str("}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rendered_file_is_valid_json_with_both_appearances() {
        let value: serde_json::Value = serde_json::from_str(&render()).expect("the generated token file must parse");
        for theme in ["library_serif", "greco_futuristic"] {
            for appearance in ["light", "dark"] {
                let palette = &value["themes"][theme][appearance];
                assert!(palette["surface_bg"].is_string(), "{theme}/{appearance} is missing surface_bg");
                assert!(palette["raised_bg"].is_string(), "{theme}/{appearance} is missing raised_bg");
            }
        }
    }

    /// The tokens the web CSS no longer references. Leaving them in the file
    /// would keep implying they mean something.
    #[test]
    fn retired_tokens_are_gone() {
        let rendered = render();
        for retired in ["app_bg", "card_bg", "white_surface", "hover_bg", "border_color", "disabled_text", "disabled_border", "split_handle", "heading_text"] {
            assert!(!rendered.contains(retired), "{retired} should no longer appear in the generated tokens");
        }
    }

    #[test]
    fn cover_dimensions_match_the_layout_contract() {
        let value: serde_json::Value = serde_json::from_str(&render()).expect("parses");
        assert_eq!(value["size"]["book_cover_width"], u64::from(LARGE_BOOK_COVER_WIDTH_PX));
        assert_eq!(value["size"]["book_cover_height"], u64::from(LARGE_BOOK_COVER_HEIGHT_PX));
    }
}
