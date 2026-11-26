//! Account, appearance and library presentation settings.
//!
//! Preference edits use the shared [`Preferences`] store.
//!
//! Appearance leads, with Account beside it when space allows. Libraries and
//! Dictionaries share the next row on wide screens and stack on narrow ones.
//! [`SyncPage`] keeps owning account and library state and is embedded here.
//!
//! Arrow keys follow the reader settings model: up/down selects a setting,
//! left/right edits its value, and an accent marker identifies the active row.
//! Tab still visits individual controls, including embedded forms and actions.
//! Both navigation modes scroll the focused setting into view.

use ui_text::text;

mod font_picker;
mod keyboard;
#[cfg(feature = "kobo")]
mod kobo_updates;
mod navigation;

use ui_components::user_facing_error;

use crate::services::UpdateControls;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use app_preferences::{BrowsingPreferencePatch, CoverText, UI_FONT_SIZE_MAX, UI_FONT_SIZE_MIN, UiFontFamily};
use gpui::prelude::*;
use gpui::{Bounds, Context, Div, Entity, FocusHandle, Focusable, IntoElement, KeyDownEvent, MouseButton, Pixels, Render, ScrollHandle, SharedString, Window, div, px};
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
        Some(app::Failure::StorageFull) => text!(settings::not_enough_storage_to_save_settings_free_up_device_space_and_retry),
        Some(app::Failure::ReadOnlyStorage) => text!(settings::device_storage_is_read_only_it_must_be_writable_to_save_settings),
        Some(app::Failure::PermissionDenied) => text!(settings::access_to_the_settings_file_or_folder_was_denied_check_their_permissions),
        _ => "",
    }
}

/// The key context the page's focus movement is bound in. A context of its own,
/// because Tab means "next control" only here.
pub(crate) const SETTINGS_KEY_CONTEXT: &str = "BrowserSettings";
pub(crate) const SETTINGS_ROW_KEY_CONTEXT: &str = "BrowserSettings && !Input && !PopupMenu && !Dialog && !ShortcutSettings && !BrowserFontPicker && !BrowserSettingsHeader";

/// Room left around a section brought into view, so a control never sits
/// against the edge of the surface it was scrolled into.
const REVEAL_MARGIN: f32 = 16.0;

/// An upper bound on the walk that wraps focus back to the last control. The
/// page has a few dozen controls at most; the bound only keeps a malformed tab
/// order from spinning.
const MAX_CONTROLS: usize = 512;

/// Settings and action groups in display order. Arrow navigation stops once
/// per setting; Tab can still reach every control inside an action group.
///
/// Appearance leads: it is what most visits come to change, and the account
/// led with a storage figure that nobody opened the page to read. The account
/// and the libraries follow together, since that is what both are about.
/// Dictionaries follow the library list.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
enum Section {
    Updates,
    #[cfg(not(feature = "kobo"))]
    Theme,
    CoverText,
    #[cfg(feature = "kobo")]
    GeneratedCovers,
    #[cfg(not(feature = "kobo"))]
    Animations,
    FontFamily,
    FontSize,
    Account,
    #[cfg(feature = "kobo")]
    KoboAutoSleep,
    #[cfg(feature = "kobo")]
    KoboAutoPowerOff,
    #[cfg(feature = "kobo")]
    KoboUninstall,
    #[cfg(target_os = "android")]
    LibraryStorage,
    Libraries,
    Dictionaries,
}

const SECTION_COUNT: usize = Section::Dictionaries as usize + 1;

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
    parallel_sections: bool,
    font_picker: Option<Entity<font_picker::FontPicker>>,
    font_choice_focus: [FocusHandle; 2],
    #[cfg(not(feature = "kobo"))]
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
    visible_sections: RefCell<Vec<Section>>,
    selected_section: Cell<Option<Section>>,
    keyboard_visible: Cell<bool>,
}

