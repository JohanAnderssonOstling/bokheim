//! Shared shortcut catalog, live bindings and conflict checks for desktop and web.

use app_preferences::KeyboardShortcuts;
use gpui::{Action, App, DummyKeyboardMapper, Global, KeyBinding, KeyBindingMetaIndex, Keystroke, SharedString, Task};
use std::rc::Rc;

const SHORTCUT_BINDING: KeyBindingMetaIndex = KeyBindingMetaIndex(0x424b);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShortcutPage {
    Browse,
    Reader,
}

impl ShortcutPage {
    pub fn label(self) -> &'static str {
        match self {
            Self::Browse => "Browse",
            Self::Reader => "Reader",
        }
    }
}

#[derive(Clone)]
pub struct ShortcutSetting {
    pub id: &'static str,
    pub label: &'static str,
    pub page: ShortcutPage,
    pub defaults: Vec<String>,
    templates: Vec<KeyBinding>,
}

#[derive(Default)]
struct Shortcuts {
    settings: Vec<ShortcutSetting>,
    overrides: KeyboardShortcuts,
}

impl Global for Shortcuts {}

/// A failed save, worded by its classification.
pub type ShortcutSaveError = Box<dyn operation_failure::Classified>;

type SaveShortcuts = Rc<dyn Fn(KeyboardShortcuts, &mut App) -> Task<Result<(), ShortcutSaveError>>>;

struct ShortcutSaver(SaveShortcuts);
impl Global for ShortcutSaver {}

pub fn configure_shortcut_saving(save: impl Fn(KeyboardShortcuts, &mut App) -> Task<Result<(), ShortcutSaveError>> + 'static, cx: &mut App) {
    cx.set_global(ShortcutSaver(Rc::new(save)));
}

pub(crate) fn save_keyboard_shortcuts(shortcuts: KeyboardShortcuts, cx: &mut App) -> Task<Result<(), ShortcutSaveError>> {
    match cx.try_global::<ShortcutSaver>().map(|saver| saver.0.clone()) {
        Some(save) => save(shortcuts, cx),
        None => Task::ready(Err(Box::new(String::from("keyboard shortcut saving is not configured")))),
    }
}

pub(crate) fn keyboard_shortcut_overrides(cx: &App) -> KeyboardShortcuts {
    cx.try_global::<Shortcuts>().map(|shortcuts| shortcuts.overrides.clone()).unwrap_or_default()
}

/// Register each action once per context. Editable actions use one shortcut.
pub fn bind_shortcut<A: Action>(cx: &mut App, page: ShortcutPage, id: &'static str, label: &'static str, keys: &[&str], action: A, context: &'static str) {
    let template = KeyBinding::new(keys[0], action, Some(context));
    let registry = cx.default_global::<Shortcuts>();
    if let Some(setting) = registry.settings.iter_mut().find(|setting| setting.id == id) {
        // The document commands are registered for both EPUB and PDF.
        if !setting.templates.iter().any(|existing| existing.predicate() == template.predicate()) {
            setting.templates.push(template);
        }
    } else {
        registry.settings.push(ShortcutSetting { id, label, page, defaults: keys.iter().map(|key| (*key).to_owned()).collect(), templates: vec![template] });
    }
    rebuild_bindings(cx);
}

pub fn shortcut_settings(cx: &App) -> Vec<ShortcutSetting> {
    cx.try_global::<Shortcuts>().map(|registry| registry.settings.iter().filter(|setting| setting.defaults.iter().any(|key| Keystroke::parse(key).is_ok_and(|key| has_shortcut_modifier(&key)))).cloned().collect()).unwrap_or_default()
}

pub fn has_shortcut_modifier(key: &Keystroke) -> bool {
    key.modifiers.control || key.modifiers.platform || key.modifiers.alt || key.modifiers.shift
}

pub fn configured_shortcuts(setting: &ShortcutSetting, overrides: &KeyboardShortcuts) -> Vec<String> {
    let mut keys = overrides.get(setting.id).unwrap_or(&setting.defaults).clone();
    // Older profiles can contain alternatives. Every editable action now has
    // one assignment; ordinary document navigation keeps its fixed keys.
    if setting.defaults.iter().any(|key| Keystroke::parse(key).is_ok_and(|key| has_shortcut_modifier(&key))) {
        keys.truncate(1);
    }
    keys
}

