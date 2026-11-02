//! Account, appearance and library presentation settings.
//!
//! Preference edits use the shared [`Preferences`] store.
//!
//! The account and its sync state are the first section rather than a
//! destination of their own: signing in is something you do once, and the
//! things it governs — quota, which libraries sync — are settings like any
//! other. [`SyncPage`] keeps owning that state and is embedded here.
//!
//! The page is reachable without a pointer. It is a tab group, so arriving here
//! puts focus on the first control rather than somewhere in the window, Tab and
//! the arrow keys walk the controls in the order they are drawn, and focus
//! stays inside the page instead of escaping into the navigation rail, which
//! has no way back. Moving focus scrolls the section it landed in into view.

mod font_picker;
mod keyboard;
#[cfg(feature = "kobo")]
mod kobo_updates;

use ui_components::user_facing_error;

use crate::services::{UpdateControls, format_storage_bytes};
use std::cell::Cell;
use std::rc::Rc;

use app_preferences::{BrowsingPreferencePatch, CoverText, UI_FONT_SIZE_MAX, UI_FONT_SIZE_MIN, UiFontFamily};
use gpui::prelude::*;
use gpui::{Bounds, Context, Entity, FocusHandle, Focusable, IntoElement, KeyDownEvent, Pixels, Render, ScrollHandle, SharedString, Window, div, px};
#[cfg(feature = "kobo")]
use gpui_component::WindowExt;
use ui_components as components;
use ui_components::Button;

use crate::stores::Preferences;
use crate::universal::sync::{LibrariesView, SyncPage};
use crate::{FocusNextSetting, FocusPreviousSetting};

#[cfg(feature = "kobo")]
fn kobo_setting_recovery(error: &std::io::Error) -> &'static str {
    log::warn!("Could not save Kobo power setting: {error}");
    match app::Failure::from_io(error.kind()) {
        Some(app::Failure::StorageFull) => "Not enough storage to save settings. Free up device space and retry.",
        Some(app::Failure::ReadOnlyStorage) => "Device storage is read-only. It must be writable to save settings.",
        Some(app::Failure::PermissionDenied) => "Access to the settings file or folder was denied. Check their permissions.",
        _ => "Restart the app and retry. If it persists, check that device storage is available and writable.",
    }
}

/// The key context the page's focus movement is bound in. A context of its own,
/// because Tab means "next control" only here.
pub(crate) const SETTINGS_KEY_CONTEXT: &str = "BrowserSettings";

/// Room left around a section brought into view, so a control never sits
/// against the edge of the surface it was scrolled into.
const REVEAL_MARGIN: f32 = 16.0;

/// An upper bound on the walk that wraps focus back to the last control. The
/// page has a few dozen controls at most; the bound only keeps a malformed tab
/// order from spinning.
const MAX_CONTROLS: usize = 512;

/// The blocks the page scrolls between, in the order it draws them. Focus moves
/// control by control, but the page scrolls section by section: a section is the
/// unit a reader recognises, and its heading is what makes the control below it
/// make sense.
///
/// Appearance leads: it is what most visits come to change, and the account
/// led with a storage figure that nobody opened the page to read. The account
/// and the libraries follow together, since that is what both are about. The
/// libraries come last: that block grows with every library on the device and
/// with whatever is syncing, and nothing should sit underneath it.
#[derive(Clone, Copy)]
#[repr(usize)]
enum Section {
    Updates,
    #[cfg(not(feature = "kobo"))]
    Theme,
    CoverText,
    #[cfg(feature = "kobo")]
    GeneratedCovers,
    FontFamily,
    FontSize,
    #[cfg(not(feature = "kobo"))]
    Animations,
    #[cfg(feature = "kobo")]
    KoboDevice,
    Dictionaries,
    #[cfg(not(feature = "kobo"))]
    Transfers,
    TextSearch,
    Account,
    #[cfg(target_os = "android")]
    LibraryStorage,
    Libraries,
}

