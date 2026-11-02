//! Reader-style row navigation for application preferences. Embedded forms
//! retain Tab navigation and their own text-editing and popup key bindings.

#[cfg(feature = "kobo")]
use ui_text::text;

use super::*;
use crate::{AdjustSettingValue, MoveSettingRow};
use gpui::{App, Modifiers};
#[cfg(feature = "kobo")]
use kobo_device::settings::{AUTO_POWER_OFF_MINUTES, AUTO_SLEEP_MINUTES, idle_timeout, power_off_timeout, set_auto_power_off_minutes, set_auto_sleep_minutes};

impl SettingsPage {
    fn focused_section(&self, window: &Window, cx: &App) -> Option<Section> {
        self.visible_sections.borrow().iter().copied().find(|id| self.sections[*id as usize].focus.contains_focused(window, cx))
    }

    pub(super) fn mark_focused_section(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.selected_section.set(self.focused_section(window, cx));
        self.keyboard_visible.set(true);
        self.reveal_focused_section(window, cx);
        cx.notify();
    }

    pub(super) fn move_setting_row(&mut self, action: &MoveSettingRow, window: &mut Window, cx: &mut Context<Self>) {
        let mut rows = self.visible_sections.borrow().clone();
        if rows.is_empty() {
            if action.0 < 0 {
                self.focus_previous(&FocusPreviousSetting, window, cx);
            } else {
                self.focus_next(&FocusNextSetting, window, cx);
            }
            return;
        }
        rows.sort_by_key(|id| *id as usize);
        let Some(first) = rows.first().copied() else { return };
        let focused = self.focused_section(window, cx);
        let selected = focused.and_then(|current| rows.iter().position(|id| *id == current)).map(|index| rows[(index as i32 + action.0).clamp(0, rows.len() as i32 - 1) as usize]).unwrap_or(first);
        self.selected_section.set(Some(selected));
        self.keyboard_visible.set(true);
        self.sections[selected as usize].focus.focus(window, cx);
        self.reveal_focused_section(window, cx);
        cx.notify();
    }

    /// Enter moves into an action group without invoking its first command.
    /// This keeps account, dictionary and library operations explicit.
    pub(super) fn on_setting_row_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.modifiers != Modifiers::default() || !matches!(event.keystroke.key.as_str(), "enter" | "space") {
            return;
        }
        let Some(id) = self.focused_section(window, cx) else { return };
        if self.sections[id as usize].focus.is_focused(window) {
            window.focus_next(cx);
            if !self.sections[id as usize].focus.contains_focused(window, cx) {
                self.sections[id as usize].focus.focus(window, cx);
            }
            self.mark_focused_section(window, cx);
            cx.stop_propagation();
        }
    }

    pub(super) fn adjust_setting_value(&mut self, action: &AdjustSettingValue, window: &mut Window, cx: &mut Context<Self>) {
        if self.visible_sections.borrow().is_empty() {
            cx.propagate();
            return;
        }
        let Some(id) = self.focused_section(window, cx) else {
            if self.focus.is_focused(window) {
                self.move_setting_row(&MoveSettingRow(0), window, cx);
            } else {
                cx.propagate();
            }
            return;
        };
        self.selected_section.set(Some(id));
        self.keyboard_visible.set(true);
        cx.notify();
        let increase = action.0 > 0;
        let preferences = self.preferences.read(cx).browsing().clone();
        let patch = match id {
            #[cfg(not(feature = "kobo"))]
            Section::Theme => {
                let choices = components::THEME_CHOICES;
                let current = cx.try_global::<components::theme::SelectedPalette>().map(|value| (value.0, value.1));
                let index = choices.iter().position(|value| Some(*value) == current).unwrap_or(0);
                let next = (index as i32 + action.0).clamp(0, choices.len() as i32 - 1) as usize;
                (next != index).then(|| BrowsingPreferencePatch::Palette(choices[next].0, choices[next].1))
            }
            Section::CoverText => {
                let value = if increase { CoverText::CoverOnly } else { CoverText::Always };
                (value != preferences.cover_text()).then_some(BrowsingPreferencePatch::CoverText(value))
            }
            Section::FontFamily => {
                if increase && (preferences.custom_ui_font_active() || preferences.ui_font_family() == UiFontFamily::SansSerif) {
                    if let Some(picker) = &self.font_picker {
                        picker.update(cx, |picker, cx| picker.show(window, cx));
                    }
                    cx.notify();
                    return;
                }
                let value = if increase || preferences.custom_ui_font_active() { UiFontFamily::SansSerif } else { UiFontFamily::Serif };
                (preferences.custom_ui_font_active() || value != preferences.ui_font_family()).then_some(BrowsingPreferencePatch::UiFontFamily(value))
            }
            Section::FontSize => {
                let current = preferences.ui_font_size_px();
                let next = if increase { current.saturating_add(1).min(UI_FONT_SIZE_MAX) } else { current.saturating_sub(1).max(UI_FONT_SIZE_MIN) };
                (next != current).then_some(BrowsingPreferencePatch::UiFontSizePx(next))
            }
            #[cfg(not(feature = "kobo"))]
            Section::Animations => (increase != preferences.animations()).then_some(BrowsingPreferencePatch::Animations(increase)),
            #[cfg(not(feature = "kobo"))]
            Section::Transfers => (increase != preferences.allow_metered_transfers()).then_some(BrowsingPreferencePatch::AllowMeteredTransfers(increase)),
            Section::TextSearch => (increase != preferences.text_search()).then_some(BrowsingPreferencePatch::TextSearch(increase)),
            #[cfg(feature = "kobo")]
            Section::GeneratedCovers => (increase != preferences.generated_covers_only()).then_some(BrowsingPreferencePatch::GeneratedCoversOnly(increase)),
            #[cfg(feature = "kobo")]
            Section::KoboAutoSleep | Section::KoboAutoPowerOff => {
                let sleep = id == Section::KoboAutoSleep;
                let choices = if sleep { &AUTO_SLEEP_MINUTES[..] } else { &AUTO_POWER_OFF_MINUTES[..] };
                let timeout = if sleep { idle_timeout() } else { power_off_timeout() };
                let current = timeout.map_or(0, |value| value.as_secs() / 60);
                let index = choices.iter().position(|minutes| u64::from(*minutes) == current).unwrap_or(0);
                let next = (index as i32 + action.0).clamp(0, choices.len() as i32 - 1) as usize;
                if next != index {
                    let result = if sleep { set_auto_sleep_minutes(choices[next]) } else { set_auto_power_off_minutes(choices[next]) };
                    if sleep {
                        self.auto_sleep_error = result.err().map(|error| format!(text!(settings::automatic_sleep_wasn_t_saved), kobo_setting_recovery(&error)).into());
                    } else {
                        self.auto_power_off_error = result.err().map(|error| format!(text!(settings::automatic_power_off_wasn_t_saved), kobo_setting_recovery(&error)).into());
                    }
                }
                None
            }
            _ => {
                cx.propagate();
                return;
            }
        };
        if let Some(patch) = patch {
            self.update_browsing(patch, cx);
        }
        self.reveal_focused_section(window, cx);
        cx.notify();
    }
}