/// Rebuild only our own bindings, preserving input, menu and platform bindings.
pub fn apply_keyboard_shortcuts(overrides: &KeyboardShortcuts, cx: &mut App) {
    cx.default_global::<Shortcuts>().overrides = overrides.clone();
    rebuild_bindings(cx);
}

fn rebuild_bindings(cx: &mut App) {
    let mut bindings: Vec<_> = cx.key_bindings().borrow().bindings().filter(|binding| binding.meta() != Some(SHORTCUT_BINDING)).cloned().collect();
    if let Some(registry) = cx.try_global::<Shortcuts>() {
        for setting in &registry.settings {
            for template in &setting.templates {
                for key in configured_shortcuts(setting, &registry.overrides) {
                    match KeyBinding::load(&key, template.action().boxed_clone(), template.predicate(), false, None, &DummyKeyboardMapper) {
                        Ok(binding) => bindings.push(binding.with_meta(SHORTCUT_BINDING)),
                        Err(error) => log::warn!("Ignoring invalid saved shortcut for {}: {error}", setting.id),
                    }
                }
            }
        }
    }
    cx.clear_key_bindings();
    cx.bind_keys(bindings);
}

/// Used by reader interceptors that must run before text fields consume a key.
pub fn matches_shortcut(id: &str, key: &Keystroke, cx: &App) -> bool {
    let Some(registry) = cx.try_global::<Shortcuts>() else { return false };
    let Some(setting) = registry.settings.iter().find(|setting| setting.id == id) else { return false };
    configured_shortcuts(setting, &registry.overrides)
        .iter()
        .any(|shortcut| KeyBinding::load(shortcut, setting.templates[0].action().boxed_clone(), None, false, None, &DummyKeyboardMapper).is_ok_and(|binding| binding.match_keystrokes(std::slice::from_ref(key)) == Some(false)))
}

/// Native labels and tooltips follow the same saved bindings as dispatch.
pub fn configured_shortcut_tooltip(label: impl Into<SharedString>, id: &str, cx: &App) -> SharedString {
    let keys = cx.try_global::<Shortcuts>().and_then(|registry| registry.settings.iter().find(|setting| setting.id == id).map(|setting| configured_shortcuts(setting, &registry.overrides))).unwrap_or_default();
    super::shortcut_tooltip_with_alternatives(label, &keys.iter().map(String::as_str).collect::<Vec<_>>())
}