const SECTION_COUNT: usize = Section::Libraries as usize + 1;

/// Where a section can be found: which focus it contains, and where it was last
/// laid out.
struct SettingsSection {
    focus: FocusHandle,
    bounds: Rc<Cell<Bounds<Pixels>>>,
}

pub(crate) struct SettingsPage {
    dictionaries: Entity<dictionary_settings::DictionarySettings>,
    keyboard: Entity<components::KeyboardShortcutEditor>,
    keyboard_tab: bool,
    font_picker: Option<Entity<font_picker::FontPicker>>,
    font_choice_focus: [FocusHandle; 2],
    updates: Option<Entity<UpdateControls>>,
    _update_changes: Option<gpui::Subscription>,
    _preference_changes: gpui::Subscription,
    preferences: Entity<Preferences>,
    account: Entity<SyncPage>,
    libraries: Entity<LibrariesView>,
    /// The last preference write that failed, and which setting it was. It is
    /// drawn inside the group that setting lives in: one alert at the foot of
    /// the page reported a theme that would not save from below everything
    /// else on it.
    error: Option<SharedString>,
    #[cfg(not(feature = "kobo"))]
    transfer_save_error: Option<SharedString>,
    text_search_error: Option<SharedString>,
    index_usage: Option<SharedString>,
    index_usage_request: Option<gpui::Task<()>>,
    #[cfg(feature = "kobo")]
    uninstall_error: Option<SharedString>,
    #[cfg(feature = "kobo")]
    auto_sleep_error: Option<SharedString>,
    #[cfg(feature = "kobo")]
    auto_power_off_error: Option<SharedString>,
    #[cfg(feature = "kobo")]
    kobo_updates: Entity<UpdateControls>,
    #[cfg(feature = "kobo")]
    _kobo_update_changes: gpui::Subscription,
    focus: FocusHandle,
    scroll: ScrollHandle,
    sections: [SettingsSection; SECTION_COUNT],
}

impl SettingsPage {
    pub(crate) fn new(updates: Option<Entity<UpdateControls>>, account: Entity<SyncPage>, preferences: Entity<Preferences>, cx: &mut Context<Self>) -> Self {
        let update_changes = updates.as_ref().map(|updates| cx.observe(updates, |_, _, cx| cx.notify()));
        let preference_changes = cx.observe(&preferences, |_, _, cx| cx.notify());
        let libraries = cx.new(|cx| LibrariesView::new(account.clone(), cx));
        let backend = preferences.read(cx).backend();
        let dictionaries = cx.new(|cx| dictionary_settings::DictionarySettings::new(backend, Vec::new(), cx));
        let sections = std::array::from_fn(|_| SettingsSection { focus: cx.focus_handle(), bounds: Rc::new(Cell::new(Bounds::default())) });
        let keyboard = cx.new(|cx| components::KeyboardShortcutEditor::new(components::ShortcutPage::Browse, cx));
        #[cfg(feature = "kobo")]
        let kobo_updates = kobo_updates::bind(cx);
        #[cfg(feature = "kobo")]
        let kobo_update_changes = cx.observe(&kobo_updates, |_, _, cx| cx.notify());
        Self {
            dictionaries,
            keyboard,
            keyboard_tab: false,
            font_picker: None,
            font_choice_focus: std::array::from_fn(|_| cx.focus_handle().tab_stop(true)),
            updates,
            _update_changes: update_changes,
            _preference_changes: preference_changes,
            preferences,
            account,
            libraries,
            error: None,
            #[cfg(not(feature = "kobo"))]
            transfer_save_error: None,
            text_search_error: None,
            index_usage: None,
            index_usage_request: None,
            #[cfg(feature = "kobo")]
            uninstall_error: None,
            #[cfg(feature = "kobo")]
            auto_sleep_error: None,
            #[cfg(feature = "kobo")]
            auto_power_off_error: None,
            #[cfg(feature = "kobo")]
            kobo_updates,
            #[cfg(feature = "kobo")]
            _kobo_update_changes: kobo_update_changes,
            focus: cx.focus_handle(),
            scroll: ScrollHandle::default(),
            sections,
        }
    }

