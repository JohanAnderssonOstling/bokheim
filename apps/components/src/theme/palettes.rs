//! Application palettes: each theme in a light and a dark appearance. Warm
//! dark is black and gold; Cool dark is blue-black with a steel-blue accent,
//! the navy of Cool light lifted until it reads on a dark surface.

use app_preferences::{ApplicationTheme, ResolvedAppearance};

use super::ApplicationPalette;

const DARK_SURFACE: u32 = 0x11100d;
const DARK_GOLD: u32 = 0xd3b271;
const COOL_DARK_SURFACE: u32 = 0x0f1216;
const COOL_DARK_BLUE: u32 = 0x8fb0dc;
const LIGHT_DANGER_TEXT: u32 = 0xa13029;
const LIGHT_DANGER_BG: u32 = 0xfde6e3;
const DARK_DANGER_TEXT: u32 = 0xdf8a83;
const DARK_DANGER_BG: u32 = 0x401511;

// Green, blue, purple, coral and teal. Dark appearances use brighter colours
// instead of lifting one accent until the last libraries all look white.
const USAGE_LIGHT: [u32; 5] = [0x466853, 0x3e6394, 0x795695, 0x9b514b, 0x287575];
const USAGE_DARK: [u32; 5] = [0x8ec7a2, 0x8fb0dc, 0xbd9bda, 0xe39e96, 0x70c5c4];

const fn over_channel(base: u32, layer: u32, alpha: u32, shift: u32) -> u32 {
    (((base >> shift) & 0xff) * (255 - alpha) + ((layer >> shift) & 0xff) * alpha + 127) / 255
}

/// Precompose a translucent layer because GPUI replaces, rather than layers,
/// a control's background on hover.
const fn over(base: u32, layer: u32, alpha: u32) -> u32 {
    (over_channel(base, layer, alpha, 16) << 16) | (over_channel(base, layer, alpha, 8) << 8) | over_channel(base, layer, alpha, 0)
}

pub const fn palette(theme: ApplicationTheme, appearance: ResolvedAppearance) -> ApplicationPalette {
    match (theme, appearance) {
        (ApplicationTheme::Warm, ResolvedAppearance::Light) => ApplicationPalette {
            surface_bg: 0xf6f2eb,
            raised_bg: 0xffffff,
            ink_text: 0x22201d,
            subtle_text: 0x6b675f,
            accent: 0x466853,
            text_accent: 0x8a6a2b,
            accent_hover: 0x3d5c49,
            accent_active: 0x34503f,
            accent_text: 0xfffdfa,
            border: 0x8c7f64,
            rule: 0xc5bdac,
            hover_overlay: (0x466853 << 8) | 0x14,
            active_highlight: 0xd9e8de,
            active_highlight_hover: 0xc7ddcf,
            danger_text: LIGHT_DANGER_TEXT,
            danger_bg: LIGHT_DANGER_BG,
            warning_text: 0x895c00,
            warning_bg: 0xfaeecf,
            usage_segments: USAGE_LIGHT,
        },
        (ApplicationTheme::Cool, ResolvedAppearance::Light) => ApplicationPalette {
            surface_bg: 0xeff3f9,
            raised_bg: 0xffffff,
            ink_text: 0x262627,
            subtle_text: 0x65686a,
            accent: 0x224a7f,
            text_accent: 0x8a6a2b,
            accent_hover: 0x1b3f6f,
            accent_active: 0x14355f,
            accent_text: 0xfdfdfe,
            border: 0x798189,
            rule: 0xb9bec4,
            hover_overlay: (0x224a7f << 8) | 0x14,
            active_highlight: 0xdbe4f1,
            active_highlight_hover: 0xcad8eb,
            danger_text: LIGHT_DANGER_TEXT,
            danger_bg: LIGHT_DANGER_BG,
            warning_text: 0x895c00,
            warning_bg: 0xfaeecf,
            usage_segments: USAGE_LIGHT,
        },
        (ApplicationTheme::Cool, ResolvedAppearance::Dark) => ApplicationPalette {
            surface_bg: COOL_DARK_SURFACE,
            raised_bg: 0x20252c,
            ink_text: 0xe3e8ef,
            subtle_text: 0x929aa5,
            accent: COOL_DARK_BLUE,
            text_accent: COOL_DARK_BLUE,
            accent_hover: over(COOL_DARK_BLUE, 0xffffff, 38),
            accent_active: over(COOL_DARK_BLUE, 0xffffff, 72),
            accent_text: COOL_DARK_SURFACE,
            border: over(COOL_DARK_SURFACE, COOL_DARK_BLUE, 136),
            rule: over(COOL_DARK_SURFACE, COOL_DARK_BLUE, 80),
            hover_overlay: (COOL_DARK_BLUE << 8) | 0x14,
            active_highlight: over(COOL_DARK_SURFACE, COOL_DARK_BLUE, 56),
            active_highlight_hover: over(COOL_DARK_SURFACE, COOL_DARK_BLUE, 72),
            danger_text: DARK_DANGER_TEXT,
            danger_bg: DARK_DANGER_BG,
            warning_text: 0xd4a444,
            warning_bg: 0x362c10,
            usage_segments: USAGE_DARK,
        },
        (ApplicationTheme::Warm, ResolvedAppearance::Dark) => ApplicationPalette {
            surface_bg: DARK_SURFACE,
            raised_bg: 0x2a2720,
            ink_text: 0xefe6d0,
            subtle_text: 0xa69c86,
            accent: DARK_GOLD,
            text_accent: DARK_GOLD,
            accent_hover: over(DARK_GOLD, 0xffffff, 38),
            accent_active: over(DARK_GOLD, 0xffffff, 72),
            accent_text: DARK_SURFACE,
            border: over(DARK_SURFACE, DARK_GOLD, 136),
            rule: over(DARK_SURFACE, DARK_GOLD, 80),
            hover_overlay: (DARK_GOLD << 8) | 0x14,
            active_highlight: over(DARK_SURFACE, DARK_GOLD, 56),
            active_highlight_hover: over(DARK_SURFACE, DARK_GOLD, 72),
            danger_text: DARK_DANGER_TEXT,
            danger_bg: DARK_DANGER_BG,
            warning_text: 0xd4a444,
            warning_bg: 0x362c10,
            usage_segments: USAGE_DARK,
        },
    }
}
