//! Native keyboard labels shared by controls and their bindings.

use ui_text::text;

use gpui::{Keystroke, SharedString};
use gpui_component::kbd::Kbd;

/// Format the same GPUI shortcut syntax used by `KeyBinding::new`.
/// `secondary` resolves to Command on macOS and Control elsewhere.
pub fn shortcut_tooltip(label: impl Into<SharedString>, shortcut: &str) -> SharedString {
    shortcut_tooltip_with_alternatives(label, &[shortcut])
}

/// List alternative bindings for the same action inside one pair of parentheses.
/// Invalid shortcut hints are omitted so they cannot prevent a control rendering.
pub fn shortcut_tooltip_with_alternatives(label: impl Into<SharedString>, shortcuts: &[&str]) -> SharedString {
    let label = label.into();
    let keys = shortcuts
        .iter()
        .filter_map(|shortcut| match Keystroke::parse(shortcut) {
            Ok(keystroke) => Some(Kbd::format(&keystroke)),
            Err(error) => {
                log::warn!("Invalid tooltip shortcut: {error}");
                None
            }
        })
        .collect::<Vec<_>>()
        .join(" / ");
    if keys.is_empty() {
        return label;
    }
    format!(text!(common::shortcut_with_keys), keys = keys, label = label).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tooltips_use_native_modifiers_and_key_names() {
        let expected = if cfg!(target_os = "macos") {
            ["Search (⌘F)", "Contents (⌘T)", "Decrease font size (⌥-)", "Previous match (⇧⏎)", "Library (⌫)", "Close (⎋)"]
        } else {
            ["Search (Ctrl+F)", "Contents (Ctrl+T)", "Decrease font size (Alt+-)", "Previous match (Shift+Enter)", "Library (Backspace)", "Close (Esc)"]
        };
        for ((label, shortcut), expected) in
            [("Search", "secondary-f"), ("Contents", "secondary-t"), ("Decrease font size", "alt--"), ("Previous match", "shift-enter"), ("Library", "backspace"), ("Close", "escape")].into_iter().zip(expected)
        {
            assert_eq!(shortcut_tooltip(label, shortcut).as_ref(), expected);
        }
    }

    #[test]
    fn alternative_shortcuts_keep_native_labels_and_regular_text() {
        let expected = if cfg!(target_os = "macos") { "Previous match (⇧⏎ / ↑)" } else { "Previous match (Shift+Enter / Up)" };
        assert_eq!(shortcut_tooltip_with_alternatives("Previous match", &["shift-enter", "up"]).as_ref(), expected);
        assert_eq!(shortcut_tooltip_with_alternatives("Cancel", &[]).as_ref(), "Cancel");
    }

    #[test]
    fn invalid_shortcut_hints_do_not_prevent_tooltips_rendering() {
        assert!(Keystroke::parse("invalid-shortcut").is_err());
        assert_eq!(shortcut_tooltip("Search", "invalid-shortcut").as_ref(), "Search");
        let expected = shortcut_tooltip("Search", "secondary-f");
        assert_eq!(shortcut_tooltip_with_alternatives("Search", &["invalid-shortcut", "secondary-f"]).as_ref(), expected.as_ref());
    }
}