    #[cfg(feature = "kobo")]
    fn confirm_uninstall(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let page = cx.weak_entity();
        window.open_alert_dialog(cx, move |dialog, window, cx| {
            let theme = components::browser_theme(cx);
            let page = page.clone();
            let cancel = components::dialog_cancel_button("cancel-uninstall-bokheim", "Cancel").on_click(|_, window, cx| window.close_dialog(cx));
            let uninstall = components::base_button("confirm-uninstall-bokheim").label("Uninstall").danger().on_click(move |_, window, cx| {
                window.close_dialog(cx);
                let _ = page.update(cx, |page, cx| {
                    let result = (|| {
                        if std::env::var("BOKHEIM_KOBO_CAN_UNINSTALL").as_deref() != Ok("1") {
                            return Err("Reinstall with the latest installer before uninstalling from Settings.".to_owned());
                        }
                        let path = std::env::var_os("BOKHEIM_KOBO_EXIT_ACTION_FILE").ok_or_else(|| "The Kobo launcher is unavailable.".to_owned())?;
                        std::fs::write(path, "uninstall").map_err(|error| user_facing_error(&app::BackendError::from(error), "Couldn't start uninstalling. Restart the app and retry.").to_owned())
                    })();
                    match result {
                        Ok(()) => cx.quit(),
                        Err(error) => {
                            page.uninstall_error = Some(error.into());
                            cx.notify();
                        }
                    }
                });
            });
            components::confirmation_dialog_row(components::confirmation_dialog(dialog, window), "Uninstall Bokheim and return to Kobo? Your books and settings will be kept.", cancel, uninstall, theme)
        });
    }

    /// One conditional banner above the settings. The backend decides which
    /// state needs attention; recoverable errors are never dialogs.
    fn update_row(&self, cx: &Context<Self>) -> Option<gpui::Div> {
        let theme = components::browser_theme(cx);
        let row = crate::services::updates::render_row(self.updates.as_ref()?, "apply-update", theme, cx)?;
        Some(self.section(Section::Updates, components::browser_settings_banner(theme).child(row)))
    }

