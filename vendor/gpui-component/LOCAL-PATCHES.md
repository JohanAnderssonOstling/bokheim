# Local widget patches

Source: https://github.com/longbridge/gpui-component
Revision: 88f102d13654fe25aa2fede076274b6b751a3704
License: Apache-2.0 (LICENSE-APACHE included).

Only the UI, assets, and macros crates are included; workspace members are narrowed accordingly. The nested workspace uses the same pinned GPUI fork revision as the main workspace so Cargo resolves one GPUI crate. The unused standalone psm Git override is removed.

- Input and OTP carets remain visible and stop scheduling blink/pause timers when GPUI reports reduced motion.
- Switches skip the animation and its completion timer under reduced motion.
- Scrollbars bypass timer-driven fading under reduced motion, including per-widget auto-hide overrides.
- `Button`'s `Ghost` variant reads the themed `button_hover`/`button_active` tokens for its hover and pressed fills, the same tokens `Default` already reads, instead of deriving an untheme-able tint from `secondary`. Every other variant already honored a theme's hover colors; `Ghost` silently didn't.
- Disabled button colors and the default cursor apply after caller styling, so custom foregrounds, fills, and pointer cursors cannot make unavailable controls look enabled. Disabled selected controls retain their selection at half opacity. Caller geometry and enabled styling remain intact.
- Context menus share an `OpenContextMenu` action bound to Ctrl+Enter, Shift+F10, and the Menu key. Generic menus and input clipboard menus reuse their right-click implementations and restore focus after dismissal; keyboard input menus keep the text selection.
- Popup menus can keep multi-select filter choices open after selection; Escape and outside clicks still dismiss them. Ordinary action menus retain their default dismissal behavior.
- Popup menus and application bottom sheets share `menu::choice_row` and its selection indicator: a solid accent fill with contrasting text and a trailing checkmark. Hover and keyboard focus use a soft highlight; disabled rows are dimmed, and checked rows retain their solid fill when hovered. Popup checks default to the right so leading action icons remain visible; explicit check-side overrides remain available.
- Dialog backdrops retain their window dimensions when rendered from a narrow pane. Centered surfaces participate in flex layout; explicit top offsets use absolute positioning. The shared dialog layer paints at deferred priority 4, above the library switcher and its menus, so confirmations and update prompts remain visible and receive clicks.
- Root text and compact widget labels use the local GPUI fork's `line_height_auto()` to measure font metrics and glyph ink, including descenders and fallback fonts. Checkbox labels keep intrinsic height instead of filling their wrapper. Text buttons retain their normal minimum height and use spare vertical space before growing; explicit paragraph line heights still override intrinsic sizing.

Kobo forces reduced motion through its GPUI platform capability. Other platforms retain the existing policy.

## Bokheim component subset

This vendored copy is intentionally tailored to Bokheim; these removals do not change the upstream repository or the shared GPUI fork.

- Removed unused chart/plot, table, docking, calendar/date-picker, color-picker, combobox/select, resizable, searchable-list, settings, accordion, avatar, badge, collapsible, description-list, form, group-box, hover-card, pagination, radio, rating, sidebar, status-bar, stepper, tab, and tag modules and their initialization hooks.
- Kept the dropdown button caret as a private button helper and the circular-progress arc as a private progress helper, so retained widgets do not depend on the removed select/plot subsystems. Removed the now-unused `IntoPlot` derive macro.
- Removed LSP providers, completion/code-action/hover/diagnostic overlays, semantic tokens, document colors, and the component inspector. Input uses a local UTF-16 `Position` type instead of `lsp-types`; ordinary editing, selection, and clipboard menus remain. Syntax parsing and highlighting were subsequently removed as described below.
- Removed theme-directory watching and reloading, its `notify` dependency, and the unused `decimal` feature/dependency. Bundled JSON themes and palettes are translated into typed Rust during the build, including the schema's fallback order. The `json-themes` feature retains loading additional themes from JSON; Bokheim leaves it disabled.
- Retained shared input, menu, focus, scrolling, and theme infrastructure. Rich-text rendering was subsequently replaced by selectable literal text as described below.
- Removed unused private list focus/reset helpers, the table-specific virtual-list scroll-handle builder, and scrollbar FPS/width helpers.

