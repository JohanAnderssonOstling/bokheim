//! Native keyboard labels shared by controls and their bindings.

use gpui::{Keystroke, Modifiers, SharedString};
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
    format!("{label} ({keys})").into()
}

/// Match exactly Command on macOS or Control on other platforms.
pub fn primary_modifier_only(modifiers: Modifiers) -> bool {
    modifiers.secondary() && modifiers.number_of_modifiers() == 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_modifier_matches_native_bindings_without_extra_modifiers() {
        let primary = Keystroke::parse("secondary-f").unwrap().modifiers;
        assert!(primary_modifier_only(primary));
        assert!(!primary_modifier_only(Modifiers::default()));
        assert_eq!(primary_modifier_only(Keystroke::parse("cmd-f").unwrap().modifiers), cfg!(target_os = "macos"));
        assert_eq!(primary_modifier_only(Keystroke::parse("ctrl-f").unwrap().modifiers), !cfg!(target_os = "macos"));
        for modifiers in [Modifiers { shift: true, ..primary }, Modifiers { alt: true, ..primary }, Modifiers { function: true, ..primary }, Modifiers { control: true, platform: true, ..primary }] {
            assert!(!primary_modifier_only(modifiers));
        }
    }

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