    /// Applies a preference, and on failure says which setting did not take and
    /// what is still on screen.
    ///
    /// "Could not save setting" is true of every control on the page, which
    /// makes it useless on the one that was just pressed — and the interface
    /// goes on showing the old value, which without being told reads as the
    /// press having done nothing at all.
    fn update_browsing(&mut self, patch: BrowsingPreferencePatch, cx: &mut Context<Self>) {
        if let BrowsingPreferencePatch::TextSearch(enabled) = patch {
            self.text_search_error = None;
            let saved = self.preferences.update(cx, |preferences, cx| preferences.save_text_search(enabled, cx));
            cx.spawn(async move |page, cx| {
                let result = saved.await;
                let _ = page.update(cx, |page, cx| {
                    page.text_search_error = result.err().map(|error| format!("Could not save text search. {}", user_facing_error(&error, "Please try again.")).into());
                    page.index_usage_request = None;
                    page.index_usage = None;
                    cx.notify();
                });
            })
            .detach();
            return;
        }
        #[cfg(not(feature = "kobo"))]
        if let BrowsingPreferencePatch::AllowMeteredTransfers(enabled) = patch {
            self.transfer_save_error = None;
            self.error = None;
            let saved = self.preferences.update(cx, |preferences, cx| preferences.save_metered_transfers(enabled, cx));
            cx.spawn(async move |page, cx| {
                let result = saved.await;
                let _ = page.update(cx, |page, cx| {
                    page.transfer_save_error = result.err().map(|error| format!("Could not save the transfer setting. {}", user_facing_error(&error, "Please try again.")).into());
                    cx.notify();
                });
            })
            .detach();
            cx.notify();
            return;
        }
        let showing = self.preferences.read(cx).browsing();
        let (setting, current) = match &patch {
            BrowsingPreferencePatch::TextSearch(_) => ("Text search", if showing.text_search() { "On" } else { "Off" }.to_owned()),
            BrowsingPreferencePatch::Animations(_) => ("Animations", if showing.animations() { "On" } else { "Off" }.to_owned()),
            BrowsingPreferencePatch::AllowMeteredTransfers(_) => ("Metered transfers", if showing.allow_metered_transfers() { "On" } else { "Off" }.to_owned()),
            BrowsingPreferencePatch::Theme(_) => ("Application theme", showing.theme().label().to_owned()),
            BrowsingPreferencePatch::Palette(_, _) => {
                let appearance = showing.appearance_override().unwrap_or_else(|| components::theme::resolved_appearance(cx));
                ("Application theme", components::theme_choice_label(showing.theme(), appearance))
            }
            BrowsingPreferencePatch::UiFontSizePx(_) => ("UI font size", format!("{}px", showing.ui_font_size_px())),
            BrowsingPreferencePatch::UiFontFamily(_) | BrowsingPreferencePatch::CustomUiFontFamily(_) => ("UI typeface", showing.ui_font_name().to_owned()),
            BrowsingPreferencePatch::CoverText(_) => ("Cover text", showing.cover_text().label().to_owned()),
            BrowsingPreferencePatch::GeneratedCoversOnly(_) => ("Covers", if showing.generated_covers_only() { "Generated" } else { "Book art" }.to_owned()),
            BrowsingPreferencePatch::CatalogGrouping(_) => ("Catalog grouping", showing.catalog_grouping().label().to_owned()),
            BrowsingPreferencePatch::CatalogLanguages(_) => ("Catalog languages", showing.catalog_languages().map_or_else(|| "Default".into(), |languages| if languages.is_empty() { "Any".into() } else { languages.join(", ") })),
            BrowsingPreferencePatch::KeyboardShortcuts(_) => ("Keyboard shortcuts", "Previous shortcuts".to_owned()),
            BrowsingPreferencePatch::KeyboardLayouts(_) => ("Keyboard layouts", showing.keyboard_layouts().join(", ")),
        };
        let result = self.preferences.update(cx, |preferences, cx| preferences.update_browsing(patch, cx));
        self.error = result.err().map(|error| format!("{setting} was not saved. {} Still showing {current}.", user_facing_error(&error, "Please try again.")).into());
        cx.notify();
    }

    pub(crate) fn pause_index_usage(&mut self) {
        self.index_usage_request = None;
    }