/// Reject a duplicate within an action or a conflicting action on the same page.
/// Browse and Reader can safely reuse a key because they are separate pages.
pub fn shortcut_conflict(settings: &[ShortcutSetting], overrides: &KeyboardShortcuts, id: &str, replacing: Option<usize>, shortcut: &str) -> Result<(), String> {
    let proposed = Keystroke::parse(shortcut).map_err(|_| "Choose a valid keyboard shortcut.".to_owned())?;
    if !has_shortcut_modifier(&proposed) {
        return Err("Include Ctrl, Shift or Alt in the shortcut (Command on Mac).".into());
    }
    let setting = settings.iter().find(|setting| setting.id == id).ok_or("This shortcut is unavailable.")?;
    for other in settings.iter().filter(|other| other.page == setting.page) {
        for (index, key) in configured_shortcuts(other, overrides).iter().enumerate() {
            if other.id == id && replacing == Some(index) {
                continue;
            }
            if Keystroke::parse(key).is_ok_and(|existing| existing.key == proposed.key && existing.modifiers == proposed.modifiers) {
                return Err(if other.id == id {
                    format!("This shortcut is already assigned to {}.", other.label)
                } else {
                    format!("This shortcut is already assigned to {} in {}. Choose another shortcut or remove that assignment first.", other.label, other.page.label())
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{KeyContext, TestAppContext, actions};

    actions!(shortcut_settings_tests, [First, Second, Unrelated]);

    #[gpui::test]
    fn remapping_preserves_other_bindings_and_restores_defaults(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.bind_keys([KeyBinding::new("enter", Unrelated, Some("Input"))]);
            bind_shortcut(cx, ShortcutPage::Reader, "reader.first", "First", &["secondary-f", "shift-f3"], First, "Reader");
            bind_shortcut(cx, ShortcutPage::Reader, "reader.first", "First", &["secondary-f", "shift-f3"], First, "Document");
            let overrides = KeyboardShortcuts::from([("reader.first".into(), vec!["alt-k".into()])]);
            apply_keyboard_shortcuts(&overrides, cx);
            assert!(matches_shortcut("reader.first", &Keystroke::parse("alt-k").unwrap(), cx));
            assert!(!matches_shortcut("reader.first", &Keystroke::parse("secondary-f").unwrap(), cx));
            assert_eq!(cx.key_bindings().borrow().bindings().len(), 3);
            apply_keyboard_shortcuts(&KeyboardShortcuts::from([("reader.first".into(), vec![])]), cx);
            assert_eq!(cx.key_bindings().borrow().bindings().len(), 1);
            apply_keyboard_shortcuts(&KeyboardShortcuts::new(), cx);
            assert!(matches_shortcut("reader.first", &Keystroke::parse("secondary-f").unwrap(), cx));
            assert!(!matches_shortcut("reader.first", &Keystroke::parse("shift-f3").unwrap(), cx));
            let keymap = cx.key_bindings();
            assert_eq!(keymap.borrow().bindings().len(), 3);
            assert_eq!(keymap.borrow().bindings_for_input(&[Keystroke::parse("enter").unwrap()], &[KeyContext::parse("Input").unwrap()]).0.len(), 1);
        });
    }

    #[gpui::test]
    fn conflicts_resolve_platform_aliases_and_allow_keys_on_other_pages(cx: &mut TestAppContext) {
        cx.update(|cx| {
            bind_shortcut(cx, ShortcutPage::Reader, "reader.first", "First", &["secondary-f", "shift-f3"], First, "Reader");
            bind_shortcut(cx, ShortcutPage::Reader, "reader.second", "Second", &["secondary-t"], Second, "Reader");
            bind_shortcut(cx, ShortcutPage::Browse, "browse.first", "Browse first", &["secondary-f"], First, "Browser");
            let settings = shortcut_settings(cx);
            let overrides = KeyboardShortcuts::new();
            let primary_f = if cfg!(target_os = "macos") { "cmd-f" } else { "ctrl-f" };
            assert!(shortcut_conflict(&settings, &overrides, "reader.first", None, primary_f).unwrap_err().contains("already assigned to First"));
            assert!(shortcut_conflict(&settings, &overrides, "reader.first", Some(0), primary_f).is_ok());
            assert!(shortcut_conflict(&settings, &overrides, "reader.second", Some(0), primary_f).is_err());
            assert!(shortcut_conflict(&settings, &overrides, "browse.first", Some(0), "shift-f3").is_ok());
            assert!(shortcut_conflict(&settings, &overrides, "reader.first", None, "invalid-shortcut").is_err());
            assert!(shortcut_conflict(&settings, &overrides, "reader.first", None, "left").unwrap_err().contains("Include Ctrl"));
        });
    }

    #[gpui::test]
    fn settings_exclude_plain_navigation_but_keep_modified_shortcuts(cx: &mut TestAppContext) {
        cx.update(|cx| {
            bind_shortcut(cx, ShortcutPage::Reader, "reader.page", "Next page", &["right", "pagedown", "space"], First, "Reader");
            bind_shortcut(cx, ShortcutPage::Reader, "reader.tab", "Previous tab", &["shift-tab"], Second, "Reader");
            let settings = shortcut_settings(cx);
            assert_eq!(settings.len(), 1);
            assert_eq!(settings[0].id, "reader.tab");
            assert!(matches_shortcut("reader.page", &Keystroke::parse("right").unwrap(), cx));
        });
    }

    #[gpui::test]
    fn saved_shortcuts_apply_when_actions_register_after_preferences_load(cx: &mut TestAppContext) {
        cx.update(|cx| {
            apply_keyboard_shortcuts(&KeyboardShortcuts::from([("reader.first".into(), vec!["alt-k".into()])]), cx);
            bind_shortcut(cx, ShortcutPage::Reader, "reader.first", "First", &["secondary-f"], First, "Reader");
            assert!(matches_shortcut("reader.first", &Keystroke::parse("alt-k").unwrap(), cx));
            assert!(!matches_shortcut("reader.first", &Keystroke::parse("secondary-f").unwrap(), cx));
            assert!(configured_shortcut_tooltip("First", "reader.first", cx).contains('K'));
        });
    }
}