impl SettingsPage {
    pub(crate) fn new(updates: Option<Entity<UpdateControls>>, account: Entity<SyncPage>, preferences: Entity<Preferences>, cx: &mut Context<Self>) -> Self {
        let update_changes = updates.as_ref().map(|updates| cx.observe(updates, |_, _, cx| cx.notify()));
        let preference_changes = cx.observe(&preferences, |_, _, cx| cx.notify());
        #[cfg(target_os = "android")]
        cx.observe_global::<crate::AndroidStoragePolicy>(|_: &mut Self, cx| cx.notify()).detach();
        let libraries = cx.new(|cx| LibrariesView::new(account.clone(), cx));
        let dictionaries = cx.new(|cx| dictionary_settings::DictionarySettings::new(dictionary_settings::client(cx), Vec::new(), cx));
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
            parallel_sections: false,
            font_picker: None,
            font_choice_focus: std::array::from_fn(|_| cx.focus_handle().tab_stop(true)),
            #[cfg(not(feature = "kobo"))]
            updates,
            _update_changes: update_changes,
            _preference_changes: preference_changes,
            preferences,
            account,
            libraries,
            error: None,
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
            visible_sections: RefCell::new(Vec::new()),
            selected_section: Cell::new(None),
            keyboard_visible: Cell::new(false),
        }
    }

    #[cfg(feature = "kobo")]
    fn confirm_uninstall(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let page = cx.weak_entity();
        window.open_alert_dialog(cx, move |dialog, window, cx| {
            let theme = components::browser_theme(cx);
            let page = page.clone();
            let cancel = components::dialog_cancel_button("cancel-uninstall-bokheim", text!(common::cancel)).on_click(|_, window, cx| window.close_dialog(cx));
            let uninstall = components::action_button("confirm-uninstall-bokheim", text!(settings::uninstall)).danger().on_click(move |_, window, cx| {
                window.close_dialog(cx);
                let _ = page.update(cx, |page, cx| {
                    let result = (|| {
                        if std::env::var("BOKHEIM_KOBO_CAN_UNINSTALL").as_deref() != Ok("1") {
                            return Err(text!(settings::reinstall_with_the_latest_installer_before_uninstalling_from_settings).to_owned());
                        }
                        let path = std::env::var_os("BOKHEIM_KOBO_EXIT_ACTION_FILE").ok_or_else(|| text!(settings::the_kobo_launcher_is_unavailable_2).to_owned())?;
                        std::fs::write(path, "uninstall").map_err(|error| user_facing_error(&app::BackendError::from(error), text!(settings::couldn_t_start_uninstalling_restart_the_app_and_retry)).to_owned())
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
            components::confirmation_dialog_row(components::confirmation_dialog(dialog, window), text!(settings::uninstall_bokheim_and_return_to_kobo_your_books_and_settings_will_be), cancel, uninstall, theme)
        });
    }

    /// One conditional banner above the settings. The backend decides which
    /// state needs attention; recoverable errors are never dialogs.
    fn update_row(&self, cx: &Context<Self>) -> Option<gpui::Div> {
        let theme = components::browser_theme(cx);
        #[cfg(feature = "kobo")]
        let controls = {
            use crate::services::updates::UpdateView;
            if matches!(self.kobo_updates.read(cx).view, UpdateView::Hidden | UpdateView::Current { .. } | UpdateView::Checking) {
                return None;
            }
            &self.kobo_updates
        };
        #[cfg(not(feature = "kobo"))]
        let controls = self.updates.as_ref()?;
        let row = crate::services::updates::render_row(controls, "apply-update", theme, cx)?;
        Some(self.section(Section::Updates, components::browser_settings_banner(theme).child(row), cx))
    }

    /// Applies a preference, and on failure says which setting did not take and
    /// what is still on screen.
    ///
    /// "Could not save setting" is true of every control on the page, which
    /// makes it useless on the one that was just pressed — and the interface
    /// goes on showing the old value, which without being told reads as the
    /// press having done nothing at all.
    fn update_browsing(&mut self, patch: BrowsingPreferencePatch, cx: &mut Context<Self>) {
        let showing = self.preferences.read(cx).browsing();
        let (setting, current) = match &patch {
            BrowsingPreferencePatch::Animations(_) => (text!(settings::animations), if showing.animations() { text!(common::on) } else { text!(common::off) }.to_owned()),
            BrowsingPreferencePatch::Theme(_) => (text!(settings::application_theme), showing.theme().label().to_owned()),
            BrowsingPreferencePatch::Palette(_, _) => {
                let appearance = showing.appearance_override().unwrap_or_else(|| components::theme::resolved_appearance(cx));
                (text!(settings::application_theme), components::theme_choice_label(showing.theme(), appearance))
            }
            BrowsingPreferencePatch::UiFontSizePx(_) => (text!(settings::ui_font_size), format!("{}px", showing.ui_font_size_px())),
            BrowsingPreferencePatch::UiFontFamily(_) | BrowsingPreferencePatch::CustomUiFontFamily(_) => (text!(settings::ui_typeface), showing.ui_font_name().to_owned()),
            BrowsingPreferencePatch::CoverText(_) => (text!(settings::cover_text), showing.cover_text().label().to_owned()),
            BrowsingPreferencePatch::BookListView(_) => (text!(browse::view), showing.book_list_view().label().to_owned()),
            BrowsingPreferencePatch::GraphView(_) => (text!(browse::view), if showing.graph_view() { text!(browse::show_as_graph) } else { showing.book_list_view().label() }.to_owned()),
            BrowsingPreferencePatch::GeneratedCoversOnly(_) => (text!(settings::covers), if showing.generated_covers_only() { text!(settings::generated) } else { text!(settings::book_art) }.to_owned()),
            BrowsingPreferencePatch::CatalogGrouping(_) => (text!(settings::catalog_grouping), showing.catalog_grouping().label().to_owned()),
            BrowsingPreferencePatch::CatalogLanguages(_) => {
                (text!(settings::catalog_languages), showing.catalog_languages().map_or_else(|| text!(settings::default).into(), |languages| if languages.is_empty() { text!(browse::any).into() } else { languages.join(", ") }))
            }
            BrowsingPreferencePatch::KeyboardShortcuts(_) => (text!(settings::keyboard_shortcuts), text!(settings::previous_shortcuts).to_owned()),
            BrowsingPreferencePatch::KeyboardLayouts(_) => (text!(settings::keyboard_layouts), showing.keyboard_layouts().join(", ")),
        };
        let result = self.preferences.update(cx, |preferences, cx| preferences.update_browsing(patch, cx));
        self.error = result.err().map(|error| format!(text!(settings::was_not_saved_still_showing), user_facing_error(&error, ""), current = current, setting = setting).into());
        cx.notify();
    }

    #[cfg(not(feature = "kobo"))]
    fn on_off_choices(ids: [&'static str; 2], enabled: bool, patch: fn(bool) -> BrowsingPreferencePatch, cx: &Context<Self>) -> Div {
        let theme = components::browser_theme(cx);
        components::browser_settings_button_group(theme).children(
            [false, true].into_iter().zip(ids).map(|(value, id)| Self::preference_button(components::settings_choice_button(id, if value { text!(common::on) } else { text!(common::off) }, enabled == value, theme), patch(value), cx)),
        )
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
        self.mark_focused_section(window, cx);
    }

    /// Moves focus one control back, wrapping at the start of the page.
    fn focus_previous(&mut self, _: &FocusPreviousSetting, window: &mut Window, cx: &mut Context<Self>) {
        window.focus_prev(cx);
        if !self.focus.contains_focused(window, cx) {
            self.focus_last_control(window, cx);
        }
        self.mark_focused_section(window, cx);
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
    fn section(&self, id: Section, child: impl IntoElement, cx: &Context<Self>) -> Div {
        self.visible_sections.borrow_mut().push(id);
        let section = &self.sections[id as usize];
        let selected = self.keyboard_visible.get() && self.selected_section.get() == Some(id);
        let theme = components::browser_theme(cx);
        components::browser_settings_section(&section.focus, section.bounds.clone(), child)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |page, _, _, cx| {
                    page.selected_section.set(Some(id));
                    page.keyboard_visible.set(false);
                    cx.notify();
                }),
            )
            .when(selected, |row| row.child(div().absolute().left(px(-components::CONTENT_INSET)).top_0().bottom_0().w(px(2.0)).bg(theme.accent)))
    }

    /// A whole preference wraps before its controls are squeezed. The basis
    /// includes the measured label and control widths, so larger text and
    /// longer font names can put a setting on a row of its own.
    fn preference_section(&self, id: Section, title: &'static str, control: impl IntoElement, minimum_width: Pixels, window: &Window, cx: &Context<Self>) -> Div {
        let width = minimum_width.max(components::interface_label_width(title, components::TEXT_XS, window, cx));
        self.section(id, components::browser_settings_control(title, control, components::browser_theme(cx)), cx).w_auto().flex_auto().flex_basis(width).min_w_0().max_w_full()
    }
}

impl Focusable for SettingsPage {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SettingsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.visible_sections.borrow_mut().clear();
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
            ("decrease-ui-font-size", "−", components::configured_shortcut_tooltip(text!(common::decrease_interface_font_size), "browse.font_decrease", cx), current_font_size.saturating_sub(1).max(UI_FONT_SIZE_MIN), UI_FONT_SIZE_MIN),
            ("increase-ui-font-size", "+", components::configured_shortcut_tooltip(text!(common::increase_interface_font_size), "browse.font_increase", cx), current_font_size.saturating_add(1).min(UI_FONT_SIZE_MAX), UI_FONT_SIZE_MAX),
        ]
        .map(|(id, label, tooltip, value, limit)| Self::preference_button(components::settings_stepper_button(id, label, current_font_size == limit).tooltip(tooltip), BrowsingPreferencePatch::UiFontSizePx(value), cx));
        let font_size = components::reader_settings_stepper(decrease, format!("{current_font_size}px"), increase, theme);

        let font_family_width;
        let font_families = {
            let labels =
                if cfg!(target_arch = "wasm32") { vec![text!(common::serif), text!(common::sans_serif)] } else { vec![text!(common::serif), text!(common::sans_serif), preferences.custom_ui_font_family().unwrap_or(text!(common::custom))] };
            let sample_width = components::interface_label_width(text!(common::aa), 1.25, window, cx);
            font_family_width = (components::equal_choice_group_width(&labels, components::TEXT_SM, f32::from(sample_width) + 2.0 * components::SPACE_SM + components::SPACE_XS, components::SETTINGS_CONTROL_GAP, window, cx) + px(2.0))
                .max(components::interface_label_width(text!(settings::ui_typeface), components::TEXT_XS, window, cx));
            let mut choices = components::settings_choice_row(labels.len() as u16, theme).id("ui-font-families");
            for (index, value) in [UiFontFamily::Serif, UiFontFamily::SansSerif].into_iter().enumerate() {
                choices = choices.child(
                    components::reader_picture_choice(
                        format!("ui-font-family-{index}"),
                        components::reader_typeface_sample(Some(value.family_name())),
                        value.label(),
                        !preferences.custom_ui_font_active() && value == preferences.ui_font_family(),
                        theme,
                    )
                    .h_full()
                    .min_h_0()
                    .min_w_auto()
                    .track_focus(&self.font_choice_focus[index])
                    .on_click(cx.listener(move |page, _, _, cx| page.update_browsing(BrowsingPreferencePatch::UiFontFamily(value), cx))),
                );
            }
            if !cfg!(target_arch = "wasm32") {
                let picker = self.font_picker.get_or_insert_with(|| {
                    let picker = cx.new(|cx| font_picker::FontPicker::new(window, cx));
                    cx.subscribe(&picker, |page, _, event: &font_picker::FontSelected, cx| {
                        page.update_browsing(BrowsingPreferencePatch::CustomUiFontFamily(event.0.clone()), cx);
                    })
                    .detach();
                    picker
                });
                picker.update(cx, |picker, cx| {
                    picker.set_selected(preferences.custom_ui_font_family().map(str::to_owned), cx);
                    picker.set_active(preferences.custom_ui_font_active());
                });
                choices = choices.child(picker.clone());
            }
            choices
        };

        let current_cover_text = preferences.cover_text();
        let cover_text_width = components::equal_choice_group_width(&[text!(settings::always), text!(settings::cover_only)], components::TEXT_SM, 2.0 * components::SPACE_SM, components::SETTINGS_CONTROL_GAP, window, cx) + px(2.0);
        let cover_texts = {
            let mut choices = components::browser_settings_button_group(theme).w_full();
            for (index, value) in [CoverText::Always, CoverText::CoverOnly].into_iter().enumerate() {
                choices = choices.child(Self::preference_button(components::settings_choice_button(format!("cover-text-{index}"), value.label(), value == current_cover_text, theme), BrowsingPreferencePatch::CoverText(value), cx));
            }
            choices
        };

        let mut appearance_controls = div().w_full().min_w_0().flex().flex_col().gap(px(10.0));
        #[cfg(not(feature = "kobo"))]
        {
            appearance_controls = appearance_controls.child(self.section(Section::Theme, components::browser_settings_control(text!(settings::application_theme), application_themes, theme), cx));
        }
        let cover_text = self.preference_section(Section::CoverText, text!(settings::cover_text), cover_texts, cover_text_width, window, cx);
        let mut cover_row = components::browser_settings_paired_controls().child(cover_text);
        // Book art or every book's generated cover; generated skips reading
        // and decoding cover images, the costliest part of a library page here.
        #[cfg(feature = "kobo")]
        {
            let generated = preferences.generated_covers_only();
            let choices = components::browser_settings_button_group(theme)
                .w_full()
                .child(Self::preference_button(components::settings_choice_button("covers-book-art", text!(settings::book_art), !generated, theme), BrowsingPreferencePatch::GeneratedCoversOnly(false), cx))
                .child(Self::preference_button(components::settings_choice_button("covers-generated", text!(settings::generated), generated, theme), BrowsingPreferencePatch::GeneratedCoversOnly(true), cx));
            let width = components::equal_choice_group_width(&[text!(settings::book_art), text!(settings::generated)], components::TEXT_SM, 2.0 * components::SPACE_SM, components::SETTINGS_CONTROL_GAP, window, cx) + px(2.0);
            cover_row = cover_row.child(self.preference_section(Section::GeneratedCovers, text!(settings::covers), choices, width, window, cx));
        }
        #[cfg(not(feature = "kobo"))]
        {
            let enabled = preferences.animations();
            let choices = Self::on_off_choices(["animations-off", "animations-on"], enabled, BrowsingPreferencePatch::Animations, cx).w_full();
            let width = components::equal_choice_group_width(&[text!(common::off), text!(common::on)], components::TEXT_SM, 2.0 * components::SPACE_SM, components::SETTINGS_CONTROL_GAP, window, cx) + px(2.0);
            cover_row = cover_row.child(self.preference_section(Section::Animations, text!(settings::animations), choices, width, window, cx));
        }
        appearance_controls = appearance_controls.child(cover_row).child(
            components::browser_settings_paired_controls()
                .child(self.preference_section(Section::FontFamily, text!(settings::ui_typeface), font_families, font_family_width, window, cx))
                .child(self.section(Section::FontSize, components::browser_settings_control(text!(settings::ui_font_size), font_size, theme), cx).w_auto().flex_none().min_w_auto().max_w_full()),
        );
        // Every preference on this page lives in this group, so this is where a
        // preference that would not save reports.
        let appearance_settings = components::browser_settings_fieldset(text!(settings::appearance), theme).child(appearance_controls).children(self.error.clone().map(|error| components::browser_settings_group_error(error, theme)));

        let account = self.section(Section::Account, self.account.clone(), cx).min_w_0();
        #[cfg(not(feature = "kobo"))]
        let device = None::<Div>;

        #[cfg(feature = "kobo")]
        let device = {
            let selected = kobo_device::settings::idle_timeout().map_or(0, |timeout| timeout.as_secs() / 60);
            let mut auto_sleep = components::settings_choice_button_grid(3, theme);
            for (index, minutes) in kobo_device::settings::AUTO_SLEEP_MINUTES.into_iter().enumerate() {
                let label = if minutes == 0 { text!(settings::never).to_owned() } else { format!(text!(settings::min), minutes = minutes) };
                auto_sleep = auto_sleep.child(components::settings_choice_button(format!("kobo-auto-sleep-{index}"), label, selected == u64::from(minutes), theme).on_click(cx.listener(move |page, _, _, cx| {
                    page.auto_sleep_error = kobo_device::settings::set_auto_sleep_minutes(minutes).err().map(|error| format!(text!(settings::automatic_sleep_wasn_t_saved), kobo_setting_recovery(&error)).into());
                    cx.notify();
                })));
            }
            let selected_power_off = kobo_device::settings::power_off_timeout().map_or(0, |timeout| timeout.as_secs() / 60);
            let mut auto_power_off = components::settings_choice_button_grid(3, theme);
            for (index, minutes) in kobo_device::settings::AUTO_POWER_OFF_MINUTES.into_iter().enumerate() {
                let label = if minutes == 0 { text!(settings::never).to_owned() } else { format!(text!(settings::min), minutes = minutes) };
                auto_power_off = auto_power_off.child(components::settings_choice_button(format!("kobo-auto-power-off-{index}"), label, selected_power_off == u64::from(minutes), theme).on_click(cx.listener(move |page, _, _, cx| {
                    page.auto_power_off_error = kobo_device::settings::set_auto_power_off_minutes(minutes).err().map(|error| format!(text!(settings::automatic_power_off_wasn_t_saved), kobo_setting_recovery(&error)).into());
                    cx.notify();
                })));
            }
            let uninstall = components::action_button("uninstall-bokheim", text!(settings::uninstall_bokheim)).danger().on_click(cx.listener(|page, _, window, cx| page.confirm_uninstall(window, cx)));
            // Three equal columns need the widest preset, including Never.
            let labels: Vec<_> = kobo_device::settings::AUTO_SLEEP_MINUTES
                .iter()
                .chain(kobo_device::settings::AUTO_POWER_OFF_MINUTES.iter())
                .map(|minutes| if *minutes == 0 { text!(settings::never).to_owned() } else { format!(text!(settings::min), minutes = minutes) })
                .collect();
            let preset_width = labels.iter().map(|label| components::interface_label_width(label, components::TEXT_SM, window, cx)).max().unwrap_or(px(0.0));
            let minimum_width = (window.rem_size() * 15.0).max((preset_width + px(2.0 * components::SPACE_SM)) * 3.0 + px(4.0));
            let sleep = div().child(auto_sleep).children(self.auto_sleep_error.clone().map(|error| components::browser_settings_group_error(error, theme)));
            let power_off = div().child(auto_power_off).children(self.auto_power_off_error.clone().map(|error| components::browser_settings_group_error(error, theme)));
            Some(
                components::browser_settings_fieldset(text!(settings::kobo), theme)
                    .child(components::browser_settings_paired_controls().child(self.preference_section(Section::KoboAutoSleep, text!(settings::automatic_sleep), sleep, minimum_width, window, cx)).child(self.preference_section(
                        Section::KoboAutoPowerOff,
                        text!(settings::automatic_power_off),
                        power_off,
                        minimum_width,
                        window,
                        cx,
                    )))
                    .child(self.section(Section::KoboUninstall, div().mt(px(components::SPACE_SM)).pt(px(components::SPACE_MD)).border_t_1().border_color(theme.rule).child(uninstall), cx))
                    .children(self.uninstall_error.clone().map(|error| components::browser_settings_group_error(error, theme))),
            )
        };

        let account_column = components::browser_settings_sections().child(account);
        #[cfg(target_os = "android")]
        let account_column = if cx.try_global::<crate::AndroidStoragePolicy>().is_some_and(|policy| policy.all_files_access_available && !policy.all_files_access_granted) {
            let access = components::browser_settings_path_button("android-file-access", text!(settings::manage_file_access), theme).on_click(|_, _, _| gpui_android::request_all_files_access());
            account_column.child(self.section(
                Section::LibraryStorage,
                components::browser_settings_fieldset(text!(settings::library_storage), theme).child(components::browser_settings_compact_row(text!(settings::android_permissions), access, theme).border_t_0()),
                cx,
            ))
        } else {
            account_column
        };

        let mut sections = components::browser_settings_sections();
        if self.parallel_sections {
            let appearance_column = components::browser_settings_sections().child(appearance_settings).children(device);
            sections = sections.child(div().w_full().min_w_0().flex().items_start().gap(px(20.0)).child(div().flex_1().min_w_0().child(appearance_column)).child(div().flex_1().min_w_0().child(account_column)));
        } else {
            sections = sections.child(appearance_settings).child(account_column).children(device);
        }
        let libraries = self.section(Section::Libraries, self.libraries.clone(), cx).min_w_0();
        let dictionaries = self.section(Section::Dictionaries, self.dictionaries.clone(), cx).min_w_0();
        sections = sections.child(
            div()
                .w_full()
                .min_w_0()
                .flex()
                .items_start()
                .gap(px(20.0))
                .when(!self.parallel_sections, |row| row.flex_col())
                .child(libraries.when(self.parallel_sections, |column| column.flex_1()))
                .child(dictionaries.when(self.parallel_sections, |column| column.flex_1())),
        );

        // Account and library entities retain their own state; Settings still
        // renders normally when their parent observes a change.
        let parallel_sections = self.parallel_sections;
        let minimum_column = (window.rem_size() * 36.0).max(font_family_width);
        let page = cx.weak_entity();
        let content = components::browser_settings_content().min_w_0().max_w(px(1440.0)).p(px(12.0)).gap(px(20.0)).child(components::browser_settings_sections().children(self.update_row(cx)).child(sections)).on_children_prepainted(
            move |children, window, cx| {
                let Some(bounds) = children.first() else { return };
                let next_parallel = bounds.size.width >= minimum_column * 2.0 + px(20.0);
                if next_parallel != parallel_sections {
                    let page = page.clone();
                    window.defer(cx, move |_, cx| {
                        let _ = page.update(cx, |page, cx| {
                            if page.parallel_sections != next_parallel {
                                page.parallel_sections = next_parallel;
                                cx.notify();
                            }
                        });
                    });
                }
            },
        );
        self.settings_container(content, desktop, cx).into_any_element()
    }
}