    fn refresh_index_usage(&mut self, cx: &mut Context<Self>) {
        let backend = self.preferences.read(cx).backend();
        self.index_usage_request = Some(cx.spawn(async move |page, cx| {
            loop {
                let result = backend.text_search_index_bytes().await;
                if page
                    .update(cx, |page, cx| {
                        match result {
                            Ok(bytes) => page.index_usage = Some(format_storage_bytes(bytes).into()),
                            Err(error) => {
                                page.index_usage = Some("Unavailable".into());
                                log::warn!("could not read text index usage: {error}");
                            }
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
                cx.background_executor().timer(std::time::Duration::from_secs(5)).await;
            }
        }));
    }

    fn preference_button(button: Button, patch: BrowsingPreferencePatch, cx: &Context<Self>) -> Button {
        button.on_click(cx.listener(move |page, _, _, cx| page.update_browsing(patch.clone(), cx)))
    }

    /// Moves focus one control forward, wrapping at the end of the page.
    fn focus_next(&mut self, _: &FocusNextSetting, window: &mut Window, cx: &mut Context<Self>) {
        window.focus_next(cx);
        if !self.focus.contains_focused(window, cx) {
            self.focus.focus(window, cx);
            window.focus_next(cx);
        }
        self.reveal_focused_section(window, cx);
    }

    /// Moves focus one control back, wrapping at the start of the page.
    fn focus_previous(&mut self, _: &FocusPreviousSetting, window: &mut Window, cx: &mut Context<Self>) {
        window.focus_prev(cx);
        if !self.focus.contains_focused(window, cx) {
            self.focus_last_control(window, cx);
        }
        self.reveal_focused_section(window, cx);
    }

    /// Walks forward from the page container to whatever the last control in it
    /// is. There is no "previous from the start" step, so the way back to the
    /// end of the page is to go round it.
    fn focus_last_control(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window, cx);
        let mut last = None;
        for _ in 0..MAX_CONTROLS {
            window.focus_next(cx);
            if !self.focus.contains_focused(window, cx) {
                break;
            }
            last = window.focused(cx);
        }
        if let Some(handle) = last {
            handle.focus(window, cx);
        }
    }

    /// Scrolls whichever section now holds focus into view.
    fn reveal_focused_section(&self, window: &mut Window, cx: &mut Context<Self>) {
        if self.keyboard_tab && self.keyboard.read(cx).reveal_focused_row(&self.scroll, window, cx) {
            cx.notify();
            return;
        }
        let Some(section) = self.sections.iter().find(|section| section.focus.contains_focused(window, cx)) else {
            return;
        };
        if components::scroll_to_reveal(&self.scroll, section.bounds.get(), px(REVEAL_MARGIN)) {
            cx.notify();
        }
    }

    /// Wraps `child` in the measured, focusable block that keyboard navigation
    /// scrolls to.
    fn section(&self, id: Section, child: impl IntoElement) -> gpui::Div {
        let section = &self.sections[id as usize];
        components::browser_settings_section(&section.focus, section.bounds.clone(), child)
    }
}

impl Focusable for SettingsPage {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SettingsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.index_usage_request.is_none() {
            self.refresh_index_usage(cx);
        }
        let theme = components::browser_theme(cx);
        let preferences = self.preferences.read(cx).browsing();
        let desktop = !components::uses_mobile_navigation(window) && !cfg!(feature = "kobo");
        if desktop && self.keyboard_tab {
            let keyboard = self.keyboard_panel();
            return self.settings_container(keyboard, desktop, cx).into_any_element();
        }
        let current_font_size = preferences.ui_font_size_px();

        #[cfg(not(feature = "kobo"))]
        let application_themes = {
            let selected = cx.try_global::<components::theme::SelectedPalette>().map(|selection| (selection.0, selection.1));
            let mut application_themes = components::theme_picker_group();
            for (index, (value, appearance)) in components::THEME_CHOICES.into_iter().enumerate() {
                application_themes = application_themes.child(Self::preference_button(
                    components::theme_picker_swatch(format!("application-theme-{index}"), value, appearance, components::theme_choice_selected(selected, value, appearance)),
                    BrowsingPreferencePatch::Palette(value, appearance),
                    cx,
                ));
            }

            application_themes
        };

        let [decrease, increase] = [
            ("decrease-ui-font-size", "−", components::configured_shortcut_tooltip("Decrease interface font size", "browse.font_decrease", cx), current_font_size.saturating_sub(1).max(UI_FONT_SIZE_MIN), UI_FONT_SIZE_MIN),
            ("increase-ui-font-size", "+", components::configured_shortcut_tooltip("Increase interface font size", "browse.font_increase", cx), current_font_size.saturating_add(1).min(UI_FONT_SIZE_MAX), UI_FONT_SIZE_MAX),
        ]
        .map(|(id, label, tooltip, value, limit)| Self::preference_button(components::settings_stepper_button(id, label, current_font_size == limit).tooltip(tooltip), BrowsingPreferencePatch::UiFontSizePx(value), cx));
        let font_size = components::settings_stepper(decrease, format!("{current_font_size}px"), increase, theme);

        let font_families = {
            let labels = if cfg!(any(target_arch = "wasm32", feature = "kobo")) { vec!["Serif", "Sans serif"] } else { vec!["Serif", "Sans serif", preferences.custom_ui_font_family().unwrap_or("Custom")] };
            let width = (window.rem_size() * components::THEME_PICKER_WIDTH_REM).max(components::equal_choice_group_width(&labels, components::TEXT_XS, 2.0, components::SPACE_SM, window, cx));
            let mut choices = components::reader_tile_group("ui-font-families").w(width).max_w_full().flex_none();
            for (index, value) in [UiFontFamily::Serif, UiFontFamily::SansSerif].into_iter().enumerate() {
                choices = choices.child(
                    components::reader_picture_tile(
                        format!("ui-font-family-{index}"),
                        components::reader_typeface_sample(Some(value.family_name())),
                        value.label(),
                        !preferences.custom_ui_font_active() && value == preferences.ui_font_family(),
                        theme,
                    )
                    .track_focus(&self.font_choice_focus[index])
                    .on_click(cx.listener(move |page, _, _, cx| page.update_browsing(BrowsingPreferencePatch::UiFontFamily(value), cx)))
                    .on_key_down(cx.listener(move |page, event: &KeyDownEvent, _, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            page.update_browsing(BrowsingPreferencePatch::UiFontFamily(value), cx);
                            cx.stop_propagation();
                        }
                    })),
                );
            }
            if !cfg!(any(target_arch = "wasm32", feature = "kobo")) {
                let picker = self.font_picker.get_or_insert_with(|| {
                    let picker = cx.new(|cx| font_picker::FontPicker::new(window, cx));
                    cx.subscribe(&picker, |page, _, event: &font_picker::FontSelected, cx| {
                        page.update_browsing(BrowsingPreferencePatch::CustomUiFontFamily(event.0.clone()), cx);
                    })
                    .detach();
                    picker
                });
                picker.update(cx, |picker, _| {
                    picker.set_selected(preferences.custom_ui_font_family().map(str::to_owned));
                    picker.set_active(preferences.custom_ui_font_active());
                });
                choices = choices.child(div().flex_1().min_w_0().overflow_hidden().child(picker.clone()));
            }
            choices
        };

