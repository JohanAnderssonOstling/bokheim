# Local widget patches

Source: https://github.com/longbridge/gpui-component
Revision: 88f102d13654fe25aa2fede076274b6b751a3704
License: Apache-2.0 (LICENSE-APACHE included).

Only the UI, assets, and macros crates are included; workspace members are narrowed accordingly. The nested workspace uses the same pinned GPUI fork revision as the main workspace so Cargo resolves one GPUI crate. The unused standalone psm Git override is removed.

- Input and OTP carets remain visible and stop scheduling blink/pause timers when GPUI reports reduced motion.
- Switches skip the animation and its completion timer under reduced motion.
- Scrollbars bypass timer-driven fading under reduced motion, including per-widget auto-hide overrides.
- `Button`'s `Ghost` variant reads the themed `button_hover`/`button_active` tokens for its hover and pressed fills, the same tokens `Default` already reads, instead of deriving an untheme-able tint from `secondary`. Every other variant already honored a theme's hover colors; `Ghost` silently didn't.

Kobo forces reduced motion through its GPUI platform capability. Other platforms retain the existing policy.

## Bokheim component subset

This vendored copy is intentionally tailored to Bokheim; these removals do not change the upstream repository or the shared GPUI fork.

- Removed unused chart/plot, table, docking, calendar/date-picker, color-picker, combobox/select, resizable, searchable-list, settings, accordion, avatar, badge, collapsible, description-list, form, group-box, hover-card, pagination, radio, rating, sidebar, status-bar, stepper, tab, and tag modules and their initialization hooks.
- Kept the dropdown button caret as a private button helper and the circular-progress arc as a private progress helper, so retained widgets do not depend on the removed select/plot subsystems. Removed the now-unused `IntoPlot` derive macro.
- Removed LSP providers, completion/code-action/hover/diagnostic overlays, semantic tokens, document colors, and the component inspector. Input uses a local UTF-16 `Position` type instead of `lsp-types`; ordinary editing, selection, clipboard menus, and syntax highlighting remain.
- Removed theme-directory watching and reloading, its `notify` dependency, and the unused `decimal` feature/dependency. The theme registry and bundled JSON defaults remain because Bokheim still uses their fallback colors.
- Retained rich-text/Markdown rendering and shared input, menu, focus, scrolling, and theme infrastructure. The fenced-code-block layout test now uses flex children instead of removed resizable panels.

The public module count falls from 57 to 30. The desktop and browser normal dependency graphs each lose five external package versions.

GPUI's broad `image-formats` default is disabled in both workspace dependency declarations. Other GPUI defaults remain explicitly enabled. Bokheim's `ui-components` manifest configures the shared image decoder for PNG, JPEG, GIF, WebP, BMP, and TIFF and retains SVG raster-image support through resvg, including on the browser target. This avoids enabling AVIF encoding and EXR through GPUI without modifying the shared GPUI fork.

The rich-text HTML parser uses `html5ever` 0.39 and the matching `markup5ever_rcdom` 0.39 adapter, sharing the parser generation already used by the reader. HTML text views and embedded HTML in Markdown remain supported; no renderer code changes were required.