The public module count falls from 57 to 29. The desktop and browser normal dependency graphs each lose five external package versions.

GPUI's broad `image-formats` default is disabled in both workspace dependency declarations. Other GPUI defaults remain explicitly enabled. Bokheim's `ui-components` manifest configures the shared image decoder for PNG, JPEG, GIF, WebP, BMP, and TIFF and retains SVG raster-image support through resvg, including on the browser target. This avoids enabling AVIF encoding and EXR through GPUI without modifying the shared GPUI fork.

The component text view now displays literal text through `TextView::plain` and `TextViewState::plain`. GPUI shapes and wraps the text; the existing window selection controller retains drag selection, double/triple click, cross-view selection, modal scopes, resize invalidation, select-all, and clipboard support. Metadata no longer escapes text or passes through an HTML parser. Reader and book-description HTML still use Bokheim's separate HTML engine.

- Custom popup rows can report keyboard selection and handle Left/Right before submenu navigation, allowing split-action rows to remain keyboard accessible. Context-menu wrappers accept a separate keyboard builder for selection-based actions. Linux client title bars expose their window menu through the shared context-menu action.

- Trees use GPUI’s variable-height list so wrapped TOC titles retain the same vertical padding as single-line entries. Visible-entry rebuilds invalidate measured heights and retain the scroll position; keyboard navigation and the scrollbar use the same list state.

- Notifications can keep their close button visible for touch use. Browser toasts opt in, and their notification type can be cleared immediately when the browser closes without removing unrelated application notifications.
- Notification widths are capped to the viewport minus both horizontal margins, and their text can shrink and wrap inside that width so right-anchored toasts stay on-screen on phones.
- The component root exposes explicit state-based dismissal for active menus, popovers, dialogs, and sheets. The app calls it from its native Back handler without keyboard dispatch. Cancellation remains window-local and honors dialog vetoes; deferred modal cancellation verifies that the same overlay is still active. Android Back has no component key bindings; Escape retains normal keyboard dismissal.

- Context menus listen for GPUI's explicit long-press event as well as desktop right-click. Reader capture handlers can consume the hold before the generic menu opens.

- Text inputs preview mobile page swipes live and settle from the gesture’s original offset, so release does not add the drag distance a second time. Cancelled swipes restore that offset and stay within the input.
- Window text selection stops when pointer movement reports that the left mouse button is released, including after a consumed release or a release outside the window.
- Popup menus use touch-sized rows when GPUI reports a touch primary input, including native mobile and Kobo.
- Inputs report their full control bounds to platform text handlers, including padding, while retaining text bounds for caret and IME candidate positioning.
- `ListItem` draws no hover of its own, and trees no longer mark their items selected or secondary-selected. The tree's only caller, the reader's contents sidebar, draws hover and the keyboard cursor in its row from the `selected` flag `render_item` receives; the item's own fills laid a second hover under the row and left a washed, outlined selection behind every click.

- Popovers, dropdowns, and context menus use centered overlays on touch platforms, including Kobo; native touch menus also use larger rows. Scrollable menus keep their intrinsic height up to the safe viewport edges instead of the previous half-window/450 px limit.

- Password reveal controls are persistent toggles with keyboard focus and Show/Hide tooltips. The input state owns visibility; setting its mask does not require a window.

- Removed the component HTML/Markdown parsers, minifier, rich-text AST/rendering, inline images, code blocks, extension callbacks, and background parsing channels. Their direct `markdown`, `html5ever`, `markup5ever_rcdom`, `chrono`, `futures`, `async-channel`, and `smol` dependencies are removed. Plain text is available synchronously for first layout and copying.
- Removed code-editor mode, syntax highlighters, language registries, grammar/query files, incremental/background syntax parsing, syntax-derived folding, syntax theme configuration, and all Tree-sitter features/dependencies. Normal single-line, multiline, auto-grow, undo/redo, IME, selection, and explicit search/decorations remain. Soft-wrapping and display-coordinate infrastructure stay shared with ordinary inputs.
- Input find/replace panels, their actions and key bindings, search-match painting, and the direct `aho-corasick` dependency require the `input-search` feature. Bokheim leaves it disabled; ordinary editing, replacement APIs, selection, and IME remain available.