        let current_cover_text = preferences.cover_text();
        let cover_texts = {
            let width = (window.rem_size() * components::THEME_PICKER_WIDTH_REM)
                .max(components::equal_choice_group_width(&["Always", "Cover only"], components::TEXT_SM, 2.0 * components::SPACE_SM, components::SETTINGS_CONTROL_GAP, window, cx) + px(2.0));
            let mut choices = components::browser_settings_button_group(theme).w(width).flex_none();
            for (index, value) in [CoverText::Always, CoverText::CoverOnly].into_iter().enumerate() {
                choices = choices.child(Self::preference_button(components::settings_choice_button(format!("cover-text-{index}"), value.label(), value == current_cover_text, theme), BrowsingPreferencePatch::CoverText(value), cx));
            }
            choices
        };

        let appearance_settings = components::browser_settings_fieldset("Appearance", theme);
        #[cfg(not(feature = "kobo"))]
        let appearance_settings = appearance_settings.child(self.section(Section::Theme, components::browser_settings_compact_row("Application theme", application_themes, theme).border_t_0()));
        let appearance_settings = appearance_settings.child(self.section(Section::CoverText, components::browser_settings_compact_row("Cover text", cover_texts, theme).when(cfg!(feature = "kobo"), |row| row.border_t_0())));
        // Book art or every book's generated cover; generated skips reading
        // and decoding cover images, the costliest part of a library page here.
        #[cfg(feature = "kobo")]
        let appearance_settings = {
            let generated = preferences.generated_covers_only();
            let choices = components::browser_settings_button_group(theme)
                .child(Self::preference_button(components::settings_choice_button("covers-book-art", "Book art", !generated, theme), BrowsingPreferencePatch::GeneratedCoversOnly(false), cx))
                .child(Self::preference_button(components::settings_choice_button("covers-generated", "Generated", generated, theme), BrowsingPreferencePatch::GeneratedCoversOnly(true), cx));
            appearance_settings.child(self.section(Section::GeneratedCovers, components::browser_settings_compact_row("Covers", choices, theme)))
        };
        let appearance_settings = appearance_settings.child(self.section(Section::FontFamily, components::browser_settings_compact_row("UI typeface", font_families, theme)));
        let appearance_settings = appearance_settings.child(self.section(Section::FontSize, components::browser_settings_compact_row("UI font size", font_size, theme)));
        #[cfg(not(feature = "kobo"))]
        let appearance_settings = {
            let enabled = preferences.animations();
            let choices = components::browser_settings_button_group(theme)
                .child(Self::preference_button(components::settings_choice_button("animations-off", "Off", !enabled, theme), BrowsingPreferencePatch::Animations(false), cx))
                .child(Self::preference_button(components::settings_choice_button("animations-on", "On", enabled, theme), BrowsingPreferencePatch::Animations(true), cx));
            appearance_settings.child(self.section(Section::Animations, components::browser_settings_compact_row("Animations", choices, theme)))
        };
        // Every preference on this page lives in this group, so this is where a
        // preference that would not save reports.
        let appearance_settings = appearance_settings.children(self.error.clone().map(|error| components::browser_settings_group_error(error, theme)));

        let enabled = preferences.text_search();
        let choices = components::browser_settings_button_group(theme)
            .child(Self::preference_button(components::settings_choice_button("text-search-off", "Off", !enabled, theme), BrowsingPreferencePatch::TextSearch(false), cx))
            .child(Self::preference_button(components::settings_choice_button("text-search-on", "On", enabled, theme), BrowsingPreferencePatch::TextSearch(true), cx));
        let usage = self.index_usage.clone().unwrap_or_else(|| "Calculating…".into());
        let text_search = components::browser_settings_fieldset("Text search", theme)
            .child(components::browser_settings_compact_row("Search downloaded book text", choices, theme).border_t_0())
            .child(components::browser_settings_compact_row("Index disk usage", div().child(usage), theme))
            .children(self.text_search_error.clone().map(|error| components::browser_settings_group_error(error, theme)));
        let mut sections = components::browser_settings_sections().child(appearance_settings).child(self.section(Section::Dictionaries, self.dictionaries.clone()));
        #[cfg(not(feature = "kobo"))]
        {
            let metered = preferences.allow_metered_transfers();
            let transfer_choices = components::browser_settings_button_group(theme)
                .child(Self::preference_button(components::settings_choice_button("transfers-unmetered", "Off", !metered, theme), BrowsingPreferencePatch::AllowMeteredTransfers(false), cx))
                .child(Self::preference_button(components::settings_choice_button("transfers-metered", "On", metered, theme), BrowsingPreferencePatch::AllowMeteredTransfers(true), cx));
            let transfers = components::browser_settings_fieldset("Transfers", theme)
                .child(components::browser_settings_compact_row("Allow automatic transfers on metered connections", transfer_choices, theme).border_t_0())
                .children(self.transfer_save_error.clone().map(|error| components::browser_settings_group_error(error, theme)));
            sections = sections.child(self.section(Section::Transfers, transfers));
        }
        sections = sections.child(self.section(Section::TextSearch, text_search)).child(self.section(Section::Account, self.account.clone()));

        #[cfg(feature = "kobo")]
        {
            let selected = kobo_device::settings::idle_timeout().map_or(0, |timeout| timeout.as_secs() / 60);
            let mut auto_sleep = components::settings_choice_button_grid(2, theme);
            for (index, minutes) in kobo_device::settings::AUTO_SLEEP_MINUTES.into_iter().enumerate() {
                let label = if minutes == 0 { "Never".to_owned() } else { format!("{minutes} min") };
                auto_sleep = auto_sleep.child(components::settings_choice_button(format!("kobo-auto-sleep-{index}"), label, selected == u64::from(minutes), theme).on_click(cx.listener(move |page, _, _, cx| {
                    page.auto_sleep_error = kobo_device::settings::set_auto_sleep_minutes(minutes).err().map(|error| format!("Automatic sleep wasn't saved. {}", kobo_setting_recovery(&error)).into());
                    cx.notify();
                })));
            }
            let selected_power_off = kobo_device::settings::power_off_timeout().map_or(0, |timeout| timeout.as_secs() / 60);
            let mut auto_power_off = components::settings_choice_button_grid(2, theme);
            for (index, minutes) in kobo_device::settings::AUTO_POWER_OFF_MINUTES.into_iter().enumerate() {
                let label = if minutes == 0 { "Never".to_owned() } else { format!("{minutes} min") };
                auto_power_off = auto_power_off.child(components::settings_choice_button(format!("kobo-auto-power-off-{index}"), label, selected_power_off == u64::from(minutes), theme).on_click(cx.listener(move |page, _, _, cx| {
                    page.auto_power_off_error = kobo_device::settings::set_auto_power_off_minutes(minutes).err().map(|error| format!("Automatic power off wasn't saved. {}", kobo_setting_recovery(&error)).into());
                    cx.notify();
                })));
            }
            let uninstall = components::base_button("uninstall-bokheim").label("Uninstall Bokheim").danger().on_click(cx.listener(|page, _, window, cx| page.confirm_uninstall(window, cx)));
            let device = components::browser_settings_fieldset("Kobo", theme)
                .children(crate::services::updates::render_row(&self.kobo_updates, "kobo-update", theme, cx))
                .child(components::browser_settings_compact_row("Automatic sleep", auto_sleep, theme))
                .child(div().text_size(gpui::rems(components::TEXT_XS)).text_color(theme.text_muted).child("Shared with Kobo's built-in reader. Changes apply immediately. Listening keeps your Kobo awake."))
                .children(self.auto_sleep_error.clone().map(|error| components::browser_settings_group_error(error, theme)))
                .child(components::browser_settings_compact_row("Automatic power off", auto_power_off, theme))
                .child(div().text_size(gpui::rems(components::TEXT_XS)).text_color(theme.text_muted).child("Shared with Kobo's built-in reader. Counts inactivity, including time asleep. Listening keeps your Kobo on."))
                .children(self.auto_power_off_error.clone().map(|error| components::browser_settings_group_error(error, theme)))
                .child(components::browser_settings_row("Uninstall Bokheim", "Remove the app and its menu entry, keep your books and settings, and return to Kobo.", uninstall, theme))
                .children(self.uninstall_error.clone().map(|error| components::browser_settings_group_error(error, theme)));
            sections = sections.child(self.section(Section::KoboDevice, device));
        }

        #[cfg(target_os = "android")]
        if cx.try_global::<crate::AndroidStoragePolicy>().is_some_and(|policy| policy.all_files_access_available) {
            let access = components::browser_settings_path_button("android-file-access", "Manage file access", theme).on_click(|_, _, _| gpui_android::request_all_files_access());
            sections =
                sections.child(self.section(Section::LibraryStorage, components::browser_settings_fieldset("Library storage", theme).child(components::browser_settings_compact_row("Android permissions", access, theme).border_t_0())));
        }

        sections = sections.child(self.section(Section::Libraries, self.libraries.clone()));

        // Keep account and library invalidations inside their own entities.
        // Transfer progress reaches the library rows several times a second and
        // must not rebuild the settings controls above them.
        self.settings_container(components::browser_settings_content().children(self.update_row(cx)).child(sections), desktop, cx).into_any_element()
    }
}
